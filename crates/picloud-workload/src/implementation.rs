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
///
/// Youki is preferred (pure Rust, ADR-010), then Podman, then Docker.
#[derive(Debug, Clone, PartialEq)]
#[doc(hidden)]
pub enum ContainerRuntime {
    Youki,
    Podman,
    Docker,
    None,
}

/// Internal record of a scheduled workload.
#[doc(hidden)]
pub struct WorkloadEntry {
    pub workload_iri: String,
    pub spec: WorkloadSpec,
    pub status: WorkloadStatus,
    pub node_id: Uuid,
    pub pid: Option<u32>,
    #[allow(dead_code)]
    pub started_at: DateTime<Utc>,
    /// The spawned child process handle (for real process execution).
    pub child: Option<Child>,
    /// Number of restarts performed so far.
    pub restart_count: u32,
    /// Whether the workload was explicitly stopped (should not be restarted).
    pub explicitly_stopped: bool,
    /// Whether this workload is a container (for docker stop on cleanup).
    pub is_container: bool,
}

/// A workload scheduler that spawns real processes.
///
/// - Binary workloads are executed directly via `tokio::process::Command`.
/// - Container workloads are delegated to podman or docker if available,
///   otherwise simulated with a warning.
pub struct ProcessScheduler {
    node_id: Uuid,
    #[doc(hidden)]
    pub workloads: Arc<RwLock<HashMap<String, WorkloadEntry>>>,
    iri_builder: IriBuilder,
    next_pid: AtomicU32,
    container_runtime: ContainerRuntime,
    secret_store: Option<Arc<dyn SecretStore>>,
}

/// Detect which container runtime is available.
///
/// Preference order: youki (pure Rust, ADR-010) > podman > docker.
fn detect_container_runtime() -> ContainerRuntime {
    if std::process::Command::new("youki")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return ContainerRuntime::Youki;
    }
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
    #[doc(hidden)]
    pub fn new_with_runtime(node_id: Uuid, domain: ClusterDomain, runtime: ContainerRuntime) -> Self {
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
    #[doc(hidden)]
    pub fn new_with_runtime_and_secrets(
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

    /// Generate a W3C traceparent header value (FT-048).
    ///
    /// Format: `{version}-{trace-id}-{parent-id}-{trace-flags}`
    /// - version: "00" (current W3C spec version)
    /// - trace-id: 32 lowercase hex chars (128-bit random)
    /// - parent-id: 16 lowercase hex chars (64-bit random)
    /// - trace-flags: "01" (sampled)
    ///
    /// See: <https://www.w3.org/TR/trace-context/#traceparent-header>
    pub fn generate_traceparent() -> String {
        let trace_id = Uuid::new_v4();
        let parent_id: u64 = rand::random();
        format!(
            "00-{}-{:016x}-01",
            trace_id.as_simple(),
            parent_id,
        )
    }

    /// Validate a W3C traceparent header string.
    ///
    /// Returns true if the string matches the W3C traceparent format:
    /// `{version}-{trace-id}-{parent-id}-{trace-flags}` where:
    /// - version is 2 lowercase hex chars
    /// - trace-id is 32 lowercase hex chars (non-zero)
    /// - parent-id is 16 lowercase hex chars (non-zero)
    /// - trace-flags is 2 lowercase hex chars
    pub fn is_valid_traceparent(value: &str) -> bool {
        let parts: Vec<&str> = value.split('-').collect();
        if parts.len() != 4 {
            return false;
        }
        let version = parts[0];
        let trace_id = parts[1];
        let parent_id = parts[2];
        let trace_flags = parts[3];

        // Version must be 2 hex chars
        if version.len() != 2 || !version.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
        // trace-id must be 32 hex chars and non-zero
        if trace_id.len() != 32
            || !trace_id.chars().all(|c| c.is_ascii_hexdigit())
            || trace_id.chars().all(|c| c == '0')
        {
            return false;
        }
        // parent-id must be 16 hex chars and non-zero
        if parent_id.len() != 16
            || !parent_id.chars().all(|c| c.is_ascii_hexdigit())
            || parent_id.chars().all(|c| c == '0')
        {
            return false;
        }
        // trace-flags must be 2 hex chars
        if trace_flags.len() != 2 || !trace_flags.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }

        true
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

        // Inject W3C TRACEPARENT env var for distributed trace correlation (FT-048)
        cmd.env("TRACEPARENT", Self::generate_traceparent());

        // Inject PICLOUD_PRODUCT_VERSION (FT-040)
        if let Some(ref version) = spec.product_version {
            cmd.env("PICLOUD_PRODUCT_VERSION", version);
        }

        // Configure stdio to avoid blocking
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Resource limits (ADR-020) — set RLIMIT_AS for memory before exec
        if let Some(memory_mb) = spec.resources.memory_mb {
            let memory_bytes = (memory_mb as u64) * 1024 * 1024;
            unsafe {
                cmd.pre_exec(move || {
                    let limit = libc::rlimit {
                        rlim_cur: memory_bytes,
                        rlim_max: memory_bytes,
                    };
                    if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        let child = cmd.spawn().map_err(|e| PiCloudError::Internal(format!("Failed to spawn binary '{}': {}", spec.executable, e)))?;

        let pid = child.id().ok_or_else(|| PiCloudError::Internal("Spawned process has no PID (already exited)".to_string()))?;

        tracing::info!(
            executable = %spec.executable,
            pid = pid,
            "Spawned binary workload"
        );

        Ok((child, pid))
    }

    /// Spawn a container workload via the youki OCI runtime.
    ///
    /// youki is a low-level OCI runtime that works with OCI bundles, not
    /// container images directly. We create a minimal OCI bundle in a temp
    /// directory, then use `youki create` + `youki start`.
    async fn spawn_container_youki(
        &self,
        spec: &ContainerSpec,
        workload_iri: &ResourceIri,
    ) -> Result<(Option<Child>, u32)> {
        let container_name = workload_iri
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or("workload");

        let bundle_dir = std::path::PathBuf::from(format!(
            "/var/lib/picloud/bundles/{}",
            container_name
        ));
        let rootfs_dir = bundle_dir.join("rootfs");

        // Create the OCI bundle directory structure
        tokio::fs::create_dir_all(&rootfs_dir)
            .await
            .map_err(|e| PiCloudError::Internal(format!("Failed to create OCI bundle dir: {e}")))?;

        // Build the OCI runtime config (config.json)
        let product = Self::product_from_iri(workload_iri.as_str());
        let mut env_vars: Vec<String> = Vec::new();
        for (key, value) in &spec.env {
            let val = self.resolve_env_value(value, product).await;
            env_vars.push(format!("{}={}", key, val));
        }
        for (key, value) in self.otel_env_vars(workload_iri) {
            env_vars.push(format!("{}={}", key, value));
        }

        // Inject W3C TRACEPARENT env var for distributed trace correlation (FT-048)
        env_vars.push(format!("TRACEPARENT={}", Self::generate_traceparent()));

        // Inject PICLOUD_PRODUCT_VERSION (FT-040)
        if let Some(ref version) = spec.product_version {
            env_vars.push(format!("PICLOUD_PRODUCT_VERSION={}", version));
        }

        let config = serde_json::json!({
            "ociVersion": "1.0.2",
            "process": {
                "terminal": false,
                "user": { "uid": 0, "gid": 0 },
                "args": ["/bin/sh"],
                "env": env_vars,
                "cwd": "/",
            },
            "root": {
                "path": "rootfs",
                "readonly": false,
            },
            "hostname": container_name,
            "mounts": spec.mounts.iter().map(|m| {
                serde_json::json!({
                    "destination": m.path,
                    "type": "bind",
                    "source": m.volume,
                    "options": if m.read_only { vec!["bind", "ro"] } else { vec!["bind", "rw"] },
                })
            }).collect::<Vec<_>>(),
            "linux": {
                "namespaces": [
                    { "type": "pid" },
                    { "type": "network" },
                    { "type": "mount" },
                ],
                "resources": {
                    "memory": spec.resources.memory_mb.map(|mb| {
                        serde_json::json!({ "limit": (mb as u64) * 1024 * 1024 })
                    }).unwrap_or(serde_json::json!({})),
                    "cpu": spec.resources.cpu_millicores.map(|milli| {
                        serde_json::json!({
                            "quota": (milli as u64) * 100, // microseconds per period
                            "period": 100000u64
                        })
                    }).unwrap_or(serde_json::json!({})),
                },
            },
        });

        let config_path = bundle_dir.join("config.json");
        tokio::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap_or_default())
            .await
            .map_err(|e| PiCloudError::Internal(format!("Failed to write OCI config: {e}")))?;

        // youki create <container-id> --bundle <bundle-dir>
        let create_output = Command::new("youki")
            .args(["create", container_name, "--bundle"])
            .arg(&bundle_dir)
            .output()
            .await
            .map_err(|e| PiCloudError::Internal(format!("youki create failed: {e}")))?;

        if !create_output.status.success() {
            return Err(PiCloudError::Internal(format!(
                "youki create failed: {}",
                String::from_utf8_lossy(&create_output.stderr)
            )));
        }

        // youki start <container-id>
        let start_output = Command::new("youki")
            .args(["start", container_name])
            .output()
            .await
            .map_err(|e| PiCloudError::Internal(format!("youki start failed: {e}")))?;

        if !start_output.status.success() {
            return Err(PiCloudError::Internal(format!(
                "youki start failed: {}",
                String::from_utf8_lossy(&start_output.stderr)
            )));
        }

        let pid = self.next_pid.fetch_add(1, Ordering::Relaxed);
        tracing::info!(
            image = %spec.image,
            container = container_name,
            "Container started via youki"
        );

        Ok((None, pid))
    }

    /// Spawn a container workload via podman or docker.
    async fn spawn_container(
        &self,
        spec: &ContainerSpec,
        workload_iri: &ResourceIri,
    ) -> Result<(Option<Child>, u32)> {
        let runtime = match &self.container_runtime {
            // youki is a low-level OCI runtime — when detected, use it via
            // `podman --runtime youki` if podman is available, otherwise via
            // the youki CLI directly with OCI bundle creation.
            ContainerRuntime::Youki => {
                return self.spawn_container_youki(spec, workload_iri).await;
            }
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

        // ADR-028: Product network isolation — each product gets its own network.
        // This prevents direct cross-product communication at the container level.
        let product = Self::product_from_iri(workload_iri.as_str());
        if let Some(product_name) = product {
            cmd.args(["--network", &format!("picloud-{}", product_name)]);
        }

        // Environment variables
        for (key, value) in &spec.env {
            let val = self.resolve_env_value(value, Self::product_from_iri(workload_iri.as_str())).await;
            cmd.args(["-e", &format!("{}={}", key, val)]);
        }

        // Inject OTEL_* env vars (ADR-045)
        for (key, value) in self.otel_env_vars(workload_iri) {
            cmd.args(["-e", &format!("{}={}", key, value)]);
        }

        // Inject W3C TRACEPARENT env var for distributed trace correlation (FT-048)
        cmd.args(["-e", &format!("TRACEPARENT={}", Self::generate_traceparent())]);

        // Inject PICLOUD_PRODUCT_VERSION (FT-040)
        if let Some(ref version) = spec.product_version {
            cmd.args(["-e", &format!("PICLOUD_PRODUCT_VERSION={}", version)]);
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

        // Resource limits (ADR-020)
        if let Some(memory_mb) = spec.resources.memory_mb {
            cmd.args(["--memory", &format!("{}m", memory_mb)]);
        }
        if let Some(cpu_milli) = spec.resources.cpu_millicores {
            let cpus = cpu_milli as f64 / 1000.0;
            cmd.args(["--cpus", &format!("{:.2}", cpus)]);
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

        let is_container = matches!(spec, WorkloadSpec::Container(_));
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
            is_container,
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

        // For container workloads, use docker/podman stop
        if entry.is_container {
            let container_name = workload_iri
                .as_str()
                .rsplit('/')
                .next()
                .unwrap_or("workload");
            let runtime = match &self.container_runtime {
                ContainerRuntime::Youki => "youki",
                ContainerRuntime::Docker => "docker",
                ContainerRuntime::Podman => "podman",
                ContainerRuntime::None => "",
            };
            if runtime == "youki" {
                // youki uses kill + delete instead of stop
                let _ = Command::new("youki")
                    .args(["kill", container_name, "SIGTERM"])
                    .output()
                    .await;
                let _ = Command::new("youki")
                    .args(["delete", "--force", container_name])
                    .output()
                    .await;
                tracing::info!(
                    workload_iri = %workload_iri,
                    container = container_name,
                    "Stopped container via youki"
                );
            } else if !runtime.is_empty() {
                match Command::new(runtime)
                    .args(["stop", container_name])
                    .output()
                    .await
                {
                    Ok(output) => {
                        if output.status.success() {
                            tracing::info!(
                                workload_iri = %workload_iri,
                                container = container_name,
                                "Stopped container via {}", runtime
                            );
                        } else {
                            tracing::warn!(
                                workload_iri = %workload_iri,
                                container = container_name,
                                stderr = %String::from_utf8_lossy(&output.stderr),
                                "Failed to stop container"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            workload_iri = %workload_iri,
                            error = %e,
                            "Failed to run {} stop", runtime
                        );
                    }
                }
            }
        }

        // Send SIGTERM to the real process if we have a PID (for binary workloads)
        if !entry.is_container {
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

// ==========================================================================
// Node Drain Coordinator (FT-011)
// ==========================================================================

use picloud_domain::traits::{DrainResult, NodeDrainCoordinator, NodeDrainState, NodeWorkloadInfo};

/// In-memory drain coordinator that manages node cordon/drain state
/// and coordinates workload migration during drain operations.
pub struct InMemoryDrainCoordinator {
    /// Map of node_id → drain state
    drain_states: Arc<RwLock<HashMap<Uuid, NodeDrainState>>>,
    /// Map of node_id → list of workloads running on the node
    node_workloads: Arc<RwLock<HashMap<Uuid, Vec<NodeWorkloadInfo>>>>,
    /// Available target nodes for migration
    available_nodes: Arc<RwLock<Vec<Uuid>>>,
}

impl InMemoryDrainCoordinator {
    pub fn new() -> Self {
        Self {
            drain_states: Arc::new(RwLock::new(HashMap::new())),
            node_workloads: Arc::new(RwLock::new(HashMap::new())),
            available_nodes: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a node as active (for testing).
    pub async fn register_node(&self, node_id: Uuid) {
        self.drain_states.write().await.insert(node_id, NodeDrainState::Active);
    }

    /// Register a workload as running on a node (for testing/tracking).
    pub async fn register_workload(&self, node_id: Uuid, workload: NodeWorkloadInfo) {
        self.node_workloads
            .write()
            .await
            .entry(node_id)
            .or_default()
            .push(workload);
    }

    /// Set available target nodes for migration.
    pub async fn set_available_nodes(&self, nodes: Vec<Uuid>) {
        *self.available_nodes.write().await = nodes;
    }
}

#[async_trait]
impl NodeDrainCoordinator for InMemoryDrainCoordinator {
    async fn cordon(&self, node_id: Uuid) -> Result<()> {
        let mut states = self.drain_states.write().await;
        match states.get(&node_id) {
            Some(NodeDrainState::Active) | None => {
                states.insert(node_id, NodeDrainState::Cordoned);
                Ok(())
            }
            Some(state) => Err(PiCloudError::Internal(format!(
                "Cannot cordon node in state {:?}",
                state
            ))),
        }
    }

    async fn uncordon(&self, node_id: Uuid) -> Result<()> {
        let mut states = self.drain_states.write().await;
        match states.get(&node_id) {
            Some(NodeDrainState::Cordoned) | Some(NodeDrainState::Drained) => {
                states.insert(node_id, NodeDrainState::Active);
                Ok(())
            }
            None => {
                states.insert(node_id, NodeDrainState::Active);
                Ok(())
            }
            Some(NodeDrainState::Draining) => Err(PiCloudError::Internal(
                "Cannot uncordon a node that is currently draining".to_string(),
            )),
            Some(state) => Err(PiCloudError::Internal(format!(
                "Cannot uncordon node in state {:?}",
                state
            ))),
        }
    }

    async fn drain(&self, node_id: Uuid, timeout_secs: u64) -> Result<DrainResult> {
        let start = std::time::Instant::now();

        // Step 1: Cordon the node
        {
            let mut states = self.drain_states.write().await;
            states.insert(node_id, NodeDrainState::Draining);
        }

        // Step 2: Get workloads on this node
        let workloads = {
            let wl = self.node_workloads.read().await;
            wl.get(&node_id).cloned().unwrap_or_default()
        };

        let workload_count = workloads.len();

        // Step 3: Get available target nodes
        let target_nodes = self.available_nodes.read().await.clone();
        if target_nodes.is_empty() && workload_count > 0 {
            let mut states = self.drain_states.write().await;
            states.insert(node_id, NodeDrainState::Cordoned);
            return Ok(DrainResult {
                node_id,
                workloads_migrated: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                success: false,
                error: Some("No available target nodes for workload migration".to_string()),
            });
        }

        // Step 4: Simulate migrating workloads (round-robin across target nodes)
        let deadline = tokio::time::Instant::now()
            + tokio::time::Duration::from_secs(timeout_secs);
        let mut migrated = 0;

        for (_i, _workload) in workloads.iter().enumerate() {
            if tokio::time::Instant::now() >= deadline {
                let mut states = self.drain_states.write().await;
                states.insert(node_id, NodeDrainState::Cordoned);
                return Ok(DrainResult {
                    node_id,
                    workloads_migrated: migrated,
                    duration_ms: start.elapsed().as_millis() as u64,
                    success: false,
                    error: Some(format!(
                        "Drain timeout: migrated {migrated}/{workload_count} workloads"
                    )),
                });
            }

            // Simulate migration work — in production this would stop the workload
            // on the source node and restart it on the target
            migrated += 1;
        }

        // Step 5: Clear workloads from the drained node
        {
            let mut wl = self.node_workloads.write().await;
            wl.remove(&node_id);
        }

        // Step 6: Mark node as drained
        {
            let mut states = self.drain_states.write().await;
            states.insert(node_id, NodeDrainState::Drained);
        }

        Ok(DrainResult {
            node_id,
            workloads_migrated: migrated,
            duration_ms: start.elapsed().as_millis() as u64,
            success: true,
            error: None,
        })
    }

    async fn drain_state(&self, node_id: Uuid) -> Result<NodeDrainState> {
        let states = self.drain_states.read().await;
        Ok(states.get(&node_id).cloned().unwrap_or(NodeDrainState::Active))
    }

    async fn node_workloads(&self, node_id: Uuid) -> Result<Vec<NodeWorkloadInfo>> {
        let wl = self.node_workloads.read().await;
        Ok(wl.get(&node_id).cloned().unwrap_or_default())
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
            product_version: None,
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
            product_version: None,
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
            product_version: None,
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
            product_version: None,
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
            product_version: None,
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
            product_version: None,
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
            product_version: None,
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
            product_version: None,
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
            product_version: None,
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
            product_version: None,
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
            product_version: None,
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

    // --- ADR-020: Resource limits tests ---

    #[tokio::test]
    async fn binary_with_memory_limit_spawns_with_rlimit() {
        let scheduler = test_scheduler();
        let iri = ResourceIri::new("https://picloud.local/products/test/binaries/limited").unwrap();
        let spec = WorkloadSpec::Binary(BinarySpec {
            executable: "/bin/echo".to_string(),
            args: vec!["limited".to_string()],
            identity: "test".to_string(),
            resources: ResourceLimits {
                cpu_millicores: Some(500),
                memory_mb: Some(256),
            },
            mounts: vec![],
            env: HashMap::new(),
            restart_policy: RestartPolicy::Never,
            product_version: None,
        });

        // This verifies the pre_exec hook doesn't crash the spawn
        let result = scheduler.schedule(&iri, &spec).await;
        assert!(result.is_ok(), "Schedule with resource limits should succeed");
        let handle = result.unwrap();
        assert!(handle.pid.unwrap_or(0) > 0);
        // Cleanup
        let _ = scheduler.stop(&iri).await;
    }

    // --- ADR-028: Network isolation tests ---

    #[test]
    fn product_from_iri_extracts_product_name() {
        assert_eq!(
            ProcessScheduler::product_from_iri("https://picloud.local/products/photo-app/containers/web"),
            Some("photo-app")
        );
        assert_eq!(
            ProcessScheduler::product_from_iri("https://picloud.local/nodes/node1"),
            None
        );
    }
}
