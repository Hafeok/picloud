/// Validator
///
/// Validates compiled deployment graphs against basic structural rules
/// and cross-reference consistency. Translates violations into
/// developer-friendly error messages (ADR-049).
///
/// Two modes:
///   offline -- validates against local rules (no cluster required)
///   online  -- also checks cross-resource references against live cluster

use picloud_domain::error::Result;
use crate::parser::{ParsedFile, PropertyValue};

/// The result of validating a deployment graph
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid:    bool,
    pub errors:   Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

impl ValidationResult {
    pub fn ok() -> Self {
        Self { valid: true, errors: Vec::new(), warnings: Vec::new() }
    }

    pub fn with_errors(errors: Vec<ValidationError>) -> Self {
        Self {
            valid: errors.is_empty(),
            errors,
            warnings: Vec::new(),
        }
    }

    pub fn print_report(&self) {
        if self.valid {
            println!("Validation passed");
        } else {
            println!("Validation failed -- {} error(s)", self.errors.len());
        }
        for e in &self.errors   { eprintln!("  error:   {}", e.message); }
        for w in &self.warnings { println!("  warning: {}", w.message); }
    }
}

/// A human-readable validation error -- translated from SHACL violations
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub message:      String,
    /// The IRI of the resource that failed validation
    pub resource_iri: Option<String>,
    /// The property that failed
    pub property:     Option<String>,
    /// Source file and line, if known
    pub location:     Option<String>,
}

#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub message:      String,
    pub resource_iri: Option<String>,
}

/// Validate parsed .picloud files against structural rules.
/// No cluster required -- checks required fields and cross-references
/// within the file set.
pub fn validate_offline(files: &[ParsedFile]) -> Result<ValidationResult> {
    let mut errors = Vec::new();

    // Collect all declared resource names by type for cross-reference checks
    let mut products: Vec<String> = Vec::new();
    let mut all_resources: Vec<(&str, &str, &str)> = Vec::new(); // (type, name, file)

    for file in files {
        for resource in &file.resources {
            if resource.resource_type == "product" {
                products.push(resource.name.clone());
            }
            all_resources.push((&resource.resource_type, &resource.name, &file.path));
        }
    }

    // Validate each resource
    for file in files {
        for resource in &file.resources {
            // Check required fields
            let required = required_fields(&resource.resource_type);
            for field in required {
                if !resource.properties.iter().any(|p| p.key == field) {
                    let type_label = type_label(&resource.resource_type);
                    errors.push(ValidationError {
                        message: format!(
                            "{} '{}': required property '{}' is missing",
                            type_label, resource.name, field
                        ),
                        resource_iri: None,
                        property: Some(field.to_string()),
                        location: Some(format!("{}:{}", file.path, resource.line)),
                    });
                }
            }

            // Check product cross-reference (if resource has a product field)
            if resource.resource_type != "product" && resource.resource_type != "group" {
                if let Some(prop) = resource.properties.iter().find(|p| p.key == "product") {
                    if let PropertyValue::String(product_name) = &prop.value {
                        if !products.contains(product_name) {
                            // Not necessarily an error -- the product may be defined elsewhere
                            // or already deployed. Emit a warning.
                            errors.push(ValidationError {
                                message: format!(
                                    "{} '{}': references product '{}' which is not defined in the current file set",
                                    type_label(&resource.resource_type),
                                    resource.name,
                                    product_name
                                ),
                                resource_iri: None,
                                property: Some("product".to_string()),
                                location: Some(format!("{}:{}", file.path, resource.line)),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(ValidationResult::with_errors(errors))
}

/// Validate a Turtle deployment graph string.
/// This is a pass-through for future SHACL validation.
pub fn validate_turtle(turtle: &str) -> Result<ValidationResult> {
    // Phase 1: basic structural check -- just verify it's non-empty
    if turtle.trim().is_empty() {
        return Ok(ValidationResult::with_errors(vec![
            ValidationError {
                message: "Empty deployment graph".to_string(),
                resource_iri: None,
                property: None,
                location: None,
            }
        ]));
    }
    Ok(ValidationResult::ok())
}

/// Validate cross-resource references against a live cluster.
/// Called after validate_offline succeeds.
pub async fn validate_online(
    _turtle:      &str,
    _cluster_url: &str,
    _token:       &str,
) -> Result<ValidationResult> {
    // Phase 1: stub -- online validation requires cluster connectivity
    // Future: check that referenced secrets, identities, volumes exist
    tracing::info!("Online validation not yet implemented -- skipping");
    Ok(ValidationResult::ok())
}

/// Required fields per resource type -- derived from SHACL sh:minCount = 1
/// In Phase 1 these are hardcoded; Phase 3+ they are read from shapes.ttl
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

/// Human-readable type label for error messages
fn type_label(resource_type: &str) -> &str {
    match resource_type {
        "product"            => "Product",
        "container"          => "Container",
        "volume"             => "Volume",
        "binary"             => "Binary",
        "feature-flag"       => "FeatureFlag",
        "rdf-store"          => "RdfStore",
        "event-store"        => "EventStore",
        "config"             => "Config",
        "inference-rule"     => "InferenceRule",
        "ingress"            => "Ingress",
        "ontology"           => "Ontology",
        "group"              => "Group",
        "event-subscription" => "EventSubscription",
        "role"               => "Role",
        "scope"              => "Scope",
        "m2m-permission"     => "M2mPermission",
        other                => other,
    }
}

/// Translate a SHACL violation into a human-readable error message.
/// Prepared for Phase 3 SHACL integration.
#[allow(dead_code)]
fn translate_violation(
    focus_node:       &str,
    property_path:    &str,
    constraint_class: &str,
    message:          Option<&str>,
) -> ValidationError {
    let resource_name = focus_node
        .rsplit('/')
        .next()
        .unwrap_or(focus_node);

    let property_name = property_path
        .rsplit(['/', '#'])
        .next()
        .unwrap_or(property_path);

    let explanation = match constraint_class {
        "MinCountConstraintComponent" =>
            format!("required property '{}' is missing", property_name),
        "MaxCountConstraintComponent" =>
            format!("property '{}' appears too many times", property_name),
        "DatatypeConstraintComponent" =>
            format!("'{}' has the wrong type", property_name),
        "InConstraintComponent" =>
            format!("'{}' has an invalid value", property_name),
        "PatternConstraintComponent" =>
            format!("'{}' does not match the required pattern", property_name),
        "MinInclusiveConstraintComponent" | "MinExclusiveConstraintComponent" =>
            format!("'{}' is below the minimum allowed value", property_name),
        "MaxInclusiveConstraintComponent" | "MaxExclusiveConstraintComponent" =>
            format!("'{}' exceeds the maximum allowed value", property_name),
        "NodeKindConstraintComponent" =>
            format!("'{}' must be an IRI reference", property_name),
        other =>
            format!("'{}' failed constraint: {}", property_name, other),
    };

    let msg = message
        .map(|m| format!("{}: {}", resource_name, m))
        .unwrap_or_else(|| format!("{}: {}", resource_name, explanation));

    ValidationError {
        message: msg,
        resource_iri: Some(focus_node.to_string()),
        property:     Some(property_name.to_string()),
        location:     None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn validate_missing_required_field() {
        let source = r#"
container "api-server" {
  product = "photo-app"
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let result = validate_offline(&[file]).unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.message.contains("image")));
    }

    #[test]
    fn validate_valid_product() {
        let source = r#"
product "photo-app" {
  version = "1.0.0"
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let result = validate_offline(&[file]).unwrap();
        assert!(result.valid, "Errors: {:?}", result.errors);
    }

    #[test]
    fn validate_cross_reference_product_exists() {
        let source = r#"
product "photo-app" {
  version = "1.0.0"
}

container "api-server" {
  product = "photo-app"
  image   = "api:1.0"
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let result = validate_offline(&[file]).unwrap();
        assert!(result.valid, "Errors: {:?}", result.errors);
    }

    #[test]
    fn validate_cross_reference_product_missing() {
        let source = r#"
container "api-server" {
  product = "nonexistent"
  image   = "api:1.0"
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let result = validate_offline(&[file]).unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.message.contains("nonexistent")));
    }

    #[test]
    fn validate_multiple_errors() {
        let source = r#"
container "broken" {
}
"#;
        let file = parser::parse(source, "test.picloud").unwrap();
        let result = validate_offline(&[file]).unwrap();
        assert!(!result.valid);
        // Should report missing product and image
        assert!(result.errors.len() >= 2, "Expected at least 2 errors, got {}", result.errors.len());
    }

    #[test]
    fn validate_empty_turtle() {
        let result = validate_turtle("").unwrap();
        assert!(!result.valid);
    }

    #[test]
    fn validate_nonempty_turtle() {
        let result = validate_turtle("<s> <p> <o> .").unwrap();
        assert!(result.valid);
    }

    #[test]
    fn translate_min_count_violation() {
        let error = translate_violation(
            "https://picloud.local/products/photo-app/containers/api-server",
            "https://picloud.local/ontology#image",
            "MinCountConstraintComponent",
            None,
        );
        assert!(error.message.contains("api-server"));
        assert!(error.message.contains("image"));
        assert!(error.message.contains("missing"));
    }
}
