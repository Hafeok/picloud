use serde::Deserialize;
use std::path::Path;

/// Cluster configuration loaded from cluster.toml
#[derive(Debug, Clone, Deserialize)]
pub struct ClusterConfig {
    pub cluster: ClusterSection,
    #[serde(default)]
    pub operator: Option<OperatorConfig>,
    #[serde(default)]
    pub nodes: Vec<NodeConfig>,
}

/// The management/operator node that runs picloud-test and drives upgrades.
#[derive(Debug, Clone, Deserialize)]
pub struct OperatorConfig {
    pub hostname: String,
    pub ip: String,
    #[serde(default)]
    pub ssh_user: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClusterSection {
    pub domain: String,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    #[serde(default = "default_platform_version")]
    pub platform_version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    pub hostname: String,
    pub ip: String,
    #[serde(default)]
    pub role: NodeRole,
    #[serde(default)]
    pub ssh_user: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub enum NodeRole {
    #[default]
    Worker,
    Build,
}

fn default_http_port() -> u16 {
    7443
}

fn default_platform_version() -> String {
    "auto".to_string()
}

impl ClusterConfig {
    /// Load configuration from a TOML file at the given path.
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        let config: ClusterConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Return the base URL for HTTP requests to the cluster.
    pub fn base_url(&self) -> String {
        format!("https://{}:{}", self.cluster.domain, self.cluster.http_port)
    }

    /// Return the first node IP, useful for direct-node probes.
    pub fn first_node_ip(&self) -> Option<&str> {
        self.nodes.first().map(|n| n.ip.as_str())
    }
}

impl std::fmt::Display for NodeRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeRole::Worker => write!(f, "Worker"),
            NodeRole::Build => write!(f, "Build"),
        }
    }
}
