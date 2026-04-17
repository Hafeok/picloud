---
id: FT-088
title: picloud sdk publish command
phase: 3
status: planned
depends-on: []
adrs:
- ADR-033
tests:
- TC-097
- TC-098
- TC-099
- TC-218
- TC-233
domains: []
domains-acknowledged: {}
---

## Description

The `picloud sdk publish` CLI command generates and publishes SDKs from any live cluster's current ontology (ADR-033).

### Usage

```bash
# Generate and publish to default registries
picloud sdk publish

# Generate only (no publish)
picloud sdk generate --output ./sdk-output/

# Publish to custom registries
picloud sdk publish --npm-registry https://npm.internal.example.com \
                    --nuget-source https://nuget.internal.example.com
```

### What it does

1. Connects to the cluster and reads the current live ontology
2. Runs the SDK generator (FT-086) for all three language targets
3. Publishes the generated packages to the configured registries
4. Reports success/failure per language target

### Authentication

- Registry authentication uses standard mechanisms (`.npmrc`, `cargo login`, `nuget` API keys)
- The operator must have platform-admin role to read the full ontology

### Use cases

- **Custom forks** — internal platform extensions get SDK support without waiting for upstream releases
- **Air-gapped clusters** — generate SDKs and publish to internal registries
- **Development** — generate SDKs from a development cluster to test against the latest ontology changes
