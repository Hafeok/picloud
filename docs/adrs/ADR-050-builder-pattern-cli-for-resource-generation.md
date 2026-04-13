---
id: ADR-050
title: Builder Pattern CLI for Resource Generation
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Developers need to create new `.picloud` resource files without memorising syntax. The platform ontology and SHACL shapes define every valid resource type and property — the CLI can use this knowledge to guide developers interactively and generate valid files automatically.

**Decision:** `picloud new {resource-type}` generates a `.picloud` file for a new resource. It accepts flags for all properties — fully specified invocations produce the file with no prompts. Partially specified invocations prompt for required fields only. After generation, `picloud compile validate` runs automatically. Generated files are never overwritten unless `--overwrite` is specified.

**Behaviour:**

```bash
# Fully specified — no prompts, CI/CD friendly
picloud new container \
  --product photo-app \
  --name api-server \
  --image photo-api:1.0.0 \
  --identity api-worker \
  --mount media-store:/data \
  --tag team=backend \
  --tag environment=production \
  --output ./photo-app/containers/api-server.picloud

# Partially specified — prompts for missing required fields only
picloud new container --product photo-app
? Container name: api-server
? Image: photo-api:1.0.0
? Workload identity: api-worker
✓ Generated: ./containers/api-server.picloud
✓ Validation passed

# Overwrite existing file
picloud new container --product photo-app --name api-server --overwrite
```

**Supported resource types:**
`product`, `container`, `binary`, `volume`, `feature-flag`, `config`, `inference-rule`, `event-store`, `rdf-store`, `ingress`, `group`, `event-subscription`, `ontology`

**Output flag:** `--output` specifies the file path. If omitted, defaults to `./{resource-type}s/{name}.picloud` relative to the current directory.

**Overwrite protection:** If the output file already exists and `--overwrite` is not set, the CLI refuses with a clear error. This prevents accidental overwrite of hand-edited files.

**Post-generation validation:** After writing the file, `picloud compile validate` runs automatically against the generated file. If validation fails (e.g. a referenced volume does not exist in the same directory), the error is reported with the human-readable messages from ADR-049.

**Flag naming:** All flags match the `.picloud` property names exactly — `--image`, `--identity`, `--mount`, `--tag`. This makes the CLI self-documenting and consistent with the resource files developers read and edit.

**Rationale:**
- Flags-first means CI/CD pipelines and LLMs can use `picloud new` non-interactively
- Interactive fallback for required fields means humans get guidance without remembering syntax
- Auto-validation closes the feedback loop — the developer knows the file is valid immediately
- Overwrite protection prevents accidental data loss on hand-edited files
- Flag names matching property names means one mental model for CLI and file format
- Generated files are plain `.picloud` text — developers can open and edit them immediately

**Consequences:**
- `picloud new` is implemented in `picloud-cli` using the `picloud-compiler` crate for generation and validation
- The builder must know which fields are required vs optional for each resource type — derived from SHACL `sh:minCount` constraints
- The interactive prompt library must handle Ctrl+C gracefully and not leave partial files