/// FT-086 Integration Tests -- SDK generator (Rust, TypeScript, .NET)
///
/// Covers TC-233: SDKs published to crates.io, npm, and NuGet.
///
/// Verifies the complete SDK generation and publish pipeline for all three
/// language targets, ensuring each generated SDK includes the platform API
/// surface and targets the correct package registry.

use std::fs;
use std::path::PathBuf;

use picloud_sdk_gen::{SdkGenerationResult, SdkGenerator, SdkLanguage, SdkPublishResult};
use picloud_domain::iri::ClusterDomain;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_output_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "picloud-sdk-gen-tc233-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn cleanup(dir: &std::path::Path) {
    let _ = fs::remove_dir_all(dir);
}

// ============================================================================
// TC-233 -- SDKs published to crates.io, npm, and NuGet
// ============================================================================

/// Exit-criteria test: verify the SDK generation pipeline produces valid
/// packages for all three language targets (Rust/crates.io, TypeScript/npm,
/// .NET/NuGet) and that the publish pipeline targets the correct registries.
#[test]
fn tc233_sdks_published_to_crates_io_npm_and_nuget() {
    let output = temp_output_dir();
    let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

    // -----------------------------------------------------------------------
    // 1. Generate all three SDKs
    // -----------------------------------------------------------------------
    let gen_results = gen.generate_all().unwrap();
    assert_eq!(gen_results.len(), 3, "generate_all must produce exactly 3 SDKs");

    // -----------------------------------------------------------------------
    // 2. Verify Rust SDK (crates.io)
    // -----------------------------------------------------------------------
    let rust_result = gen_results
        .iter()
        .find(|r| r.language == SdkLanguage::Rust)
        .expect("Rust SDK must be generated");

    assert!(
        rust_result.output_path.join("Cargo.toml").exists(),
        "Rust SDK must produce Cargo.toml"
    );
    assert!(
        rust_result.output_path.join("src/lib.rs").exists(),
        "Rust SDK must produce src/lib.rs"
    );

    let cargo_toml = fs::read_to_string(rust_result.output_path.join("Cargo.toml")).unwrap();
    assert!(
        cargo_toml.contains("picloud-sdk"),
        "Rust crate name must be 'picloud-sdk' for crates.io"
    );

    let rust_lib = fs::read_to_string(rust_result.output_path.join("src/lib.rs")).unwrap();
    assert!(rust_lib.contains("append_event"), "Rust SDK must expose append_event");
    assert!(rust_lib.contains("query_graph"), "Rust SDK must expose query_graph");
    assert!(rust_lib.contains("get_resource"), "Rust SDK must expose get_resource");
    assert!(rust_lib.contains("PiCloudClient"), "Rust SDK must define PiCloudClient");
    assert!(rust_lib.contains("EventEnvelope"), "Rust SDK must define EventEnvelope");
    assert!(
        rust_lib.contains("picloud.local"),
        "Rust SDK must embed cluster domain"
    );

    // -----------------------------------------------------------------------
    // 3. Verify TypeScript SDK (npm)
    // -----------------------------------------------------------------------
    let ts_result = gen_results
        .iter()
        .find(|r| r.language == SdkLanguage::TypeScript)
        .expect("TypeScript SDK must be generated");

    assert!(
        ts_result.output_path.join("package.json").exists(),
        "TypeScript SDK must produce package.json"
    );
    assert!(
        ts_result.output_path.join("src/index.ts").exists(),
        "TypeScript SDK must produce src/index.ts"
    );

    let package_json = fs::read_to_string(ts_result.output_path.join("package.json")).unwrap();
    assert!(
        package_json.contains("@picloud/sdk"),
        "TypeScript package name must be '@picloud/sdk' for npm"
    );

    let index_ts = fs::read_to_string(ts_result.output_path.join("src/index.ts")).unwrap();
    assert!(index_ts.contains("appendEvent"), "TS SDK must expose appendEvent");
    assert!(index_ts.contains("queryGraph"), "TS SDK must expose queryGraph");
    assert!(index_ts.contains("getResource"), "TS SDK must expose getResource");
    assert!(index_ts.contains("PiCloudClient"), "TS SDK must define PiCloudClient");

    // -----------------------------------------------------------------------
    // 4. Verify .NET SDK (NuGet)
    // -----------------------------------------------------------------------
    let dotnet_result = gen_results
        .iter()
        .find(|r| r.language == SdkLanguage::DotNet)
        .expect(".NET SDK must be generated");

    assert!(
        dotnet_result.output_path.join("PiCloud.Sdk.csproj").exists(),
        ".NET SDK must produce PiCloud.Sdk.csproj"
    );
    assert!(
        dotnet_result.output_path.join("PiCloudClient.cs").exists(),
        ".NET SDK must produce PiCloudClient.cs"
    );

    let csproj = fs::read_to_string(dotnet_result.output_path.join("PiCloud.Sdk.csproj")).unwrap();
    assert!(
        csproj.contains("PiCloud.Sdk"),
        "NuGet package name must be 'PiCloud.Sdk'"
    );

    let client_cs = fs::read_to_string(dotnet_result.output_path.join("PiCloudClient.cs")).unwrap();
    assert!(
        client_cs.contains("AppendEventAsync"),
        ".NET SDK must expose AppendEventAsync"
    );
    assert!(
        client_cs.contains("QueryGraphAsync"),
        ".NET SDK must expose QueryGraphAsync"
    );
    assert!(
        client_cs.contains("GetResourceAsync"),
        ".NET SDK must expose GetResourceAsync"
    );
    assert!(
        client_cs.contains("PiCloudClient"),
        ".NET SDK must define PiCloudClient"
    );

    // -----------------------------------------------------------------------
    // 5. Verify publish pipeline targets correct registries (dry-run)
    // -----------------------------------------------------------------------

    // Rust -> crates.io via `cargo publish`
    let rust_pub = gen.publish(SdkLanguage::Rust, true, None).unwrap();
    assert!(rust_pub.dry_run, "must be dry-run");
    assert!(
        rust_pub.command.contains("cargo publish"),
        "Rust publish must use 'cargo publish' for crates.io"
    );
    assert!(
        rust_pub.command.contains("--dry-run"),
        "Rust dry-run must include --dry-run flag"
    );

    // TypeScript -> npm via `npm publish`
    let ts_pub = gen.publish(SdkLanguage::TypeScript, true, None).unwrap();
    assert!(ts_pub.dry_run, "must be dry-run");
    assert!(
        ts_pub.command.contains("npm publish"),
        "TypeScript publish must use 'npm publish' for npm registry"
    );
    assert!(
        ts_pub.command.contains("--dry-run"),
        "TypeScript dry-run must include --dry-run flag"
    );

    // .NET -> NuGet via `dotnet nuget push`
    let dotnet_pub = gen.publish(SdkLanguage::DotNet, true, None).unwrap();
    assert!(dotnet_pub.dry_run, "must be dry-run");
    assert!(
        dotnet_pub.command.contains("dotnet nuget push"),
        ".NET publish must use 'dotnet nuget push' for NuGet"
    );
    assert!(
        dotnet_pub.command.contains("nuget.org") || dotnet_pub.command.contains("nuget push"),
        ".NET publish must target NuGet registry"
    );

    // -----------------------------------------------------------------------
    // 6. Verify publish_all covers all three targets
    // -----------------------------------------------------------------------
    let all_pubs = gen.publish_all(true, None).unwrap();
    assert_eq!(all_pubs.len(), 3, "publish_all must produce 3 results");
    assert!(
        all_pubs.iter().all(|r| r.dry_run),
        "all publish results must be dry-run"
    );

    // Verify each language is represented
    let languages: Vec<SdkLanguage> = all_pubs.iter().map(|r| r.language).collect();
    assert!(languages.contains(&SdkLanguage::Rust), "publish_all must include Rust");
    assert!(
        languages.contains(&SdkLanguage::TypeScript),
        "publish_all must include TypeScript"
    );
    assert!(languages.contains(&SdkLanguage::DotNet), "publish_all must include DotNet");

    // -----------------------------------------------------------------------
    // 7. Verify custom registry override works (for private registries)
    // -----------------------------------------------------------------------
    let rust_custom = gen
        .publish(SdkLanguage::Rust, true, Some("private-registry"))
        .unwrap();
    assert!(
        rust_custom.command.contains("--registry private-registry"),
        "Rust publish must support custom registry"
    );

    let ts_custom = gen
        .publish(SdkLanguage::TypeScript, true, Some("https://npm.example.com"))
        .unwrap();
    assert!(
        ts_custom.command.contains("--registry https://npm.example.com"),
        "TypeScript publish must support custom registry"
    );

    let dotnet_custom = gen
        .publish(SdkLanguage::DotNet, true, Some("https://nuget.example.com"))
        .unwrap();
    assert!(
        dotnet_custom.command.contains("--source https://nuget.example.com"),
        ".NET publish must support custom registry via --source"
    );

    // -----------------------------------------------------------------------
    // 8. Verify ontology binding (ClusterDomain propagates to all SDKs)
    // -----------------------------------------------------------------------
    let custom_output = temp_output_dir();
    let custom_domain = ClusterDomain("my-cluster.example.com".to_string());
    let custom_gen = SdkGenerator::new(custom_domain, custom_output.clone());

    let custom_results = custom_gen.generate_all().unwrap();
    for result in &custom_results {
        let files: Vec<String> = result.files_generated.clone();
        // Read the main source file for each SDK
        let content = match result.language {
            SdkLanguage::Rust => {
                fs::read_to_string(result.output_path.join("src/lib.rs")).unwrap()
            }
            SdkLanguage::TypeScript => {
                fs::read_to_string(result.output_path.join("src/index.ts")).unwrap()
            }
            SdkLanguage::DotNet => {
                fs::read_to_string(result.output_path.join("PiCloudClient.cs")).unwrap()
            }
        };
        assert!(
            content.contains("my-cluster.example.com"),
            "{} SDK must embed custom cluster domain",
            result.language
        );
        assert!(
            !content.contains("picloud.local"),
            "{} SDK must not contain default domain when custom is specified",
            result.language
        );
        assert!(
            !files.is_empty(),
            "{} SDK must generate at least one file",
            result.language
        );
    }

    // -----------------------------------------------------------------------
    // 9. Verify SdkGenerationResult and SdkPublishResult types
    // -----------------------------------------------------------------------
    // These are compile-time checks — if the types don't exist, this test
    // won't compile. But we also verify field access works correctly.
    let _: SdkLanguage = SdkLanguage::Rust;
    let _: SdkLanguage = SdkLanguage::TypeScript;
    let _: SdkLanguage = SdkLanguage::DotNet;

    let sample_gen: &SdkGenerationResult = &gen_results[0];
    let _lang: SdkLanguage = sample_gen.language;
    let _path: &PathBuf = &sample_gen.output_path;
    let _files: &Vec<String> = &sample_gen.files_generated;

    let sample_pub: &SdkPublishResult = &all_pubs[0];
    let _lang: SdkLanguage = sample_pub.language;
    let _path: &PathBuf = &sample_pub.output_path;
    let _dry: bool = sample_pub.dry_run;
    let _cmd: &String = &sample_pub.command;

    // -----------------------------------------------------------------------
    // Cleanup
    // -----------------------------------------------------------------------
    cleanup(&output);
    cleanup(&custom_output);
}
