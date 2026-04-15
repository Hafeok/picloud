//! SDK Generation Framework
//!
//! Template-based code generation for Rust, TypeScript, and .NET SDKs.
//! Each generated SDK provides a typed client that connects to the PiCloud
//! cluster and exposes `append_event`, `query_graph`, and `get_resource` methods.

use std::fmt;
use std::fs;
use std::path::PathBuf;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::iri::ClusterDomain;

/// Target language for SDK generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkLanguage {
    Rust,
    TypeScript,
    DotNet,
}

impl fmt::Display for SdkLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdkLanguage::Rust => write!(f, "rust"),
            SdkLanguage::TypeScript => write!(f, "typescript"),
            SdkLanguage::DotNet => write!(f, "dotnet"),
        }
    }
}

/// Result of generating an SDK for a single language.
#[derive(Debug, Clone)]
pub struct SdkGenerationResult {
    pub language: SdkLanguage,
    pub output_path: PathBuf,
    pub files_generated: Vec<String>,
}

/// Result of publishing an SDK to a package registry.
#[derive(Debug, Clone)]
pub struct SdkPublishResult {
    pub language: SdkLanguage,
    pub output_path: PathBuf,
    pub dry_run: bool,
    pub command: String,
}

/// Generates typed SDK clients from template files.
pub struct SdkGenerator {
    pub cluster_domain: ClusterDomain,
    pub output_dir: PathBuf,
}

impl SdkGenerator {
    pub fn new(cluster_domain: ClusterDomain, output_dir: PathBuf) -> Self {
        Self {
            cluster_domain,
            output_dir,
        }
    }

    /// Generate an SDK for the specified language.
    pub fn generate(&self, language: SdkLanguage) -> Result<SdkGenerationResult> {
        match language {
            SdkLanguage::Rust => self.generate_rust_sdk(),
            SdkLanguage::TypeScript => self.generate_typescript_sdk(),
            SdkLanguage::DotNet => self.generate_dotnet_sdk(),
        }
    }

    /// Generate SDKs for all supported languages.
    pub fn generate_all(&self) -> Result<Vec<SdkGenerationResult>> {
        let languages = [SdkLanguage::Rust, SdkLanguage::TypeScript, SdkLanguage::DotNet];
        let mut results = Vec::with_capacity(languages.len());
        for lang in languages {
            results.push(self.generate(lang)?);
        }
        Ok(results)
    }

    /// Publish a previously generated SDK to its package registry.
    ///
    /// When `dry_run` is true (the default), the publish command is constructed
    /// but not executed. Set `dry_run` to false (via `--publish` flag) to
    /// actually run the registry publish command.
    ///
    /// - Rust:       `cargo publish` in the generated crate directory
    /// - TypeScript: `npm publish` in the generated package directory
    /// - .NET:       `dotnet nuget push` in the generated package directory
    pub fn publish(
        &self,
        language: SdkLanguage,
        dry_run: bool,
        registry: Option<&str>,
    ) -> Result<SdkPublishResult> {
        // Ensure the SDK has been generated first
        let gen_result = self.generate(language)?;

        let command = match language {
            SdkLanguage::Rust => {
                let mut cmd = "cargo publish".to_string();
                if dry_run {
                    cmd.push_str(" --dry-run");
                }
                if let Some(reg) = registry {
                    cmd.push_str(&format!(" --registry {}", reg));
                }
                cmd
            }
            SdkLanguage::TypeScript => {
                let mut cmd = "npm publish".to_string();
                if dry_run {
                    cmd.push_str(" --dry-run");
                }
                if let Some(reg) = registry {
                    cmd.push_str(&format!(" --registry {}", reg));
                }
                cmd
            }
            SdkLanguage::DotNet => {
                let nupkg_pattern = gen_result.output_path.join("*.nupkg");
                let mut cmd = format!(
                    "dotnet nuget push {}",
                    nupkg_pattern.display()
                );
                if let Some(reg) = registry {
                    cmd.push_str(&format!(" --source {}", reg));
                } else {
                    cmd.push_str(" --source https://api.nuget.org/v3/index.json");
                }
                if dry_run {
                    // dotnet nuget push has no built-in dry-run; we just skip execution
                    cmd.push_str(" # (dry-run — not executed)");
                }
                cmd
            }
        };

        if !dry_run {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .current_dir(&gen_result.output_path)
                .output()
                .map_err(|e| PiCloudError::Internal(format!(
                    "Failed to run publish command for {}: {}", language, e
                )))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(PiCloudError::Internal(format!(
                    "Publish failed for {}: {}", language, stderr
                )));
            }
        }

        Ok(SdkPublishResult {
            language,
            output_path: gen_result.output_path,
            dry_run,
            command,
        })
    }

    /// Publish SDKs for all supported languages.
    pub fn publish_all(
        &self,
        dry_run: bool,
        registry: Option<&str>,
    ) -> Result<Vec<SdkPublishResult>> {
        let languages = [SdkLanguage::Rust, SdkLanguage::TypeScript, SdkLanguage::DotNet];
        let mut results = Vec::with_capacity(languages.len());
        for lang in languages {
            results.push(self.publish(lang, dry_run, registry)?);
        }
        Ok(results)
    }

    /// Generate the Rust SDK: `Cargo.toml` and `src/lib.rs`.
    fn generate_rust_sdk(&self) -> Result<SdkGenerationResult> {
        let base_url = format!("https://{}", self.cluster_domain.0);
        let sdk_dir = self.output_dir.join("rust");
        let src_dir = sdk_dir.join("src");
        Self::ensure_dir(&src_dir)?;

        let cargo_toml = format!(
            r#"[package]
name = "picloud-sdk"
version = "0.1.0"
edition = "2021"
description = "Generated PiCloud SDK client"

[dependencies]
reqwest = {{ version = "0.11", features = ["json"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tokio = {{ version = "1", features = ["full"] }}
"#
        );

        let lib_rs = format!(
            r#"//! PiCloud SDK — generated client library
//!
//! Base URL: {base_url}

use serde::{{Deserialize, Serialize}};
use serde_json::Value;

/// PiCloud platform client.
pub struct PiCloudClient {{
    base_url: String,
    client: reqwest::Client,
}}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventEnvelope {{
    pub event_type: String,
    pub payload: Value,
}}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResult {{
    pub bindings: Vec<Value>,
}}

impl PiCloudClient {{
    /// Create a new client targeting the PiCloud cluster.
    pub fn new() -> Self {{
        Self {{
            base_url: "{base_url}".to_string(),
            client: reqwest::Client::new(),
        }}
    }}

    /// Append an event to the platform event log.
    pub async fn append_event(&self, envelope: &EventEnvelope) -> Result<(), Box<dyn std::error::Error>> {{
        self.client
            .post(format!("{{}}/api/events", self.base_url))
            .json(envelope)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }}

    /// Execute a SPARQL query against the platform graph.
    pub async fn query_graph(&self, sparql: &str) -> Result<QueryResult, Box<dyn std::error::Error>> {{
        let result = self.client
            .post(format!("{{}}/api/sparql", self.base_url))
            .body(sparql.to_string())
            .send()
            .await?
            .error_for_status()?
            .json::<QueryResult>()
            .await?;
        Ok(result)
    }}

    /// Fetch a resource by its IRI path.
    pub async fn get_resource(&self, path: &str) -> Result<Value, Box<dyn std::error::Error>> {{
        let result = self.client
            .get(format!("{{}}/{{}}", self.base_url, path.trim_start_matches('/')))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        Ok(result)
    }}
}}
"#,
            base_url = base_url,
        );

        Self::write_file(&sdk_dir.join("Cargo.toml"), &cargo_toml)?;
        Self::write_file(&src_dir.join("lib.rs"), &lib_rs)?;

        Ok(SdkGenerationResult {
            language: SdkLanguage::Rust,
            output_path: sdk_dir,
            files_generated: vec!["Cargo.toml".to_string(), "src/lib.rs".to_string()],
        })
    }

    /// Generate the TypeScript SDK: `package.json` and `src/index.ts`.
    fn generate_typescript_sdk(&self) -> Result<SdkGenerationResult> {
        let base_url = format!("https://{}", self.cluster_domain.0);
        let sdk_dir = self.output_dir.join("typescript");
        let src_dir = sdk_dir.join("src");
        Self::ensure_dir(&src_dir)?;

        let package_json = r#"{
  "name": "@picloud/sdk",
  "version": "0.1.0",
  "description": "Generated PiCloud SDK client",
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "scripts": {
    "build": "tsc",
    "test": "jest"
  },
  "dependencies": {},
  "devDependencies": {
    "typescript": "^5.0.0"
  }
}
"#;

        let index_ts = format!(
            r#"/**
 * PiCloud SDK — generated client library
 *
 * Base URL: {base_url}
 */

export interface EventEnvelope {{
  eventType: string;
  payload: Record<string, unknown>;
}}

export interface QueryResult {{
  bindings: Record<string, unknown>[];
}}

export class PiCloudClient {{
  private readonly baseUrl: string;

  constructor(baseUrl: string = "{base_url}") {{
    this.baseUrl = baseUrl;
  }}

  /** Append an event to the platform event log. */
  async appendEvent(envelope: EventEnvelope): Promise<void> {{
    const response = await fetch(`${{this.baseUrl}}/api/events`, {{
      method: "POST",
      headers: {{ "Content-Type": "application/json" }},
      body: JSON.stringify(envelope),
    }});
    if (!response.ok) {{
      throw new Error(`appendEvent failed: ${{response.status}}`);
    }}
  }}

  /** Execute a SPARQL query against the platform graph. */
  async queryGraph(sparql: string): Promise<QueryResult> {{
    const response = await fetch(`${{this.baseUrl}}/api/sparql`, {{
      method: "POST",
      headers: {{ "Content-Type": "application/sparql-query" }},
      body: sparql,
    }});
    if (!response.ok) {{
      throw new Error(`queryGraph failed: ${{response.status}}`);
    }}
    return response.json() as Promise<QueryResult>;
  }}

  /** Fetch a resource by its IRI path. */
  async getResource(path: string): Promise<Record<string, unknown>> {{
    const cleanPath = path.startsWith("/") ? path.slice(1) : path;
    const response = await fetch(`${{this.baseUrl}}/${{cleanPath}}`);
    if (!response.ok) {{
      throw new Error(`getResource failed: ${{response.status}}`);
    }}
    return response.json() as Promise<Record<string, unknown>>;
  }}
}}
"#,
            base_url = base_url,
        );

        Self::write_file(&sdk_dir.join("package.json"), package_json)?;
        Self::write_file(&src_dir.join("index.ts"), &index_ts)?;

        Ok(SdkGenerationResult {
            language: SdkLanguage::TypeScript,
            output_path: sdk_dir,
            files_generated: vec!["package.json".to_string(), "src/index.ts".to_string()],
        })
    }

    /// Generate the .NET Aspire integration package: companion package to
    /// PiCloud.Sdk that registers PiCloud resources as Aspire hosting
    /// components.
    ///
    /// Generates:
    /// - `PiCloud.Sdk.Aspire.csproj` (NuGet package with Aspire.Hosting dependency)
    /// - `PiCloudEventStoreResource.cs` (event store as Aspire resource)
    /// - `PiCloudSparqlResource.cs` (SPARQL client as Aspire resource)
    /// - `PiCloudIamResource.cs` (IAM client as Aspire resource)
    /// - `PiCloudResourceExtensions.cs` (builder extension methods)
    pub fn generate_aspire_integration(&self) -> Result<SdkGenerationResult> {
        let base_url = format!("https://{}", self.cluster_domain.0);
        let aspire_dir = self.output_dir.join("dotnet-aspire");
        Self::ensure_dir(&aspire_dir)?;

        // -- PiCloud.Sdk.Aspire.csproj --
        let csproj = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <RootNamespace>PiCloud.Sdk.Aspire</RootNamespace>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <PackageId>PiCloud.Sdk.Aspire</PackageId>
    <Description>PiCloud .NET Aspire integration — event stores, SPARQL, and IAM as Aspire hosting resources</Description>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Aspire.Hosting.Abstractions" Version="8.0.0" />
    <PackageReference Include="System.Net.Http.Json" Version="8.0.0" />
  </ItemGroup>
</Project>
"#;

        // -- PiCloudEventStoreResource.cs --
        let event_store_cs = format!(
            r#"// PiCloud Aspire Integration — Event Store Resource
// Cluster: {base_url}

namespace PiCloud.Sdk.Aspire;

/// <summary>
/// Aspire resource representing a PiCloud event store.
/// Provides connection details for appending and subscribing to events.
/// </summary>
public class PiCloudEventStoreResource(string name)
    : Resource(name), IResourceWithConnectionString
{{
    /// <summary>The PiCloud cluster base URL.</summary>
    public string ClusterUrl {{ get; set; }} = "{base_url}";

    /// <summary>Connection string expression for dependency injection.</summary>
    public ReferenceExpression ConnectionStringExpression =>
        ReferenceExpression.Create(
            $"Endpoint={base_url}/api/events;Store={{Name}}");
}}
"#,
            base_url = base_url,
        );

        // -- PiCloudSparqlResource.cs --
        let sparql_cs = format!(
            r#"// PiCloud Aspire Integration — SPARQL Client Resource
// Cluster: {base_url}

namespace PiCloud.Sdk.Aspire;

/// <summary>
/// Aspire resource representing a PiCloud SPARQL query client.
/// Provides connection details for executing SPARQL queries against the
/// platform RDF graph or a product's named graph.
/// </summary>
public class PiCloudSparqlResource(string name)
    : Resource(name), IResourceWithConnectionString
{{
    /// <summary>The PiCloud cluster base URL.</summary>
    public string ClusterUrl {{ get; set; }} = "{base_url}";

    /// <summary>Connection string expression for dependency injection.</summary>
    public ReferenceExpression ConnectionStringExpression =>
        ReferenceExpression.Create(
            $"Endpoint={base_url}/api/sparql;Client={{Name}}");
}}
"#,
            base_url = base_url,
        );

        // -- PiCloudIamResource.cs --
        let iam_cs = format!(
            r#"// PiCloud Aspire Integration — IAM Client Resource
// Cluster: {base_url}

namespace PiCloud.Sdk.Aspire;

/// <summary>
/// Aspire resource representing a PiCloud IAM client.
/// Provides connection details for token issuance, validation, and
/// workload identity via the platform OIDC provider.
/// </summary>
public class PiCloudIamResource(string name)
    : Resource(name), IResourceWithConnectionString
{{
    /// <summary>The PiCloud cluster base URL.</summary>
    public string ClusterUrl {{ get; set; }} = "{base_url}";

    /// <summary>Connection string expression for dependency injection.</summary>
    public ReferenceExpression ConnectionStringExpression =>
        ReferenceExpression.Create(
            $"Endpoint={base_url}/api/iam;Client={{Name}}");
}}
"#,
            base_url = base_url,
        );

        // -- PiCloudResourceExtensions.cs --
        let extensions_cs = format!(
            r#"// PiCloud Aspire Integration — Builder Extension Methods
// Cluster: {base_url}

using Aspire.Hosting;
using Aspire.Hosting.ApplicationModel;

namespace PiCloud.Sdk.Aspire;

/// <summary>
/// Extension methods for adding PiCloud resources to an Aspire
/// <see cref="IDistributedApplicationBuilder"/>.
/// </summary>
public static class PiCloudResourceExtensions
{{
    /// <summary>
    /// Add a PiCloud event store resource to the application model.
    /// </summary>
    /// <param name="builder">The distributed application builder.</param>
    /// <param name="name">A name for the event store resource.</param>
    /// <returns>A resource builder for further configuration.</returns>
    /// <example>
    /// <code>
    /// var photoStore = builder.AddPiCloudEventStore("photos");
    /// builder.AddProject&lt;Projects.PhotoApi&gt;("api")
    ///     .WithReference(photoStore);
    /// </code>
    /// </example>
    public static IResourceBuilder<PiCloudEventStoreResource> AddPiCloudEventStore(
        this IDistributedApplicationBuilder builder,
        string name)
    {{
        var resource = new PiCloudEventStoreResource(name);
        return builder.AddResource(resource)
            .WithInitialState(new CustomResourceSnapshot
            {{
                ResourceType = "PiCloud Event Store",
                State = new ResourceStateSnapshot("Running", KnownResourceStateStyles.Success),
                Properties = [new("ClusterUrl", "{base_url}")]
            }});
    }}

    /// <summary>
    /// Add a PiCloud SPARQL client resource to the application model.
    /// </summary>
    /// <param name="builder">The distributed application builder.</param>
    /// <param name="name">A name for the SPARQL client resource.</param>
    /// <returns>A resource builder for further configuration.</returns>
    /// <example>
    /// <code>
    /// var sparql = builder.AddPiCloudSparqlClient("graph");
    /// builder.AddProject&lt;Projects.QueryService&gt;("queries")
    ///     .WithReference(sparql);
    /// </code>
    /// </example>
    public static IResourceBuilder<PiCloudSparqlResource> AddPiCloudSparqlClient(
        this IDistributedApplicationBuilder builder,
        string name)
    {{
        var resource = new PiCloudSparqlResource(name);
        return builder.AddResource(resource)
            .WithInitialState(new CustomResourceSnapshot
            {{
                ResourceType = "PiCloud SPARQL Client",
                State = new ResourceStateSnapshot("Running", KnownResourceStateStyles.Success),
                Properties = [new("ClusterUrl", "{base_url}")]
            }});
    }}

    /// <summary>
    /// Add a PiCloud IAM client resource to the application model.
    /// </summary>
    /// <param name="builder">The distributed application builder.</param>
    /// <param name="name">A name for the IAM client resource.</param>
    /// <returns>A resource builder for further configuration.</returns>
    /// <example>
    /// <code>
    /// var iam = builder.AddPiCloudIamClient("auth");
    /// builder.AddProject&lt;Projects.AuthService&gt;("auth-svc")
    ///     .WithReference(iam);
    /// </code>
    /// </example>
    public static IResourceBuilder<PiCloudIamResource> AddPiCloudIamClient(
        this IDistributedApplicationBuilder builder,
        string name)
    {{
        var resource = new PiCloudIamResource(name);
        return builder.AddResource(resource)
            .WithInitialState(new CustomResourceSnapshot
            {{
                ResourceType = "PiCloud IAM Client",
                State = new ResourceStateSnapshot("Running", KnownResourceStateStyles.Success),
                Properties = [new("ClusterUrl", "{base_url}")]
            }});
    }}
}}
"#,
            base_url = base_url,
        );

        Self::write_file(&aspire_dir.join("PiCloud.Sdk.Aspire.csproj"), &csproj)?;
        Self::write_file(&aspire_dir.join("PiCloudEventStoreResource.cs"), &event_store_cs)?;
        Self::write_file(&aspire_dir.join("PiCloudSparqlResource.cs"), &sparql_cs)?;
        Self::write_file(&aspire_dir.join("PiCloudIamResource.cs"), &iam_cs)?;
        Self::write_file(&aspire_dir.join("PiCloudResourceExtensions.cs"), &extensions_cs)?;

        Ok(SdkGenerationResult {
            language: SdkLanguage::DotNet,
            output_path: aspire_dir,
            files_generated: vec![
                "PiCloud.Sdk.Aspire.csproj".to_string(),
                "PiCloudEventStoreResource.cs".to_string(),
                "PiCloudSparqlResource.cs".to_string(),
                "PiCloudIamResource.cs".to_string(),
                "PiCloudResourceExtensions.cs".to_string(),
            ],
        })
    }

    /// Publish a previously generated Aspire integration package to NuGet.
    pub fn publish_aspire_integration(
        &self,
        dry_run: bool,
        registry: Option<&str>,
    ) -> Result<SdkPublishResult> {
        let gen_result = self.generate_aspire_integration()?;

        let nupkg_pattern = gen_result.output_path.join("*.nupkg");
        let mut command = format!(
            "dotnet nuget push {}",
            nupkg_pattern.display()
        );
        if let Some(reg) = registry {
            command.push_str(&format!(" --source {}", reg));
        } else {
            command.push_str(" --source https://api.nuget.org/v3/index.json");
        }
        if dry_run {
            command.push_str(" # (dry-run — not executed)");
        }

        if !dry_run {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .current_dir(&gen_result.output_path)
                .output()
                .map_err(|e| PiCloudError::Internal(format!(
                    "Failed to run Aspire publish command: {}", e
                )))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(PiCloudError::Internal(format!(
                    "Aspire publish failed: {}", stderr
                )));
            }
        }

        Ok(SdkPublishResult {
            language: SdkLanguage::DotNet,
            output_path: gen_result.output_path,
            dry_run,
            command,
        })
    }

    /// Generate the .NET SDK: `PiCloud.Sdk.csproj` and `PiCloudClient.cs`.
    fn generate_dotnet_sdk(&self) -> Result<SdkGenerationResult> {
        let base_url = format!("https://{}", self.cluster_domain.0);
        let sdk_dir = self.output_dir.join("dotnet");
        Self::ensure_dir(&sdk_dir)?;

        let csproj = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <RootNamespace>PiCloud.Sdk</RootNamespace>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="System.Net.Http.Json" Version="8.0.0" />
  </ItemGroup>
</Project>
"#;

        let client_cs = format!(
            r#"// PiCloud SDK — generated client library
//
// Base URL: {base_url}

using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace PiCloud.Sdk;

public class EventEnvelope
{{
    [JsonPropertyName("eventType")]
    public string EventType {{ get; set; }} = string.Empty;

    [JsonPropertyName("payload")]
    public JsonElement Payload {{ get; set; }}
}}

public class QueryResult
{{
    [JsonPropertyName("bindings")]
    public List<JsonElement> Bindings {{ get; set; }} = new();
}}

public class PiCloudClient : IDisposable
{{
    private readonly HttpClient _httpClient;
    private readonly string _baseUrl;

    public PiCloudClient(string baseUrl = "{base_url}")
    {{
        _baseUrl = baseUrl;
        _httpClient = new HttpClient {{ BaseAddress = new Uri(baseUrl) }};
    }}

    /// <summary>Append an event to the platform event log.</summary>
    public async Task AppendEventAsync(EventEnvelope envelope)
    {{
        var response = await _httpClient.PostAsJsonAsync("/api/events", envelope);
        response.EnsureSuccessStatusCode();
    }}

    /// <summary>Execute a SPARQL query against the platform graph.</summary>
    public async Task<QueryResult> QueryGraphAsync(string sparql)
    {{
        var content = new StringContent(sparql, Encoding.UTF8, "application/sparql-query");
        var response = await _httpClient.PostAsync("/api/sparql", content);
        response.EnsureSuccessStatusCode();
        var result = await response.Content.ReadFromJsonAsync<QueryResult>();
        return result ?? new QueryResult();
    }}

    /// <summary>Fetch a resource by its IRI path.</summary>
    public async Task<JsonElement> GetResourceAsync(string path)
    {{
        var cleanPath = path.TrimStart('/');
        var response = await _httpClient.GetAsync($"/{{cleanPath}}");
        response.EnsureSuccessStatusCode();
        var json = await response.Content.ReadAsStringAsync();
        return JsonSerializer.Deserialize<JsonElement>(json);
    }}

    public void Dispose()
    {{
        _httpClient.Dispose();
    }}
}}
"#,
            base_url = base_url,
        );

        Self::write_file(&sdk_dir.join("PiCloud.Sdk.csproj"), csproj)?;
        Self::write_file(&sdk_dir.join("PiCloudClient.cs"), &client_cs)?;

        Ok(SdkGenerationResult {
            language: SdkLanguage::DotNet,
            output_path: sdk_dir,
            files_generated: vec![
                "PiCloud.Sdk.csproj".to_string(),
                "PiCloudClient.cs".to_string(),
            ],
        })
    }

    /// Create a directory and all parents if they don't exist.
    fn ensure_dir(path: &PathBuf) -> Result<()> {
        fs::create_dir_all(path).map_err(|e| PiCloudError::Internal(format!(
            "Failed to create directory {}: {}", path.display(), e
        )))
    }

    /// Write content to a file, creating parent directories as needed.
    fn write_file(path: &PathBuf, content: &str) -> Result<()> {
        fs::write(path, content).map_err(|e| PiCloudError::Internal(format!(
            "Failed to write file {}: {}", path.display(), e
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_output_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("picloud-sdk-gen-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_generate_rust_sdk() {
        let output = temp_output_dir();
        let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

        let result = gen.generate(SdkLanguage::Rust).unwrap();

        assert_eq!(result.language, SdkLanguage::Rust);
        assert_eq!(result.files_generated.len(), 2);
        assert!(result.files_generated.contains(&"Cargo.toml".to_string()));
        assert!(result.files_generated.contains(&"src/lib.rs".to_string()));
        assert!(output.join("rust/Cargo.toml").exists());
        assert!(output.join("rust/src/lib.rs").exists());

        let lib_content = fs::read_to_string(output.join("rust/src/lib.rs")).unwrap();
        assert!(lib_content.contains("picloud.local"));
        assert!(lib_content.contains("append_event"));
        assert!(lib_content.contains("query_graph"));
        assert!(lib_content.contains("get_resource"));

        cleanup(&output);
    }

    #[test]
    fn test_generate_typescript_sdk() {
        let output = temp_output_dir();
        let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

        let result = gen.generate(SdkLanguage::TypeScript).unwrap();

        assert_eq!(result.language, SdkLanguage::TypeScript);
        assert_eq!(result.files_generated.len(), 2);
        assert!(result.files_generated.contains(&"package.json".to_string()));
        assert!(result.files_generated.contains(&"src/index.ts".to_string()));
        assert!(output.join("typescript/package.json").exists());
        assert!(output.join("typescript/src/index.ts").exists());

        let index_content = fs::read_to_string(output.join("typescript/src/index.ts")).unwrap();
        assert!(index_content.contains("picloud.local"));
        assert!(index_content.contains("appendEvent"));
        assert!(index_content.contains("queryGraph"));
        assert!(index_content.contains("getResource"));

        cleanup(&output);
    }

    #[test]
    fn test_generate_dotnet_sdk() {
        let output = temp_output_dir();
        let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

        let result = gen.generate(SdkLanguage::DotNet).unwrap();

        assert_eq!(result.language, SdkLanguage::DotNet);
        assert_eq!(result.files_generated.len(), 2);
        assert!(result.files_generated.contains(&"PiCloud.Sdk.csproj".to_string()));
        assert!(result.files_generated.contains(&"PiCloudClient.cs".to_string()));
        assert!(output.join("dotnet/PiCloud.Sdk.csproj").exists());
        assert!(output.join("dotnet/PiCloudClient.cs").exists());

        let client_content = fs::read_to_string(output.join("dotnet/PiCloudClient.cs")).unwrap();
        assert!(client_content.contains("picloud.local"));
        assert!(client_content.contains("AppendEventAsync"));
        assert!(client_content.contains("QueryGraphAsync"));
        assert!(client_content.contains("GetResourceAsync"));

        cleanup(&output);
    }

    #[test]
    fn test_generate_all() {
        let output = temp_output_dir();
        let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

        let results = gen.generate_all().unwrap();

        assert_eq!(results.len(), 3);
        assert!(output.join("rust/Cargo.toml").exists());
        assert!(output.join("typescript/package.json").exists());
        assert!(output.join("dotnet/PiCloud.Sdk.csproj").exists());

        cleanup(&output);
    }

    #[test]
    fn test_publish_rust_dry_run() {
        let output = temp_output_dir();
        let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

        let result = gen.publish(SdkLanguage::Rust, true, None).unwrap();

        assert_eq!(result.language, SdkLanguage::Rust);
        assert!(result.dry_run);
        assert!(result.command.contains("cargo publish"));
        assert!(result.command.contains("--dry-run"));
        // Verify the SDK was generated as part of publish
        assert!(output.join("rust/Cargo.toml").exists());

        cleanup(&output);
    }

    #[test]
    fn test_publish_typescript_dry_run() {
        let output = temp_output_dir();
        let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

        let result = gen.publish(SdkLanguage::TypeScript, true, None).unwrap();

        assert!(result.dry_run);
        assert!(result.command.contains("npm publish"));
        assert!(result.command.contains("--dry-run"));

        cleanup(&output);
    }

    #[test]
    fn test_publish_dotnet_dry_run() {
        let output = temp_output_dir();
        let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

        let result = gen.publish(SdkLanguage::DotNet, true, None).unwrap();

        assert!(result.dry_run);
        assert!(result.command.contains("dotnet nuget push"));
        assert!(result.command.contains("dry-run"));

        cleanup(&output);
    }

    #[test]
    fn test_publish_with_custom_registry() {
        let output = temp_output_dir();
        let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

        let result = gen.publish(SdkLanguage::Rust, true, Some("my-registry")).unwrap();
        assert!(result.command.contains("--registry my-registry"));

        let result = gen.publish(SdkLanguage::TypeScript, true, Some("https://npm.example.com")).unwrap();
        assert!(result.command.contains("--registry https://npm.example.com"));

        let result = gen.publish(SdkLanguage::DotNet, true, Some("https://nuget.example.com")).unwrap();
        assert!(result.command.contains("--source https://nuget.example.com"));

        cleanup(&output);
    }

    #[test]
    fn test_publish_all_dry_run() {
        let output = temp_output_dir();
        let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

        let results = gen.publish_all(true, None).unwrap();

        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.dry_run));

        cleanup(&output);
    }

    #[test]
    fn test_custom_cluster_domain() {
        let output = temp_output_dir();
        let domain = ClusterDomain("my-cluster.example.com".to_string());
        let gen = SdkGenerator::new(domain, output.clone());

        let result = gen.generate(SdkLanguage::Rust).unwrap();

        let lib_content = fs::read_to_string(result.output_path.join("src/lib.rs")).unwrap();
        assert!(lib_content.contains("my-cluster.example.com"));
        assert!(!lib_content.contains("picloud.local"));

        cleanup(&output);
    }
}
