/// `picloud new` command handler
///
/// Generates .picloud resource files from flags or interactive prompts.
/// After writing, runs picloud compile validate automatically.
/// Refuses to overwrite existing files unless --overwrite is specified.
///
/// ADR-050: Builder Pattern CLI for Resource Generation

use picloud_compiler::builder::ResourceBuilder;
use picloud_domain::error::{PiCloudError, Result};
use std::path::Path;

/// Options common to all `picloud new` subcommands
pub struct NewOptions {
    pub output: Option<String>,
    pub overwrite: bool,
}

/// Write a generated .picloud file -- checking for overwrite conflicts
pub fn write_resource_file(
    content: &str,
    resource_type: &str,
    name: &str,
    opts: &NewOptions,
) -> Result<String> {
    let path = match &opts.output {
        Some(p) => p.clone(),
        None => default_output_path(resource_type, name),
    };

    if Path::new(&path).exists() && !opts.overwrite {
        return Err(PiCloudError::ResourceAlreadyExists {
            iri: format!(
                "{} -- use --overwrite to replace it",
                path
            ),
        });
    }

    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            PiCloudError::Internal(format!(
                "Failed to create directory {}: {}",
                parent.display(),
                e
            ))
        })?;
    }

    std::fs::write(&path, content).map_err(|e| {
        PiCloudError::Internal(format!("Failed to write {}: {}", path, e))
    })?;

    Ok(path)
}

/// Default output path when --output is not specified
fn default_output_path(resource_type: &str, name: &str) -> String {
    let dir = match resource_type {
        "binary" => "binaries",
        "inference-rule" => "inference-rules",
        "event-subscription" => "event-subscriptions",
        "feature-flag" => "feature-flags",
        other => return format!("./{}s/{}.picloud", other, name),
    };
    format!("./{}/{}.picloud", dir, name)
}

/// Parse "key=value" tag strings into (key, value) pairs
pub fn parse_tags(tags: &[String]) -> Result<Vec<(String, String)>> {
    tags.iter()
        .map(|t| {
            t.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| PiCloudError::ResourceValidationFailed {
                    reason: format!("Tag '{}' must be in key=value format", t),
                })
        })
        .collect()
}

/// Parse "volume:path" mount strings into (volume, path) pairs
pub fn parse_mounts(mounts: &[String]) -> Result<Vec<(String, String)>> {
    mounts
        .iter()
        .map(|m| {
            m.split_once(':')
                .map(|(vol, path)| (vol.to_string(), path.to_string()))
                .ok_or_else(|| PiCloudError::ResourceValidationFailed {
                    reason: format!("Mount '{}' must be in volume:path format", m),
                })
        })
        .collect()
}

/// Build a product .picloud file
pub fn build_product(
    name: &str,
    version: &str,
    description: Option<&str>,
    tags: &[String],
    opts: &NewOptions,
) -> Result<String> {
    let mut builder = ResourceBuilder::new("product")?.name(name);
    builder = builder.property("version", version);
    if let Some(desc) = description {
        builder = builder.property("description", desc);
    }
    if !tags.is_empty() {
        builder = builder.pairs("tag", parse_tags(tags)?);
    }
    builder.validate_required()?;
    let content = builder.render()?;
    let path = write_resource_file(&content, "product", name, opts)?;
    validate_generated_file(&path);
    Ok(path)
}

/// Build a container .picloud file
pub fn build_container(
    name: &str,
    product: &str,
    image: &str,
    identity: Option<&str>,
    mounts: &[String],
    tags: &[String],
    opts: &NewOptions,
) -> Result<String> {
    let mut builder = ResourceBuilder::new("container")?.name(name);
    builder = builder.property("product", product);
    builder = builder.property("image", image);
    if let Some(id) = identity {
        builder = builder.property("identity", id);
    }
    if !mounts.is_empty() {
        let mount_pairs = parse_mounts(mounts)?;
        for (vol, path) in &mount_pairs {
            builder = builder.property("mount", &format!("{}:{}", vol, path));
        }
    }
    if !tags.is_empty() {
        builder = builder.pairs("tag", parse_tags(tags)?);
    }
    builder.validate_required()?;
    let content = builder.render()?;
    let path = write_resource_file(&content, "container", name, opts)?;
    validate_generated_file(&path);
    Ok(path)
}

/// Build a volume .picloud file
pub fn build_volume(
    name: &str,
    product: &str,
    size: &str,
    durability: Option<&str>,
    tags: &[String],
    opts: &NewOptions,
) -> Result<String> {
    let mut builder = ResourceBuilder::new("volume")?.name(name);
    builder = builder.property("product", product);
    builder = builder.property("size", size);
    if let Some(d) = durability {
        builder = builder.property("durability", d);
    }
    if !tags.is_empty() {
        builder = builder.pairs("tag", parse_tags(tags)?);
    }
    builder.validate_required()?;
    let content = builder.render()?;
    let path = write_resource_file(&content, "volume", name, opts)?;
    validate_generated_file(&path);
    Ok(path)
}

/// Build a feature-flag .picloud file
pub fn build_feature_flag(
    name: &str,
    product: &str,
    version: &str,
    enabled: bool,
    description: Option<&str>,
    opts: &NewOptions,
) -> Result<String> {
    let mut builder = ResourceBuilder::new("feature-flag")?.name(name);
    builder = builder.property("product", product);
    builder = builder.property("version", version);
    builder = builder.property("enabled", if enabled { "true" } else { "false" });
    if let Some(desc) = description {
        builder = builder.property("description", desc);
    }
    builder.validate_required()?;
    let content = builder.render()?;
    let path = write_resource_file(&content, "feature-flag", name, opts)?;
    validate_generated_file(&path);
    Ok(path)
}

/// Build an ingress .picloud file
pub fn build_ingress(
    name: &str,
    product: &str,
    target: &str,
    port: u16,
    host: Option<&str>,
    opts: &NewOptions,
) -> Result<String> {
    let mut builder = ResourceBuilder::new("ingress")?.name(name);
    builder = builder.property("product", product);
    builder = builder.property("target", target);
    builder = builder.property("port", &port.to_string());
    if let Some(h) = host {
        builder = builder.property("host", h);
    }
    builder.validate_required()?;
    let content = builder.render()?;
    let path = write_resource_file(&content, "ingress", name, opts)?;
    validate_generated_file(&path);
    Ok(path)
}

/// Build a binary .picloud file
pub fn build_binary(
    name: &str,
    product: &str,
    executable: &str,
    identity: Option<&str>,
    tags: &[String],
    opts: &NewOptions,
) -> Result<String> {
    let mut builder = ResourceBuilder::new("binary")?.name(name);
    builder = builder.property("product", product);
    builder = builder.property("executable", executable);
    if let Some(id) = identity {
        builder = builder.property("identity", id);
    }
    if !tags.is_empty() {
        builder = builder.pairs("tag", parse_tags(tags)?);
    }
    builder.validate_required()?;
    let content = builder.render()?;
    let path = write_resource_file(&content, "binary", name, opts)?;
    validate_generated_file(&path);
    Ok(path)
}

/// Build a config .picloud file
pub fn build_config(
    name: &str,
    product: &str,
    description: Option<&str>,
    tags: &[String],
    opts: &NewOptions,
) -> Result<String> {
    let mut builder = ResourceBuilder::new("config")?.name(name);
    builder = builder.property("product", product);
    if let Some(desc) = description {
        builder = builder.property("description", desc);
    }
    if !tags.is_empty() {
        builder = builder.pairs("tag", parse_tags(tags)?);
    }
    builder.validate_required()?;
    let content = builder.render()?;
    let path = write_resource_file(&content, "config", name, opts)?;
    validate_generated_file(&path);
    Ok(path)
}

/// Build an inference-rule .picloud file
pub fn build_inference_rule(
    name: &str,
    product: &str,
    description: Option<&str>,
    opts: &NewOptions,
) -> Result<String> {
    let mut builder = ResourceBuilder::new("inference-rule")?.name(name);
    builder = builder.property("product", product);
    builder = builder.property("construct", "# TODO: add SPARQL CONSTRUCT query");
    if let Some(desc) = description {
        builder = builder.property("description", desc);
    }
    builder.validate_required()?;
    let content = builder.render()?;
    let path = write_resource_file(&content, "inference-rule", name, opts)?;
    validate_generated_file(&path);
    Ok(path)
}

/// Build a group .picloud file
pub fn build_group(
    name: &str,
    description: Option<&str>,
    tags: &[String],
    opts: &NewOptions,
) -> Result<String> {
    let mut builder = ResourceBuilder::new("group")?.name(name);
    if let Some(desc) = description {
        builder = builder.property("description", desc);
    }
    if !tags.is_empty() {
        builder = builder.pairs("tag", parse_tags(tags)?);
    }
    builder.validate_required()?;
    let content = builder.render()?;
    let path = write_resource_file(&content, "group", name, opts)?;
    validate_generated_file(&path);
    Ok(path)
}

/// Run offline validation against the generated file
fn validate_generated_file(path: &str) {
    use picloud_compiler::{parser, validator};
    match parser::parse_directory(path) {
        Ok(files) if !files.is_empty() => {
            match validator::validate_offline(&files) {
                Ok(result) => {
                    if result.valid {
                        println!("  Validation passed");
                    } else {
                        eprintln!("  Validation failed:");
                        for e in &result.errors {
                            eprintln!("    {}", e.message);
                        }
                    }
                }
                Err(e) => eprintln!("  Validation error: {}", e),
            }
        }
        Ok(_) => eprintln!("  Warning: no .picloud files parsed for validation"),
        Err(e) => eprintln!("  Validation error: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> String {
        let dir = format!(
            "/tmp/picloud-test-new-cmd-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn generate_product_file() {
        let dir = temp_dir();
        let output = format!("{}/product.picloud", dir);
        let opts = NewOptions {
            output: Some(output.clone()),
            overwrite: false,
        };
        let path = build_product("photo-app", "1.0.0", Some("Photo app"), &[], &opts).unwrap();
        assert_eq!(path, output);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("product \"photo-app\""));
        assert!(content.contains("version  = \"1.0.0\""));
        assert!(content.contains("description  = \"Photo app\""));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn generate_container_with_mounts_and_tags() {
        let dir = temp_dir();
        let output = format!("{}/container.picloud", dir);
        let opts = NewOptions {
            output: Some(output.clone()),
            overwrite: false,
        };
        let mounts = vec!["media-store:/data".to_string()];
        let tags = vec!["team=backend".to_string(), "env=prod".to_string()];
        let path = build_container(
            "api-server",
            "photo-app",
            "photo-api:1.0.0",
            Some("api-worker"),
            &mounts,
            &tags,
            &opts,
        )
        .unwrap();
        assert_eq!(path, output);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("container \"api-server\""));
        assert!(content.contains("image  = \"photo-api:1.0.0\""));
        assert!(content.contains("product  = \"photo-app\""));
        assert!(content.contains("identity  = \"api-worker\""));
        assert!(content.contains("tag { key = \"team\"; value = \"backend\" }"));
        assert!(content.contains("tag { key = \"env\"; value = \"prod\" }"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn overwrite_protection_works() {
        let dir = temp_dir();
        let output = format!("{}/protected.picloud", dir);

        // Write a file first
        fs::write(&output, "existing content").unwrap();

        let opts = NewOptions {
            output: Some(output.clone()),
            overwrite: false,
        };
        let result = build_product("test", "1.0.0", None, &[], &opts);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("--overwrite"));

        // Verify original content is preserved
        let content = fs::read_to_string(&output).unwrap();
        assert_eq!(content, "existing content");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn overwrite_flag_allows_replace() {
        let dir = temp_dir();
        let output = format!("{}/replace.picloud", dir);

        fs::write(&output, "old content").unwrap();

        let opts = NewOptions {
            output: Some(output.clone()),
            overwrite: true,
        };
        let path = build_product("photo-app", "2.0.0", None, &[], &opts).unwrap();
        assert_eq!(path, output);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("product \"photo-app\""));
        assert!(content.contains("version  = \"2.0.0\""));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn default_output_path_pluralises() {
        assert_eq!(default_output_path("product", "app"), "./products/app.picloud");
        assert_eq!(default_output_path("container", "api"), "./containers/api.picloud");
        assert_eq!(default_output_path("binary", "worker"), "./binaries/worker.picloud");
        assert_eq!(
            default_output_path("feature-flag", "new-ui"),
            "./feature-flags/new-ui.picloud"
        );
        assert_eq!(
            default_output_path("inference-rule", "rule1"),
            "./inference-rules/rule1.picloud"
        );
        assert_eq!(default_output_path("volume", "data"), "./volumes/data.picloud");
        assert_eq!(default_output_path("ingress", "web"), "./ingresss/web.picloud");
        assert_eq!(default_output_path("group", "team"), "./groups/team.picloud");
    }

    #[test]
    fn parse_tags_valid() {
        let tags = vec!["team=backend".to_string(), "env=prod".to_string()];
        let result = parse_tags(&tags).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("team".to_string(), "backend".to_string()));
        assert_eq!(result[1], ("env".to_string(), "prod".to_string()));
    }

    #[test]
    fn parse_tags_invalid() {
        let tags = vec!["no-equals".to_string()];
        assert!(parse_tags(&tags).is_err());
    }

    #[test]
    fn parse_mounts_valid() {
        let mounts = vec!["media-store:/data".to_string()];
        let result = parse_mounts(&mounts).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            ("media-store".to_string(), "/data".to_string())
        );
    }

    #[test]
    fn parse_mounts_invalid() {
        let mounts = vec!["no-colon".to_string()];
        assert!(parse_mounts(&mounts).is_err());
    }

    #[test]
    fn generate_volume_file() {
        let dir = temp_dir();
        let output = format!("{}/volume.picloud", dir);
        let opts = NewOptions {
            output: Some(output.clone()),
            overwrite: false,
        };
        let path = build_volume("media-store", "photo-app", "100", Some("full-replication"), &[], &opts).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("volume \"media-store\""));
        assert!(content.contains("size  = \"100\""));
        assert!(content.contains("durability  = \"full-replication\""));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn generate_feature_flag_file() {
        let dir = temp_dir();
        let output = format!("{}/flag.picloud", dir);
        let opts = NewOptions {
            output: Some(output.clone()),
            overwrite: false,
        };
        let path = build_feature_flag("new-upload", "photo-app", ">= 2", true, None, &opts).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("feature-flag \"new-upload\""));
        assert!(content.contains("enabled  = \"true\""));
        assert!(content.contains("version  = \">= 2\""));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn generate_ingress_file() {
        let dir = temp_dir();
        let output = format!("{}/ingress.picloud", dir);
        let opts = NewOptions {
            output: Some(output.clone()),
            overwrite: false,
        };
        let path = build_ingress("web", "photo-app", "api", 8080, Some("photos.picloud.local"), &opts).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("ingress \"web\""));
        assert!(content.contains("target  = \"api\""));
        assert!(content.contains("port  = \"8080\""));
        assert!(content.contains("host  = \"photos.picloud.local\""));
        fs::remove_dir_all(&dir).ok();
    }
}
