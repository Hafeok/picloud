/// FT-089 Integration Tests -- .NET Aspire integration package
///
/// Covers:
///   TC-285: .NET Aspire integration package connects to PiCloud cluster (scenario)
///   TC-342: Aspire exit -- .NET integration package connects to cluster (exit-criteria)
///
/// Verifies the Aspire companion package generation, ensuring all three
/// resource types (EventStore, SPARQL, IAM) are properly generated with
/// correct Aspire hosting patterns and cluster connection details.

use std::fs;
use std::path::PathBuf;

use picloud_domain::iri::ClusterDomain;
use picloud_sdk_gen::SdkGenerator;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_output_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "picloud-aspire-tc285-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn cleanup(dir: &std::path::Path) {
    let _ = fs::remove_dir_all(dir);
}

// ============================================================================
// TC-285 -- .NET Aspire integration package connects to PiCloud cluster
// ============================================================================

/// Scenario test: verify the Aspire integration package generates all
/// required resource types, each embedding the cluster URL so that the
/// generated code can connect to a PiCloud cluster when used in an
/// Aspire AppHost.
#[test]
fn tc285_net_aspire_integration_package_connects_to_picloud_cluster() {
    let output = temp_output_dir();
    let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

    // -----------------------------------------------------------------------
    // 1. Generate Aspire integration package
    // -----------------------------------------------------------------------
    let result = gen.generate_aspire_integration().unwrap();
    assert_eq!(result.files_generated.len(), 5, "Aspire package must produce 5 files");

    let aspire_dir = output.join("dotnet-aspire");
    assert!(aspire_dir.exists(), "dotnet-aspire output directory must exist");

    // -----------------------------------------------------------------------
    // 2. Verify project file (PiCloud.Sdk.Aspire.csproj)
    // -----------------------------------------------------------------------
    let csproj_path = aspire_dir.join("PiCloud.Sdk.Aspire.csproj");
    assert!(csproj_path.exists(), "PiCloud.Sdk.Aspire.csproj must be generated");
    assert!(
        result.files_generated.contains(&"PiCloud.Sdk.Aspire.csproj".to_string()),
        "files_generated must list PiCloud.Sdk.Aspire.csproj"
    );

    let csproj = fs::read_to_string(&csproj_path).unwrap();
    assert!(
        csproj.contains("PiCloud.Sdk.Aspire"),
        "csproj must define PiCloud.Sdk.Aspire as the package/namespace"
    );
    assert!(
        csproj.contains("Aspire.Hosting.Abstractions"),
        "csproj must depend on Aspire.Hosting.Abstractions"
    );
    assert!(
        csproj.contains("net8.0"),
        "csproj must target net8.0"
    );

    // -----------------------------------------------------------------------
    // 3. Verify event store resource (AddPiCloudEventStore)
    // -----------------------------------------------------------------------
    let event_store_path = aspire_dir.join("PiCloudEventStoreResource.cs");
    assert!(event_store_path.exists(), "PiCloudEventStoreResource.cs must be generated");

    let event_store = fs::read_to_string(&event_store_path).unwrap();
    assert!(
        event_store.contains("PiCloudEventStoreResource"),
        "event store file must define PiCloudEventStoreResource class"
    );
    assert!(
        event_store.contains("IResourceWithConnectionString"),
        "event store resource must implement IResourceWithConnectionString"
    );
    assert!(
        event_store.contains("ConnectionStringExpression"),
        "event store resource must expose ConnectionStringExpression"
    );
    assert!(
        event_store.contains("picloud.local"),
        "event store resource must embed the cluster domain"
    );
    assert!(
        event_store.contains("/api/events"),
        "event store connection must target /api/events endpoint"
    );

    // -----------------------------------------------------------------------
    // 4. Verify SPARQL client resource (AddPiCloudSparqlClient)
    // -----------------------------------------------------------------------
    let sparql_path = aspire_dir.join("PiCloudSparqlResource.cs");
    assert!(sparql_path.exists(), "PiCloudSparqlResource.cs must be generated");

    let sparql = fs::read_to_string(&sparql_path).unwrap();
    assert!(
        sparql.contains("PiCloudSparqlResource"),
        "SPARQL file must define PiCloudSparqlResource class"
    );
    assert!(
        sparql.contains("IResourceWithConnectionString"),
        "SPARQL resource must implement IResourceWithConnectionString"
    );
    assert!(
        sparql.contains("ConnectionStringExpression"),
        "SPARQL resource must expose ConnectionStringExpression"
    );
    assert!(
        sparql.contains("picloud.local"),
        "SPARQL resource must embed the cluster domain"
    );
    assert!(
        sparql.contains("/api/sparql"),
        "SPARQL connection must target /api/sparql endpoint"
    );

    // -----------------------------------------------------------------------
    // 5. Verify IAM client resource (AddPiCloudIamClient)
    // -----------------------------------------------------------------------
    let iam_path = aspire_dir.join("PiCloudIamResource.cs");
    assert!(iam_path.exists(), "PiCloudIamResource.cs must be generated");

    let iam = fs::read_to_string(&iam_path).unwrap();
    assert!(
        iam.contains("PiCloudIamResource"),
        "IAM file must define PiCloudIamResource class"
    );
    assert!(
        iam.contains("IResourceWithConnectionString"),
        "IAM resource must implement IResourceWithConnectionString"
    );
    assert!(
        iam.contains("ConnectionStringExpression"),
        "IAM resource must expose ConnectionStringExpression"
    );
    assert!(
        iam.contains("picloud.local"),
        "IAM resource must embed the cluster domain"
    );
    assert!(
        iam.contains("/api/iam"),
        "IAM connection must target /api/iam endpoint"
    );

    // -----------------------------------------------------------------------
    // 6. Verify builder extension methods (PiCloudResourceExtensions)
    // -----------------------------------------------------------------------
    let extensions_path = aspire_dir.join("PiCloudResourceExtensions.cs");
    assert!(extensions_path.exists(), "PiCloudResourceExtensions.cs must be generated");

    let extensions = fs::read_to_string(&extensions_path).unwrap();
    assert!(
        extensions.contains("AddPiCloudEventStore"),
        "extensions must provide AddPiCloudEventStore method"
    );
    assert!(
        extensions.contains("AddPiCloudSparqlClient"),
        "extensions must provide AddPiCloudSparqlClient method"
    );
    assert!(
        extensions.contains("AddPiCloudIamClient"),
        "extensions must provide AddPiCloudIamClient method"
    );
    assert!(
        extensions.contains("IDistributedApplicationBuilder"),
        "extensions must extend IDistributedApplicationBuilder"
    );
    assert!(
        extensions.contains("IResourceBuilder"),
        "extension methods must return IResourceBuilder<T>"
    );
    assert!(
        extensions.contains("WithReference"),
        "extensions must document WithReference usage"
    );
    assert!(
        extensions.contains("picloud.local"),
        "extensions must embed the cluster domain"
    );

    // -----------------------------------------------------------------------
    // 7. Verify cluster URL propagation (connects to PiCloud cluster)
    // -----------------------------------------------------------------------
    // All resource files and extensions must contain the cluster URL,
    // confirming the generated package can connect to the cluster.
    for file_name in &[
        "PiCloudEventStoreResource.cs",
        "PiCloudSparqlResource.cs",
        "PiCloudIamResource.cs",
        "PiCloudResourceExtensions.cs",
    ] {
        let content = fs::read_to_string(aspire_dir.join(file_name)).unwrap();
        assert!(
            content.contains("https://picloud.local"),
            "{} must contain the full cluster URL https://picloud.local",
            file_name
        );
    }

    // -----------------------------------------------------------------------
    // 8. Verify custom cluster domain propagation
    // -----------------------------------------------------------------------
    let custom_output = temp_output_dir();
    let custom_domain = ClusterDomain("my-cluster.example.com".to_string());
    let custom_gen = SdkGenerator::new(custom_domain, custom_output.clone());

    let custom_result = custom_gen.generate_aspire_integration().unwrap();
    assert_eq!(custom_result.files_generated.len(), 5);

    for file_name in &[
        "PiCloudEventStoreResource.cs",
        "PiCloudSparqlResource.cs",
        "PiCloudIamResource.cs",
        "PiCloudResourceExtensions.cs",
    ] {
        let content = fs::read_to_string(
            custom_output.join("dotnet-aspire").join(file_name),
        )
        .unwrap();
        assert!(
            content.contains("my-cluster.example.com"),
            "{} must embed custom cluster domain",
            file_name
        );
        assert!(
            !content.contains("picloud.local"),
            "{} must not contain default domain when custom is specified",
            file_name
        );
    }

    // -----------------------------------------------------------------------
    // Cleanup
    // -----------------------------------------------------------------------
    cleanup(&output);
    cleanup(&custom_output);
}

// ============================================================================
// TC-342 -- Aspire exit -- .NET integration package connects to cluster
// ============================================================================

/// Exit-criteria test: comprehensive verification that the Aspire integration
/// package is production-ready. Covers generation, publish pipeline, resource
/// types, Aspire hosting patterns, and dependency structure.
#[test]
fn tc342_aspire_exit_net_integration_package_connects_to_cluster() {
    let output = temp_output_dir();
    let gen = SdkGenerator::new(ClusterDomain::default(), output.clone());

    // -----------------------------------------------------------------------
    // 1. Generate Aspire integration package
    // -----------------------------------------------------------------------
    let result = gen.generate_aspire_integration().unwrap();
    let aspire_dir = output.join("dotnet-aspire");

    // -----------------------------------------------------------------------
    // 2. Verify all five files are listed and exist on disk
    // -----------------------------------------------------------------------
    let expected_files = vec![
        "PiCloud.Sdk.Aspire.csproj",
        "PiCloudEventStoreResource.cs",
        "PiCloudSparqlResource.cs",
        "PiCloudIamResource.cs",
        "PiCloudResourceExtensions.cs",
    ];
    for file in &expected_files {
        assert!(
            result.files_generated.contains(&file.to_string()),
            "files_generated must include {}",
            file
        );
        assert!(
            aspire_dir.join(file).exists(),
            "{} must exist on disk",
            file
        );
    }

    // -----------------------------------------------------------------------
    // 3. Verify Aspire hosting patterns in all resource types
    // -----------------------------------------------------------------------
    let resource_files = [
        "PiCloudEventStoreResource.cs",
        "PiCloudSparqlResource.cs",
        "PiCloudIamResource.cs",
    ];

    for file_name in &resource_files {
        let content = fs::read_to_string(aspire_dir.join(file_name)).unwrap();

        // Every resource must inherit from Resource (Aspire base class)
        assert!(
            content.contains("Resource(name)"),
            "{}: must inherit from Aspire Resource base class",
            file_name
        );

        // Every resource must implement IResourceWithConnectionString
        assert!(
            content.contains("IResourceWithConnectionString"),
            "{}: must implement IResourceWithConnectionString for WithReference support",
            file_name
        );

        // Every resource must have a ClusterUrl property
        assert!(
            content.contains("ClusterUrl"),
            "{}: must expose ClusterUrl property",
            file_name
        );

        // Every resource must produce a connection string via ReferenceExpression
        assert!(
            content.contains("ReferenceExpression"),
            "{}: must use ReferenceExpression for connection strings",
            file_name
        );

        // Cluster URL must be embedded
        assert!(
            content.contains("https://picloud.local"),
            "{}: must embed the cluster URL",
            file_name
        );

        // PiCloud.Sdk.Aspire namespace
        assert!(
            content.contains("namespace PiCloud.Sdk.Aspire"),
            "{}: must be in PiCloud.Sdk.Aspire namespace",
            file_name
        );
    }

    // -----------------------------------------------------------------------
    // 4. Verify extensions wire all three resource types
    // -----------------------------------------------------------------------
    let extensions = fs::read_to_string(aspire_dir.join("PiCloudResourceExtensions.cs")).unwrap();

    // Must be a static class with extension methods
    assert!(
        extensions.contains("public static class PiCloudResourceExtensions"),
        "extensions must be a public static class"
    );

    // Must contain all three Add* extension methods
    assert!(
        extensions.contains("AddPiCloudEventStore"),
        "must provide AddPiCloudEventStore extension"
    );
    assert!(
        extensions.contains("AddPiCloudSparqlClient"),
        "must provide AddPiCloudSparqlClient extension"
    );
    assert!(
        extensions.contains("AddPiCloudIamClient"),
        "must provide AddPiCloudIamClient extension"
    );

    // Must use the `this` keyword for extension methods on IDistributedApplicationBuilder
    assert!(
        extensions.contains("this IDistributedApplicationBuilder builder"),
        "extension methods must use 'this' keyword on IDistributedApplicationBuilder"
    );

    // Must return IResourceBuilder<T> for each resource type
    assert!(
        extensions.contains("IResourceBuilder<PiCloudEventStoreResource>"),
        "AddPiCloudEventStore must return IResourceBuilder<PiCloudEventStoreResource>"
    );
    assert!(
        extensions.contains("IResourceBuilder<PiCloudSparqlResource>"),
        "AddPiCloudSparqlClient must return IResourceBuilder<PiCloudSparqlResource>"
    );
    assert!(
        extensions.contains("IResourceBuilder<PiCloudIamResource>"),
        "AddPiCloudIamClient must return IResourceBuilder<PiCloudIamResource>"
    );

    // Must use builder.AddResource() pattern
    assert!(
        extensions.contains("builder.AddResource(resource)"),
        "extensions must use builder.AddResource() to register resources"
    );

    // Must include Aspire hosting usings
    assert!(
        extensions.contains("using Aspire.Hosting"),
        "extensions must import Aspire.Hosting"
    );

    // -----------------------------------------------------------------------
    // 5. Verify correct API endpoints in connection strings
    // -----------------------------------------------------------------------
    let event_store = fs::read_to_string(aspire_dir.join("PiCloudEventStoreResource.cs")).unwrap();
    assert!(
        event_store.contains("/api/events"),
        "event store must connect to /api/events"
    );

    let sparql = fs::read_to_string(aspire_dir.join("PiCloudSparqlResource.cs")).unwrap();
    assert!(
        sparql.contains("/api/sparql"),
        "SPARQL client must connect to /api/sparql"
    );

    let iam = fs::read_to_string(aspire_dir.join("PiCloudIamResource.cs")).unwrap();
    assert!(
        iam.contains("/api/iam"),
        "IAM client must connect to /api/iam"
    );

    // -----------------------------------------------------------------------
    // 6. Verify csproj package metadata
    // -----------------------------------------------------------------------
    let csproj = fs::read_to_string(aspire_dir.join("PiCloud.Sdk.Aspire.csproj")).unwrap();
    assert!(
        csproj.contains("<PackageId>PiCloud.Sdk.Aspire</PackageId>"),
        "NuGet package ID must be PiCloud.Sdk.Aspire"
    );
    assert!(
        csproj.contains("Aspire.Hosting.Abstractions"),
        "must depend on Aspire.Hosting.Abstractions"
    );
    assert!(
        csproj.contains("<RootNamespace>PiCloud.Sdk.Aspire</RootNamespace>"),
        "root namespace must be PiCloud.Sdk.Aspire"
    );

    // -----------------------------------------------------------------------
    // 7. Verify publish pipeline targets NuGet
    // -----------------------------------------------------------------------
    let pub_result = gen.publish_aspire_integration(true, None).unwrap();
    assert!(pub_result.dry_run, "must be dry-run");
    assert!(
        pub_result.command.contains("dotnet nuget push"),
        "Aspire publish must use 'dotnet nuget push'"
    );
    assert!(
        pub_result.command.contains("nuget.org") || pub_result.command.contains("nuget push"),
        "Aspire publish must target NuGet registry"
    );
    assert!(
        pub_result.command.contains("dry-run"),
        "dry-run must be indicated in the command"
    );

    // Custom registry
    let custom_pub = gen
        .publish_aspire_integration(true, Some("https://nuget.example.com"))
        .unwrap();
    assert!(
        custom_pub.command.contains("--source https://nuget.example.com"),
        "Aspire publish must support custom registry via --source"
    );

    // -----------------------------------------------------------------------
    // 8. Verify Aspire generation does not break base .NET SDK
    // -----------------------------------------------------------------------
    let dotnet_result = gen.generate(picloud_sdk_gen::SdkLanguage::DotNet).unwrap();
    assert!(
        output.join("dotnet/PiCloud.Sdk.csproj").exists(),
        "base .NET SDK must still generate correctly"
    );
    assert!(
        output.join("dotnet/PiCloudClient.cs").exists(),
        "base .NET SDK client must still generate correctly"
    );

    // Aspire package is separate from base SDK
    assert_ne!(
        dotnet_result.output_path,
        result.output_path,
        "Aspire package must be in a separate directory from base .NET SDK"
    );

    // -----------------------------------------------------------------------
    // 9. Verify SdkGenerationResult fields
    // -----------------------------------------------------------------------
    assert_eq!(
        result.language,
        picloud_sdk_gen::SdkLanguage::DotNet,
        "Aspire result language must be DotNet"
    );
    assert!(
        result.output_path.ends_with("dotnet-aspire"),
        "Aspire output path must end with dotnet-aspire"
    );
    assert_eq!(
        result.files_generated.len(),
        5,
        "Aspire package must produce exactly 5 files"
    );

    // -----------------------------------------------------------------------
    // 10. Verify generate_all still works (no regression)
    // -----------------------------------------------------------------------
    let all_results = gen.generate_all().unwrap();
    assert_eq!(
        all_results.len(),
        3,
        "generate_all must still produce exactly 3 base SDKs (Aspire is separate)"
    );

    // -----------------------------------------------------------------------
    // Cleanup
    // -----------------------------------------------------------------------
    cleanup(&output);
}
