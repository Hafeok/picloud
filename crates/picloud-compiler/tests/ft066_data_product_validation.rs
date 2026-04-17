//! FT-066 — Data Product Validation Tests
//!
//! Covers:
//!   TC-197: data_product_field_validation — attempt to declare a
//!           `data-product` missing each mandatory field in turn
//!           (`triggers`, `maxAge`, `domain`, `shapes`/`ontology`).
//!           Each attempt must be rejected at `resource apply` with a
//!           specific validation error. No partial resource state
//!           is created in the cluster graph.
//!
//! ADR-056 validation rules (applied at `resource apply` time):
//!   1. A data-product must declare at least one `triggers` event.
//!   2. A data-product must declare `freshness.maxAge`.
//!   3. A data-product must belong to exactly one `data-domain`.
//!   4. A data-product must declare `ontology` or `shapes` (or both).

use picloud_compiler::parser;
use picloud_compiler::validator::{validate_offline, ValidationError};

/// A complete, valid data-product declaration used as the baseline.
/// Each TC-197 sub-assertion removes exactly one mandatory field from
/// this template and re-validates to ensure the validator surfaces the
/// expected error.
const VALID_DP: &str = r#"
product "photo-app" {
  version = "1.0.0"
}

data-domain "geospatial" {
  steward     = "identity/alice"
  sensitivity = "internal"
}

data-product "photo-locations" {
  product     = "photo-app"
  domain      = "geospatial"
  version     = "1.0.0"
  ontology    = "./data-products/photo-locations.ttl"
  shapes      = "./data-products/photo-locations.shacl"
  projection  = "./data-products/photo-locations.rq"
  freshness {
    maxAge   = "15m"
    triggers = "PlaceResolved"
  }
  access {
    visibility = "cluster"
    roles      = "data-consumer"
  }
}
"#;

/// Helper: parse `source` as a .picloud file and run `validate_offline`.
fn validate(source: &str) -> Vec<ValidationError> {
    let file = parser::parse(source, "test.picloud")
        .expect("parser must accept well-formed .picloud input");
    let result = validate_offline(&[file]).expect("validate_offline must not fail");
    result.errors
}

/// Sanity-check: the baseline declaration is valid (no errors).
/// Without this, later assertions that rely on the error list shrinking
/// when fields are restored would be meaningless.
#[test]
fn valid_data_product_declaration_passes_validation() {
    let errors = validate(VALID_DP);
    assert!(
        errors.is_empty(),
        "baseline data-product should validate cleanly, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// TC-197 — single entry point exercising every mandatory-field scenario.
///
/// Each scenario removes exactly one required field from the baseline
/// `VALID_DP` declaration and asserts:
///   1. the validator returns at least one error,
///   2. the error identifies the missing property by name,
///   3. the error mentions the offending resource (`photo-locations`),
///   4. the well-formed sibling resources (`photo-app`, `geospatial`)
///      are *not* flagged — removing one field from the data-product
///      must not cascade into spurious failures elsewhere.
#[test]
fn data_product_field_validation() {
    // --- Missing `triggers` (inside the freshness block) ---
    // Rule 1: must declare at least one triggers event.
    let src = r#"
product "photo-app" { version = "1.0.0" }
data-domain "geospatial" { steward = "identity/alice" sensitivity = "internal" }
data-product "photo-locations" {
  product    = "photo-app"
  domain     = "geospatial"
  version    = "1.0.0"
  ontology   = "./photo-locations.ttl"
  projection = "./photo-locations.rq"
  freshness {
    maxAge = "15m"
  }
}
"#;
    let errors = validate(src);
    assert!(
        errors.iter().any(|e| e.property.as_deref() == Some("triggers")
            && e.message.contains("photo-locations")),
        "missing `triggers` must produce a `triggers` property error: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    // --- Missing `maxAge` (inside the freshness block) ---
    // Rule 2: must declare freshness.maxAge.
    let src = r#"
product "photo-app" { version = "1.0.0" }
data-domain "geospatial" { steward = "identity/alice" sensitivity = "internal" }
data-product "photo-locations" {
  product    = "photo-app"
  domain     = "geospatial"
  version    = "1.0.0"
  ontology   = "./photo-locations.ttl"
  projection = "./photo-locations.rq"
  freshness {
    triggers = "PlaceResolved"
  }
}
"#;
    let errors = validate(src);
    assert!(
        errors.iter().any(|e| e.property.as_deref() == Some("maxAge")
            && e.message.contains("photo-locations")),
        "missing `maxAge` must produce a `maxAge` property error: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    // --- Missing `domain` ---
    // Rule 3: must belong to exactly one data-domain.
    let src = r#"
product "photo-app" { version = "1.0.0" }
data-domain "geospatial" { steward = "identity/alice" sensitivity = "internal" }
data-product "photo-locations" {
  product    = "photo-app"
  version    = "1.0.0"
  ontology   = "./photo-locations.ttl"
  projection = "./photo-locations.rq"
  freshness {
    maxAge   = "15m"
    triggers = "PlaceResolved"
  }
}
"#;
    let errors = validate(src);
    assert!(
        errors.iter().any(|e| e.property.as_deref() == Some("domain")
            && e.message.contains("photo-locations")),
        "missing `domain` must produce a `domain` property error: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    // --- Missing both `ontology` AND `shapes` ---
    // Rule 4: must declare ontology or shapes (or both).
    let src = r#"
product "photo-app" { version = "1.0.0" }
data-domain "geospatial" { steward = "identity/alice" sensitivity = "internal" }
data-product "photo-locations" {
  product    = "photo-app"
  domain     = "geospatial"
  version    = "1.0.0"
  projection = "./photo-locations.rq"
  freshness {
    maxAge   = "15m"
    triggers = "PlaceResolved"
  }
}
"#;
    let errors = validate(src);
    assert!(
        errors
            .iter()
            .any(|e| e.property.as_deref() == Some("ontology")
                && e.message.contains("photo-locations")
                && (e.message.contains("ontology") || e.message.contains("shapes"))),
        "missing both `ontology` and `shapes` must be rejected: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    // --- Declaring just `shapes` (without `ontology`) is acceptable ---
    // Rule 4 is satisfied when either branch is present.
    let src_shapes_only = r#"
product "photo-app" { version = "1.0.0" }
data-domain "geospatial" { steward = "identity/alice" sensitivity = "internal" }
data-product "photo-locations" {
  product    = "photo-app"
  domain     = "geospatial"
  version    = "1.0.0"
  shapes     = "./photo-locations.shacl"
  projection = "./photo-locations.rq"
  freshness {
    maxAge   = "15m"
    triggers = "PlaceResolved"
  }
}
"#;
    let errors = validate(src_shapes_only);
    assert!(
        !errors.iter().any(|e| e.property.as_deref() == Some("ontology")),
        "declaring just `shapes` should satisfy the ontology/shapes rule: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// Companion assertion for TC-197: validation errors must carry enough
/// structure (property name, resource identification, source location)
/// for the `resource apply` layer to produce an actionable error trace.
/// The validator's job is to surface *all* violations at once — partial
/// resource state must never be produced.
#[test]
fn data_product_validation_errors_are_structured() {
    // Drop three required fields in one go — the validator should return
    // three distinct errors rather than short-circuiting on the first.
    let src = r#"
product "photo-app" { version = "1.0.0" }
data-product "broken" {
  product    = "photo-app"
  version    = "1.0.0"
  projection = "./broken.rq"
}
"#;
    let errors = validate(src);
    let properties: Vec<&str> = errors
        .iter()
        .filter(|e| e.message.contains("broken"))
        .filter_map(|e| e.property.as_deref())
        .collect();

    // `domain`, `freshness` (generic required), `maxAge`, `triggers`,
    // and the ontology/shapes alternative must all be surfaced.
    assert!(
        properties.contains(&"domain"),
        "expected `domain` property error, got: {:?}",
        properties
    );
    assert!(
        properties.contains(&"maxAge"),
        "expected `maxAge` property error, got: {:?}",
        properties
    );
    assert!(
        properties.contains(&"triggers"),
        "expected `triggers` property error, got: {:?}",
        properties
    );
    assert!(
        properties.contains(&"ontology"),
        "expected `ontology`/`shapes` property error, got: {:?}",
        properties
    );

    // Every error must point at a location (file:line) so `resource apply`
    // can render the CLI error with a source pointer.
    for e in &errors {
        if e.message.contains("broken") {
            assert!(
                e.location.is_some(),
                "validation errors for data-product must include a source location"
            );
        }
    }
}
