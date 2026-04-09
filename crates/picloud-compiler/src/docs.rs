/// Documentation Generator
///
/// Generates Markdown documentation listing all resource types
/// and their properties from the platform ontology (ADR-049).
///
/// Phase 1: generates from hardcoded resource type metadata.
/// Phase 3+: derives from SHACL shapes and ontology .ttl files.

use picloud_domain::error::Result;

/// Output format for documentation generation
#[derive(Debug, Clone)]
pub enum DocFormat {
    /// Human-readable Markdown -- one file per resource type
    Markdown,
    /// JSON Schema -- one schema per resource type
    JsonSchema,
    /// OpenAPI 3.1 spec -- describes the platform HTTP API
    OpenApi,
}

/// A generated documentation file
pub struct GeneratedFile {
    pub path:    String,
    pub content: String,
}

/// Generate documentation from resource type metadata
pub fn generate(
    format:     DocFormat,
    output_dir: &str,
) -> Result<Vec<GeneratedFile>> {
    match format {
        DocFormat::Markdown  => generate_markdown(output_dir),
        DocFormat::JsonSchema => generate_json_schema(output_dir),
        DocFormat::OpenApi   => generate_openapi(output_dir),
    }
}

/// Resource type metadata for documentation
struct ResourceTypeDocs {
    name:        &'static str,
    rdf_type:    &'static str,
    description: &'static str,
    properties:  Vec<PropertyDoc>,
}

struct PropertyDoc {
    name:        &'static str,
    prop_type:   &'static str,
    required:    bool,
    description: &'static str,
}

/// Return documentation metadata for all known resource types
fn resource_types() -> Vec<ResourceTypeDocs> {
    vec![
        ResourceTypeDocs {
            name: "product",
            rdf_type: "pc:Product",
            description: "A product is a hermetically sealed unit of deployment. Products never share resources.",
            properties: vec![
                PropertyDoc { name: "version", prop_type: "string", required: true, description: "Semantic version of the product" },
                PropertyDoc { name: "description", prop_type: "string", required: false, description: "Human-readable description" },
            ],
        },
        ResourceTypeDocs {
            name: "container",
            rdf_type: "pc:Container",
            description: "An OCI container workload scheduled on the cluster.",
            properties: vec![
                PropertyDoc { name: "product", prop_type: "string", required: true, description: "Product this container belongs to" },
                PropertyDoc { name: "image", prop_type: "string", required: true, description: "OCI image reference (name:tag)" },
                PropertyDoc { name: "identity", prop_type: "string", required: false, description: "Workload identity name" },
                PropertyDoc { name: "mount", prop_type: "block", required: false, description: "Volume mount (volume + path)" },
                PropertyDoc { name: "tag", prop_type: "block", required: false, description: "Resource tag (key + value)" },
            ],
        },
        ResourceTypeDocs {
            name: "volume",
            rdf_type: "pc:Volume",
            description: "A persistent block storage volume.",
            properties: vec![
                PropertyDoc { name: "product", prop_type: "string", required: true, description: "Product this volume belongs to" },
                PropertyDoc { name: "size", prop_type: "string", required: true, description: "Volume size (e.g. '500GB')" },
                PropertyDoc { name: "durability", prop_type: "enum", required: false, description: "Durability tier: full-replication, quorum, local, none" },
                PropertyDoc { name: "snapshots", prop_type: "block", required: false, description: "Snapshot configuration" },
                PropertyDoc { name: "offsite", prop_type: "block", required: false, description: "Offsite backup configuration" },
            ],
        },
        ResourceTypeDocs {
            name: "feature-flag",
            rdf_type: "pc:FeatureFlag",
            description: "A feature flag controlling runtime behaviour.",
            properties: vec![
                PropertyDoc { name: "product", prop_type: "string", required: true, description: "Product this flag belongs to" },
                PropertyDoc { name: "enabled", prop_type: "boolean", required: true, description: "Whether the flag is currently enabled" },
                PropertyDoc { name: "version", prop_type: "string", required: true, description: "Version expression (e.g. '>= 2')" },
                PropertyDoc { name: "description", prop_type: "string", required: false, description: "Human-readable description" },
            ],
        },
        ResourceTypeDocs {
            name: "binary",
            rdf_type: "pc:Binary",
            description: "A native binary workload scheduled on the cluster.",
            properties: vec![
                PropertyDoc { name: "product", prop_type: "string", required: true, description: "Product this binary belongs to" },
                PropertyDoc { name: "executable", prop_type: "string", required: true, description: "Path to the executable binary" },
                PropertyDoc { name: "identity", prop_type: "string", required: false, description: "Workload identity name" },
            ],
        },
        ResourceTypeDocs {
            name: "ingress",
            rdf_type: "pc:Ingress",
            description: "An ingress route exposing a workload to the network.",
            properties: vec![
                PropertyDoc { name: "product", prop_type: "string", required: true, description: "Product this ingress belongs to" },
                PropertyDoc { name: "target", prop_type: "string", required: true, description: "Target container or binary name" },
                PropertyDoc { name: "port", prop_type: "integer", required: true, description: "Port number to expose" },
                PropertyDoc { name: "host", prop_type: "string", required: false, description: "Hostname for host-based routing" },
                PropertyDoc { name: "tls", prop_type: "boolean", required: false, description: "Enable TLS termination" },
            ],
        },
        ResourceTypeDocs {
            name: "group",
            rdf_type: "pc:Group",
            description: "A grouping of resources across products.",
            properties: vec![],
        },
    ]
}

fn generate_markdown(output_dir: &str) -> Result<Vec<GeneratedFile>> {
    let types = resource_types();
    let mut files = Vec::new();

    // Generate index page
    let mut index = String::from("# PiCloud Resource Types\n\n");
    index.push_str("| Type | RDF Class | Description |\n");
    index.push_str("|------|-----------|-------------|\n");
    for t in &types {
        index.push_str(&format!(
            "| `{}` | `{}` | {} |\n",
            t.name, t.rdf_type, t.description
        ));
    }
    index.push('\n');
    files.push(GeneratedFile {
        path: format!("{}/index.md", output_dir),
        content: index,
    });

    // Generate one page per resource type
    for t in &types {
        let mut page = format!("# {}\n\n", t.name);
        page.push_str(&format!("**RDF Type:** `{}`\n\n", t.rdf_type));
        page.push_str(&format!("{}\n\n", t.description));

        if !t.properties.is_empty() {
            page.push_str("## Properties\n\n");
            page.push_str("| Property | Type | Required | Description |\n");
            page.push_str("|----------|------|----------|-------------|\n");
            for p in &t.properties {
                page.push_str(&format!(
                    "| `{}` | {} | {} | {} |\n",
                    p.name,
                    p.prop_type,
                    if p.required { "Yes" } else { "No" },
                    p.description
                ));
            }
            page.push('\n');

            // Generate example .picloud snippet
            page.push_str("## Example\n\n");
            page.push_str("```picloud\n");
            page.push_str(&format!("{} \"example\" {{\n", t.name));
            for p in &t.properties {
                if p.prop_type == "block" {
                    continue;
                }
                let example_value = match p.prop_type {
                    "string" => format!("\"example-{}\"", p.name),
                    "boolean" => "true".to_string(),
                    "integer" => "8080".to_string(),
                    "enum" => "\"default\"".to_string(),
                    _ => format!("\"{}\"", p.name),
                };
                page.push_str(&format!("  {} = {}\n", p.name, example_value));
            }
            page.push_str("}\n");
            page.push_str("```\n");
        }

        files.push(GeneratedFile {
            path: format!("{}/{}.md", output_dir, t.name),
            content: page,
        });
    }

    Ok(files)
}

fn generate_json_schema(output_dir: &str) -> Result<Vec<GeneratedFile>> {
    let types = resource_types();
    let mut files = Vec::new();

    for t in &types {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for p in &t.properties {
            let json_type = match p.prop_type {
                "string" | "enum" => "string",
                "integer" => "integer",
                "boolean" => "boolean",
                "block" => "object",
                _ => "string",
            };
            let mut prop_schema = serde_json::Map::new();
            prop_schema.insert("type".to_string(), serde_json::Value::String(json_type.to_string()));
            prop_schema.insert("description".to_string(), serde_json::Value::String(p.description.to_string()));
            properties.insert(p.name.to_string(), serde_json::Value::Object(prop_schema));

            if p.required {
                required.push(serde_json::Value::String(p.name.to_string()));
            }
        }

        let schema = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": format!("https://picloud.local/schemas/{}.json", t.name),
            "title": t.name,
            "description": t.description,
            "type": "object",
            "properties": properties,
            "required": required,
        });

        files.push(GeneratedFile {
            path: format!("{}/{}.json", output_dir, t.name),
            content: serde_json::to_string_pretty(&schema).unwrap_or_default(),
        });
    }

    Ok(files)
}

fn generate_openapi(output_dir: &str) -> Result<Vec<GeneratedFile>> {
    // Phase 1: generate a minimal OpenAPI stub
    let types = resource_types();
    let mut paths = String::new();

    for t in &types {
        if t.name == "group" {
            continue; // Groups don't follow the product/{type}/{name} pattern
        }
        paths.push_str(&format!(
            "  /products/{{product}}/{name}s/{{name}}:\n    get:\n      summary: Get a {name}\n      tags: [{name}]\n    put:\n      summary: Create or update a {name}\n      tags: [{name}]\n    delete:\n      summary: Delete a {name}\n      tags: [{name}]\n",
            name = t.name
        ));
    }

    let openapi = format!(
        r#"openapi: "3.1.0"
info:
  title: PiCloud Platform API
  version: "0.1.0"
  description: Auto-generated from platform resource types
paths:
{}
"#,
        paths
    );

    Ok(vec![GeneratedFile {
        path: format!("{}/openapi.yaml", output_dir),
        content: openapi,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_markdown_docs() {
        let files = generate(DocFormat::Markdown, "docs/resources").unwrap();
        assert!(!files.is_empty());
        // Should have an index file
        assert!(files.iter().any(|f| f.path.contains("index.md")));
        // Should have a product page
        assert!(files.iter().any(|f| f.path.contains("product.md")));
        // Product page should list the version property
        let product = files.iter().find(|f| f.path.contains("product.md")).unwrap();
        assert!(product.content.contains("version"));
    }

    #[test]
    fn generate_json_schema_docs() {
        let files = generate(DocFormat::JsonSchema, "schemas").unwrap();
        assert!(!files.is_empty());
        let container = files.iter().find(|f| f.path.contains("container.json")).unwrap();
        assert!(container.content.contains("image"));
        assert!(container.content.contains("required"));
    }

    #[test]
    fn generate_openapi_docs() {
        let files = generate(DocFormat::OpenApi, "api").unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].content.contains("openapi"));
        assert!(files[0].content.contains("container"));
    }
}
