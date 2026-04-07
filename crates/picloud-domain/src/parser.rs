/// Resource File Parser
///
/// Parses `.picloud` resource definition files into typed resource declarations.
///
/// Phase 1 uses JSON format for resource files. The Bicep-inspired syntax
/// described in the PRD (ADR-007) will be implemented in a later phase as a
/// syntactic sugar layer that compiles down to the same typed declarations.
///
/// A `.picloud` file is a JSON object with a `resources` array:
/// ```json
/// {
///   "resources": [
///     { "type": "product", "name": "photo-app", "version": "1.0.0" },
///     { "type": "volume", "name": "media-store", "product": "photo-app", "size_gb": 100 },
///     { "type": "container", "name": "api-server", "product": "photo-app", "image": "photo-api:1.0.0" }
///   ]
/// }
/// ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{PiCloudError, Result};
use crate::storage::{DurabilityTier, PerformanceTier, StorageIntent};
use crate::workload::{
    BinarySpec, ContainerSpec, EnvValue, PortMapping, ResourceLimits, RestartPolicy,
    VolumeMount,
};

/// A parsed resource file containing one or more resource declarations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceFile {
    pub resources: Vec<ResourceDeclaration>,
}

/// A single resource declaration parsed from a `.picloud` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceDeclaration {
    Product(ProductDecl),
    Volume(VolumeDecl),
    Container(ContainerDecl),
    Binary(BinaryDecl),
    #[serde(rename = "event-subscription")]
    EventSubscription(EventSubscriptionDecl),
    Ingress(IngressDecl),
    Secret(SecretDecl),
    Role(RoleDecl),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductDecl {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeDecl {
    pub name: String,
    pub product: String,
    #[serde(default = "default_size_gb")]
    pub size_gb: u64,
    #[serde(default)]
    pub durability: Option<String>,
    #[serde(default)]
    pub performance: Option<String>,
}

fn default_size_gb() -> u64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerDecl {
    pub name: String,
    pub product: String,
    pub image: String,
    #[serde(default = "default_identity")]
    pub identity: String,
    #[serde(default)]
    pub cpu: Option<String>,
    #[serde(default)]
    pub memory: Option<String>,
    #[serde(default)]
    pub mounts: Vec<MountDecl>,
    #[serde(default)]
    pub env: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub ports: Vec<u16>,
}

fn default_identity() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountDecl {
    pub volume: String,
    pub path: String,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryDecl {
    pub name: String,
    pub product: String,
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_identity")]
    pub identity: String,
    #[serde(default)]
    pub cpu: Option<String>,
    #[serde(default)]
    pub memory: Option<String>,
    #[serde(default)]
    pub mounts: Vec<MountDecl>,
    #[serde(default)]
    pub env: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubscriptionDecl {
    pub name: String,
    pub product: String,
    pub source: String,
    pub event: String,
    pub handler: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressDecl {
    pub name: String,
    pub product: String,
    pub target: String,
    pub port: u16,
    pub path: String,
    #[serde(default = "default_true")]
    pub tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretDecl {
    pub name: String,
    pub product: String,
    /// The plaintext value (will be encrypted by the platform on apply)
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDecl {
    pub name: String,
    /// Product scope — if set, this is a product-level role
    #[serde(default)]
    pub product: Option<String>,
    pub permissions: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl ResourceFile {
    /// Parse a `.picloud` resource file from JSON string.
    pub fn parse(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| PiCloudError::ResourceValidationFailed {
            reason: format!("invalid resource file JSON: {e}"),
        })
    }

    /// Validate all declarations in the file.
    pub fn validate(&self) -> Result<()> {
        for decl in &self.resources {
            decl.validate()?;
        }

        // Check that every product-scoped resource references a product declared in this file
        let product_names: Vec<&str> = self
            .resources
            .iter()
            .filter_map(|r| match r {
                ResourceDeclaration::Product(p) => Some(p.name.as_str()),
                _ => None,
            })
            .collect();

        for decl in &self.resources {
            if let Some(product) = decl.product_name() {
                if !product_names.contains(&product) {
                    return Err(PiCloudError::ResourceValidationFailed {
                        reason: format!(
                            "{}: references product '{}' which is not declared in this file",
                            decl.resource_name(),
                            product
                        ),
                    });
                }
            }
        }

        Ok(())
    }
}

fn validation_err(resource: &str, reason: &str) -> PiCloudError {
    PiCloudError::ResourceValidationFailed {
        reason: format!("{resource}: {reason}"),
    }
}

impl ResourceDeclaration {
    /// Validate a single resource declaration.
    pub fn validate(&self) -> Result<()> {
        match self {
            ResourceDeclaration::Product(p) => {
                if p.name.is_empty() {
                    return Err(validation_err("product", "name cannot be empty"));
                }
                if p.version.is_empty() {
                    return Err(validation_err(&p.name, "version cannot be empty"));
                }
                Ok(())
            }
            ResourceDeclaration::Volume(v) => {
                if v.name.is_empty() || v.product.is_empty() {
                    return Err(validation_err("volume", "name and product cannot be empty"));
                }
                if v.size_gb == 0 {
                    return Err(validation_err(&v.name, "size_gb must be greater than 0"));
                }
                Ok(())
            }
            ResourceDeclaration::Container(c) => {
                if c.name.is_empty() || c.product.is_empty() || c.image.is_empty() {
                    return Err(validation_err("container", "name, product, and image cannot be empty"));
                }
                Ok(())
            }
            ResourceDeclaration::Binary(b) => {
                if b.name.is_empty() || b.product.is_empty() || b.executable.is_empty() {
                    return Err(validation_err("binary", "name, product, and executable cannot be empty"));
                }
                Ok(())
            }
            ResourceDeclaration::EventSubscription(e) => {
                if e.name.is_empty() || e.product.is_empty() || e.source.is_empty() {
                    return Err(validation_err("event-subscription", "name, product, and source cannot be empty"));
                }
                Ok(())
            }
            ResourceDeclaration::Ingress(i) => {
                if i.name.is_empty() || i.product.is_empty() || i.target.is_empty() {
                    return Err(validation_err("ingress", "name, product, and target cannot be empty"));
                }
                Ok(())
            }
            ResourceDeclaration::Secret(s) => {
                if s.name.is_empty() || s.product.is_empty() || s.value.is_empty() {
                    return Err(validation_err("secret", "name, product, and value cannot be empty"));
                }
                Ok(())
            }
            ResourceDeclaration::Role(r) => {
                if r.name.is_empty() || r.permissions.is_empty() {
                    return Err(validation_err("role", "name and permissions cannot be empty"));
                }
                Ok(())
            }
        }
    }

    /// Get the product name this resource belongs to, if any.
    pub fn product_name(&self) -> Option<&str> {
        match self {
            ResourceDeclaration::Product(_) => None,
            ResourceDeclaration::Volume(v) => Some(&v.product),
            ResourceDeclaration::Container(c) => Some(&c.product),
            ResourceDeclaration::Binary(b) => Some(&b.product),
            ResourceDeclaration::EventSubscription(e) => Some(&e.product),
            ResourceDeclaration::Ingress(i) => Some(&i.product),
            ResourceDeclaration::Secret(s) => Some(&s.product),
            ResourceDeclaration::Role(r) => r.product.as_deref(),
        }
    }

    /// Get the resource name.
    pub fn resource_name(&self) -> &str {
        match self {
            ResourceDeclaration::Product(p) => &p.name,
            ResourceDeclaration::Volume(v) => &v.name,
            ResourceDeclaration::Container(c) => &c.name,
            ResourceDeclaration::Binary(b) => &b.name,
            ResourceDeclaration::EventSubscription(e) => &e.name,
            ResourceDeclaration::Ingress(i) => &i.name,
            ResourceDeclaration::Secret(s) => &s.name,
            ResourceDeclaration::Role(r) => &r.name,
        }
    }

    /// Get the resource type name.
    pub fn resource_type(&self) -> &str {
        match self {
            ResourceDeclaration::Product(_) => "product",
            ResourceDeclaration::Volume(_) => "volume",
            ResourceDeclaration::Container(_) => "container",
            ResourceDeclaration::Binary(_) => "binary",
            ResourceDeclaration::EventSubscription(_) => "event-subscription",
            ResourceDeclaration::Ingress(_) => "ingress",
            ResourceDeclaration::Secret(_) => "secret",
            ResourceDeclaration::Role(_) => "role",
        }
    }
}

impl VolumeDecl {
    /// Convert the parsed durability/performance strings into a `StorageIntent`.
    pub fn storage_intent(&self) -> StorageIntent {
        let durability = match self.durability.as_deref() {
            Some("full-replication") | None => DurabilityTier::FullReplication,
            Some("quorum") => DurabilityTier::Quorum,
            Some("local") => DurabilityTier::Local,
            Some("none") => DurabilityTier::None,
            _ => DurabilityTier::FullReplication,
        };
        let performance = match self.performance.as_deref() {
            Some("fast") => PerformanceTier::Fast,
            Some("archive") => PerformanceTier::Archive,
            Some("standard") | None => PerformanceTier::Standard,
            _ => PerformanceTier::Standard,
        };
        StorageIntent {
            durability,
            performance,
        }
    }
}

impl ContainerDecl {
    /// Convert to a `ContainerSpec`.
    pub fn to_spec(&self) -> ContainerSpec {
        let resources = ResourceLimits {
            cpu_millicores: self.cpu.as_ref().and_then(|s| parse_cpu_millis(s)),
            memory_mb: self.memory.as_ref().and_then(|s| parse_memory_mb(s)),
        };

        let mounts = self
            .mounts
            .iter()
            .map(|m| VolumeMount {
                volume: m.volume.clone(),
                path: m.path.clone(),
                read_only: m.read_only,
            })
            .collect();

        let env = self
            .env
            .iter()
            .map(|(k, v)| {
                let env_val = if let Some(obj) = v.as_object() {
                    if let Some(secret) = obj.get("secret").and_then(|s| s.as_str()) {
                        EnvValue::Secret {
                            secret: secret.to_string(),
                        }
                    } else {
                        EnvValue::Literal(v.to_string())
                    }
                } else {
                    EnvValue::Literal(v.as_str().unwrap_or_default().to_string())
                };
                (k.clone(), env_val)
            })
            .collect();

        let ports = self
            .ports
            .iter()
            .map(|&p| PortMapping {
                port: p,
                protocol: crate::workload::Protocol::Tcp,
            })
            .collect();

        ContainerSpec {
            image: self.image.clone(),
            identity: self.identity.clone(),
            resources,
            mounts,
            env,
            ports,
            health_check: None,
            restart_policy: RestartPolicy::Always,
        }
    }
}

impl BinaryDecl {
    /// Convert to a `BinarySpec`.
    pub fn to_spec(&self) -> BinarySpec {
        let resources = ResourceLimits {
            cpu_millicores: self.cpu.as_ref().and_then(|s| parse_cpu_millis(s)),
            memory_mb: self.memory.as_ref().and_then(|s| parse_memory_mb(s)),
        };

        let mounts = self
            .mounts
            .iter()
            .map(|m| VolumeMount {
                volume: m.volume.clone(),
                path: m.path.clone(),
                read_only: m.read_only,
            })
            .collect();

        let env = self
            .env
            .iter()
            .map(|(k, v)| {
                let env_val = if let Some(obj) = v.as_object() {
                    if let Some(secret) = obj.get("secret").and_then(|s| s.as_str()) {
                        EnvValue::Secret {
                            secret: secret.to_string(),
                        }
                    } else {
                        EnvValue::Literal(v.to_string())
                    }
                } else {
                    EnvValue::Literal(v.as_str().unwrap_or_default().to_string())
                };
                (k.clone(), env_val)
            })
            .collect();

        BinarySpec {
            executable: self.executable.clone(),
            args: self.args.clone(),
            identity: self.identity.clone(),
            resources,
            mounts,
            env,
            restart_policy: RestartPolicy::Always,
        }
    }
}

/// Parse CPU string like "500m" to millicores.
fn parse_cpu_millis(s: &str) -> Option<u32> {
    if let Some(millis) = s.strip_suffix('m') {
        millis.parse().ok()
    } else {
        // Interpret as whole cores
        s.parse::<f64>().ok().map(|cores| (cores * 1000.0) as u32)
    }
}

/// Parse memory string like "512MB" or "1GB" to megabytes.
fn parse_memory_mb(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(gb) = s
        .strip_suffix("GB")
        .or_else(|| s.strip_suffix("gb"))
        .or_else(|| s.strip_suffix("Gi"))
    {
        gb.trim().parse::<u32>().ok().map(|g| g * 1024)
    } else if let Some(mb) = s
        .strip_suffix("MB")
        .or_else(|| s.strip_suffix("mb"))
        .or_else(|| s.strip_suffix("Mi"))
    {
        mb.trim().parse().ok()
    } else {
        // Assume megabytes
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_product() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "my-app", "version": "1.0.0" }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert_eq!(file.resources.len(), 1);
        match &file.resources[0] {
            ResourceDeclaration::Product(p) => {
                assert_eq!(p.name, "my-app");
                assert_eq!(p.version, "1.0.0");
            }
            _ => panic!("expected Product"),
        }
        file.validate().unwrap();
    }

    #[test]
    fn parse_full_product_with_resources() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "photo-app", "version": "1.0.0", "description": "A photo app" },
                { "type": "volume", "name": "media-store", "product": "photo-app", "size_gb": 100, "durability": "full-replication" },
                { "type": "container", "name": "api-server", "product": "photo-app", "image": "photo-api:1.0.0", "identity": "api-worker",
                  "cpu": "500m", "memory": "512MB",
                  "mounts": [{ "volume": "media-store", "path": "/data" }],
                  "env": { "LOG_LEVEL": "info", "DB_URL": { "secret": "db-connection" } },
                  "ports": [8080] }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert_eq!(file.resources.len(), 3);
        file.validate().unwrap();

        // Check volume storage intent
        match &file.resources[1] {
            ResourceDeclaration::Volume(v) => {
                assert_eq!(v.size_gb, 100);
                let intent = v.storage_intent();
                assert!(matches!(intent.durability, DurabilityTier::FullReplication));
            }
            _ => panic!("expected Volume"),
        }

        // Check container spec conversion
        match &file.resources[2] {
            ResourceDeclaration::Container(c) => {
                let spec = c.to_spec();
                assert_eq!(spec.image, "photo-api:1.0.0");
                assert_eq!(spec.identity, "api-worker");
                assert_eq!(spec.resources.cpu_millicores, Some(500));
                assert_eq!(spec.resources.memory_mb, Some(512));
                assert_eq!(spec.mounts.len(), 1);
                assert_eq!(spec.mounts[0].volume, "media-store");
                assert_eq!(spec.ports.len(), 1);
                assert!(matches!(spec.env.get("DB_URL"), Some(EnvValue::Secret { secret }) if secret == "db-connection"));
            }
            _ => panic!("expected Container"),
        }
    }

    #[test]
    fn parse_binary_workload() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "my-app", "version": "1.0.0" },
                { "type": "binary", "name": "worker", "product": "my-app", "executable": "worker-arm64", "args": ["--port", "9090"], "cpu": "250m", "memory": "256MB" }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        file.validate().unwrap();

        match &file.resources[1] {
            ResourceDeclaration::Binary(b) => {
                let spec = b.to_spec();
                assert_eq!(spec.executable, "worker-arm64");
                assert_eq!(spec.args, vec!["--port", "9090"]);
                assert_eq!(spec.resources.cpu_millicores, Some(250));
                assert_eq!(spec.resources.memory_mb, Some(256));
            }
            _ => panic!("expected Binary"),
        }
    }

    #[test]
    fn parse_event_subscription() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "photo-app", "version": "1.0.0" },
                { "type": "event-subscription", "name": "on-user-created", "product": "photo-app", "source": "user-service@1.0.0", "event": "UserCreated", "handler": "api-server" }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        file.validate().unwrap();
    }

    #[test]
    fn parse_ingress() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "photo-app", "version": "1.0.0" },
                { "type": "ingress", "name": "api-ingress", "product": "photo-app", "target": "api-server", "port": 8080, "path": "/products/photo-app/api" }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        file.validate().unwrap();

        match &file.resources[1] {
            ResourceDeclaration::Ingress(i) => {
                assert!(i.tls); // default true
                assert_eq!(i.port, 8080);
            }
            _ => panic!("expected Ingress"),
        }
    }

    #[test]
    fn validation_rejects_empty_name() {
        let json = r#"{ "resources": [{ "type": "product", "name": "", "version": "1.0.0" }] }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert!(file.validate().is_err());
    }

    #[test]
    fn validation_rejects_missing_product_ref() {
        let json = r#"{
            "resources": [
                { "type": "volume", "name": "vol", "product": "nonexistent", "size_gb": 10 }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert!(file.validate().is_err());
    }

    #[test]
    fn validation_rejects_zero_size_volume() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "volume", "name": "vol", "product": "app", "size_gb": 0 }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert!(file.validate().is_err());
    }

    #[test]
    fn parse_cpu_millis_values() {
        assert_eq!(parse_cpu_millis("500m"), Some(500));
        assert_eq!(parse_cpu_millis("1000m"), Some(1000));
        assert_eq!(parse_cpu_millis("2"), Some(2000));
        assert_eq!(parse_cpu_millis("0.5"), Some(500));
    }

    #[test]
    fn parse_memory_mb_values() {
        assert_eq!(parse_memory_mb("512MB"), Some(512));
        assert_eq!(parse_memory_mb("1GB"), Some(1024));
        assert_eq!(parse_memory_mb("256"), Some(256));
        assert_eq!(parse_memory_mb("2Gi"), Some(2048));
    }
}
