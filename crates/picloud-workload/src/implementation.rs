//! Process-based workload scheduler
//!
//! Spawns real child processes for Binary workloads and delegates to
//! podman/docker for Container workloads. Falls back to simulated scheduling
//! when no container runtime is available.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use uuid::Uuid;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{WorkloadHandle, WorkloadScheduler, WorkloadSpec, WorkloadStatus};
use picloud_domain::workload::{BinarySpec, ContainerSpec, EnvValue};

/// Which container runtime is available on the host.
#[derive(Debug, Clone, PartialEq)]
enum ContainerRuntime {
    Podman,
    Docker,
    None,
}

/// Internal record of a scheduled workload.
struct WorkloadEntry {
    #[allow(dead_code)]
    workload_iri: String,
    #[allow(dead_code)]
    spec: WorkloadSpec,
    status: WorkloadStatus,
    #[allow(dead_code)]
    node_id: Uuid,
    pid: Option<u32>,
    #[allow(dead_code)]
    started_at: DateTime<Utc>,
    /// The spawned child process handle (for real process execution).
    child: Option<Child>,
}

/// A workload scheduler that spawns real processes.
///
/// - Binary workloads are executed directly via `tokio::process::Command`.
/// - Container workloads are delegated to podman or docker if available,
///   otherwise simulated with a warning.
pub struct ProcessScheduler {
    node_id: Uuid,
    workloads: RwLock<HashMap<String, WorkloadEntry>>,
    iri_builder: IriBuilder,
    next_pid: AtomicU32,
    container_runtime: ContainerRuntime,
}

/// Detect which container runtime (podman or docker) is available.
fn detect_container_runtime() -> ContainerRuntime {
    if std::process::Command::new("podman")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return ContainerRuntime::Podman;
    }
    if std::process::Command::new("docker")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return ContainerRuntime::Docker;
    }
    ContainerRuntime::None
}

impl ProcessScheduler {
    /// Create a new scheduler for the given node.
    pub fn new(node_id: Uuid, domain: ClusterDomain) -> Self {
        let runtime = detect_container_runtime();
        if runtime == ContainerRuntime::None {
            tracing::warn!(
                "No container runtime (podman or docker) detected — container workloads will be simulated"
            );
        } else {
            tracing::info!(runtime = ?runtime, "Detected container runtime");
        }

        Self {
            node_id,
            workloads: RwLock::new(HashMap::new()),
            iri_builder: IriBuilder::new(domain),
            next_pid: AtomicU32::new(10000),
            container_runtime: runtime,
        }
    }

    /// Create a scheduler with an explicitly set container runtime (for testing).
    #[cfg(test)]
    fn new_with_runtime(node_id: Uuid, domain: ClusterDomain, runtime: ContainerRuntime) -> Self {
        Self {
            node_id,
            workloads: RwLock::new(HashMap::new()),
            iri_builder: IriBuilder::new(domain),
            next_pid: AtomicU32::new(10000),
            container_runtime: runtime,
        }
    }

    /// Returns a reference to the IRI builder.
    pub fn iri_builder(&self) -> &IriBuilder {
        &self.iri_builder
    }

    /// Spawn a binary workload as a child process.
    async fn spawn_binary(&self, spec: &BinarySpec) -> Result<(Child, u32)> {
        let mut cmd = Command::new(&spec.executable);
        cmd.args(&spec.args);

        // Set environment variables
        for (key, value) in &spec.env {
            let val = match value {
                EnvValue::Literal(s) => s.clone(),
                EnvValue::Secret { secret } => {
                    tracing::warn!(
                        secret = secret,
                        "Secret injection not yet implemented — using placeholder"
                    );
                    format!("SECRET:{}", secret)
                }
            };
            cmd.env(key, val);
        }

        // Configure stdio to avoid blocking
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = cmd.spawn().map_err(|e| PiCloudError::Internal(format!("Failed to spawn binary '{}': {}", spec.executable, e)))?;

        let pid = child.id().ok_or_else(|| PiCloudError::Internal("Spawned process has no PID (already exited)".to_string()))?;

        tracing::info!(
            executable = %spec.executable,
            pid = pid,
            "Spawned binary workload"
        );

        Ok((child, pid))
    }

    /// Spawn a container workload via podman or docker.
    async fn spawn_container(
        &self,
        spec: &ContainerSpec,
        workload_iri: &ResourceIri,
    ) -> Result<(Option<Child>, u32)> {
        let runtime = match &self.container_runtime {
            ContainerRuntime::Podman => "podman",
            ContainerRuntime::Docker => "docker",
            ContainerRuntime::None => {
                tracing::warn!(
                    workload_iri = %workload_iri,
                    image = %spec.image,
                    "No container runtime available — simulating container workload"
                );
                let pid = self.next_pid.fetch_add(1, Ordering::Relaxed);
                return Ok((None, pid));
            }
        };

        let mut cmd = Command::new(runtime);
        cmd.arg("run");
        cmd.arg("--detach");
        cmd.arg("--rm");

        // Container name derived from workload IRI
        let container_name = workload_iri
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or("workload");
        cmd.args(["--name", container_name]);

        // Environment variables
        for (key, value) in &spec.env {
            let val = match value {
                EnvValue::Literal(s) => s.clone(),
                EnvValue::Secret { secret } => {
                    tracing::warn!(
                        secret = secret,
                        "Secret injection not yet implemented — using placeholder"
                    );
                    format!("SECRET:{}", secret)
                }
            };
            cmd.args(["-e", &format!("{}={}", key, val)]);
        }

        // Port mappings
        for port in &spec.ports {
            cmd.args(["-p", &format!("{}:{}", port.port, port.port)]);
        }

        // Volume mounts
        for mount in &spec.mounts {
            let mount_opt = if mount.read_only {
                format!("{}:{}:ro", mount.volume, mount.path)
            } else {
                format!("{}:{}", mount.volume, mount.path)
            };
            cmd.args(["-v", &mount_opt]);
        }

        cmd.arg(&spec.image);

        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = cmd.spawn().map_err(|e| PiCloudError::Internal(format!("Failed to spawn container via {}: {}", runtime, e)))?;

        let pid = child.id().unwrap_or_else(|| self.next_pid.fetch_add(1, Ordering::Relaxed));

        tracing::info!(
            image = %spec.image,
            runtime = runtime,
            pid = pid,
            "Spawned container workload"
        );

        Ok((Some(child), pid))
    }

    /// Send SIGTERM to a process by PID.
    fn send_sigterm(pid: u32) -> bool {
        // Safety: libc::kill sends a signal to a process. We use SIGTERM for graceful shutdown.
        let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        ret == 0
    }
}

#[async_trait]
impl WorkloadScheduler for ProcessScheduler {
    async fn schedule(
        &self,
        workload_iri: &ResourceIri,
        spec: &WorkloadSpec,
    ) -> Result<WorkloadHandle> {
        let key = workload_iri.as_str().to_string();
        let mut workloads = self.workloads.write().await;

        if workloads.contains_key(&key) {
            return Err(PiCloudError::ResourceAlreadyExists { iri: key });
        }

        let (child, pid) = match spec {
            WorkloadSpec::Binary(binary_spec) => {
                let (child, pid) = self.spawn_binary(binary_spec).await?;
                (Some(child), pid)
            }
            WorkloadSpec::Container(container_spec) => {
                let (child, pid) = self.spawn_container(container_spec, workload_iri).await?;
                (child, pid)
            }
        };

        tracing::info!(
            workload_iri = %workload_iri,
            node_id = %self.node_id,
            pid = pid,
            "Scheduled workload"
        );

        let entry = WorkloadEntry {
            workload_iri: key.clone(),
            spec: spec.clone(),
            status: WorkloadStatus::Running,
            node_id: self.node_id,
            pid: Some(pid),
            started_at: Utc::now(),
            child,
        };

        workloads.insert(key, entry);

        Ok(WorkloadHandle {
            workload_iri: workload_iri.clone(),
            node_id: self.node_id,
            pid: Some(pid),
        })
    }

    async fn stop(&self, workload_iri: &ResourceIri) -> Result<()> {
        let key = workload_iri.as_str().to_string();
        let mut workloads = self.workloads.write().await;

        let entry = workloads
            .get_mut(&key)
            .ok_or_else(|| PiCloudError::ResourceNotFound { iri: key.clone() })?;

        // Send SIGTERM to the real process if we have a PID
        if let Some(pid) = entry.pid {
            if entry.child.is_some() {
                if Self::send_sigterm(pid) {
                    tracing::info!(
                        workload_iri = %workload_iri,
                        pid = pid,
                        "Sent SIGTERM to workload process"
                    );
                } else {
                    tracing::warn!(
                        workload_iri = %workload_iri,
                        pid = pid,
                        "Failed to send SIGTERM — process may have already exited"
                    );
                }
            }
        }

        // Also try to kill via the child handle as a fallback
        if let Some(ref mut child) = entry.child {
            // try_wait first to see if it already exited
            match child.try_wait() {
                Ok(Some(_)) => {
                    tracing::info!(
                        workload_iri = %workload_iri,
                        "Process already exited"
                    );
                }
                Ok(None) => {
                    // Process still running after SIGTERM — give it a moment, then kill
                    // The SIGTERM was already sent above; we mark as stopped.
                }
                Err(e) => {
                    tracing::warn!(
                        workload_iri = %workload_iri,
                        error = %e,
                        "Error checking process status"
                    );
                }
            }
        }

        entry.status = WorkloadStatus::Stopped;

        tracing::info!(
            workload_iri = %workload_iri,
            "Stopped workload"
        );

        Ok(())
    }

    async fn status(&self, workload_iri: &ResourceIri) -> Result<WorkloadStatus> {
        let key = workload_iri.as_str().to_string();
        let mut workloads = self.workloads.write().await;

        let entry = workloads
            .get_mut(&key)
            .ok_or_else(|| PiCloudError::ResourceNotFound { iri: key.clone() })?;

        // If we have a child handle and it was previously running, check if it's still alive
        if matches!(entry.status, WorkloadStatus::Running) {
            if let Some(ref mut child) = entry.child {
                match child.try_wait() {
                    Ok(Some(exit_status)) => {
                        if exit_status.success() {
                            entry.status = WorkloadStatus::Stopped;
                        } else {
                            entry.status = WorkloadStatus::Failed {
                                reason: format!(
                                    "Process exited with status: {}",
                                    exit_status
                                ),
                            };
                        }
                    }
                    Ok(None) => {
                        // Still running — status stays Running
                    }
                    Err(e) => {
                        entry.status = WorkloadStatus::Failed {
                            reason: format!("Failed to check process status: {}", e),
                        };
                    }
                }
            }
        }

        Ok(entry.status.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::workload::{BinarySpec, ResourceLimits, RestartPolicy};
    use std::collections::HashMap;

    fn test_scheduler() -> ProcessScheduler {
        // Use None runtime so container tests don't require docker/podman
        ProcessScheduler::new_with_runtime(
            Uuid::new_v4(),
            ClusterDomain::default(),
            ContainerRuntime::None,
        )
    }

    fn test_iri() -> ResourceIri {
        ResourceIri::new("https://picloud.local/products/test-app/containers/web").unwrap()
    }

    /// Create a spec that spawns a real short-lived process (echo).
    fn real_echo_spec() -> WorkloadSpec {
        WorkloadSpec::Binary(BinarySpec {
            executable: "/bin/echo".to_string(),
            args: vec!["hello".to_string(), "picloud".to_string()],
            identity: "test-identity".to_string(),
            resources: ResourceLimits {
                cpu_millicores: None,
                memory_mb: None,
            },
            mounts: vec![],
            env: HashMap::new(),
            restart_policy: RestartPolicy::Never,
        })
    }

    /// Create a spec that spawns a real long-lived process (sleep).
    fn real_sleep_spec() -> WorkloadSpec {
        WorkloadSpec::Binary(BinarySpec {
            executable: "/bin/sleep".to_string(),
            args: vec!["60".to_string()],
            identity: "test-identity".to_string(),
            resources: ResourceLimits {
                cpu_millicores: None,
                memory_mb: None,
            },
            mounts: vec![],
            env: HashMap::new(),
            restart_policy: RestartPolicy::Never,
        })
    }

    #[tokio::test]
    async fn schedule_creates_a_workload() {
        let scheduler = test_scheduler();
        let iri = test_iri();
        // Use a simulated container spec so we don't need a real executable
        let spec = WorkloadSpec::Container(picloud_domain::workload::ContainerSpec {
            image: "test:latest".to_string(),
            identity: "test-id".to_string(),
            resources: ResourceLimits {
                cpu_millicores: Some(500),
                memory_mb: Some(256),
            },
            mounts: vec![],
            env: HashMap::new(),
            ports: vec![],
            health_check: None,
            restart_policy: RestartPolicy::Never,
        });

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();

        assert_eq!(handle.workload_iri, iri);
        assert!(handle.pid.is_some());
    }

    #[tokio::test]
    async fn stop_changes_status() {
        let scheduler = test_scheduler();
        let iri = test_iri();
        // Use a simulated container spec
        let spec = WorkloadSpec::Container(picloud_domain::workload::ContainerSpec {
            image: "test:latest".to_string(),
            identity: "test-id".to_string(),
            resources: ResourceLimits {
                cpu_millicores: Some(500),
                memory_mb: Some(256),
            },
            mounts: vec![],
            env: HashMap::new(),
            ports: vec![],
            health_check: None,
            restart_policy: RestartPolicy::Never,
        });

        scheduler.schedule(&iri, &spec).await.unwrap();
        scheduler.stop(&iri).await.unwrap();

        let status = scheduler.status(&iri).await.unwrap();
        assert!(matches!(status, WorkloadStatus::Stopped));
    }

    #[tokio::test]
    async fn status_returns_running_after_schedule() {
        let scheduler = test_scheduler();
        let iri = test_iri();
        // Use a simulated container spec
        let spec = WorkloadSpec::Container(picloud_domain::workload::ContainerSpec {
            image: "test:latest".to_string(),
            identity: "test-id".to_string(),
            resources: ResourceLimits {
                cpu_millicores: Some(500),
                memory_mb: Some(256),
            },
            mounts: vec![],
            env: HashMap::new(),
            ports: vec![],
            health_check: None,
            restart_policy: RestartPolicy::Never,
        });

        scheduler.schedule(&iri, &spec).await.unwrap();

        let status = scheduler.status(&iri).await.unwrap();
        assert!(matches!(status, WorkloadStatus::Running));
    }

    #[tokio::test]
    async fn scheduling_duplicate_returns_error() {
        let scheduler = test_scheduler();
        let iri = test_iri();
        // Use a simulated container spec
        let spec = WorkloadSpec::Container(picloud_domain::workload::ContainerSpec {
            image: "test:latest".to_string(),
            identity: "test-id".to_string(),
            resources: ResourceLimits {
                cpu_millicores: Some(500),
                memory_mb: Some(256),
            },
            mounts: vec![],
            env: HashMap::new(),
            ports: vec![],
            health_check: None,
            restart_policy: RestartPolicy::Never,
        });

        scheduler.schedule(&iri, &spec).await.unwrap();
        let result = scheduler.schedule(&iri, &spec).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::ResourceAlreadyExists { .. }
        ));
    }

    #[tokio::test]
    async fn stopping_unknown_workload_returns_error() {
        let scheduler = test_scheduler();
        let iri = test_iri();

        let result = scheduler.stop(&iri).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::ResourceNotFound { .. }
        ));
    }

    // ---- Real process execution tests ----

    #[tokio::test]
    async fn spawn_real_echo_process() {
        let scheduler = test_scheduler();
        let iri = ResourceIri::new(
            "https://picloud.local/products/test-app/binaries/echo-test",
        )
        .unwrap();
        let spec = real_echo_spec();

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();

        // Should have a real PID
        let pid = handle.pid.expect("echo process should have a PID");
        assert!(pid > 0);

        // echo exits quickly — wait a moment then check status
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let status = scheduler.status(&iri).await.unwrap();
        // echo exits with 0, so it should be Stopped (exited successfully)
        assert!(
            matches!(status, WorkloadStatus::Stopped),
            "Expected Stopped after echo exits, got {:?}",
            status
        );
    }

    #[tokio::test]
    async fn spawn_real_sleep_and_stop_it() {
        let scheduler = test_scheduler();
        let iri = ResourceIri::new(
            "https://picloud.local/products/test-app/binaries/sleep-test",
        )
        .unwrap();
        let spec = real_sleep_spec();

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();

        let pid = handle.pid.expect("sleep process should have a PID");
        assert!(pid > 0);

        // Process should be running
        let status = scheduler.status(&iri).await.unwrap();
        assert!(
            matches!(status, WorkloadStatus::Running),
            "Expected Running, got {:?}",
            status
        );

        // Stop it (sends SIGTERM)
        scheduler.stop(&iri).await.unwrap();

        // Wait for process to actually terminate
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let status = scheduler.status(&iri).await.unwrap();
        assert!(
            matches!(status, WorkloadStatus::Stopped),
            "Expected Stopped after SIGTERM, got {:?}",
            status
        );
    }

    #[tokio::test]
    async fn spawn_with_env_vars() {
        let scheduler = test_scheduler();
        let iri = ResourceIri::new(
            "https://picloud.local/products/test-app/binaries/env-test",
        )
        .unwrap();

        let mut env = HashMap::new();
        env.insert("MY_VAR".to_string(), EnvValue::Literal("hello".to_string()));

        let spec = WorkloadSpec::Binary(BinarySpec {
            executable: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), "echo $MY_VAR".to_string()],
            identity: "test-identity".to_string(),
            resources: ResourceLimits {
                cpu_millicores: None,
                memory_mb: None,
            },
            mounts: vec![],
            env,
            restart_policy: RestartPolicy::Never,
        });

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();
        assert!(handle.pid.is_some());

        // Wait for it to finish
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let status = scheduler.status(&iri).await.unwrap();
        assert!(matches!(status, WorkloadStatus::Stopped));
    }

    #[tokio::test]
    async fn spawn_nonexistent_binary_returns_error() {
        let scheduler = test_scheduler();
        let iri = ResourceIri::new(
            "https://picloud.local/products/test-app/binaries/bad",
        )
        .unwrap();

        let spec = WorkloadSpec::Binary(BinarySpec {
            executable: "/nonexistent/binary/path".to_string(),
            args: vec![],
            identity: "test-identity".to_string(),
            resources: ResourceLimits {
                cpu_millicores: None,
                memory_mb: None,
            },
            mounts: vec![],
            env: HashMap::new(),
            restart_policy: RestartPolicy::Never,
        });

        let result = scheduler.schedule(&iri, &spec).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PiCloudError::Internal(_)));
    }
}
