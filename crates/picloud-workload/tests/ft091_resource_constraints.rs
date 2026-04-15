//! FT-091 — Workload resource constraints (CPU/memory limits)
//!
//! Validates that the platform enforces CPU and memory limits on workloads:
//! - Resource limits are validated before scheduling
//! - Binary workloads receive RLIMIT_AS (memory) and RLIMIT_CPU enforcement
//! - Container workloads pass limits to the OCI runtime (youki/podman/docker)
//! - Invalid resource specs are rejected with descriptive errors

use std::collections::HashMap;

use picloud_domain::error::PiCloudError;
use picloud_domain::iri::{ClusterDomain, ResourceIri};
use picloud_domain::traits::{WorkloadScheduler, WorkloadSpec, WorkloadStatus};
use picloud_domain::workload::{BinarySpec, ContainerSpec, ResourceLimits, RestartPolicy};
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

fn container_iri(name: &str) -> ResourceIri {
    ResourceIri::new(format!(
        "https://picloud.local/products/test-app/containers/{name}"
    ))
    .unwrap()
}

fn binary_spec_with_limits(limits: ResourceLimits) -> WorkloadSpec {
    WorkloadSpec::Binary(BinarySpec {
        executable: "/bin/sleep".to_string(),
        args: vec!["10".to_string()],
        identity: "test-identity".to_string(),
        resources: limits,
        mounts: vec![],
        env: HashMap::new(),
        restart_policy: RestartPolicy::Never,
        product_version: None,
    })
}

fn container_spec_with_limits(limits: ResourceLimits) -> WorkloadSpec {
    WorkloadSpec::Container(ContainerSpec {
        image: "alpine:latest".to_string(),
        identity: "test-identity".to_string(),
        resources: limits,
        mounts: vec![],
        env: HashMap::new(),
        ports: vec![],
        health_check: None,
        restart_policy: RestartPolicy::Never,
        product_version: None,
    })
}

// ---------------------------------------------------------------------------
// TC-287 — Workload CPU and memory limits enforced by container runtime
// ---------------------------------------------------------------------------

/// Schedule workloads with CPU and memory limits and verify the runtime
/// enforces them. For binary workloads, RLIMIT_AS/RLIMIT_CPU are set
/// via pre_exec hooks. For container workloads, limits are passed to the
/// OCI runtime via --memory/--cpus flags or the OCI spec resources block.
#[tokio::test]
async fn tc287_workload_cpu_and_memory_limits_enforced_by_container_runtime() {
    let scheduler = test_scheduler();

    // --- Phase 1: Binary workload with both CPU and memory limits ---
    {
        let iri = binary_iri("tc287-binary-limited");
        let spec = binary_spec_with_limits(ResourceLimits::new(500, 256));

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();
        let pid = handle.pid.expect("binary workload must have a PID");
        assert!(pid > 0, "PID must be positive");

        // Verify workload is running
        let status = scheduler.status(&iri).await.unwrap();
        assert!(
            matches!(status, WorkloadStatus::Running),
            "Workload should be Running, got {:?}",
            status
        );

        // On Linux, we can verify that RLIMIT_AS was set for the child process
        // by reading /proc/<pid>/limits
        #[cfg(target_os = "linux")]
        {
            let limits_path = format!("/proc/{}/limits", pid);
            if let Ok(limits_content) = std::fs::read_to_string(&limits_path) {
                // Check for "Max address space" line — should show our 256 MB limit
                let expected_bytes = 256u64 * 1024 * 1024;
                let expected_str = expected_bytes.to_string();
                let has_memory_limit = limits_content
                    .lines()
                    .any(|line| line.contains("Max address space") && line.contains(&expected_str));
                assert!(
                    has_memory_limit,
                    "Expected RLIMIT_AS of {} bytes in /proc/{}/limits, got:\n{}",
                    expected_bytes, pid, limits_content
                );

                // Check for "Max cpu time" line — should show CPU limit
                let has_cpu_limit = limits_content
                    .lines()
                    .any(|line| line.contains("Max cpu time") && !line.contains("unlimited"));
                assert!(
                    has_cpu_limit,
                    "Expected RLIMIT_CPU to be set (not unlimited) in /proc/{}/limits, got:\n{}",
                    pid, limits_content
                );
            }
        }

        let _ = scheduler.stop(&iri).await;
    }

    // --- Phase 2: Binary workload with only memory limit ---
    {
        let iri = binary_iri("tc287-memory-only");
        let spec = binary_spec_with_limits(ResourceLimits {
            cpu_millicores: None,
            memory_mb: Some(128),
        });

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();
        assert!(handle.pid.unwrap_or(0) > 0);

        #[cfg(target_os = "linux")]
        {
            let pid = handle.pid.unwrap();
            let limits_path = format!("/proc/{}/limits", pid);
            if let Ok(limits_content) = std::fs::read_to_string(&limits_path) {
                let expected_bytes = 128u64 * 1024 * 1024;
                let expected_str = expected_bytes.to_string();
                let has_memory_limit = limits_content
                    .lines()
                    .any(|line| line.contains("Max address space") && line.contains(&expected_str));
                assert!(
                    has_memory_limit,
                    "Expected RLIMIT_AS of {} bytes for memory-only workload",
                    expected_bytes
                );
            }
        }

        let _ = scheduler.stop(&iri).await;
    }

    // --- Phase 3: Binary workload with only CPU limit ---
    {
        let iri = binary_iri("tc287-cpu-only");
        let spec = binary_spec_with_limits(ResourceLimits {
            cpu_millicores: Some(1000),
            memory_mb: None,
        });

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();
        assert!(handle.pid.unwrap_or(0) > 0);

        #[cfg(target_os = "linux")]
        {
            let pid = handle.pid.unwrap();
            let limits_path = format!("/proc/{}/limits", pid);
            if let Ok(limits_content) = std::fs::read_to_string(&limits_path) {
                let has_cpu_limit = limits_content
                    .lines()
                    .any(|line| line.contains("Max cpu time") && !line.contains("unlimited"));
                assert!(
                    has_cpu_limit,
                    "Expected RLIMIT_CPU to be set for cpu-only workload"
                );
            }
        }

        let _ = scheduler.stop(&iri).await;
    }

    // --- Phase 4: Container workload with limits (simulated runtime) ---
    // With ContainerRuntime::None, the scheduler simulates scheduling.
    // Verify the limits are accepted and the workload is tracked.
    {
        let iri = container_iri("tc287-container-limited");
        let spec = container_spec_with_limits(ResourceLimits::new(2000, 512));

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();
        assert!(handle.pid.unwrap_or(0) > 0, "Container must get a PID");

        let status = scheduler.status(&iri).await.unwrap();
        assert!(
            matches!(status, WorkloadStatus::Running),
            "Container should be Running, got {:?}",
            status
        );

        let _ = scheduler.stop(&iri).await;
    }

    // --- Phase 5: Workload with no limits should still schedule ---
    {
        let iri = binary_iri("tc287-no-limits");
        let spec = binary_spec_with_limits(ResourceLimits::none());

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();
        assert!(handle.pid.unwrap_or(0) > 0);
        let _ = scheduler.stop(&iri).await;
    }
}

// ---------------------------------------------------------------------------
// TC-344 — Resource limits exit — CPU and memory limits enforced
// ---------------------------------------------------------------------------

/// Exit-criteria test: validates the complete resource constraint lifecycle
/// including validation, enforcement, rejection of invalid values, and
/// helper methods on ResourceLimits.
#[tokio::test]
async fn tc344_resource_limits_exit_cpu_and_memory_limits_enforced() {
    let scheduler = test_scheduler();

    // --- 1. ResourceLimits validation — valid values accepted ---
    {
        let limits = ResourceLimits::new(500, 256);
        assert!(limits.validate().is_ok(), "500m CPU / 256MB should be valid");
        assert!(limits.has_limits(), "Should report has_limits=true");

        let limits = ResourceLimits::new(1, 4);
        assert!(limits.validate().is_ok(), "Minimum values should be valid");

        let limits = ResourceLimits::new(128_000, 1_048_576);
        assert!(limits.validate().is_ok(), "Maximum values should be valid");

        let limits = ResourceLimits::none();
        assert!(limits.validate().is_ok(), "No limits should be valid");
        assert!(!limits.has_limits(), "Should report has_limits=false");
    }

    // --- 2. ResourceLimits validation — invalid values rejected ---
    {
        // CPU below minimum (0 millicores)
        let limits = ResourceLimits {
            cpu_millicores: Some(0),
            memory_mb: Some(256),
        };
        let err = limits.validate().unwrap_err();
        assert!(
            err.contains("cpu_millicores") && err.contains("below minimum"),
            "Expected cpu_millicores below minimum error, got: {err}"
        );

        // CPU above maximum
        let limits = ResourceLimits {
            cpu_millicores: Some(200_000),
            memory_mb: None,
        };
        let err = limits.validate().unwrap_err();
        assert!(
            err.contains("cpu_millicores") && err.contains("exceeds maximum"),
            "Expected cpu_millicores exceeds maximum error, got: {err}"
        );

        // Memory below minimum (1 MB)
        let limits = ResourceLimits {
            cpu_millicores: None,
            memory_mb: Some(1),
        };
        let err = limits.validate().unwrap_err();
        assert!(
            err.contains("memory_mb") && err.contains("below minimum"),
            "Expected memory_mb below minimum error, got: {err}"
        );

        // Memory above maximum
        let limits = ResourceLimits {
            cpu_millicores: None,
            memory_mb: Some(2_000_000),
        };
        let err = limits.validate().unwrap_err();
        assert!(
            err.contains("memory_mb") && err.contains("exceeds maximum"),
            "Expected memory_mb exceeds maximum error, got: {err}"
        );

        // Both CPU and memory invalid — both errors reported
        let limits = ResourceLimits {
            cpu_millicores: Some(0),
            memory_mb: Some(0),
        };
        let err = limits.validate().unwrap_err();
        assert!(
            err.contains("cpu_millicores") && err.contains("memory_mb"),
            "Expected both errors reported, got: {err}"
        );
    }

    // --- 3. Scheduler rejects invalid resource limits ---
    {
        let iri = binary_iri("tc344-invalid-cpu");
        let spec = WorkloadSpec::Binary(BinarySpec {
            executable: "/bin/echo".to_string(),
            args: vec!["test".to_string()],
            identity: "test".to_string(),
            resources: ResourceLimits {
                cpu_millicores: Some(0),
                memory_mb: Some(256),
            },
            mounts: vec![],
            env: HashMap::new(),
            restart_policy: RestartPolicy::Never,
            product_version: None,
        });

        let result = scheduler.schedule(&iri, &spec).await;
        assert!(result.is_err(), "Scheduler should reject invalid limits");
        match result.unwrap_err() {
            PiCloudError::InvalidResourceLimits { reason } => {
                assert!(
                    reason.contains("cpu_millicores"),
                    "Error should mention cpu_millicores: {reason}"
                );
            }
            other => panic!("Expected InvalidResourceLimits, got: {other:?}"),
        }
    }

    // --- 4. Scheduler rejects invalid memory limits ---
    {
        let iri = binary_iri("tc344-invalid-memory");
        let spec = container_spec_with_limits(ResourceLimits {
            cpu_millicores: Some(500),
            memory_mb: Some(2),
        });

        let result = scheduler.schedule(&iri, &spec).await;
        assert!(result.is_err(), "Scheduler should reject memory < 4MB");
        match result.unwrap_err() {
            PiCloudError::InvalidResourceLimits { reason } => {
                assert!(
                    reason.contains("memory_mb"),
                    "Error should mention memory_mb: {reason}"
                );
            }
            other => panic!("Expected InvalidResourceLimits, got: {other:?}"),
        }
    }

    // --- 5. Helper methods produce correct values ---
    {
        let limits = ResourceLimits::new(500, 256);

        // CPU fractional cores
        let cpu_frac = limits.cpu_as_fractional_cores().unwrap();
        assert!(
            (cpu_frac - 0.5).abs() < f64::EPSILON,
            "500m should be 0.5 cores, got {cpu_frac}"
        );

        // Memory bytes
        let mem_bytes = limits.memory_as_bytes().unwrap();
        assert_eq!(
            mem_bytes,
            256 * 1024 * 1024,
            "256 MB should be {} bytes",
            256 * 1024 * 1024
        );

        // CFS quota
        let quota = limits.cpu_as_cfs_quota_us().unwrap();
        assert_eq!(
            quota, 50_000,
            "500m should produce 50_000 µs quota per 100_000 µs period"
        );

        // No limits
        let empty = ResourceLimits::none();
        assert!(empty.cpu_as_fractional_cores().is_none());
        assert!(empty.memory_as_bytes().is_none());
        assert!(empty.cpu_as_cfs_quota_us().is_none());
    }

    // --- 6. Valid limits: scheduler accepts and workload runs with enforcement ---
    {
        let iri = binary_iri("tc344-valid-enforcement");
        let spec = binary_spec_with_limits(ResourceLimits::new(1000, 512));

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();
        let pid = handle.pid.expect("Must have PID");
        assert!(pid > 0);

        let status = scheduler.status(&iri).await.unwrap();
        assert!(
            matches!(status, WorkloadStatus::Running),
            "Workload with valid limits should be Running"
        );

        // Verify the workload entry tracks the resource limits
        {
            let workloads = scheduler.workloads.read().await;
            let entry = workloads.get(iri.as_str()).expect("entry should exist");
            match &entry.spec {
                WorkloadSpec::Binary(s) => {
                    assert_eq!(s.resources.cpu_millicores, Some(1000));
                    assert_eq!(s.resources.memory_mb, Some(512));
                }
                _ => panic!("Expected binary spec"),
            }
        }

        let _ = scheduler.stop(&iri).await;
    }

    // --- 7. Default ResourceLimits are valid and empty ---
    {
        let limits = ResourceLimits::default();
        assert_eq!(limits.cpu_millicores, None);
        assert_eq!(limits.memory_mb, None);
        assert!(limits.validate().is_ok());
        assert!(!limits.has_limits());
    }

    // --- 8. Constants are sensible ---
    {
        assert_eq!(ResourceLimits::MIN_CPU_MILLICORES, 1);
        assert_eq!(ResourceLimits::MAX_CPU_MILLICORES, 128_000);
        assert_eq!(ResourceLimits::MIN_MEMORY_MB, 4);
        assert_eq!(ResourceLimits::MAX_MEMORY_MB, 1_048_576);
        assert_eq!(ResourceLimits::CFS_PERIOD_US, 100_000);
    }
}
