/// .picloud -> Turtle Compiler
///
/// Translates parsed .picloud AST nodes into Turtle RDF triples.
/// Every nested structure gets a stable, dereferenceable IRI -- no blank nodes.
///
/// IRI generation rules (ADR-049):
///   {parent}/{property}          for singleton nested objects
///   {parent}/{property}/{key}    for keyed collections (tags, mounts by volume name)
///   {parent}/{property}/{index}  for ordered lists
///
/// The same .picloud files always produce the same IRIs.
/// Diffs are meaningful. SPARQL queries always work.

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use crate::parser::{ParsedFile, ResourceDecl, PropertyValue};

/// The result of compiling one or more .picloud files
pub struct CompileResult {
    /// Turtle representation ready for the merger
    pub turtle: String,
    /// IRIs produced -- for cross-reference validation
    pub declared_iris: Vec<String>,
}

/// Compile a parsed .picloud file to Turtle
pub fn compile(file: &ParsedFile, domain: &ClusterDomain) -> Result<CompileResult> {
    let builder = IriBuilder::new(domain.clone());
    let mut output = TurtleWriter::new();

    output.prefix("pc",  "https://picloud.local/ontology#");
    output.prefix("xsd", "http://www.w3.org/2001/XMLSchema#");

    let mut declared_iris = Vec::new();

    for resource in &file.resources {
        let iri = resource_iri(&builder, resource)?;
        declared_iris.push(iri.clone());
        compile_resource(&mut output, resource, &iri, &builder)?;
    }

    Ok(CompileResult {
        turtle:        output.finish(),
        declared_iris,
    })
}

fn resource_iri(builder: &IriBuilder, resource: &ResourceDecl) -> Result<String> {
    // Extract product from properties
    let product = resource.properties.iter()
        .find(|p| p.key == "product")
        .and_then(|p| if let PropertyValue::String(s) = &p.value { Some(s.as_str()) } else { None });

    match resource.resource_type.as_str() {
        "product" => Ok(builder.product(&resource.name).to_string()),
        "container" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("Container '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "containers", &resource.name).to_string())
        }
        "volume" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("Volume '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "volumes", &resource.name).to_string())
        }
        "feature-flag" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("FeatureFlag '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.feature_flag(product, &resource.name).to_string())
        }
        "binary" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("Binary '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "binaries", &resource.name).to_string())
        }
        "rdf-store" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("RdfStore '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "rdf-stores", &resource.name).to_string())
        }
        "event-store" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("EventStore '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "event-stores", &resource.name).to_string())
        }
        "config" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("Config '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "configs", &resource.name).to_string())
        }
        "ingress" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("Ingress '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "ingress", &resource.name).to_string())
        }
        "ontology" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("Ontology '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "ontologies", &resource.name).to_string())
        }
        "inference-rule" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("InferenceRule '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "inference-rules", &resource.name).to_string())
        }
        "group" => {
            Ok(builder.group(&resource.name).to_string())
        }
        "event-subscription" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("EventSubscription '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "event-subscriptions", &resource.name).to_string())
        }
        "role" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("Role '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "roles", &resource.name).to_string())
        }
        "scope" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("Scope '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "scopes", &resource.name).to_string())
        }
        "m2m-permission" => {
            let product = product.ok_or_else(|| PiCloudError::ResourceValidationFailed {
                reason: format!("M2mPermission '{}' missing required property: product", resource.name),
            })?;
            Ok(builder.resource(product, "m2m-permissions", &resource.name).to_string())
        }
        other => Err(PiCloudError::ResourceValidationFailed {
            reason: format!("Unknown resource type: '{}'", other),
        }),
    }
}

fn compile_resource(
    out:      &mut TurtleWriter,
    resource: &ResourceDecl,
    iri:      &str,
    builder:  &IriBuilder,
) -> Result<()> {
    let rdf_type = resource_rdf_type(&resource.resource_type);
    out.triple(iri, "a", &format!("pc:{}", rdf_type));

    for prop in &resource.properties {
        compile_property(out, resource, iri, &prop.key, &prop.value, builder)?;
    }

    out.blank_line();
    Ok(())
}

fn compile_property(
    out:           &mut TurtleWriter,
    resource:      &ResourceDecl,
    parent_iri:    &str,
    key:           &str,
    value:         &PropertyValue,
    builder:       &IriBuilder,
) -> Result<()> {
    match value {
        PropertyValue::String(s)  => out.triple(parent_iri, &format!("pc:{}", camel(key)), &format!("\"{}\"", s)),
        PropertyValue::Number(n)  => {
            // Emit integers without decimal point
            if *n == (*n as i64) as f64 {
                out.triple(parent_iri, &format!("pc:{}", camel(key)), &format!("{}", *n as i64));
            } else {
                out.triple(parent_iri, &format!("pc:{}", camel(key)), &n.to_string());
            }
        }
        PropertyValue::Bool(b)    => out.triple(parent_iri, &format!("pc:{}", camel(key)), &b.to_string()),
        PropertyValue::Secret(s)  => out.triple(parent_iri, &format!("pc:{}", camel(key)), &format!("\"secret:{}\"", s)),

        PropertyValue::Block(props) => {
            // For collection-type blocks (mount, tag), use keyed IRI pattern
            // even when there's only one instance
            if is_collection_type(key) {
                let block_key = props.iter()
                    .find(|p| p.key == "key" || p.key == "volume" || p.key == "name")
                    .and_then(|p| if let PropertyValue::String(s) = &p.value { Some(s.as_str()) } else { None })
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "0".to_string());

                let nested_iri = format!("{}/{}s/{}", parent_iri, key, block_key);
                out.triple(parent_iri, &format!("pc:{}", camel(key)), &format!("<{}>", nested_iri));

                if let Some(t) = nested_rdf_type(key) {
                    out.triple_raw(&format!("<{}>", nested_iri), "a", &format!("pc:{}", t));
                }

                for prop in props {
                    compile_property(out, resource, &nested_iri, &prop.key, &prop.value, builder)?;
                }
                out.blank_line();
            } else {
                // Stable IRI for the nested object: {parent}/{property-name}
                let nested_iri = format!("{}/{}", parent_iri, key);
                out.triple(parent_iri, &format!("pc:{}", camel(key)), &format!("<{}>", nested_iri));

                // Infer rdf:type for well-known nested types
                if let Some(t) = nested_rdf_type(key) {
                    out.triple_raw(&format!("<{}>", nested_iri), "a", &format!("pc:{}", t));
                }

                for prop in props {
                    compile_property(out, resource, &nested_iri, &prop.key, &prop.value, builder)?;
                }
                out.blank_line();
            }
        }

        PropertyValue::Blocks(blocks) => {
            // Repeated blocks: tags, mounts etc.
            // Key for each block is derived from the block's own "key" or "volume" or "name" property,
            // falling back to index for unnamed blocks
            for (i, props) in blocks.iter().enumerate() {
                let block_key = props.iter()
                    .find(|p| p.key == "key" || p.key == "volume" || p.key == "name")
                    .and_then(|p| if let PropertyValue::String(s) = &p.value { Some(s.as_str()) } else { None })
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| i.to_string());

                let nested_iri = format!("{}/{}s/{}", parent_iri, key, block_key);
                out.triple(parent_iri, &format!("pc:{}", camel(key)), &format!("<{}>", nested_iri));

                if let Some(t) = nested_rdf_type(key) {
                    out.triple_raw(&format!("<{}>", nested_iri), "a", &format!("pc:{}", t));
                }

                for prop in props {
                    compile_property(out, resource, &nested_iri, &prop.key, &prop.value, builder)?;
                }
                out.blank_line();
            }
        }
    }
    Ok(())
}

/// Map resource type names to RDF class names
fn resource_rdf_type(resource_type: &str) -> &str {
    match resource_type {
        "product"            => "Product",
        "container"          => "Container",
        "volume"             => "Volume",
        "binary"             => "Binary",
        "rdf-store"          => "RdfStore",
        "event-store"        => "EventStore",
        "feature-flag"       => "FeatureFlag",
        "inference-rule"     => "InferenceRule",
        "ingress"            => "Ingress",
        "config"             => "ConfigStore",
        "ontology"           => "Ontology",
        "group"              => "Group",
        "event-subscription" => "EventSubscription",
        "role"               => "ProductRole",
        "scope"              => "ProductScope",
        "m2m-permission"     => "M2mPermission",
        _                    => "Resource",
    }
}

/// Check if a property name represents a collection (repeated blocks)
/// These always use the keyed IRI pattern: {parent}/{key}s/{name}
fn is_collection_type(key: &str) -> bool {
    matches!(key, "mount" | "tag" | "port" | "env")
}

/// Map nested property block names to RDF class names
fn nested_rdf_type(key: &str) -> Option<&str> {
    match key {
        "snapshots"  => Some("SnapshotConfig"),
        "retention"  => Some("SnapshotRetention"),
        "offsite"    => Some("OffsiteBackupConfig"),
        "mount"      => Some("VolumeMount"),
        "tag"        => Some("Tag"),
        "otel"       => Some("OtelConfig"),
        "resources"  => Some("ResourceLimits"),
        _            => None,
    }
}

/// Convert kebab-case property names to camelCase for RDF predicates
fn camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalise = false;
    for c in s.chars() {
        if c == '-' {
            capitalise = true;
        } else if capitalise {
            result.push(c.to_ascii_uppercase());
            capitalise = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Simple Turtle writer -- accumulates triples into a string
struct TurtleWriter {
    lines: Vec<String>,
}

impl TurtleWriter {
    fn new() -> Self { Self { lines: Vec::new() } }

    fn prefix(&mut self, prefix: &str, iri: &str) {
        self.lines.push(format!("@prefix {}: <{}> .", prefix, iri));
    }

    /// Write a triple where the subject is wrapped in angle brackets automatically
    fn triple(&mut self, subject: &str, predicate: &str, object: &str) {
        // If subject looks like a full IRI (starts with http), wrap in <>
        if subject.starts_with("http") {
            self.lines.push(format!("<{}> {} {} .", subject, predicate, object));
        } else {
            self.lines.push(format!("{} {} {} .", subject, predicate, object));
        }
    }

    /// Write a triple where subject is already formatted (e.g. already wrapped in <>)
    fn triple_raw(&mut self, subject: &str, predicate: &str, object: &str) {
        self.lines.push(format!("{} {} {} .", subject, predicate, object));
    }

    fn blank_line(&mut self) {
        self.lines.push(String::new());
    }

    fn finish(self) -> String {
        self.lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn camel_conversion() {
        assert_eq!(camel("feature-flag"),   "featureFlag");
        assert_eq!(camel("rdf-store"),      "rdfStore");
        assert_eq!(camel("max-upload-mb"),  "maxUploadMb");
        assert_eq!(camel("image"),          "image");
    }

    #[test]
    fn compile_product() {
        let source = r#"
product "photo-app" {
  version     = "1.0.0"
  description = "Photo sharing application"
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let domain = ClusterDomain::default();
        let result = compile(&file, &domain).unwrap();

        assert!(result.turtle.contains("pc:Product"));
        assert!(result.turtle.contains("pc:version \"1.0.0\""));
        assert!(result.turtle.contains("pc:description \"Photo sharing application\""));
        assert!(result.declared_iris.contains(&"https://picloud.local/products/photo-app".to_string()));
    }

    #[test]
    fn compile_container_with_mounts_and_tags() {
        let source = r#"
container "api-server" {
  product = "photo-app"
  image   = "photo-api:1.0.0"
  mount {
    volume = "media-store"
    path   = "/data"
  }
  tag { key = "team"; value = "backend" }
  tag { key = "environment"; value = "production" }
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let domain = ClusterDomain::default();
        let result = compile(&file, &domain).unwrap();

        // Container IRI
        assert!(result.turtle.contains("https://picloud.local/products/photo-app/containers/api-server"));
        assert!(result.turtle.contains("pc:Container"));

        // Mount gets a stable IRI based on volume name
        assert!(result.turtle.contains("mounts/media-store"));
        assert!(result.turtle.contains("pc:VolumeMount"));

        // Tags get stable IRIs based on key
        assert!(result.turtle.contains("tags/team"));
        assert!(result.turtle.contains("tags/environment"));
        assert!(result.turtle.contains("pc:Tag"));

        // No blank nodes
        assert!(!result.turtle.contains("_:"));
    }

    #[test]
    fn compile_feature_flag() {
        let source = r#"
feature-flag "new-upload-flow" {
  product     = "photo-app"
  description = "Redesigned upload flow"
  enabled     = true
  version     = ">= 2"
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let domain = ClusterDomain::default();
        let result = compile(&file, &domain).unwrap();

        assert!(result.turtle.contains("pc:FeatureFlag"));
        assert!(result.turtle.contains("flags/new-upload-flow"));
        assert!(result.turtle.contains("pc:enabled true"));
    }

    #[test]
    fn iri_determinism() {
        let source = r#"
container "api-server" {
  product = "photo-app"
  image   = "api:1.0"
  tag { key = "team"; value = "backend" }
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let domain = ClusterDomain::default();
        let r1 = compile(&file, &domain).unwrap();
        let r2 = compile(&file, &domain).unwrap();
        assert_eq!(r1.turtle, r2.turtle, "Same input must produce identical output");
    }

    #[test]
    fn compile_unknown_resource_type_fails() {
        let source = r#"
spaceship "enterprise" {
  warp = "9.9"
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let domain = ClusterDomain::default();
        assert!(compile(&file, &domain).is_err());
    }

    #[test]
    fn compile_numbers_as_integers() {
        let source = r#"
product "test" {
  version = "1.0.0"
  replicas = 3
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let domain = ClusterDomain::default();
        let result = compile(&file, &domain).unwrap();
        assert!(result.turtle.contains("pc:replicas 3"));
        // Should not contain "3.0"
        assert!(!result.turtle.contains("3.0"));
    }
}
