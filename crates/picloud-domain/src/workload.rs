/// Workload Types
///
/// PiCloud runs two workload primitives: OCI containers and raw ARM64 binaries.
/// Both receive identical treatment: identity injection, secret injection,
/// volume mounts, and networking (ADR-010).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Specification for an OCI container workload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSpec {
    pub image: String,
    /// Workload identity name — the platform injects credentials at runtime
    pub identity: String,
    pub resources: ResourceLimits,
    pub mounts: Vec<VolumeMount>,
    pub env: HashMap<String, EnvValue>,
    pub ports: Vec<PortMapping>,
    pub health_check: Option<HealthCheck>,
    pub restart_policy: RestartPolicy,
    /// Product version — injected as PICLOUD_PRODUCT_VERSION env var (FT-040)
    #[serde(default)]
    pub product_version: Option<String>,
}

/// Specification for a raw ARM64 binary workload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinarySpec {
    /// Path to the executable, relative to the deployment artifact
    pub executable: String,
    pub args: Vec<String>,
    pub identity: String,
    pub resources: ResourceLimits,
    pub mounts: Vec<VolumeMount>,
    pub env: HashMap<String, EnvValue>,
    pub restart_policy: RestartPolicy,
    /// Product version — injected as PICLOUD_PRODUCT_VERSION env var (FT-040)
    #[serde(default)]
    pub product_version: Option<String>,
}

/// CPU and memory limits for a workload (FT-091)
///
/// Both fields are optional. When set, the platform enforces them:
/// - Container workloads: limits are passed to the OCI runtime (cgroups)
/// - Binary workloads: memory via RLIMIT_AS, CPU via RLIMIT_CPU
///
/// The `validate()` method enforces minimum/maximum bounds and rejects
/// nonsensical values before scheduling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// CPU in millicores — e.g. 500 = 0.5 CPU.
    /// Range: 1..=128_000 (128 cores). Zero is rejected.
    pub cpu_millicores: Option<u32>,
    /// Memory in megabytes.
    /// Range: 4..=1_048_576 (1 TiB). Values below 4 MB are rejected.
    pub memory_mb: Option<u32>,
}

impl ResourceLimits {
    /// Minimum CPU in millicores (1m = 0.001 CPU)
    pub const MIN_CPU_MILLICORES: u32 = 1;
    /// Maximum CPU in millicores (128 full cores)
    pub const MAX_CPU_MILLICORES: u32 = 128_000;
    /// Minimum memory in megabytes (4 MB — below this nothing useful runs)
    pub const MIN_MEMORY_MB: u32 = 4;
    /// Maximum memory in megabytes (1 TiB)
    pub const MAX_MEMORY_MB: u32 = 1_048_576;

    /// Create a new ResourceLimits with no constraints.
    pub fn none() -> Self {
        Self {
            cpu_millicores: None,
            memory_mb: None,
        }
    }

    /// Create a new ResourceLimits with both CPU and memory set.
    pub fn new(cpu_millicores: u32, memory_mb: u32) -> Self {
        Self {
            cpu_millicores: Some(cpu_millicores),
            memory_mb: Some(memory_mb),
        }
    }

    /// Validate resource limits are within acceptable bounds.
    ///
    /// Returns `Ok(())` if limits are valid, or a descriptive error string
    /// listing all violations.
    pub fn validate(&self) -> Result<(), String> {
        let mut errors: Vec<String> = Vec::new();

        if let Some(cpu) = self.cpu_millicores {
            if cpu < Self::MIN_CPU_MILLICORES {
                errors.push(format!(
                    "cpu_millicores ({cpu}) below minimum ({})",
                    Self::MIN_CPU_MILLICORES
                ));
            }
            if cpu > Self::MAX_CPU_MILLICORES {
                errors.push(format!(
                    "cpu_millicores ({cpu}) exceeds maximum ({})",
                    Self::MAX_CPU_MILLICORES
                ));
            }
        }

        if let Some(mem) = self.memory_mb {
            if mem < Self::MIN_MEMORY_MB {
                errors.push(format!(
                    "memory_mb ({mem}) below minimum ({})",
                    Self::MIN_MEMORY_MB
                ));
            }
            if mem > Self::MAX_MEMORY_MB {
                errors.push(format!(
                    "memory_mb ({mem}) exceeds maximum ({})",
                    Self::MAX_MEMORY_MB
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Returns true if any resource limit is set.
    pub fn has_limits(&self) -> bool {
        self.cpu_millicores.is_some() || self.memory_mb.is_some()
    }

    /// Convert CPU millicores to fractional cores (for Docker/Podman --cpus flag).
    ///
    /// Example: 500 millicores → 0.5 CPUs
    pub fn cpu_as_fractional_cores(&self) -> Option<f64> {
        self.cpu_millicores.map(|m| m as f64 / 1000.0)
    }

    /// Convert memory_mb to bytes (for cgroup / RLIMIT_AS).
    pub fn memory_as_bytes(&self) -> Option<u64> {
        self.memory_mb.map(|mb| (mb as u64) * 1024 * 1024)
    }

    /// Convert CPU millicores to cgroup v1 cpu.cfs_quota_us.
    ///
    /// With a 100 ms (100_000 µs) period, quota = millicores × 100.
    /// Example: 500m → 50_000 µs quota per 100_000 µs period = 50% of one CPU.
    pub fn cpu_as_cfs_quota_us(&self) -> Option<u64> {
        self.cpu_millicores.map(|m| (m as u64) * 100)
    }

    /// The cgroup CPU period in microseconds (100 ms).
    pub const CFS_PERIOD_US: u64 = 100_000;
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self::none()
    }
}

/// A volume mount — maps a platform volume into a workload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    /// Name of the volume resource in this product
    pub volume: String,
    /// Path inside the workload where the volume is mounted
    pub path: String,
    pub read_only: bool,
}

/// An environment variable — either a literal value or a secret reference
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EnvValue {
    /// Literal string value
    Literal(String),
    /// Reference to a platform-managed secret — injected at runtime
    Secret { secret: String },
}

/// A port mapping for networking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub port: u16,
    pub protocol: Protocol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Tcp,
    Udp,
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub kind: HealthCheckKind,
    pub interval_seconds: u32,
    pub timeout_seconds: u32,
    pub failure_threshold: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckKind {
    /// HTTP GET — expects 2xx response
    Http { path: String, port: u16 },
    /// TCP connection
    Tcp { port: u16 },
    /// Process liveness — is the process running?
    Process,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    /// Always restart on failure
    Always,
    /// Restart up to max_retries times
    OnFailure { max_retries: u32 },
    /// Never restart
    Never,
}
