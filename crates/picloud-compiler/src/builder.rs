/// Resource Builder
///
/// Generates .picloud files from structured input.
/// Used by `picloud new` -- accepts flags or interactive prompts,
/// produces a valid .picloud file, then runs validation.
///
/// ADR-050: Builder Pattern CLI for Resource Generation

use picloud_domain::error::{PiCloudError, Result};
use std::collections::HashMap;

/// A resource builder -- accumulates property values and renders to .picloud
pub struct ResourceBuilder {
    resource_type: String,
    name:          Option<String>,
    properties:    HashMap<String, BuilderValue>,
}

#[derive(Debug, Clone)]
pub enum BuilderValue {
    Single(String),
    Secret(String),
    List(Vec<String>),
    /// Key-value pairs (tags, env vars)
    Pairs(Vec<(String, String)>),
    /// Nested block properties
    Block(HashMap<String, BuilderValue>),
}

impl ResourceBuilder {
    pub fn new(resource_type: impl Into<String>) -> Result<Self> {
        let rt = resource_type.into();
        if !KNOWN_RESOURCE_TYPES.contains(&rt.as_str()) {
            return Err(PiCloudError::ResourceValidationFailed {
                reason: format!(
                    "Unknown resource type '{}'. Valid types: {}",
                    rt,
                    KNOWN_RESOURCE_TYPES.join(", ")
                ),
            });
        }
        Ok(Self {
            resource_type: rt,
            name:          None,
            properties:    HashMap::new(),
        })
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.properties.insert(key.into(), BuilderValue::Single(value.into()));
        self
    }

    pub fn secret(mut self, key: impl Into<String>, secret_name: impl Into<String>) -> Self {
        self.properties.insert(key.into(), BuilderValue::Secret(secret_name.into()));
        self
    }

    pub fn list(mut self, key: impl Into<String>, values: Vec<String>) -> Self {
        self.properties.insert(key.into(), BuilderValue::List(values));
        self
    }

    pub fn pairs(mut self, key: impl Into<String>, pairs: Vec<(String, String)>) -> Self {
        self.properties.insert(key.into(), BuilderValue::Pairs(pairs));
        self
    }

    /// Render to .picloud format string
    pub fn render(&self) -> Result<String> {
        let name = self.name.as_deref().ok_or_else(|| {
            PiCloudError::ResourceValidationFailed {
                reason: "Resource name is required".into(),
            }
        })?;

        let mut lines = Vec::new();
        lines.push(format!("{} \"{}\" {{", self.resource_type, name));

        // Sort keys for deterministic output
        let mut keys: Vec<&String> = self.properties.keys().collect();
        keys.sort();

        for key in keys {
            let value = &self.properties[key];
            render_property(&mut lines, key, value, 1);
        }

        lines.push("}".to_string());
        lines.push(String::new()); // trailing newline

        Ok(lines.join("\n"))
    }

    /// Validate required fields are present for this resource type
    pub fn validate_required(&self) -> Result<()> {
        let required = required_fields(&self.resource_type);
        let missing: Vec<&str> = required
            .iter()
            .filter(|f| !self.properties.contains_key(**f))
            .copied()
            .collect();

        if !missing.is_empty() {
            return Err(PiCloudError::ResourceValidationFailed {
                reason: format!(
                    "{} '{}' missing required fields: {}",
                    self.resource_type,
                    self.name.as_deref().unwrap_or("<unnamed>"),
                    missing.join(", ")
                ),
            });
        }
        Ok(())
    }

    /// Return the list of required fields for this resource type
    pub fn required_fields(&self) -> Vec<&'static str> {
        required_fields(&self.resource_type)
    }

    /// Return the list of optional fields for this resource type with descriptions
    pub fn optional_fields(&self) -> Vec<FieldDescriptor> {
        optional_fields(&self.resource_type)
    }
}

fn render_property(lines: &mut Vec<String>, key: &str, value: &BuilderValue, indent: usize) {
    let pad = "  ".repeat(indent);
    match value {
        BuilderValue::Single(v) => {
            lines.push(format!("{}{}  = \"{}\"", pad, key, v));
        }
        BuilderValue::Secret(s) => {
            lines.push(format!("{}{}  = secret(\"{}\")", pad, key, s));
        }
        BuilderValue::List(items) => {
            for item in items {
                lines.push(format!("{}{}  = \"{}\"", pad, key, item));
            }
        }
        BuilderValue::Pairs(pairs) => {
            for (k, v) in pairs {
                lines.push(format!("{}{} {{ key = \"{}\"; value = \"{}\" }}", pad, key, k, v));
            }
        }
        BuilderValue::Block(props) => {
            lines.push(format!("{}{} {{", pad, key));
            let mut keys: Vec<&String> = props.keys().collect();
            keys.sort();
            for k in keys {
                render_property(lines, k, &props[k], indent + 1);
            }
            lines.push(format!("{}}}", pad));
        }
    }
}

/// Descriptor for a resource field -- used to drive interactive prompts
#[derive(Debug, Clone)]
pub struct FieldDescriptor {
    pub name:        &'static str,
    pub description: &'static str,
    pub field_type:  FieldType,
    pub example:     Option<&'static str>,
}

#[derive(Debug, Clone)]
pub enum FieldType {
    String,
    Int,
    Bool,
    Secret,
    /// "volume:path" pairs
    Mount,
    /// "key=value" pairs
    TagList,
    /// One of a fixed set
    Enum(Vec<&'static str>),
}

/// All supported resource types
pub const KNOWN_RESOURCE_TYPES: &[&str] = &[
    "product",
    "container",
    "binary",
    "volume",
    "feature-flag",
    "config",
    "inference-rule",
    "event-store",
    "rdf-store",
    "ingress",
    "group",
    "event-subscription",
    "ontology",
    "role",
    "scope",
    "m2m-permission",
];

/// Required fields per resource type -- derived from SHACL sh:minCount = 1
fn required_fields(resource_type: &str) -> Vec<&'static str> {
    match resource_type {
        "product"            => vec!["version"],
        "container"          => vec!["product", "image"],
        "binary"             => vec!["product", "executable"],
        "volume"             => vec!["product", "size"],
        "feature-flag"       => vec!["product", "enabled", "version"],
        "config"             => vec!["product"],
        "inference-rule"     => vec!["product", "construct"],
        "event-store"        => vec!["product"],
        "rdf-store"          => vec!["product"],
        "ingress"            => vec!["product", "target", "port"],
        "group"              => vec![],
        "event-subscription" => vec!["product", "source", "event-type", "handler"],
        "ontology"           => vec!["product", "file"],
        "role"               => vec!["product"],
        "scope"              => vec!["product"],
        "m2m-permission"     => vec!["product", "client", "scopes"],
        _                    => vec![],
    }
}

fn optional_fields(resource_type: &str) -> Vec<FieldDescriptor> {
    match resource_type {
        "container" => vec![
            FieldDescriptor {
                name:        "identity",
                description: "Workload identity name",
                field_type:  FieldType::String,
                example:     Some("api-worker"),
            },
            FieldDescriptor {
                name:        "mount",
                description: "Volume to mount into the container",
                field_type:  FieldType::Mount,
                example:     Some("media-store:/data"),
            },
            FieldDescriptor {
                name:        "tag",
                description: "Resource tags (key=value)",
                field_type:  FieldType::TagList,
                example:     Some("team=backend"),
            },
            FieldDescriptor {
                name:        "cpu",
                description: "CPU limit in millicores",
                field_type:  FieldType::Int,
                example:     Some("500"),
            },
            FieldDescriptor {
                name:        "memory",
                description: "Memory limit in MB",
                field_type:  FieldType::Int,
                example:     Some("512"),
            },
        ],
        "volume" => vec![
            FieldDescriptor {
                name:        "durability",
                description: "Storage durability tier",
                field_type:  FieldType::Enum(vec!["full-replication", "quorum", "local", "none"]),
                example:     Some("full-replication"),
            },
            FieldDescriptor {
                name:        "snapshots",
                description: "Enable point-in-time snapshots to NAS",
                field_type:  FieldType::Bool,
                example:     Some("true"),
            },
            FieldDescriptor {
                name:        "offsite",
                description: "Enable offsite backup to S3",
                field_type:  FieldType::Bool,
                example:     Some("true"),
            },
            FieldDescriptor {
                name:        "tag",
                description: "Resource tags (key=value)",
                field_type:  FieldType::TagList,
                example:     Some("tier=storage"),
            },
        ],
        "feature-flag" => vec![
            FieldDescriptor {
                name:        "description",
                description: "Human-readable description of the flag",
                field_type:  FieldType::String,
                example:     Some("Enables the redesigned upload flow"),
            },
        ],
        "ingress" => vec![
            FieldDescriptor {
                name:        "host",
                description: "Hostname for host-based routing",
                field_type:  FieldType::String,
                example:     Some("photos.picloud.local"),
            },
            FieldDescriptor {
                name:        "tls",
                description: "Enable TLS termination",
                field_type:  FieldType::Bool,
                example:     Some("true"),
            },
            FieldDescriptor {
                name:        "internal",
                description: "Internal mesh only -- not externally reachable",
                field_type:  FieldType::Bool,
                example:     Some("false"),
            },
        ],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_container() {
        let result = ResourceBuilder::new("container")
            .unwrap()
            .name("api-server")
            .property("product", "photo-app")
            .property("image", "photo-api:1.0.0")
            .property("identity", "api-worker")
            .pairs("tag", vec![
                ("team".into(), "backend".into()),
                ("environment".into(), "production".into()),
            ])
            .render()
            .unwrap();

        assert!(result.contains("container \"api-server\""));
        assert!(result.contains("image  = \"photo-api:1.0.0\""));
        assert!(result.contains("tag { key = \"team\"; value = \"backend\" }"));
    }

    #[test]
    fn rejects_unknown_resource_type() {
        assert!(ResourceBuilder::new("spaceship").is_err());
    }

    #[test]
    fn renders_feature_flag() {
        let result = ResourceBuilder::new("feature-flag")
            .unwrap()
            .name("new-upload-flow")
            .property("product", "photo-app")
            .property("enabled", "true")
            .property("version", ">= 2")
            .render()
            .unwrap();

        assert!(result.contains("feature-flag \"new-upload-flow\""));
        assert!(result.contains("version  = \">= 2\""));
    }

    #[test]
    fn renders_product() {
        let result = ResourceBuilder::new("product")
            .unwrap()
            .name("photo-app")
            .property("version", "1.0.0")
            .property("description", "Photo sharing application")
            .render()
            .unwrap();

        assert!(result.contains("product \"photo-app\""));
        assert!(result.contains("version  = \"1.0.0\""));
    }

    #[test]
    fn validates_missing_required_fields() {
        let builder = ResourceBuilder::new("container")
            .unwrap()
            .name("test");
        let err = builder.validate_required();
        assert!(err.is_err());
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("product"));
        assert!(msg.contains("image"));
    }

    #[test]
    fn validates_all_required_present() {
        let builder = ResourceBuilder::new("container")
            .unwrap()
            .name("test")
            .property("product", "app")
            .property("image", "img:1.0");
        assert!(builder.validate_required().is_ok());
    }

    #[test]
    fn renders_secret() {
        let result = ResourceBuilder::new("volume")
            .unwrap()
            .name("data")
            .property("product", "app")
            .property("size", "100GB")
            .secret("backup-target", "backblaze-config")
            .render()
            .unwrap();

        assert!(result.contains("secret(\"backblaze-config\")"));
    }

    #[test]
    fn known_resource_types_not_empty() {
        assert!(!KNOWN_RESOURCE_TYPES.is_empty());
        assert!(KNOWN_RESOURCE_TYPES.contains(&"product"));
        assert!(KNOWN_RESOURCE_TYPES.contains(&"container"));
    }

    #[test]
    fn render_requires_name() {
        let builder = ResourceBuilder::new("product").unwrap();
        assert!(builder.render().is_err());
    }
}
