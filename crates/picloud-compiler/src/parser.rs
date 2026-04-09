/// .picloud Surface Format Parser
///
/// Parses .picloud files into an intermediate AST.
/// The AST is then compiled to Turtle by compiler.rs.
///
/// .picloud syntax is a simplified, readable surface over Turtle:
///   - Block syntax instead of subject-predicate-object triples
///   - Unquoted property names
///   - String, number, boolean literals
///   - Nested blocks for complex properties
///   - secret("name") references
///
/// Example:
///   container "api-server" {
///     product  = "photo-app"
///     image    = "photo-api:1.0.0"
///     mount {
///       volume = "media-store"
///       path   = "/data"
///     }
///   }

use picloud_domain::error::{PiCloudError, Result};

/// A parsed .picloud file -- a list of resource declarations
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub path:      String,
    pub resources: Vec<ResourceDecl>,
}

/// A single resource declaration
#[derive(Debug, Clone)]
pub struct ResourceDecl {
    /// Resource type (e.g. "container", "volume", "feature-flag")
    pub resource_type: String,
    /// Resource name (e.g. "api-server")
    pub name: String,
    /// Property assignments
    pub properties: Vec<Property>,
    /// Line number in source file -- for error messages
    pub line: usize,
}

/// A property assignment within a resource block
#[derive(Debug, Clone)]
pub struct Property {
    pub key:   String,
    pub value: PropertyValue,
    pub line:  usize,
}

/// A property value -- scalar, nested block, or secret reference
#[derive(Debug, Clone)]
pub enum PropertyValue {
    String(String),
    Number(f64),
    Bool(bool),
    /// secret("name") -- resolved at deploy time via platform secret store
    Secret(String),
    /// Nested block (e.g. mount { ... }, snapshots { ... })
    Block(Vec<Property>),
    /// Repeated block (e.g. multiple tag { ... } entries)
    Blocks(Vec<Vec<Property>>),
}

/// Tracks parser state while scanning through a .picloud source string
struct Parser<'a> {
    source: &'a str,
    pos:    usize,
    line:   usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, pos: 0, line: 1 }
    }

    /// Return remaining unparsed source
    fn remaining(&self) -> &'a str {
        &self.source[self.pos..]
    }

    /// Skip whitespace and comments
    fn skip_ws(&mut self) {
        let bytes = self.source.as_bytes();
        while self.pos < bytes.len() {
            let b = bytes[self.pos];
            if b == b'\n' {
                self.pos += 1;
                self.line += 1;
            } else if b == b' ' || b == b'\t' || b == b'\r' {
                self.pos += 1;
            } else if self.remaining().starts_with("//") {
                // Line comment -- skip to end of line
                while self.pos < bytes.len() && bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// Peek at the current character
    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    /// Consume and return one character
    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    /// Expect and consume a specific character
    fn expect_char(&mut self, expected: char) -> Result<()> {
        self.skip_ws();
        match self.peek() {
            Some(c) if c == expected => {
                self.advance();
                Ok(())
            }
            Some(c) => Err(PiCloudError::ResourceValidationFailed {
                reason: format!("line {}: expected '{}', found '{}'", self.line, expected, c),
            }),
            None => Err(PiCloudError::ResourceValidationFailed {
                reason: format!("line {}: expected '{}', found end of file", self.line, expected),
            }),
        }
    }

    /// Parse an identifier: [a-zA-Z][a-zA-Z0-9_-]*
    fn parse_ident(&mut self) -> Result<String> {
        self.skip_ws();
        let start = self.pos;
        let bytes = self.source.as_bytes();
        if self.pos >= bytes.len() || !(bytes[self.pos] as char).is_ascii_alphabetic() {
            return Err(PiCloudError::ResourceValidationFailed {
                reason: format!("line {}: expected identifier", self.line),
            });
        }
        while self.pos < bytes.len() {
            let c = bytes[self.pos] as char;
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(self.source[start..self.pos].to_string())
    }

    /// Parse a quoted string: "..."
    fn parse_string(&mut self) -> Result<String> {
        self.skip_ws();
        self.expect_char('"')?;
        let start = self.pos;
        let bytes = self.source.as_bytes();
        while self.pos < bytes.len() {
            let b = bytes[self.pos];
            if b == b'"' {
                let s = self.source[start..self.pos].to_string();
                self.pos += 1; // consume closing quote
                return Ok(s);
            }
            if b == b'\n' {
                self.line += 1;
            }
            self.pos += 1;
        }
        Err(PiCloudError::ResourceValidationFailed {
            reason: format!("line {}: unterminated string", self.line),
        })
    }

    /// Parse a property value: string, number, boolean, secret(...), or nested block
    fn parse_value(&mut self) -> Result<PropertyValue> {
        self.skip_ws();
        match self.peek() {
            Some('"') => Ok(PropertyValue::String(self.parse_string()?)),
            Some(c) if c.is_ascii_digit() || c == '-' => self.parse_number(),
            Some('t') | Some('f') => self.parse_bool(),
            Some('s') if self.remaining().starts_with("secret(") => self.parse_secret(),
            Some('{') => self.parse_block_value(),
            Some(c) => Err(PiCloudError::ResourceValidationFailed {
                reason: format!("line {}: unexpected character '{}'", self.line, c),
            }),
            None => Err(PiCloudError::ResourceValidationFailed {
                reason: format!("line {}: unexpected end of file", self.line),
            }),
        }
    }

    fn parse_number(&mut self) -> Result<PropertyValue> {
        let start = self.pos;
        let bytes = self.source.as_bytes();
        if self.pos < bytes.len() && bytes[self.pos] == b'-' {
            self.pos += 1;
        }
        while self.pos < bytes.len() && (bytes[self.pos] as char).is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos < bytes.len() && bytes[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < bytes.len() && (bytes[self.pos] as char).is_ascii_digit() {
                self.pos += 1;
            }
        }
        let s = &self.source[start..self.pos];
        let n: f64 = s.parse().map_err(|_| PiCloudError::ResourceValidationFailed {
            reason: format!("line {}: invalid number '{}'", self.line, s),
        })?;
        Ok(PropertyValue::Number(n))
    }

    fn parse_bool(&mut self) -> Result<PropertyValue> {
        if self.remaining().starts_with("true") {
            // Make sure it's not just a prefix of an identifier
            let after = self.source.as_bytes().get(self.pos + 4).copied();
            if after.is_none() || !(after.unwrap() as char).is_ascii_alphanumeric() {
                self.pos += 4;
                return Ok(PropertyValue::Bool(true));
            }
        }
        if self.remaining().starts_with("false") {
            let after = self.source.as_bytes().get(self.pos + 5).copied();
            if after.is_none() || !(after.unwrap() as char).is_ascii_alphanumeric() {
                self.pos += 5;
                return Ok(PropertyValue::Bool(false));
            }
        }
        Err(PiCloudError::ResourceValidationFailed {
            reason: format!("line {}: expected 'true' or 'false'", self.line),
        })
    }

    fn parse_secret(&mut self) -> Result<PropertyValue> {
        // secret("name")
        self.pos += 7; // skip "secret("
        self.skip_ws();
        let name = self.parse_string()?;
        self.skip_ws();
        self.expect_char(')')?;
        Ok(PropertyValue::Secret(name))
    }

    fn parse_block_value(&mut self) -> Result<PropertyValue> {
        self.expect_char('{')?;
        let props = self.parse_properties()?;
        self.expect_char('}')?;
        Ok(PropertyValue::Block(props))
    }

    /// Parse properties inside a block until '}'
    fn parse_properties(&mut self) -> Result<Vec<Property>> {
        let mut props: Vec<Property> = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some('}') | None => break,
                _ => {}
            }
            let line = self.line;
            let key = self.parse_ident()?;
            self.skip_ws();

            match self.peek() {
                Some('=') => {
                    // key = value
                    self.advance(); // consume '='
                    let value = self.parse_value()?;
                    // Optional semicolon separator
                    self.skip_ws();
                    if self.peek() == Some(';') {
                        self.advance();
                    }
                    // Check if this key already exists -- if so, merge into Blocks
                    if let Some(existing) = props.iter_mut().find(|p| p.key == key) {
                        match &mut existing.value {
                            PropertyValue::Blocks(blocks) => {
                                if let PropertyValue::Block(new_props) = value {
                                    blocks.push(new_props);
                                }
                            }
                            _ => {}
                        }
                    } else {
                        props.push(Property { key, value, line });
                    }
                }
                Some('{') => {
                    // key { ... } -- nested block
                    let block_value = self.parse_block_value()?;
                    let block_props = match block_value {
                        PropertyValue::Block(p) => p,
                        _ => unreachable!(),
                    };
                    // Check if this key already exists -- if so it's a repeated block
                    if let Some(existing) = props.iter_mut().find(|p| p.key == key) {
                        match &mut existing.value {
                            PropertyValue::Blocks(blocks) => {
                                blocks.push(block_props);
                            }
                            PropertyValue::Block(first_props) => {
                                let first = std::mem::take(first_props);
                                existing.value = PropertyValue::Blocks(vec![first, block_props]);
                            }
                            _ => {
                                // Shouldn't happen -- duplicate key with different types
                                return Err(PiCloudError::ResourceValidationFailed {
                                    reason: format!(
                                        "line {}: duplicate key '{}' with conflicting types",
                                        line, key
                                    ),
                                });
                            }
                        }
                    } else {
                        props.push(Property {
                            key,
                            value: PropertyValue::Block(block_props),
                            line,
                        });
                    }
                }
                other => {
                    return Err(PiCloudError::ResourceValidationFailed {
                        reason: format!(
                            "line {}: expected '=' or '{{' after property name '{}', found {:?}",
                            self.line, key, other
                        ),
                    });
                }
            }
        }
        Ok(props)
    }

    /// Parse a top-level resource declaration: resource_type "name" { ... }
    fn parse_resource(&mut self) -> Result<ResourceDecl> {
        let line = self.line;
        let resource_type = self.parse_ident()?;
        let name = self.parse_string()?;
        self.expect_char('{')?;
        let properties = self.parse_properties()?;
        self.expect_char('}')?;
        Ok(ResourceDecl {
            resource_type,
            name,
            properties,
            line,
        })
    }

    /// Parse the entire file: a sequence of resource declarations
    fn parse_file(&mut self, path: &str) -> Result<ParsedFile> {
        let mut resources = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                break;
            }
            resources.push(self.parse_resource()?);
        }
        Ok(ParsedFile {
            path: path.to_string(),
            resources,
        })
    }
}

/// Parse a .picloud file from a string
pub fn parse(source: &str, path: &str) -> Result<ParsedFile> {
    let mut parser = Parser::new(source);
    parser.parse_file(path)
}

/// Parse a directory of .picloud and .ttl files
pub fn parse_directory(dir_path: &str) -> Result<Vec<ParsedFile>> {
    let mut files = Vec::new();
    let path = std::path::Path::new(dir_path);
    if !path.exists() {
        return Err(PiCloudError::Internal(format!(
            "Directory does not exist: {}", dir_path
        )));
    }
    if path.is_file() {
        // Single file
        let source = std::fs::read_to_string(path)
            .map_err(|e| PiCloudError::Internal(format!("Failed to read {}: {}", dir_path, e)))?;
        if dir_path.ends_with(".picloud") {
            files.push(parse(&source, dir_path)?);
        }
        return Ok(files);
    }

    let entries = std::fs::read_dir(path)
        .map_err(|e| PiCloudError::Internal(format!("Failed to read directory {}: {}", dir_path, e)))?;

    for entry in entries {
        let entry = entry
            .map_err(|e| PiCloudError::Internal(format!("Failed to read entry: {}", e)))?;
        let entry_path = entry.path();
        if let Some(ext) = entry_path.extension() {
            if ext == "picloud" {
                let source = std::fs::read_to_string(&entry_path)
                    .map_err(|e| PiCloudError::Internal(format!(
                        "Failed to read {}: {}", entry_path.display(), e
                    )))?;
                files.push(parse(&source, &entry_path.to_string_lossy())?);
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_product() {
        let source = r#"
product "photo-app" {
  version = "1.0.0"
  description = "Photo sharing application"
}
"#;
        let file = parse(source, "test.picloud").unwrap();
        assert_eq!(file.resources.len(), 1);
        let r = &file.resources[0];
        assert_eq!(r.resource_type, "product");
        assert_eq!(r.name, "photo-app");
        assert_eq!(r.properties.len(), 2);
    }

    #[test]
    fn parse_container_with_nested_blocks() {
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
        let file = parse(source, "test.picloud").unwrap();
        assert_eq!(file.resources.len(), 1);
        let r = &file.resources[0];
        assert_eq!(r.resource_type, "container");
        assert_eq!(r.name, "api-server");

        // mount is a single block
        let mount = r.properties.iter().find(|p| p.key == "mount").unwrap();
        assert!(matches!(mount.value, PropertyValue::Block(_)));

        // tag is a repeated block (Blocks)
        let tag = r.properties.iter().find(|p| p.key == "tag").unwrap();
        match &tag.value {
            PropertyValue::Blocks(blocks) => assert_eq!(blocks.len(), 2),
            _ => panic!("Expected Blocks for tag"),
        }
    }

    #[test]
    fn parse_volume_with_deep_nesting() {
        let source = r#"
volume "media-store" {
  product    = "photo-app"
  size       = "500GB"
  durability = "full-replication"
  snapshots {
    enabled  = true
    schedule = "daily"
    storage  = secret("nas-config")
    retention {
      daily   = 30
      weekly  = 26
      monthly = 0
    }
  }
  offsite {
    enabled    = true
    target     = secret("backblaze-config")
    frequency  = "daily"
    encryption = true
  }
}
"#;
        let file = parse(source, "test.picloud").unwrap();
        assert_eq!(file.resources.len(), 1);
        let r = &file.resources[0];
        assert_eq!(r.resource_type, "volume");
        assert_eq!(r.name, "media-store");

        // snapshots should be a nested block
        let snap = r.properties.iter().find(|p| p.key == "snapshots").unwrap();
        if let PropertyValue::Block(props) = &snap.value {
            assert!(props.iter().any(|p| p.key == "enabled"));
            // retention is nested inside snapshots
            let retention = props.iter().find(|p| p.key == "retention").unwrap();
            assert!(matches!(retention.value, PropertyValue::Block(_)));
            // storage should be a secret reference
            let storage = props.iter().find(|p| p.key == "storage").unwrap();
            assert!(matches!(storage.value, PropertyValue::Secret(_)));
        } else {
            panic!("Expected Block for snapshots");
        }
    }

    #[test]
    fn parse_feature_flag() {
        let source = r#"
feature-flag "new-upload-flow" {
  product     = "photo-app"
  description = "Redesigned upload flow"
  enabled     = true
  version     = ">= 2"
}
"#;
        let file = parse(source, "test.picloud").unwrap();
        assert_eq!(file.resources.len(), 1);
        let r = &file.resources[0];
        assert_eq!(r.resource_type, "feature-flag");
        assert_eq!(r.name, "new-upload-flow");
    }

    #[test]
    fn parse_multiple_resources() {
        let source = r#"
product "photo-app" {
  version = "1.0.0"
}

container "api-server" {
  product = "photo-app"
  image   = "api:1.0"
}
"#;
        let file = parse(source, "test.picloud").unwrap();
        assert_eq!(file.resources.len(), 2);
        assert_eq!(file.resources[0].resource_type, "product");
        assert_eq!(file.resources[1].resource_type, "container");
    }

    #[test]
    fn parse_comments() {
        let source = r#"
// This is a comment
product "test" {
  // Another comment
  version = "1.0.0"
}
"#;
        let file = parse(source, "test.picloud").unwrap();
        assert_eq!(file.resources.len(), 1);
    }

    #[test]
    fn parse_numbers() {
        let source = r#"
product "test" {
  version = "1.0.0"
  replicas = 3
  ratio = 0.5
}
"#;
        let file = parse(source, "test.picloud").unwrap();
        let r = &file.resources[0];
        let replicas = r.properties.iter().find(|p| p.key == "replicas").unwrap();
        assert!(matches!(replicas.value, PropertyValue::Number(n) if (n - 3.0).abs() < f64::EPSILON));
    }

    #[test]
    fn parse_error_unterminated_string() {
        let source = r#"product "test {
  version = "1.0.0"
}"#;
        assert!(parse(source, "test.picloud").is_err());
    }

    #[test]
    fn parse_error_missing_brace() {
        let source = r#"product "test"
  version = "1.0.0"
}"#;
        assert!(parse(source, "test.picloud").is_err());
    }
}
