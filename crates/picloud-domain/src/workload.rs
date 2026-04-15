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

/// CPU and memory limits for a workload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// CPU in millicores — e.g. 500 = 0.5 CPU
    pub cpu_millicores: Option<u32>,
    /// Memory in megabytes
    pub memory_mb: Option<u32>,
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
