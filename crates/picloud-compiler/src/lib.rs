/// PiCloud Compiler
///
/// Translates .picloud surface format files to Turtle (RDF),
/// validates against the platform ontology and SHACL shapes,
/// merges into a single deployment graph, and generates documentation.
///
/// Pipeline:
///   .picloud files -> parser -> compiler -> merger -> validator -> deployment.ttl
///
/// Documentation generation:
///   platform.ttl + shapes.ttl -> docs generator -> Markdown / JSON Schema / OpenAPI
///
/// ADR-049: .picloud Format as Compiler Target -- Turtle as Canonical IaC

pub mod parser;
pub mod compiler;
pub mod validator;
pub mod merger;
pub mod docs;
pub mod builder;

pub use compiler::CompileResult;
pub use validator::{ValidationResult, ValidationError};
pub use builder::{ResourceBuilder, BuilderValue, FieldDescriptor, KNOWN_RESOURCE_TYPES};
