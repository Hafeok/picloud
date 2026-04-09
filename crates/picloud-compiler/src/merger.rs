/// Graph Merger
///
/// Merges compiled Turtle output from multiple .picloud and .ttl files
/// into a single deployment graph.
///
/// The merged graph is what gets SHACL-validated and submitted to the cluster.
/// .picloud files are never submitted -- only the merged deployment.ttl.
///
/// Conflict detection:
///   If two files declare the same IRI with conflicting triples,
///   the merger reports an error before the graph is submitted.

use picloud_domain::error::{PiCloudError, Result};
use crate::compiler::CompileResult;

/// A merged deployment graph ready for SHACL validation and submission
pub struct DeploymentGraph {
    /// The complete merged Turtle graph
    pub turtle:        String,
    /// All IRIs declared across all input files
    pub declared_iris: Vec<String>,
    /// Source files that contributed to this graph
    pub sources:       Vec<String>,
}

/// A raw .ttl file passed through unchanged
pub struct RawTtlFile {
    pub path:    String,
    pub content: String,
}

/// Merge compiled Turtle outputs and raw .ttl files into one graph
pub fn merge(
    compiled:  Vec<CompileResult>,
    raw_ttl:   Vec<RawTtlFile>,
) -> Result<DeploymentGraph> {
    let mut all_turtle   = Vec::new();
    let mut declared_iris = Vec::new();
    let mut sources       = Vec::new();

    // Collect compiled outputs
    for result in compiled {
        all_turtle.push(result.turtle);
        declared_iris.extend(result.declared_iris);
    }

    // Collect raw .ttl files (passed through unchanged)
    for raw in raw_ttl {
        sources.push(raw.path.clone());
        all_turtle.push(raw.content);
    }

    // Check for duplicate IRIs across files
    let mut seen = std::collections::HashSet::new();
    let mut duplicates = Vec::new();
    for iri in &declared_iris {
        if !seen.insert(iri.as_str()) {
            duplicates.push(iri.clone());
        }
    }
    if !duplicates.is_empty() {
        return Err(PiCloudError::ResourceValidationFailed {
            reason: format!(
                "Duplicate resource IRIs found across files: {}",
                duplicates.join(", ")
            ),
        });
    }

    let merged_turtle = all_turtle.join("\n\n");

    Ok(DeploymentGraph {
        turtle: merged_turtle,
        declared_iris,
        sources,
    })
}

/// Write the merged deployment graph to a file
pub fn write_deployment(graph: &DeploymentGraph, output_path: &str) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(output_path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| PiCloudError::Internal(format!(
                "Failed to create directory {}: {}", parent.display(), e
            )))?;
    }
    std::fs::write(output_path, &graph.turtle)
        .map_err(|e| PiCloudError::Internal(format!(
            "Failed to write deployment graph to {}: {}", output_path, e
        )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_two_compiled() {
        let a = CompileResult {
            turtle: "<a> a pc:Product .".to_string(),
            declared_iris: vec!["https://picloud.local/products/a".to_string()],
        };
        let b = CompileResult {
            turtle: "<b> a pc:Container .".to_string(),
            declared_iris: vec!["https://picloud.local/products/a/containers/b".to_string()],
        };
        let graph = merge(vec![a, b], vec![]).unwrap();
        assert!(graph.turtle.contains("<a> a pc:Product ."));
        assert!(graph.turtle.contains("<b> a pc:Container ."));
        assert_eq!(graph.declared_iris.len(), 2);
    }

    #[test]
    fn merge_with_raw_ttl() {
        let compiled = CompileResult {
            turtle: "<a> a pc:Product .".to_string(),
            declared_iris: vec!["https://picloud.local/products/a".to_string()],
        };
        let raw = RawTtlFile {
            path: "schemas/events.ttl".to_string(),
            content: "<schema> a owl:Ontology .".to_string(),
        };
        let graph = merge(vec![compiled], vec![raw]).unwrap();
        assert!(graph.turtle.contains("<a> a pc:Product ."));
        assert!(graph.turtle.contains("<schema> a owl:Ontology ."));
        assert_eq!(graph.sources.len(), 1);
    }

    #[test]
    fn merge_detects_duplicate_iris() {
        let a = CompileResult {
            turtle: "<x> a pc:Product .".to_string(),
            declared_iris: vec!["https://picloud.local/products/x".to_string()],
        };
        let b = CompileResult {
            turtle: "<x> a pc:Product .".to_string(),
            declared_iris: vec!["https://picloud.local/products/x".to_string()],
        };
        let result = merge(vec![a, b], vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn merge_empty() {
        let graph = merge(vec![], vec![]).unwrap();
        assert!(graph.turtle.is_empty());
        assert!(graph.declared_iris.is_empty());
    }
}
