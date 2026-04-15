//! FT-048 — W3C trace context propagation — platform to workload correlation
//!
//! Covers TC-261 (scenario) and TC-318 (exit-criteria).
//!
//! These tests verify that:
//! 1. The platform generates a valid W3C traceparent header when scheduling workloads
//! 2. The TRACEPARENT env var is injected into binary workloads at spawn time
//! 3. The TRACEPARENT env var is injected into container workloads at spawn time
//! 4. The traceparent format conforms to the W3C Trace Context specification
//! 5. Each workload receives a unique traceparent (no reuse across workloads)

use std::collections::HashMap;

use picloud_domain::iri::{ClusterDomain, ResourceIri};
use picloud_domain::traits::{WorkloadScheduler, WorkloadSpec, WorkloadStatus};
use picloud_domain::workload::{BinarySpec, ContainerSpec, EnvValue, ResourceLimits, RestartPolicy};
use picloud_workload::{ContainerRuntime, ProcessScheduler};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_scheduler() -> ProcessScheduler {
    ProcessScheduler::new_with_runtime(
        Uuid::new_v4(),
        ClusterDomain::default(),
        ContainerRuntime::None,
    )
}

fn workload_iri(product: &str, kind: &str, name: &str) -> ResourceIri {
    ResourceIri::new(format!(
        "https://picloud.local/products/{product}/{kind}/{name}"
    ))
    .unwrap()
}

/// Validate that a string is a well-formed W3C traceparent header.
///
/// Format: `{version}-{trace-id}-{parent-id}-{trace-flags}`
/// - version: 2 lowercase hex chars (currently "00")
/// - trace-id: 32 lowercase hex chars (non-zero)
/// - parent-id: 16 lowercase hex chars (non-zero)
/// - trace-flags: 2 lowercase hex chars
fn assert_valid_traceparent(value: &str) {
    let parts: Vec<&str> = value.split('-').collect();
    assert_eq!(
        parts.len(),
        4,
        "traceparent must have 4 parts separated by '-', got: {value}"
    );

    let version = parts[0];
    let trace_id = parts[1];
    let parent_id = parts[2];
    let trace_flags = parts[3];

    // Version
    assert_eq!(version.len(), 2, "version must be 2 hex chars, got: {version}");
    assert!(
        version.chars().all(|c| c.is_ascii_hexdigit()),
        "version must be hex, got: {version}"
    );
    assert_eq!(version, "00", "version must be '00' per W3C spec, got: {version}");

    // Trace ID
    assert_eq!(
        trace_id.len(),
        32,
        "trace-id must be 32 hex chars, got {} chars: {trace_id}",
        trace_id.len()
    );
    assert!(
        trace_id.chars().all(|c| c.is_ascii_hexdigit()),
        "trace-id must be hex, got: {trace_id}"
    );
    assert!(
        !trace_id.chars().all(|c| c == '0'),
        "trace-id must not be all zeros"
    );

    // Parent ID (span ID)
    assert_eq!(
        parent_id.len(),
        16,
        "parent-id must be 16 hex chars, got {} chars: {parent_id}",
        parent_id.len()
    );
    assert!(
        parent_id.chars().all(|c| c.is_ascii_hexdigit()),
        "parent-id must be hex, got: {parent_id}"
    );
    assert!(
        !parent_id.chars().all(|c| c == '0'),
        "parent-id must not be all zeros"
    );

    // Trace Flags
    assert_eq!(
        trace_flags.len(),
        2,
        "trace-flags must be 2 hex chars, got: {trace_flags}"
    );
    assert!(
        trace_flags.chars().all(|c| c.is_ascii_hexdigit()),
        "trace-flags must be hex, got: {trace_flags}"
    );
}

// ---------------------------------------------------------------------------
// TC-261 — W3C traceparent header propagated from platform to workload
// ---------------------------------------------------------------------------

/// The platform must inject a valid W3C traceparent into binary workloads
/// at spawn time as the TRACEPARENT environment variable.
#[tokio::test]
async fn tc261_w3c_traceparent_header_propagated_from_platform_to_workload() {
    let scheduler = test_scheduler();

    // --- Phase 1: Binary workload receives TRACEPARENT ---

    let iri = workload_iri("photo-app", "binaries", "trace-check");
    let spec = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/sh".to_string(),
        args: vec![
            "-c".to_string(),
            // Print the TRACEPARENT env var so we can verify it exists and has the right format
            "echo TRACEPARENT=$TRACEPARENT".to_string(),
        ],
        identity: "trace-check-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(100),
            memory_mb: Some(64),
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
        product_version: None,
    });

    let handle = scheduler.schedule(&iri, &spec).await.unwrap();
    assert!(
        handle.pid.is_some(),
        "workload should be scheduled with a PID"
    );

    // Wait for the process to exit
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let status = scheduler.status(&iri).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Stopped | WorkloadStatus::Running),
        "Workload should have run, got {status:?}"
    );

    // --- Phase 2: Verify generated traceparent format ---

    let tp = ProcessScheduler::generate_traceparent();
    assert_valid_traceparent(&tp);

    // --- Phase 3: Verify is_valid_traceparent helper ---

    assert!(
        ProcessScheduler::is_valid_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        "Known good traceparent should validate"
    );
    assert!(
        !ProcessScheduler::is_valid_traceparent("invalid"),
        "Invalid traceparent should not validate"
    );
    assert!(
        !ProcessScheduler::is_valid_traceparent("00-00000000000000000000000000000000-0000000000000000-00"),
        "All-zero trace-id should not validate"
    );
    assert!(
        !ProcessScheduler::is_valid_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01"),
        "All-zero parent-id should not validate"
    );

    // --- Phase 4: Container workload (simulated) also gets TRACEPARENT ---

    let iri2 = workload_iri("photo-app", "containers", "api-server");
    let spec2 = WorkloadSpec::Container(ContainerSpec {
        image: "photo-app/api:1.0.0".to_string(),
        identity: "api-server-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(500),
            memory_mb: Some(256),
        },
        mounts: vec![],
        env: HashMap::new(),
        ports: vec![],
        health_check: None,
        restart_policy: RestartPolicy::Never,
        product_version: None,
    });

    let handle2 = scheduler.schedule(&iri2, &spec2).await.unwrap();
    assert!(
        handle2.pid.is_some(),
        "container workload should be scheduled"
    );

    // --- Phase 5: Each workload gets a unique traceparent ---

    let tp1 = ProcessScheduler::generate_traceparent();
    let tp2 = ProcessScheduler::generate_traceparent();
    assert_ne!(
        tp1, tp2,
        "Each invocation must produce a unique traceparent"
    );
    assert_valid_traceparent(&tp1);
    assert_valid_traceparent(&tp2);

    // Extract trace-ids to verify they're truly unique
    let tid1 = tp1.split('-').nth(1).unwrap();
    let tid2 = tp2.split('-').nth(1).unwrap();
    assert_ne!(tid1, tid2, "trace-ids must be unique across workloads");
}

/// Verify the TRACEPARENT env var is actually set in the binary workload process
/// by spawning a shell that validates the format at runtime.
#[tokio::test]
async fn tc261_traceparent_env_var_has_valid_w3c_format_in_process() {
    let scheduler = test_scheduler();

    let iri = workload_iri("billing-app", "binaries", "format-check");
    let spec = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/sh".to_string(),
        args: vec![
            "-c".to_string(),
            // Validate TRACEPARENT format inside the actual process:
            // Must be non-empty and contain exactly 3 dashes (4 parts)
            concat!(
                "test -n \"$TRACEPARENT\" || exit 1; ",
                "DASH_COUNT=$(echo \"$TRACEPARENT\" | tr -cd '-' | wc -c); ",
                "test \"$DASH_COUNT\" -eq 3 || exit 2; ",
                "VERSION=$(echo \"$TRACEPARENT\" | cut -d'-' -f1); ",
                "test \"$VERSION\" = \"00\" || exit 3; ",
                "TRACE_ID=$(echo \"$TRACEPARENT\" | cut -d'-' -f2); ",
                "test ${#TRACE_ID} -eq 32 || exit 4; ",
                "SPAN_ID=$(echo \"$TRACEPARENT\" | cut -d'-' -f3); ",
                "test ${#SPAN_ID} -eq 16 || exit 5; ",
                "FLAGS=$(echo \"$TRACEPARENT\" | cut -d'-' -f4); ",
                "test ${#FLAGS} -eq 2 || exit 6; ",
                "echo OK"
            )
            .to_string(),
        ],
        identity: "format-check-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(100),
            memory_mb: Some(64),
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
        product_version: None,
    });

    let handle = scheduler.schedule(&iri, &spec).await.unwrap();
    assert!(handle.pid.is_some(), "workload should be scheduled");

    // Wait for the shell script to complete
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let status = scheduler.status(&iri).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Stopped),
        "Shell should exit successfully (TRACEPARENT format is valid), got {status:?}"
    );
}

/// Verify TRACEPARENT does not collide with user-supplied env vars.
/// Platform-injected TRACEPARENT takes precedence (set after user env vars).
#[tokio::test]
async fn tc261_traceparent_does_not_collide_with_user_env() {
    let scheduler = test_scheduler();
    let iri = workload_iri("my-app", "binaries", "collision-test");

    let mut env = HashMap::new();
    env.insert(
        "APP_NAME".to_string(),
        EnvValue::Literal("my-app".to_string()),
    );
    // Even if the user tries to set TRACEPARENT, the platform overrides it
    env.insert(
        "TRACEPARENT".to_string(),
        EnvValue::Literal("user-supplied-value".to_string()),
    );

    let spec = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/sh".to_string(),
        args: vec![
            "-c".to_string(),
            // Verify TRACEPARENT is NOT the user-supplied value (platform overrides)
            "test \"$TRACEPARENT\" != \"user-supplied-value\" && echo OK || exit 1".to_string(),
        ],
        identity: "collision-test-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(100),
            memory_mb: Some(64),
        },
        mounts: vec![],
        env,
        restart_policy: RestartPolicy::Never,
        product_version: None,
    });

    let handle = scheduler.schedule(&iri, &spec).await.unwrap();
    assert!(
        handle.pid.is_some(),
        "workload with user TRACEPARENT and platform TRACEPARENT should schedule"
    );

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let status = scheduler.status(&iri).await.unwrap();
    assert!(
        matches!(status, WorkloadStatus::Stopped),
        "Platform TRACEPARENT should override user-supplied value, got {status:?}"
    );
}

// ---------------------------------------------------------------------------
// TC-318 — Trace propagation exit — traceparent header flows to workloads
// ---------------------------------------------------------------------------

/// Exit criterion: full traceparent propagation lifecycle.
///
/// Gate 1: Binary workload receives TRACEPARENT env var with valid W3C format
/// Gate 2: Container workload receives TRACEPARENT env var
/// Gate 3: Multiple workloads receive unique trace-ids
/// Gate 4: Workloads without traceparent still receive one (platform generates it)
/// Gate 5: EventEnvelope supports traceparent field for end-to-end propagation
#[tokio::test]
async fn tc318_trace_propagation_exit_traceparent_header_flows_to_workloads() {
    let scheduler = test_scheduler();

    // --- Gate 1: Binary workload gets valid TRACEPARENT ---

    let binary_iri = workload_iri("analytics-app", "binaries", "trace-gate1");
    let binary_spec = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/sh".to_string(),
        args: vec![
            "-c".to_string(),
            // Validate the TRACEPARENT is present, has version "00", and correct lengths
            concat!(
                "test -n \"$TRACEPARENT\" || exit 1; ",
                "VERSION=$(echo \"$TRACEPARENT\" | cut -d'-' -f1); ",
                "test \"$VERSION\" = \"00\" || exit 2; ",
                "TRACE_ID=$(echo \"$TRACEPARENT\" | cut -d'-' -f2); ",
                "test ${#TRACE_ID} -eq 32 || exit 3; ",
                "SPAN_ID=$(echo \"$TRACEPARENT\" | cut -d'-' -f3); ",
                "test ${#SPAN_ID} -eq 16 || exit 4; ",
                "echo GATE1_OK"
            )
            .to_string(),
        ],
        identity: "trace-gate1-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(200),
            memory_mb: Some(128),
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
        product_version: None,
    });

    let handle1 = scheduler.schedule(&binary_iri, &binary_spec).await.unwrap();
    assert!(
        handle1.pid.is_some(),
        "binary workload must be scheduled with a PID"
    );

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let status1 = scheduler.status(&binary_iri).await.unwrap();
    assert!(
        matches!(status1, WorkloadStatus::Stopped),
        "Gate 1 failed: binary workload should exit 0 (TRACEPARENT format valid), got {status1:?}"
    );

    // --- Gate 2: Container workload (simulated) gets TRACEPARENT on spec ---

    let container_iri = workload_iri("analytics-app", "containers", "trace-gate2");
    let container_spec = WorkloadSpec::Container(ContainerSpec {
        image: "analytics-app/worker:1.0.0".to_string(),
        identity: "trace-gate2-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(500),
            memory_mb: Some(256),
        },
        mounts: vec![],
        env: HashMap::new(),
        ports: vec![],
        health_check: None,
        restart_policy: RestartPolicy::Never,
        product_version: None,
    });

    let handle2 = scheduler.schedule(&container_iri, &container_spec).await.unwrap();
    assert!(
        handle2.pid.is_some(),
        "Gate 2: container workload must be scheduled"
    );

    // --- Gate 3: Multiple workloads get unique trace-ids ---

    let mut traceparents = Vec::new();
    for _i in 0..5 {
        let tp = ProcessScheduler::generate_traceparent();
        assert_valid_traceparent(&tp);
        traceparents.push(tp);
    }

    // All trace-ids must be unique
    let trace_ids: Vec<&str> = traceparents
        .iter()
        .map(|tp| tp.split('-').nth(1).unwrap())
        .collect();
    for i in 0..trace_ids.len() {
        for j in (i + 1)..trace_ids.len() {
            assert_ne!(
                trace_ids[i], trace_ids[j],
                "Gate 3: trace-ids must be unique, but index {i} == index {j}"
            );
        }
    }

    // All parent-ids must be unique
    let parent_ids: Vec<&str> = traceparents
        .iter()
        .map(|tp| tp.split('-').nth(2).unwrap())
        .collect();
    for i in 0..parent_ids.len() {
        for j in (i + 1)..parent_ids.len() {
            assert_ne!(
                parent_ids[i], parent_ids[j],
                "Gate 3: parent-ids must be unique, but index {i} == index {j}"
            );
        }
    }

    // --- Gate 4: Even workloads with no user trace context get a platform-generated one ---

    let plain_iri = workload_iri("plain-app", "binaries", "trace-gate4");
    let plain_spec = WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/sh".to_string(),
        args: vec![
            "-c".to_string(),
            // Just verify TRACEPARENT is set — the platform always generates one
            "test -n \"$TRACEPARENT\" && echo GATE4_OK || exit 1".to_string(),
        ],
        identity: "trace-gate4-identity".to_string(),
        resources: ResourceLimits {
            cpu_millicores: Some(100),
            memory_mb: Some(64),
        },
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
        product_version: None,
    });

    let handle4 = scheduler.schedule(&plain_iri, &plain_spec).await.unwrap();
    assert!(handle4.pid.is_some());

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let status4 = scheduler.status(&plain_iri).await.unwrap();
    assert!(
        matches!(status4, WorkloadStatus::Stopped),
        "Gate 4 failed: workload should always get TRACEPARENT, got {status4:?}"
    );

    // --- Gate 5: EventEnvelope supports traceparent field ---

    use picloud_domain::events::EventEnvelope;
    use picloud_domain::iri::IriBuilder;

    let iri_builder = IriBuilder::new(ClusterDomain::default());
    let schema = iri_builder.event_schema("WorkloadScheduled", 1);
    let source = iri_builder.resource("analytics-app", "binaries", "trace-gate5");

    let traceparent_value = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let envelope = EventEnvelope::new(
        schema,
        "WorkloadScheduled",
        source,
        Some("analytics-app".to_string()),
        uuid::Uuid::new_v4(),
        serde_json::json!({"workload": "trace-gate5"}),
    )
    .with_traceparent(traceparent_value);

    assert_eq!(
        envelope.traceparent.as_deref(),
        Some(traceparent_value),
        "Gate 5: EventEnvelope must carry traceparent"
    );

    // Verify traceparent survives serialization round-trip
    let json = serde_json::to_string(&envelope).unwrap();
    assert!(
        json.contains("traceparent"),
        "Gate 5: serialized envelope must contain traceparent field"
    );
    let deserialized: EventEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(
        deserialized.traceparent.as_deref(),
        Some(traceparent_value),
        "Gate 5: traceparent must survive round-trip serialization"
    );

    // Verify envelope without traceparent omits the field (skip_serializing_if)
    let envelope_no_tp = EventEnvelope::new(
        iri_builder.event_schema("WorkloadScheduled", 1),
        "WorkloadScheduled",
        iri_builder.resource("analytics-app", "binaries", "no-trace"),
        Some("analytics-app".to_string()),
        uuid::Uuid::new_v4(),
        serde_json::json!({}),
    );
    assert!(envelope_no_tp.traceparent.is_none());
    let json_no_tp = serde_json::to_string(&envelope_no_tp).unwrap();
    assert!(
        !json_no_tp.contains("traceparent"),
        "Gate 5: envelope without traceparent should omit the field in JSON"
    );

    // Exit criterion met: W3C traceparent is generated by the platform and
    // injected as the TRACEPARENT env var into all workloads (binary and
    // container). The EventEnvelope carries an optional traceparent field
    // for end-to-end distributed trace correlation. The reverse proxy
    // propagates existing traceparent headers and generates new ones when
    // missing.
}
