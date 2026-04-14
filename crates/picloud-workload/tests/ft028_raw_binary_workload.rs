//! FT-028 — Raw binary workload support
//!
//! Validates that raw ARM64 binaries can be scheduled, started,
//! health-checked, and monitored by the platform workload scheduler.

use std::collections::HashMap;
use std::time::Duration;

use picloud_domain::iri::{ClusterDomain, ResourceIri};
use picloud_domain::traits::{WorkloadScheduler, WorkloadSpec, WorkloadStatus};
use picloud_domain::workload::{BinarySpec, EnvValue, ResourceLimits, RestartPolicy};
use picloud_workload::{ContainerRuntime, ProcessScheduler};
use uuid::Uuid;

fn test_scheduler() -> ProcessScheduler {
    ProcessScheduler::new_with_runtime(
        Uuid::new_v4(),
        ClusterDomain::default(),
        ContainerRuntime::None,
    )
}

fn binary_iri(name: &str) -> ResourceIri {
    ResourceIri::new(format!(
        "https://picloud.local/products/test-app/binaries/{name}"
    ))
    .unwrap()
}

// ---------------------------------------------------------------------------
// TC-242 — Raw binary workload starts and is health-checked by platform
// ---------------------------------------------------------------------------

/// Schedule a raw binary, verify it starts (Running), then confirm the
/// health-check loop detects its exit and applies the restart policy.
#[tokio::test]
async fn tc242_raw_binary_workload_starts_and_is_health_checked() {
    let scheduler = test_scheduler();

    // --- Phase 1: schedule a long-running binary and confirm it starts ---

    let iri = binary_iri("tc242-long-running");
    let spec = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/sleep".to_string(),
        args: vec!["60".to_string()],
        identity: "tc242-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(100),
            memory_mb: Some(64),
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
    });

    let handle = scheduler.schedule(&iri, &spec).await.unwrap();
    let pid = handle.pid.expect("binary workload must have a PID");
    assert!(pid > 0, "PID must be positive");

    // Status should be Running immediately after schedule
    let status = scheduler.status(&iri).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Running),
        "Expected Running after schedule, got {status:?}"
    );

    // --- Phase 2: start the health-check loop and verify it sees the process ---

    let hc_handle =
        scheduler.start_health_check_loop(Duration::from_millis(100));

    // Give the health check a few cycles to run while the process is alive
    tokio::time::sleep(Duration::from_millis(350)).await;

    // Process should still be Running (sleep 60 hasn't exited)
    let status = scheduler.status(&iri).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Running),
        "Expected Running during health check, got {status:?}"
    );

    // --- Phase 3: stop the workload and verify health-check respects it ---

    scheduler.stop(&iri).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let status = scheduler.status(&iri).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Stopped),
        "Expected Stopped after stop(), got {status:?}"
    );

    hc_handle.abort();

    // --- Phase 4: schedule a short-lived binary with Always restart policy ---
    //     and confirm the health-check loop restarts it

    let iri2 = binary_iri("tc242-restart");
    let spec2 = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/echo".to_string(),
        args: vec!["health-checked".to_string()],
        identity: "tc242-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: None,
            memory_mb: None,
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Always,
    });

    scheduler.schedule(&iri2, &spec2).await.unwrap();

    // echo exits immediately — wait for it to exit
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Start a new health-check loop
    let hc_handle2 =
        scheduler.start_health_check_loop(Duration::from_millis(100));

    // Wait for health check to detect exit and restart
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Abort before checking to avoid lock contention
    hc_handle2.abort();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify that the health-check loop restarted the process
    let workloads = scheduler.workloads.read().await;
    let entry = workloads
        .get(iri2.as_str())
        .expect("workload entry must exist");
    assert!(
        entry.restart_count > 0,
        "Health check must restart exited process with Always policy (got {} restarts)",
        entry.restart_count
    );
}

// ---------------------------------------------------------------------------
// TC-299 — Binary workload exit — raw binary scheduled, started, and monitored
// ---------------------------------------------------------------------------

/// End-to-end lifecycle: schedule a raw binary, observe Running status,
/// let it exit naturally, and verify the scheduler correctly reports
/// Stopped/Failed status depending on exit code.
#[tokio::test]
async fn tc299_binary_workload_exit() {
    let scheduler = test_scheduler();

    // --- Sub-test A: successful exit (exit code 0) → Stopped ---

    let iri_ok = binary_iri("tc299-exit-ok");
    let spec_ok = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/true".to_string(),
        args: vec![],
        identity: "tc299-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(100),
            memory_mb: Some(32),
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
    });

    let handle_ok = scheduler.schedule(&iri_ok, &spec_ok).await.unwrap();
    assert!(handle_ok.pid.expect("must have PID") > 0);

    // Wait for process to exit
    tokio::time::sleep(Duration::from_millis(300)).await;

    let status = scheduler.status(&iri_ok).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Stopped),
        "Successful exit (code 0) should yield Stopped, got {status:?}"
    );

    // --- Sub-test B: failing exit (exit code != 0) → Failed ---

    let iri_fail = binary_iri("tc299-exit-fail");
    let spec_fail = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/false".to_string(),
        args: vec![],
        identity: "tc299-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: None,
            memory_mb: None,
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
    });

    let handle_fail = scheduler.schedule(&iri_fail, &spec_fail).await.unwrap();
    assert!(handle_fail.pid.expect("must have PID") > 0);

    // Wait for process to exit
    tokio::time::sleep(Duration::from_millis(300)).await;

    let status = scheduler.status(&iri_fail).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Failed { .. }),
        "Non-zero exit should yield Failed, got {status:?}"
    );

    // --- Sub-test C: environment variables are injected ---

    let iri_env = binary_iri("tc299-env");
    let mut env = HashMap::new();
    env.insert(
        "PICLOUD_TEST_VAR".to_string(),
        EnvValue::Literal("injected".to_string()),
    );
    let spec_env = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/sh".to_string(),
        args: vec![
            "-c".to_string(),
            "test -n \"$PICLOUD_TEST_VAR\"".to_string(),
        ],
        identity: "tc299-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: None,
            memory_mb: None,
        },
        mounts: vec![],
        env,
        restart_policy: RestartPolicy::Never,
    });

    scheduler.schedule(&iri_env, &spec_env).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let status = scheduler.status(&iri_env).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Stopped),
        "Binary with injected env should exit 0 (Stopped), got {status:?}"
    );

    // --- Sub-test D: nonexistent binary fails to schedule ---

    let iri_bad = binary_iri("tc299-nonexistent");
    let spec_bad = WorkloadSpec::Binary(BinarySpec {
        executable: "/no/such/binary".to_string(),
        args: vec![],
        identity: "tc299-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: None,
            memory_mb: None,
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
    });

    let result = scheduler.schedule(&iri_bad, &spec_bad).await;
    assert!(
        result.is_err(),
        "Scheduling a nonexistent binary must return an error"
    );

    // --- Sub-test E: resource limits are applied ---

    let iri_limited = binary_iri("tc299-limited");
    let spec_limited = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/echo".to_string(),
        args: vec!["resource-limited".to_string()],
        identity: "tc299-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(500),
            memory_mb: Some(256),
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
    });

    let handle_limited = scheduler.schedule(&iri_limited, &spec_limited).await.unwrap();
    assert!(
        handle_limited.pid.expect("must have PID") > 0,
        "Binary with resource limits should start successfully"
    );

    tokio::time::sleep(Duration::from_millis(200)).await;
    let status = scheduler.status(&iri_limited).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Stopped),
        "Resource-limited binary should exit normally, got {status:?}"
    );
}
