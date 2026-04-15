---
id: TC-285
title: .NET Aspire integration package connects to PiCloud cluster
type: scenario
status: passing
runner: cargo-test
runner-args: "tc285_net_aspire_integration_package_connects_to_picloud_cluster"
validates:
  features: [FT-089]
  adrs: [ADR-033]
phase: 3
last-run: 2026-04-15T17:52:22.095193142+00:00
last-run-duration: 0.5s
---

## Description

Scenario test verifying the .NET Aspire integration package generates all required
resource types (EventStore, SPARQL, IAM), each embedding the cluster URL so the
generated code can connect to a PiCloud cluster when used in an Aspire AppHost.

Checks:
- All five Aspire package files are generated (csproj + 3 resources + extensions)
- Each resource implements IResourceWithConnectionString with correct API endpoints
- Builder extension methods (AddPiCloudEventStore, AddPiCloudSparqlClient, AddPiCloudIamClient) are present
- Cluster domain propagates to all generated files
- Custom cluster domain override works correctly