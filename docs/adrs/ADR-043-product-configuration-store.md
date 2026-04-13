---
id: ADR-043
title: Product Configuration Store
status: accepted
features: [FT-009, FT-038]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Applications need runtime configuration — connection strings, feature endpoints, tuning parameters — that should not be baked into container images or resource definition files. Azure App Configuration solves this with a central, tagged key-value store. PiCloud needs the same capability, consistent with its event-driven and RDF-native model.

**Decision:** Every Product has a managed configuration store. Configuration entries are typed key-value pairs with tags. Workloads can declare their own configuration that merges over the product config — workload values win on conflict. Configuration changes emit `ConfigChanged` events. Workloads receive live updates via event subscription without restarting.

**Configuration resource:**
```bicep
config 'app-config' = {
  product: 'photo-app'
  entries: [
    { key: 'storage.max-upload-mb',  value: '50',                    type: 'int',    tags: { tier: 'storage' } }
    { key: 'api.base-url',           value: 'https://api.acme.local', type: 'string', tags: { tier: 'network' } }
    { key: 'cache.ttl-seconds',      value: '300',                   type: 'int',    tags: { tier: 'cache'   } }
    { key: 'feature.maintenance',    value: 'false',                  type: 'bool',   tags: { tier: 'ops'     } }
  ]
}
```

**Workload config override:**
```bicep
container 'worker' = {
  product: 'photo-app'
  image:   'photo-worker:1.0.0'
  config: {
    // Overrides product-level value for this workload only
    'cache.ttl-seconds': '60'
  }
}
```

**Effective config resolution — merge, workload wins:**
```
effective_config = product_config ∪ workload_config
                   (workload values override on key collision)
```

**Value types (Phase 1: flat strings. Types are metadata for SDK deserialisation):**

| Type | Description |
|---|---|
| `string` | Raw string value |
| `int` | Integer — SDK deserialises to i64 |
| `float` | Floating point — SDK deserialises to f64 |
| `bool` | Boolean — `"true"` / `"false"` |
| `json` | JSON string — SDK deserialises to typed object (future) |

**HTTP API:**
```
GET  https://picloud.local/products/photo-app/config              → all entries
GET  https://picloud.local/products/photo-app/config/storage.max-upload-mb
POST https://picloud.local/products/photo-app/config              → set entry
DEL  https://picloud.local/products/photo-app/config/storage.max-upload-mb
```

Workload-effective config (merged view):
```
GET https://picloud.local/products/photo-app/containers/worker/config
```

**Live reload:**
When a config entry changes, the platform emits `ConfigChanged`. Workloads subscribed to the product event bus receive the update. The SDK handles the subscription and invalidates its local cache automatically — workloads call `config.get("key")` and always get the current value without restarting.

**RDF representation:**
```turtle
<https://picloud.local/products/photo-app/config/storage.max-upload-mb>
    a picloud:ConfigEntry ;
    picloud:configKey   "storage.max-upload-mb" ;
    picloud:configValue "50" ;
    picloud:configType  "int" ;
    picloud:tag [ picloud:tagKey "tier" ; picloud:tagValue "storage" ] .
```

**Rationale:**
- Central config store decouples runtime values from deployment artifacts — change config without redeployment
- Workload override with merge-and-win gives fine-grained control without duplicating the full product config
- Live reload via events is consistent with the platform's event-driven model — no polling, no restart
- Tags on config entries enable SPARQL queries across config — e.g. "all config entries tagged `environment:production`"
- Typed values let the SDK deserialise correctly without the workload parsing strings manually

**Rejected alternatives:**
- **Environment variables only** — no versioning, no event-driven updates, and no integration with the RDF graph; changes require container restarts.
- **External config service (Consul KV, etcd)** — adds an external dependency when the platform already has an event log and RDF graph for state management.

**Consequences:**
- `config` is a new Product-scoped resource type
- `ConfigChanged` is a new platform event
- The SDK config client maintains a local cache and subscription — see ADR-044 for SDK integration
- Secrets are not config entries — sensitive values use the existing secret resource (they are injected, not polled)