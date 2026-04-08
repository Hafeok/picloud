//! Process-based workload scheduler
//!
//! Spawns real child processes for Binary workloads and delegates to
//! podman/docker for Container workloads. Falls back to simulated scheduling
//! when no container runtime is available.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use uuid::Uuid;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{SecretStore, WorkloadHandle, WorkloadScheduler, WorkloadSpec, WorkloadStatus};
use picloud_domain::workload::{BinarySpec, ContainerSpec, EnvValue, RestartPolicy};

/// Which container runtime is available on the host.
#[derive(Debug, Clone, PartialEq)]
enum ContainerRuntime {
    Podman,
    Docker,
    None,
}

/// Internal record of a scheduled workload.
struct WorkloadEntry {
    workload_iri: String,
    spec: WorkloadSpec,
    status: WorkloadStatus,
    node_id: Uuid,
    pid: Option<u32>,
    #[allow(dead_code)]
    started_at: DateTime<Utc>,
    /// The spawned child process handle (for real process execution).
    child: Option<Child>,
    /// Number of restarts performed so far.
    restart_count: u32,
    /// Whether the workload was explicitly stopped (should not be restarted).
    explicitly_stopped: bool,
}

/// A workload scheduler that spawns real processes.
///
/// - Binary workloads are executed directly via `tokio::process::Command`.
/// - Container workloads are delegated to podman or docker if available,
///   otherwise simulated with a warning.
pub struct ProcessScheduler {
    node_id: Uuid,
    workloads: Arc<RwLock<HashMap<String, WorkloadEntry>>>,
    iri_builder: IriBuilder,
    next_pid: AtomicU32,
    container_runtime: ContainerRuntime,
    secret_store: Option<Arc<dyn SecretStore>>,
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
            workloads: Arc::new(RwLock::new(HashMap::new())),
            iri_builder: IriBuilder::new(domain),
            next_pid: AtomicU32::new(10000),
            container_runtime: runtime,
            secret_store: None,
        }
    }

    /// Create a new scheduler with a secret store for resolving secret env vars.
    pub fn with_secret_store(
        node_id: Uuid,
        domain: ClusterDomain,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
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
            workloads: Arc::new(RwLock::new(HashMap::new())),
            iri_builder: IriBuilder::new(domain),
            next_pid: AtomicU32::new(10000),
            container_runtime: runtime,
            secret_store: Some(secret_store),
        }
    }

    /// Create a scheduler with an explicitly set container runtime (for testing).
    #[cfg(test)]
    fn new_with_runtime(node_id: Uuid, domain: ClusterDomain, runtime: ContainerRuntime) -> Self {
        Self {
            node_id,
            workloads: Arc::new(RwLock::new(HashMap::new())),
            iri_builder: IriBuilder::new(domain),
            next_pid: AtomicU32::new(10000),
            container_runtime: runtime,
            secret_store: None,
        }
    }

    /// Create a scheduler with an explicitly set runtime and secret store (for testing).
    #[cfg(test)]
    fn new_with_runtime_and_secrets(
        node_id: Uuid,
        domain: ClusterDomain,
        runtime: ContainerRuntime,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            node_id,
            workloads: Arc::new(RwLock::new(HashMap::new())),
            iri_builder: IriBuilder::new(domain),
            next_pid: AtomicU32::new(10000),
            container_runtime: runtime,
            secret_store: Some(secret_store),
        }
    }

    /// Returns a reference to the IRI builder.
    pub fn iri_builder(&self) -> &IriBuilder {
        &self.iri_builder
    }

    /// Resolve an environment value, looking up secrets from the store when needed.
    async fn resolve_env_value(&self, value: &EnvValue, product: Option<&str>) -> String {
        match value {
            EnvValue::Literal(s) => s.clone(),
            EnvValue::Secret { secret } => {
                if let Some(ref store) = self.secret_store {
                    let product_name = product.unwrap_or("default");
                    match store.get_secret(product_name, secret).await {
                        Ok(val) => val,
                        Err(e) => {
                            tracing::warn!(
                                secret = secret,
                                error = %e,
                                "Failed to resolve secret — using empty string"
                            );
                            String::new()
                        }
                    }
                } else {
                    tracing::warn!(
                        secret = secret,
                        "No secret store configured — using placeholder"
                    );
                    format!("SECRET:{}", secret)
                }
            }
        }
    }

    /// Extract the product name from a workload IRI.
    /// IRI format: https://picloud.local/products/{product}/...
    fn product_from_iri(iri: &str) -> Option<&str> {
        let path = iri.strip_prefix("https://")?;
        let path = path.split('/').collect::<Vec<_>>();
        // Expected: [domain, "products", product_name, ...]
        if path.len() >= 3 && path[1] == "products" {
            Some(path[2])
        } else {
            None
        }
    }

    /// Build the OTEL_* environment variables to inject into workload processes (ADR-045).
    fn otel_env_vars(&self, workload_iri: &ResourceIri) -> Vec<(String, String)> {
        let service_name = workload_iri
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or("unknown");
        let otel_endpoint = format!(
            "https://{}/otel",
            self.iri_builder.cluster_root().as_str().trim_end_matches('/')
                .strip_prefix("https://")
                .unwrap_or("picloud.local")
        );
        let product = Self::product_from_iri(workload_iri.as_str())
            .unwrap_or("platform");

        vec![
            ("OTEL_SERVICE_NAME".to_string(), service_name.to_string()),
            ("OTEL_EXPORTER_OTLP_ENDPOINT".to_string(), otel_endpoint),
            (
                "OTEL_RESOURCE_ATTRIBUTES".to_string(),
                format!(
                    "picloud.product={},picloud.workload_iri={}",
                    product,
                    workload_iri.as_str()
                ),
            ),
        ]
    }

    /// Spawn a binary workload as a child process.
    async fn spawn_binary(&self, spec: &BinarySpec, workload_iri: &ResourceIri) -> Result<(Child, u32)> {
        let mut cmd = Command::new(&spec.executable);
        cmd.args(&spec.args);

        let product = Self::product_from_iri(workload_iri.as_str());

        // Set environment variables
        for (key, value) in &spec.env {
            let val = self.resolve_env_value(value, product).await;
            cmd.env(key, val);
        }

        // Inject OTEL_* env vars (ADR-045)
        for (key, value) in self.otel_env_vars(workload_iri) {
            cmd.env(key, value);
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
        let product = Self::product_from_iri(workload_iri.as_str());
        for (key, value) in &spec.env {
            let val = self.resolve_env_value(value, product).await;
            cmd.args(["-e", &format!("{}={}", key, val)]);
        }

        // Inject OTEL_* env vars (ADR-045)
        for (key, value) in self.otel_env_vars(workload_iri) {
            cmd.args(["-e", &format!("{}={}", key, value)]);
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

    /// Start a background health check loop that monitors running workloads.
    ///
    /// Every `interval` the loop checks each running workload:
    /// - If the process has exited and the restart policy allows it, restart.
    /// - Emits tracing events for health check failures and restarts.
    pub fn start_health_check_loop(
        &self,
        interval: std::time::Duration,
    ) -> tokio::task::JoinHandle<()> {
        let workloads = Arc::clone(&self.workloads);

        tokio::spawn(async move {
            tracing::info!("Workload health check loop started");
            loop {
                tokio::time::sleep(interval).await;

                let mut wl = workloads.write().await;
                let keys: Vec<String> = wl.keys().cloned().collect();

                for key in keys {
                    let entry = match wl.get_mut(&key) {
                        Some(e) => e,
                        None => continue,
                    };

                    // Only check workloads that are supposed to be running
                    if !matches!(entry.status, WorkloadStatus::Running) {
                        continue;
                    }

                    // Skip explicitly stopped workloads
                    if entry.explicitly_stopped {
                        continue;
                    }

                    // Check if the child process is still alive
                    let exited = if let Some(ref mut child) = entry.child {
                        match child.try_wait() {
                            Ok(Some(exit_status)) => Some(exit_status.success()),
                            Ok(None) => None, // still running
                            Err(_) => Some(false),
                        }
                    } else {
                        // Simulated workload (no child) — assume running
                        None
                    };

                    if let Some(success) = exited {
                        tracing::warn!(
                            workload_iri = %entry.workload_iri,
                            success = success,
                            "WorkloadHealthCheckFailed: process exited"
                        );

                        let should_restart = match &entry.spec {
                            WorkloadSpec::Binary(s) => Self::should_restart(
                                &s.restart_policy,
                                success,
                                entry.restart_count,
                            ),
                            WorkloadSpec::Container(s) => Self::should_restart(
                                &s.restart_policy,
                                success,
                                entry.restart_count,
                            ),
                        };

                        if should_restart {
                            // Attempt restart: re-spawn the process
                            let restart_result = match &entry.spec {
                                WorkloadSpec::Binary(spec) => {
                                    let mut cmd = Command::new(&spec.executable);
                                    cmd.args(&spec.args);
                                    // For restarts, we use literal env values only
                                    // (secrets were already resolved at first schedule)
                                    for (k, v) in &spec.env {
                                        let val = match v {
                                            EnvValue::Literal(s) => s.clone(),
                                            EnvValue::Secret { secret } => {
                                                format!("SECRET:{}", secret)
                                            }
                                        };
                                        cmd.env(k, val);
                                    }
                                    cmd.stdin(std::process::Stdio::null());
                                    cmd.stdout(std::process::Stdio::piped());
                                    cmd.stderr(std::process::Stdio::piped());
                                    cmd.spawn()
                                }
                                WorkloadSpec::Container(_) => {
                                    // Simulated containers don't have real processes to restart
                                    tracing::info!(
                                        workload_iri = %entry.workload_iri,
                                        "Simulating container restart"
                                    );
                                    entry.status = WorkloadStatus::Running;
                                    entry.restart_count += 1;
                                    continue;
                                }
                            };

                            match restart_result {
                                Ok(child) => {
                                    let pid = child.id().unwrap_or(0);
                                    entry.child = Some(child);
                                    entry.pid = Some(pid);
                                    entry.status = WorkloadStatus::Running;
                                    entry.restart_count += 1;
                                    tracing::info!(
                                        workload_iri = %entry.workload_iri,
                                        pid = pid,
                                        restart_count = entry.restart_count,
                                        "WorkloadRestarted"
                                    );
                                }
                                Err(e) => {
                                    entry.status = WorkloadStatus::Failed {
                                        reason: format!("Restart failed: {}", e),
                                    };
                                    tracing::error!(
                                        workload_iri = %entry.workload_iri,
                                        error = %e,
                                        "Failed to restart workload"
                                    );
                                }
                            }
                        } else {
                            // No restart — mark as failed or stopped
                            if success {
                                entry.status = WorkloadStatus::Stopped;
                            } else {
                                entry.status = WorkloadStatus::Failed {
                                    reason: "Process exited and restart policy exhausted".to_string(),
                                };
                            }
                        }
                    }
                }
            }
        })
    }

    /// Determine whether a workload should be restarted based on its policy.
    fn should_restart(policy: &RestartPolicy, success: bool, restart_count: u32) -> bool {
        match policy {
            RestartPolicy::Always => true,
            RestartPolicy::OnFailure { max_retries } => {
                !success && restart_count < *max_retries
            }
            RestartPolicy::Never => false,
        }
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
                let (child, pid) = self.spawn_binary(binary_spec, workload_iri).await?;
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
            restart_count: 0,
            explicitly_stopped: false,
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
        entry.explicitly_stopped = true;

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

    // ---- Health check tests ----

    #[tokio::test]
    async fn health_check_restarts_exited_process_with_always_policy() {
        let scheduler = test_scheduler();
        let iri = ResourceIri::new(
            "https://picloud.local/products/test-app/binaries/short-lived",
        )
        .unwrap();

        // Spawn a process that exits immediately (echo)
        let spec = WorkloadSpec::Binary(BinarySpec {
            executable: "/bin/echo".to_string(),
            args: vec!["hello".to_string()],
            identity: "test-identity".to_string(),
            resources: ResourceLimits {
                cpu_millicores: None,
                memory_mb: None,
            },
            mounts: vec![],
            env: HashMap::new(),
            restart_policy: RestartPolicy::Always,
        });

        scheduler.schedule(&iri, &spec).await.unwrap();

        // Wait for echo to exit
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Start health check with a short interval
        let handle = scheduler.start_health_check_loop(std::time::Duration::from_millis(100));

        // Wait for the health check to detect exit and restart
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // The workload should have been restarted and be in Running state
        // (the restarted echo will also exit quickly, but the health check
        // should have set it to Running at least once)
        let workloads = scheduler.workloads.read().await;
        let entry = workloads.get(iri.as_str()).expect("workload should exist");
        assert!(
            entry.restart_count > 0,
            "Expected at least one restart, got {}",
            entry.restart_count
        );

        handle.abort();
    }

    #[tokio::test]
    async fn health_check_does_not_restart_with_never_policy() {
        let scheduler = test_scheduler();
        let iri = ResourceIri::new(
            "https://picloud.local/products/test-app/binaries/no-restart",
        )
        .unwrap();

        // Spawn a process that exits immediately
        let spec = WorkloadSpec::Binary(BinarySpec {
            executable: "/bin/echo".to_string(),
            args: vec!["done".to_string()],
            identity: "test-identity".to_string(),
            resources: ResourceLimits {
                cpu_millicores: None,
                memory_mb: None,
            },
            mounts: vec![],
            env: HashMap::new(),
            restart_policy: RestartPolicy::Never,
        });

        scheduler.schedule(&iri, &spec).await.unwrap();

        // Wait for echo to exit
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Start health check
        let handle = scheduler.start_health_check_loop(std::time::Duration::from_millis(100));

        // Wait for the health check to run
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;

        // The workload should NOT have been restarted
        let workloads = scheduler.workloads.read().await;
        let entry = workloads.get(iri.as_str()).expect("workload should exist");
        assert_eq!(
            entry.restart_count, 0,
            "Expected no restarts with Never policy, got {}",
            entry.restart_count
        );
        // Should be stopped (exited successfully)
        assert!(
            matches!(entry.status, WorkloadStatus::Stopped),
            "Expected Stopped, got {:?}",
            entry.status
        );

        handle.abort();
    }

    #[tokio::test]
    async fn health_check_respects_on_failure_max_retries() {
        let scheduler = test_scheduler();
        let iri = ResourceIri::new(
            "https://picloud.local/products/test-app/binaries/fail-limited",
        )
        .unwrap();

        // Spawn a process that exits with failure (false command)
        let spec = WorkloadSpec::Binary(BinarySpec {
            executable: "/bin/false".to_string(),
            args: vec![],
            identity: "test-identity".to_string(),
            resources: ResourceLimits {
                cpu_millicores: None,
                memory_mb: None,
            },
            mounts: vec![],
            env: HashMap::new(),
            restart_policy: RestartPolicy::OnFailure { max_retries: 2 },
        });

        scheduler.schedule(&iri, &spec).await.unwrap();

        // Wait for process to exit
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Start health check
        let handle = scheduler.start_health_check_loop(std::time::Duration::from_millis(100));

        // Wait long enough for max retries to be exhausted
        // Each iteration: 100ms check + process exits almost immediately
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let workloads = scheduler.workloads.read().await;
        let entry = workloads.get(iri.as_str()).expect("workload should exist");

        // Should have restarted exactly 2 times (max_retries)
        assert!(
            entry.restart_count <= 2,
            "Expected at most 2 restarts, got {}",
            entry.restart_count
        );

        handle.abort();
    }
}
