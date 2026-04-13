---
id: FT-007
title: IaC & CLI Design
phase: 1
status: in-progress
depends-on:
- FT-001
adrs:
- ADR-015
- ADR-042
- ADR-049
- ADR-050
tests:
- TC-042
- TC-043
- TC-044
- TC-133
- TC-134
- TC-135
- TC-163
- TC-164
- TC-165
- TC-166
- TC-167
- TC-168
- TC-169
- TC-170
domains:
- api
domains-acknowledged: {}
---

### Resource files

Resources are declared in `.picloud` files. A Product and all its resources can be declared in a single file or split across multiple files. The platform resolves dependencies across files.

Files are the source of truth. Deleting a resource from a file and redeploying cascades deletion to the platform.

### CLI commands

```bash
# Cluster management
picloud cluster init                               # default tenant (picloud.local)
picloud cluster init --domain acme.local           # named tenant
picloud cluster init --domain acme.local \         # BYO CA
  --ca-cert ./acme-ca.pem --ca-key ./acme-ca-key.pem
picloud cluster recover                            # physical recovery
picloud cluster status                             # query cluster state from RDF graph

# Resource operations
picloud resource apply ./photo-app/     # deploy all .picloud files in directory
picloud resource delete ./photo-app/    # delete all resources declared in directory
picloud resource status photo-app       # query product status from RDF graph

# Identity operations
picloud identity create --name alice    # create human identity
picloud identity token                  # get CLI token for current user

# Event stream
picloud events stream                   # subscribe to platform event stream
picloud events stream --product photo-app  # subscribe to product event stream

# Replay
picloud cluster replay --from "2025-06-01T00:00:00Z"               # platform replay
picloud resource replay photo-app --from "2025-06-01T00:00:00Z"    # product replay
picloud resource replay photo-app \                                 # aggregate replay
  --aggregate Photo \
  --id 123e4567-e89b-12d3-a456-426614174000 \
  --from "2025-06-01T00:00:00Z"
picloud resource replay photo-app \                                 # batch replay
  --aggregate Photo --ids-file ./photo-ids.txt \
  --from "2025-06-01T00:00:00Z"

# Graph queries
picloud graph query --sparql "SELECT ?s ?p ?o WHERE { ?s ?p ?o }"
picloud graph query --product photo-app --sparql "..."

# Telemetry queries (ADR-046)
picloud telemetry query --signal traces \
  --from "2025-07-01T00:00:00Z" --to "2025-07-01T01:00:00Z" \
  --sql "SELECT operation_name, AVG(duration_ms) FROM traces GROUP BY operation_name"
picloud telemetry query --signal metrics --sql "SELECT * FROM metrics WHERE product = 'photo-app'"
```

### Command execution model

Every CLI command follows the same model:

1. The CLI authenticates using the current identity token
2. The command is serialized as a command event and submitted to the cluster
3. The CLI subscribes to the result stream, filtered by the command's correlation ID
4. The platform processes the command, emits result events
5. The CLI renders the result when the terminal event arrives (e.g. `ResourceReady` or `ResourceFailed`)

This means all CLI operations are non-blocking by default. Long-running operations (large volume allocation, multi-container deployment) stream progress events to the CLI in real time.

### Idempotency

Every command event carries a client-generated idempotency key. The platform deduplicates commands by key. Re-running `picloud resource apply` on an unchanged set of files is safe and produces no effect.

---