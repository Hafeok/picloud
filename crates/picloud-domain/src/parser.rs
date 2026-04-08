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
    Config(ConfigDecl),
    #[serde(rename = "feature-flag")]
    FeatureFlag(FeatureFlagDecl),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductDecl {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
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
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
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
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
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
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubscriptionDecl {
    pub name: String,
    pub product: String,
    pub source: String,
    pub event: String,
    pub handler: String,
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
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
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretDecl {
    pub name: String,
    pub product: String,
    /// The plaintext value (will be encrypted by the platform on apply)
    pub value: String,
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDecl {
    pub name: String,
    /// Product scope — if set, this is a product-level role
    #[serde(default)]
    pub product: Option<String>,
    pub permissions: Vec<String>,
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

/// A config entry declaration for parsing (ADR-043)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntryDecl {
    pub key: String,
    pub value: String,
    #[serde(default = "default_config_type")]
    pub config_type: String,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

fn default_config_type() -> String {
    "string".to_string()
}

/// A config store declaration (ADR-043)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDecl {
    pub name: String,
    pub product: String,
    pub entries: Vec<ConfigEntryDecl>,
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

/// A feature flag declaration (ADR-044)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagDecl {
    pub name: String,
    pub product: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub version: String,
    /// Tags attached to this resource (ADR-036)
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

impl ResourceFile {
    /// Parse a `.picloud` resource file from either JSON or Bicep-inspired syntax.
    ///
    /// Detection: if the trimmed input starts with `{`, it is treated as JSON.
    /// Otherwise it is parsed as the Bicep-inspired syntax described in ADR-007.
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        if trimmed.starts_with('{') {
            Self::parse_json(trimmed)
        } else {
            parse_bicep(trimmed)
        }
    }

    /// Parse a `.picloud` resource file from JSON string.
    fn parse_json(json: &str) -> Result<Self> {
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

        // Collect all volume names declared in this file, keyed by product.
        let mut volume_names: HashMap<&str, Vec<&str>> = HashMap::new();
        for decl in &self.resources {
            if let ResourceDeclaration::Volume(v) = decl {
                volume_names
                    .entry(v.product.as_str())
                    .or_default()
                    .push(v.name.as_str());
            }
        }

        // Check that every container/binary mount references a volume declared in this file.
        for decl in &self.resources {
            let (name, product, mounts) = match decl {
                ResourceDeclaration::Container(c) => {
                    (c.name.as_str(), c.product.as_str(), &c.mounts)
                }
                ResourceDeclaration::Binary(b) => {
                    (b.name.as_str(), b.product.as_str(), &b.mounts)
                }
                _ => continue,
            };

            let product_volumes = volume_names.get(product).cloned().unwrap_or_default();
            for mount in mounts {
                if !product_volumes.contains(&mount.volume.as_str()) {
                    return Err(PiCloudError::ResourceValidationFailed {
                        reason: format!(
                            "{}: mount references volume '{}' which is not declared in this file for product '{}'",
                            name, mount.volume, product
                        ),
                    });
                }
            }
        }

        Ok(())
    }

    /// Sort resources in dependency order for provisioning.
    ///
    /// Order: products first, then volumes, then containers/binaries,
    /// then everything else. This ensures volumes are provisioned before
    /// the containers that mount them.
    pub fn sort_for_provisioning(&mut self) {
        self.resources.sort_by_key(|decl| match decl {
            ResourceDeclaration::Product(_) => 0,
            ResourceDeclaration::Volume(_) => 1,
            ResourceDeclaration::Container(_) | ResourceDeclaration::Binary(_) => 2,
            _ => 3,
        });
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
            ResourceDeclaration::Config(c) => {
                if c.name.is_empty() || c.product.is_empty() {
                    return Err(validation_err("config", "name and product cannot be empty"));
                }
                for entry in &c.entries {
                    if entry.key.is_empty() {
                        return Err(validation_err(&c.name, "config entry key cannot be empty"));
                    }
                }
                Ok(())
            }
            ResourceDeclaration::FeatureFlag(f) => {
                if f.name.is_empty() || f.product.is_empty() || f.version.is_empty() {
                    return Err(validation_err("feature-flag", "name, product, and version cannot be empty"));
                }
                if crate::resources::VersionOp::parse(&f.version).is_none() {
                    return Err(validation_err(&f.name, &format!("invalid version expression: '{}'", f.version)));
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
            ResourceDeclaration::Config(c) => Some(&c.product),
            ResourceDeclaration::FeatureFlag(f) => Some(&f.product),
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
            ResourceDeclaration::Config(c) => &c.name,
            ResourceDeclaration::FeatureFlag(f) => &f.name,
        }
    }

    /// Get the tags for this resource (ADR-036).
    pub fn tags(&self) -> &HashMap<String, String> {
        match self {
            ResourceDeclaration::Product(p) => &p.tags,
            ResourceDeclaration::Volume(v) => &v.tags,
            ResourceDeclaration::Container(c) => &c.tags,
            ResourceDeclaration::Binary(b) => &b.tags,
            ResourceDeclaration::EventSubscription(e) => &e.tags,
            ResourceDeclaration::Ingress(i) => &i.tags,
            ResourceDeclaration::Secret(s) => &s.tags,
            ResourceDeclaration::Role(r) => &r.tags,
            ResourceDeclaration::Config(c) => &c.tags,
            ResourceDeclaration::FeatureFlag(f) => &f.tags,
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
            ResourceDeclaration::Config(_) => "config",
            ResourceDeclaration::FeatureFlag(_) => "feature-flag",
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

// ---------------------------------------------------------------------------
// Bicep-inspired syntax parser
// ---------------------------------------------------------------------------

/// Parse a `.picloud` file written in the Bicep-inspired syntax.
///
/// Each resource declaration looks like:
/// ```text
/// resource_type 'name' = {
///   key: 'value'
///   key: 123
///   key: [ ... ]
/// }
/// ```
///
/// Supported resource types: product, volume, container, binary,
/// event-subscription, ingress, secret, role.
pub fn parse_bicep(input: &str) -> Result<ResourceFile> {
    let declarations = split_bicep_declarations(input)?;
    let mut resources = Vec::new();
    for (resource_type, name, body) in declarations {
        let decl = parse_bicep_declaration(&resource_type, &name, &body)?;
        resources.push(decl);
    }
    Ok(ResourceFile { resources })
}

/// Split the input into individual `(type, name, body)` declarations.
fn split_bicep_declarations(input: &str) -> Result<Vec<(String, String, String)>> {
    let mut results = Vec::new();
    let mut pos = 0;
    let input_bytes = input.as_bytes();

    while pos < input.len() {
        // Skip whitespace and comments
        while pos < input.len() && input_bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= input.len() {
            break;
        }
        // Skip line comments
        if pos + 1 < input.len() && &input[pos..pos + 2] == "//" {
            while pos < input.len() && input_bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }

        // Read resource type keyword
        let type_start = pos;
        while pos < input.len() && !input_bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let resource_type = input[type_start..pos].to_string();

        // Skip whitespace
        while pos < input.len() && input_bytes[pos].is_ascii_whitespace() && input_bytes[pos] != b'\n' {
            pos += 1;
        }

        // Read quoted name
        let name = read_quoted_string(input, &mut pos)?;

        // Skip whitespace and '='
        while pos < input.len() && (input_bytes[pos].is_ascii_whitespace() || input_bytes[pos] == b'=') {
            pos += 1;
        }

        // Read body between { ... }
        if pos >= input.len() || input_bytes[pos] != b'{' {
            return Err(PiCloudError::ResourceValidationFailed {
                reason: format!("expected '{{' after {} '{}'", resource_type, name),
            });
        }
        let body = read_braced_block(input, &mut pos)?;

        results.push((resource_type, name, body));
    }

    Ok(results)
}

/// Read a single-quoted string starting at `pos`, advancing past the closing quote.
fn read_quoted_string(input: &str, pos: &mut usize) -> Result<String> {
    let bytes = input.as_bytes();
    if *pos >= input.len() || (bytes[*pos] != b'\'' && bytes[*pos] != b'"') {
        return Err(PiCloudError::ResourceValidationFailed {
            reason: format!("expected quoted string at position {}", pos),
        });
    }
    let quote_char = bytes[*pos];
    *pos += 1;
    let start = *pos;
    while *pos < input.len() && bytes[*pos] != quote_char {
        *pos += 1;
    }
    if *pos >= input.len() {
        return Err(PiCloudError::ResourceValidationFailed {
            reason: "unterminated string".to_string(),
        });
    }
    let s = input[start..*pos].to_string();
    *pos += 1; // skip closing quote
    Ok(s)
}

/// Read a balanced `{ ... }` block, returning the inner content (without outer braces).
fn read_braced_block(input: &str, pos: &mut usize) -> Result<String> {
    let bytes = input.as_bytes();
    if bytes[*pos] != b'{' {
        return Err(PiCloudError::ResourceValidationFailed {
            reason: "expected '{'".to_string(),
        });
    }
    *pos += 1;
    let start = *pos;
    let mut depth = 1u32;
    while *pos < input.len() && depth > 0 {
        match bytes[*pos] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'\'' | b'"' => {
                // Skip over quoted strings so braces inside them don't confuse us
                let q = bytes[*pos];
                *pos += 1;
                while *pos < input.len() && bytes[*pos] != q {
                    *pos += 1;
                }
                // pos will be incremented below
            }
            _ => {}
        }
        if depth > 0 {
            *pos += 1;
        }
    }
    if depth != 0 {
        return Err(PiCloudError::ResourceValidationFailed {
            reason: "unterminated block — missing '}'".to_string(),
        });
    }
    let body = input[start..*pos].to_string();
    *pos += 1; // skip closing '}'
    Ok(body)
}

/// Parse the key-value body of one Bicep declaration into a `ResourceDeclaration`.
fn parse_bicep_declaration(
    resource_type: &str,
    name: &str,
    body: &str,
) -> Result<ResourceDeclaration> {
    let kv = parse_bicep_kv(body)?;

    match resource_type {
        "product" => {
            Ok(ResourceDeclaration::Product(ProductDecl {
                name: name.to_string(),
                version: get_string_required(&kv, "version", name)?,
                description: get_string_optional(&kv, "description"),
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        "volume" => {
            Ok(ResourceDeclaration::Volume(VolumeDecl {
                name: name.to_string(),
                product: get_string_required(&kv, "product", name)?,
                size_gb: get_u64_or_default(&kv, "size_gb", 10),
                durability: get_string_optional(&kv, "durability"),
                performance: get_string_optional(&kv, "performance"),
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        "container" => {
            let mounts = get_mount_array(&kv, "mounts")?;
            let env = get_env_map(&kv, "env")?;
            let ports = get_u16_array(&kv, "ports")?;
            Ok(ResourceDeclaration::Container(ContainerDecl {
                name: name.to_string(),
                product: get_string_required(&kv, "product", name)?,
                image: get_string_required(&kv, "image", name)?,
                identity: get_string_optional(&kv, "identity").unwrap_or_else(|| "default".to_string()),
                cpu: get_string_optional(&kv, "cpu"),
                memory: get_string_optional(&kv, "memory"),
                mounts,
                env,
                ports,
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        "binary" => {
            let mounts = get_mount_array(&kv, "mounts")?;
            let env = get_env_map(&kv, "env")?;
            let args = get_string_array(&kv, "args")?;
            Ok(ResourceDeclaration::Binary(BinaryDecl {
                name: name.to_string(),
                product: get_string_required(&kv, "product", name)?,
                executable: get_string_required(&kv, "executable", name)?,
                args,
                identity: get_string_optional(&kv, "identity").unwrap_or_else(|| "default".to_string()),
                cpu: get_string_optional(&kv, "cpu"),
                memory: get_string_optional(&kv, "memory"),
                mounts,
                env,
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        "event-subscription" => {
            Ok(ResourceDeclaration::EventSubscription(EventSubscriptionDecl {
                name: name.to_string(),
                product: get_string_required(&kv, "product", name)?,
                source: get_string_required(&kv, "source", name)?,
                event: get_string_required(&kv, "event", name)?,
                handler: get_string_required(&kv, "handler", name)?,
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        "ingress" => {
            Ok(ResourceDeclaration::Ingress(IngressDecl {
                name: name.to_string(),
                product: get_string_required(&kv, "product", name)?,
                target: get_string_required(&kv, "target", name)?,
                port: get_u64_or_default(&kv, "port", 0) as u16,
                path: get_string_required(&kv, "path", name)?,
                tls: get_bool_or_default(&kv, "tls", true),
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        "secret" => {
            Ok(ResourceDeclaration::Secret(SecretDecl {
                name: name.to_string(),
                product: get_string_required(&kv, "product", name)?,
                value: get_string_required(&kv, "value", name)?,
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        "role" => {
            let permissions = get_string_array(&kv, "permissions")?;
            Ok(ResourceDeclaration::Role(RoleDecl {
                name: name.to_string(),
                product: get_string_optional(&kv, "product"),
                permissions,
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        "config" => {
            let entries = get_config_entries(&kv, "entries")?;
            Ok(ResourceDeclaration::Config(ConfigDecl {
                name: name.to_string(),
                product: get_string_required(&kv, "product", name)?,
                entries,
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        "feature-flag" => {
            Ok(ResourceDeclaration::FeatureFlag(FeatureFlagDecl {
                name: name.to_string(),
                product: get_string_required(&kv, "product", name)?,
                description: get_string_optional(&kv, "description"),
                enabled: get_bool_or_default(&kv, "enabled", true),
                version: get_string_required(&kv, "version", name)?,
                tags: get_tags_map(&kv, "tags"),
            }))
        }
        _ => Err(PiCloudError::ResourceValidationFailed {
            reason: format!("unknown resource type: {}", resource_type),
        }),
    }
}

/// A parsed Bicep value — either a string, a number, a bool, an array, or an object (sub-block).
#[derive(Debug, Clone)]
enum BicepValue {
    Str(String),
    Num(f64),
    Bool(bool),
    Array(Vec<BicepValue>),
    Object(Vec<(String, BicepValue)>),
}

/// Parse top-level key: value pairs from a Bicep block body.
fn parse_bicep_kv(body: &str) -> Result<Vec<(String, BicepValue)>> {
    let mut pairs = Vec::new();
    let mut pos = 0usize;
    let bytes = body.as_bytes();

    while pos < body.len() {
        skip_ws_and_comments(body, &mut pos);
        if pos >= body.len() {
            break;
        }

        // Skip commas between key-value pairs (for inline object syntax)
        if bytes[pos] == b',' {
            pos += 1;
            continue;
        }

        // Read key — may be quoted ('key' or "key") or bare (key)
        let key = if pos < body.len() && (bytes[pos] == b'\'' || bytes[pos] == b'"') {
            read_quoted_string(body, &mut pos)?
        } else {
            let key_start = pos;
            while pos < body.len() && bytes[pos] != b':' && !bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            body[key_start..pos].trim().to_string()
        };
        if key.is_empty() {
            break;
        }

        // Skip to colon
        while pos < body.len() && bytes[pos] != b':' {
            pos += 1;
        }
        if pos >= body.len() {
            break;
        }
        pos += 1; // skip ':'

        skip_ws_inline(body, &mut pos);

        // Parse value
        let value = parse_bicep_value(body, &mut pos)?;
        pairs.push((key, value));
    }

    Ok(pairs)
}

fn skip_ws_and_comments(input: &str, pos: &mut usize) {
    let bytes = input.as_bytes();
    loop {
        while *pos < input.len() && bytes[*pos].is_ascii_whitespace() {
            *pos += 1;
        }
        if *pos + 1 < input.len() && &input[*pos..*pos + 2] == "//" {
            while *pos < input.len() && bytes[*pos] != b'\n' {
                *pos += 1;
            }
        } else {
            break;
        }
    }
}

fn skip_ws_inline(input: &str, pos: &mut usize) {
    let bytes = input.as_bytes();
    while *pos < input.len() && (bytes[*pos] == b' ' || bytes[*pos] == b'\t') {
        *pos += 1;
    }
}

fn parse_bicep_value(input: &str, pos: &mut usize) -> Result<BicepValue> {
    let bytes = input.as_bytes();
    skip_ws_and_comments(input, pos);
    if *pos >= input.len() {
        return Err(PiCloudError::ResourceValidationFailed {
            reason: "unexpected end of input while parsing value".to_string(),
        });
    }

    match bytes[*pos] {
        b'\'' | b'"' => {
            let s = read_quoted_string(input, pos)?;
            Ok(BicepValue::Str(s))
        }
        b'[' => {
            *pos += 1; // skip '['
            let mut items = Vec::new();
            loop {
                skip_ws_and_comments(input, pos);
                if *pos >= input.len() {
                    return Err(PiCloudError::ResourceValidationFailed {
                        reason: "unterminated array".to_string(),
                    });
                }
                if bytes[*pos] == b']' {
                    *pos += 1;
                    break;
                }
                // Skip optional commas
                if bytes[*pos] == b',' {
                    *pos += 1;
                    continue;
                }
                let val = parse_bicep_value(input, pos)?;
                items.push(val);
            }
            Ok(BicepValue::Array(items))
        }
        b'{' => {
            let block = read_braced_block(input, pos)?;
            let kv = parse_bicep_kv(&block)?;
            Ok(BicepValue::Object(kv))
        }
        _ => {
            // Number, bool, or unquoted identifier
            let start = *pos;
            while *pos < input.len()
                && !bytes[*pos].is_ascii_whitespace()
                && bytes[*pos] != b','
                && bytes[*pos] != b']'
                && bytes[*pos] != b'}'
            {
                *pos += 1;
            }
            let token = input[start..*pos].trim();
            if token == "true" {
                Ok(BicepValue::Bool(true))
            } else if token == "false" {
                Ok(BicepValue::Bool(false))
            } else if let Ok(n) = token.parse::<f64>() {
                Ok(BicepValue::Num(n))
            } else {
                // Treat as unquoted string
                Ok(BicepValue::Str(token.to_string()))
            }
        }
    }
}

// --- Helpers to extract typed values from parsed key-value pairs ---

fn get_string_required(kv: &[(String, BicepValue)], key: &str, resource: &str) -> Result<String> {
    for (k, v) in kv {
        if k == key {
            return match v {
                BicepValue::Str(s) => Ok(s.clone()),
                BicepValue::Num(n) => Ok(n.to_string()),
                _ => Err(PiCloudError::ResourceValidationFailed {
                    reason: format!("{}: expected string for key '{}'", resource, key),
                }),
            };
        }
    }
    Err(PiCloudError::ResourceValidationFailed {
        reason: format!("{}: missing required key '{}'", resource, key),
    })
}

fn get_string_optional(kv: &[(String, BicepValue)], key: &str) -> Option<String> {
    for (k, v) in kv {
        if k == key {
            return match v {
                BicepValue::Str(s) => Some(s.clone()),
                BicepValue::Num(n) => Some(n.to_string()),
                _ => None,
            };
        }
    }
    None
}

fn get_u64_or_default(kv: &[(String, BicepValue)], key: &str, default: u64) -> u64 {
    for (k, v) in kv {
        if k == key {
            return match v {
                BicepValue::Num(n) => *n as u64,
                BicepValue::Str(s) => s.parse().unwrap_or(default),
                _ => default,
            };
        }
    }
    default
}

fn get_bool_or_default(kv: &[(String, BicepValue)], key: &str, default: bool) -> bool {
    for (k, v) in kv {
        if k == key {
            return match v {
                BicepValue::Bool(b) => *b,
                BicepValue::Str(s) => s == "true",
                _ => default,
            };
        }
    }
    default
}

fn get_string_array(kv: &[(String, BicepValue)], key: &str) -> Result<Vec<String>> {
    for (k, v) in kv {
        if k == key {
            return match v {
                BicepValue::Array(items) => {
                    items.iter().map(|item| match item {
                        BicepValue::Str(s) => Ok(s.clone()),
                        BicepValue::Num(n) => Ok(n.to_string()),
                        _ => Err(PiCloudError::ResourceValidationFailed {
                            reason: format!("expected string in array '{}'", key),
                        }),
                    }).collect()
                }
                _ => Err(PiCloudError::ResourceValidationFailed {
                    reason: format!("expected array for key '{}'", key),
                }),
            };
        }
    }
    Ok(Vec::new())
}

fn get_u16_array(kv: &[(String, BicepValue)], key: &str) -> Result<Vec<u16>> {
    for (k, v) in kv {
        if k == key {
            return match v {
                BicepValue::Array(items) => {
                    items.iter().map(|item| match item {
                        BicepValue::Num(n) => Ok(*n as u16),
                        BicepValue::Str(s) => s.parse().map_err(|_| PiCloudError::ResourceValidationFailed {
                            reason: format!("invalid number in array '{}'", key),
                        }),
                        _ => Err(PiCloudError::ResourceValidationFailed {
                            reason: format!("expected number in array '{}'", key),
                        }),
                    }).collect()
                }
                _ => Err(PiCloudError::ResourceValidationFailed {
                    reason: format!("expected array for key '{}'", key),
                }),
            };
        }
    }
    Ok(Vec::new())
}

fn get_mount_array(kv: &[(String, BicepValue)], key: &str) -> Result<Vec<MountDecl>> {
    for (k, v) in kv {
        if k == key {
            return match v {
                BicepValue::Array(items) => {
                    items.iter().map(|item| match item {
                        BicepValue::Object(obj) => {
                            Ok(MountDecl {
                                volume: get_string_required(obj, "volume", "mount")?,
                                path: get_string_required(obj, "path", "mount")?,
                                read_only: get_bool_or_default(obj, "read_only", false),
                            })
                        }
                        _ => Err(PiCloudError::ResourceValidationFailed {
                            reason: "expected object in mounts array".to_string(),
                        }),
                    }).collect()
                }
                _ => Err(PiCloudError::ResourceValidationFailed {
                    reason: "expected array for 'mounts'".to_string(),
                }),
            };
        }
    }
    Ok(Vec::new())
}

fn get_env_map(kv: &[(String, BicepValue)], key: &str) -> Result<HashMap<String, serde_json::Value>> {
    for (k, v) in kv {
        if k == key {
            return match v {
                BicepValue::Object(obj) => {
                    let mut map = HashMap::new();
                    for (ek, ev) in obj {
                        let jv = bicep_value_to_json(ev);
                        map.insert(ek.clone(), jv);
                    }
                    Ok(map)
                }
                _ => Err(PiCloudError::ResourceValidationFailed {
                    reason: "expected object for 'env'".to_string(),
                }),
            };
        }
    }
    Ok(HashMap::new())
}

/// Extract a `HashMap<String, String>` from a Bicep key-value block (for tags).
///
/// In Bicep syntax, keys inside object blocks may be quoted (`'team': 'backend'`),
/// so we strip surrounding quotes from the key names.
fn get_tags_map(kv: &[(String, BicepValue)], key: &str) -> HashMap<String, String> {
    for (k, v) in kv {
        if k == key {
            if let BicepValue::Object(obj) = v {
                let mut map = HashMap::new();
                for (tk, tv) in obj {
                    let clean_key = tk
                        .trim_start_matches('\'')
                        .trim_end_matches('\'')
                        .trim_start_matches('"')
                        .trim_end_matches('"')
                        .to_string();
                    if let BicepValue::Str(s) = tv {
                        map.insert(clean_key, s.clone());
                    }
                }
                return map;
            }
        }
    }
    HashMap::new()
}

fn get_config_entries(kv: &[(String, BicepValue)], key: &str) -> Result<Vec<ConfigEntryDecl>> {
    for (k, v) in kv {
        if k == key {
            return match v {
                BicepValue::Array(items) => {
                    items.iter().map(|item| match item {
                        BicepValue::Object(obj) => {
                            Ok(ConfigEntryDecl {
                                key: get_string_required(obj, "key", "config-entry")?,
                                value: get_string_required(obj, "value", "config-entry")?,
                                config_type: get_string_optional(obj, "type").unwrap_or_else(|| "string".to_string()),
                                tags: get_tags_map(obj, "tags"),
                            })
                        }
                        _ => Err(PiCloudError::ResourceValidationFailed {
                            reason: "expected object in entries array".to_string(),
                        }),
                    }).collect()
                }
                _ => Err(PiCloudError::ResourceValidationFailed {
                    reason: "expected array for 'entries'".to_string(),
                }),
            };
        }
    }
    Ok(Vec::new())
}

fn bicep_value_to_json(v: &BicepValue) -> serde_json::Value {
    match v {
        BicepValue::Str(s) => serde_json::Value::String(s.clone()),
        BicepValue::Num(n) => serde_json::json!(*n),
        BicepValue::Bool(b) => serde_json::Value::Bool(*b),
        BicepValue::Array(items) => {
            serde_json::Value::Array(items.iter().map(bicep_value_to_json).collect())
        }
        BicepValue::Object(kv) => {
            let mut map = serde_json::Map::new();
            for (k, v) in kv {
                map.insert(k.clone(), bicep_value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
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
    fn validation_rejects_mount_referencing_undeclared_volume() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "container", "name": "api", "product": "app", "image": "img:1",
                  "mounts": [{ "volume": "missing-vol", "path": "/data" }] }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        let err = file.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing-vol"), "error should mention the missing volume: {msg}");
    }

    #[test]
    fn validation_accepts_mount_referencing_declared_volume() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "volume", "name": "data", "product": "app", "size_gb": 10 },
                { "type": "container", "name": "api", "product": "app", "image": "img:1",
                  "mounts": [{ "volume": "data", "path": "/data" }] }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        file.validate().unwrap();
    }

    #[test]
    fn validation_rejects_mount_referencing_volume_from_different_product() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app-a", "version": "1.0.0" },
                { "type": "product", "name": "app-b", "version": "1.0.0" },
                { "type": "volume", "name": "data", "product": "app-a", "size_gb": 10 },
                { "type": "container", "name": "api", "product": "app-b", "image": "img:1",
                  "mounts": [{ "volume": "data", "path": "/data" }] }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert!(file.validate().is_err());
    }

    #[test]
    fn sort_for_provisioning_orders_correctly() {
        let json = r#"{
            "resources": [
                { "type": "container", "name": "api", "product": "app", "image": "img:1" },
                { "type": "ingress", "name": "web", "product": "app", "target": "api", "port": 8080, "path": "/" },
                { "type": "volume", "name": "data", "product": "app", "size_gb": 10 },
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "binary", "name": "worker", "product": "app", "executable": "worker" }
            ]
        }"#;
        let mut file = ResourceFile::parse(json).unwrap();
        file.sort_for_provisioning();

        let types: Vec<&str> = file.resources.iter().map(|r| r.resource_type()).collect();
        assert_eq!(types, vec!["product", "volume", "container", "binary", "ingress"]);
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

    // ---- Bicep syntax tests ----

    #[test]
    fn bicep_parse_minimal_product() {
        let input = r#"
product 'my-app' = {
  version: '1.0.0'
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        assert_eq!(file.resources.len(), 1);
        match &file.resources[0] {
            ResourceDeclaration::Product(p) => {
                assert_eq!(p.name, "my-app");
                assert_eq!(p.version, "1.0.0");
                assert!(p.description.is_none());
            }
            _ => panic!("expected Product"),
        }
        file.validate().unwrap();
    }

    #[test]
    fn bicep_parse_product_with_description() {
        let input = r#"
product 'photo-app' = {
  version: '1.0.0'
  description: 'Photo sharing app'
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        match &file.resources[0] {
            ResourceDeclaration::Product(p) => {
                assert_eq!(p.name, "photo-app");
                assert_eq!(p.description, Some("Photo sharing app".to_string()));
            }
            _ => panic!("expected Product"),
        }
    }

    #[test]
    fn bicep_parse_full_example() {
        let input = r#"
product 'photo-app' = {
  version: '1.0.0'
  description: 'Photo sharing app'
}

volume 'media-store' = {
  product: 'photo-app'
  size_gb: 100
  durability: 'full-replication'
}

container 'api-server' = {
  product: 'photo-app'
  image: 'photo-api:1.0.0'
  identity: 'api-worker'
  mounts: [
    { volume: 'media-store', path: '/data' }
  ]
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        assert_eq!(file.resources.len(), 3);
        file.validate().unwrap();

        match &file.resources[1] {
            ResourceDeclaration::Volume(v) => {
                assert_eq!(v.name, "media-store");
                assert_eq!(v.product, "photo-app");
                assert_eq!(v.size_gb, 100);
                assert_eq!(v.durability, Some("full-replication".to_string()));
            }
            _ => panic!("expected Volume"),
        }

        match &file.resources[2] {
            ResourceDeclaration::Container(c) => {
                assert_eq!(c.name, "api-server");
                assert_eq!(c.image, "photo-api:1.0.0");
                assert_eq!(c.identity, "api-worker");
                assert_eq!(c.mounts.len(), 1);
                assert_eq!(c.mounts[0].volume, "media-store");
                assert_eq!(c.mounts[0].path, "/data");
            }
            _ => panic!("expected Container"),
        }
    }

    #[test]
    fn bicep_parse_container_with_ports_and_env() {
        let input = r#"
product 'app' = {
  version: '1.0.0'
}

container 'web' = {
  product: 'app'
  image: 'web:latest'
  ports: [8080, 9090]
  env: {
    LOG_LEVEL: 'debug'
  }
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        file.validate().unwrap();

        match &file.resources[1] {
            ResourceDeclaration::Container(c) => {
                assert_eq!(c.ports, vec![8080, 9090]);
                assert_eq!(
                    c.env.get("LOG_LEVEL"),
                    Some(&serde_json::Value::String("debug".to_string()))
                );
            }
            _ => panic!("expected Container"),
        }
    }

    #[test]
    fn bicep_parse_binary() {
        let input = r#"
product 'app' = {
  version: '1.0.0'
}

binary 'worker' = {
  product: 'app'
  executable: 'worker-arm64'
  args: ['--port', '9090']
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        file.validate().unwrap();

        match &file.resources[1] {
            ResourceDeclaration::Binary(b) => {
                assert_eq!(b.executable, "worker-arm64");
                assert_eq!(b.args, vec!["--port", "9090"]);
            }
            _ => panic!("expected Binary"),
        }
    }

    #[test]
    fn bicep_parse_ingress() {
        let input = r#"
product 'app' = {
  version: '1.0.0'
}

ingress 'web-ingress' = {
  product: 'app'
  target: 'web'
  port: 8080
  path: '/api'
  tls: false
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        file.validate().unwrap();

        match &file.resources[1] {
            ResourceDeclaration::Ingress(i) => {
                assert_eq!(i.target, "web");
                assert_eq!(i.port, 8080);
                assert!(!i.tls);
            }
            _ => panic!("expected Ingress"),
        }
    }

    #[test]
    fn bicep_parse_secret() {
        let input = r#"
product 'app' = {
  version: '1.0.0'
}

secret 'db-password' = {
  product: 'app'
  value: 'super-secret-123'
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        file.validate().unwrap();

        match &file.resources[1] {
            ResourceDeclaration::Secret(s) => {
                assert_eq!(s.name, "db-password");
                assert_eq!(s.value, "super-secret-123");
            }
            _ => panic!("expected Secret"),
        }
    }

    #[test]
    fn bicep_parse_role() {
        let input = r#"
role 'admin' = {
  product: 'my-app'
  permissions: ['read', 'write', 'delete']
}
"#;
        // Roles don't require a product declaration in the same file if product is optional
        let file = parse_bicep(input).unwrap();
        match &file.resources[0] {
            ResourceDeclaration::Role(r) => {
                assert_eq!(r.name, "admin");
                assert_eq!(r.product, Some("my-app".to_string()));
                assert_eq!(r.permissions, vec!["read", "write", "delete"]);
            }
            _ => panic!("expected Role"),
        }
    }

    #[test]
    fn bicep_rejects_unknown_resource_type() {
        let input = r#"
foobar 'test' = {
  version: '1.0.0'
}
"#;
        let result = ResourceFile::parse(input);
        assert!(result.is_err());
    }

    #[test]
    fn bicep_json_detection_still_works() {
        // JSON input should still be parsed as JSON
        let json = r#"{ "resources": [{ "type": "product", "name": "app", "version": "1.0.0" }] }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert_eq!(file.resources.len(), 1);
    }

    #[test]
    fn bicep_parse_event_subscription() {
        let input = r#"
product 'photo-app' = {
  version: '1.0.0'
}

event-subscription 'on-upload' = {
  product: 'photo-app'
  source: 'upload-service@1.0.0'
  event: 'FileUploaded'
  handler: 'api-server'
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        file.validate().unwrap();

        match &file.resources[1] {
            ResourceDeclaration::EventSubscription(e) => {
                assert_eq!(e.name, "on-upload");
                assert_eq!(e.event, "FileUploaded");
            }
            _ => panic!("expected EventSubscription"),
        }
    }

    #[test]
    fn parse_json_with_tags() {
        let json = r#"{
            "resources": [
                {
                    "type": "product",
                    "name": "photo-app",
                    "version": "1.0.0",
                    "tags": { "team": "backend", "environment": "production" }
                },
                {
                    "type": "container",
                    "name": "api-server",
                    "product": "photo-app",
                    "image": "photo-api:1.0.0",
                    "tags": { "tier": "api" }
                }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        file.validate().unwrap();

        match &file.resources[0] {
            ResourceDeclaration::Product(p) => {
                assert_eq!(p.tags.len(), 2);
                assert_eq!(p.tags.get("team").unwrap(), "backend");
                assert_eq!(p.tags.get("environment").unwrap(), "production");
            }
            _ => panic!("expected Product"),
        }

        match &file.resources[1] {
            ResourceDeclaration::Container(c) => {
                assert_eq!(c.tags.len(), 1);
                assert_eq!(c.tags.get("tier").unwrap(), "api");
            }
            _ => panic!("expected Container"),
        }
    }

    #[test]
    fn parse_json_without_tags_defaults_empty() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "my-app", "version": "1.0.0" }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        match &file.resources[0] {
            ResourceDeclaration::Product(p) => {
                assert!(p.tags.is_empty());
            }
            _ => panic!("expected Product"),
        }
    }

    #[test]
    fn bicep_parse_with_tags() {
        let input = r#"
            product 'photo-app' = {
                version: '1.0.0'
                tags: {
                    'team': 'backend'
                    'environment': 'production'
                }
            }
        "#;
        let file = ResourceFile::parse(input).unwrap();
        file.validate().unwrap();

        match &file.resources[0] {
            ResourceDeclaration::Product(p) => {
                assert_eq!(p.tags.len(), 2);
                assert_eq!(p.tags.get("team").unwrap(), "backend");
                assert_eq!(p.tags.get("environment").unwrap(), "production");
            }
            _ => panic!("expected Product"),
        }
    }

    #[test]
    fn resource_declaration_tags_accessor() {
        let json = r#"{
            "resources": [
                {
                    "type": "volume",
                    "name": "data",
                    "product": "my-app",
                    "size_gb": 50,
                    "tags": { "tier": "storage" }
                },
                {
                    "type": "product",
                    "name": "my-app",
                    "version": "1.0.0"
                }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();

        let tags = file.resources[0].tags();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags.get("tier").unwrap(), "storage");

        let tags = file.resources[1].tags();
        assert!(tags.is_empty());
    }

    // ---- Config store tests (ADR-043) ----

    #[test]
    fn parse_config_json() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "photo-app", "version": "1.0.0" },
                { "type": "config", "name": "app-config", "product": "photo-app",
                  "entries": [
                    { "key": "cache.ttl", "value": "300", "config_type": "int", "tags": { "tier": "cache" } },
                    { "key": "api.url", "value": "https://api.local", "config_type": "string" }
                  ] }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        file.validate().unwrap();
        assert_eq!(file.resources.len(), 2);
        match &file.resources[1] {
            ResourceDeclaration::Config(c) => {
                assert_eq!(c.name, "app-config");
                assert_eq!(c.product, "photo-app");
                assert_eq!(c.entries.len(), 2);
                assert_eq!(c.entries[0].key, "cache.ttl");
                assert_eq!(c.entries[0].value, "300");
                assert_eq!(c.entries[0].config_type, "int");
                assert_eq!(c.entries[0].tags.get("tier").unwrap(), "cache");
                assert_eq!(c.entries[1].key, "api.url");
                assert_eq!(c.entries[1].config_type, "string");
            }
            _ => panic!("expected Config"),
        }
    }

    #[test]
    fn parse_config_bicep() {
        let input = r#"
product 'photo-app' = {
  version: '1.0.0'
}

config 'app-config' = {
  product: 'photo-app'
  entries: [
    { key: 'cache.ttl', value: '300', type: 'int' }
    { key: 'api.url', value: 'https://api.local', type: 'string' }
  ]
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        file.validate().unwrap();
        assert_eq!(file.resources.len(), 2);
        match &file.resources[1] {
            ResourceDeclaration::Config(c) => {
                assert_eq!(c.name, "app-config");
                assert_eq!(c.entries.len(), 2);
                assert_eq!(c.entries[0].key, "cache.ttl");
                assert_eq!(c.entries[0].value, "300");
                assert_eq!(c.entries[0].config_type, "int");
            }
            _ => panic!("expected Config"),
        }
    }

    #[test]
    fn config_validation_rejects_empty_name() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "config", "name": "", "product": "app", "entries": [] }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert!(file.validate().is_err());
    }

    #[test]
    fn config_validation_rejects_empty_entry_key() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "config", "name": "cfg", "product": "app",
                  "entries": [{ "key": "", "value": "x", "config_type": "string" }] }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert!(file.validate().is_err());
    }

    // ---- Feature flag tests (ADR-044) ----

    #[test]
    fn parse_feature_flag_json() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "photo-app", "version": "2.0.0" },
                { "type": "feature-flag", "name": "new-upload-flow", "product": "photo-app",
                  "description": "Enables the redesigned upload flow",
                  "enabled": true, "version": "= 2" }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        file.validate().unwrap();
        match &file.resources[1] {
            ResourceDeclaration::FeatureFlag(f) => {
                assert_eq!(f.name, "new-upload-flow");
                assert_eq!(f.product, "photo-app");
                assert_eq!(f.description, Some("Enables the redesigned upload flow".to_string()));
                assert!(f.enabled);
                assert_eq!(f.version, "= 2");
            }
            _ => panic!("expected FeatureFlag"),
        }
    }

    #[test]
    fn parse_feature_flag_bicep() {
        let input = r#"
product 'photo-app' = {
  version: '2.0.0'
}

feature-flag 'new-upload-flow' = {
  product: 'photo-app'
  description: 'Enables the redesigned upload flow'
  enabled: true
  version: '= 2'
}

feature-flag 'dark-mode' = {
  product: 'photo-app'
  description: 'Dark mode UI'
  enabled: true
  version: '>= 2'
}
"#;
        let file = ResourceFile::parse(input).unwrap();
        file.validate().unwrap();
        assert_eq!(file.resources.len(), 3);

        match &file.resources[1] {
            ResourceDeclaration::FeatureFlag(f) => {
                assert_eq!(f.name, "new-upload-flow");
                assert!(f.enabled);
                assert_eq!(f.version, "= 2");
            }
            _ => panic!("expected FeatureFlag"),
        }

        match &file.resources[2] {
            ResourceDeclaration::FeatureFlag(f) => {
                assert_eq!(f.name, "dark-mode");
                assert_eq!(f.version, ">= 2");
            }
            _ => panic!("expected FeatureFlag"),
        }
    }

    #[test]
    fn feature_flag_validation_rejects_invalid_version_expr() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "feature-flag", "name": "f1", "product": "app",
                  "enabled": true, "version": "not-a-version" }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert!(file.validate().is_err());
    }

    #[test]
    fn feature_flag_validation_rejects_empty_version() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "feature-flag", "name": "f1", "product": "app",
                  "enabled": true, "version": "" }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert!(file.validate().is_err());
    }

    #[test]
    fn feature_flag_resource_type_name() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "feature-flag", "name": "f1", "product": "app",
                  "enabled": true, "version": "= 1" }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert_eq!(file.resources[1].resource_type(), "feature-flag");
        assert_eq!(file.resources[1].resource_name(), "f1");
        assert_eq!(file.resources[1].product_name(), Some("app"));
    }

    #[test]
    fn config_resource_type_name() {
        let json = r#"{
            "resources": [
                { "type": "product", "name": "app", "version": "1.0.0" },
                { "type": "config", "name": "cfg", "product": "app", "entries": [] }
            ]
        }"#;
        let file = ResourceFile::parse(json).unwrap();
        assert_eq!(file.resources[1].resource_type(), "config");
        assert_eq!(file.resources[1].resource_name(), "cfg");
        assert_eq!(file.resources[1].product_name(), Some("app"));
    }
}
