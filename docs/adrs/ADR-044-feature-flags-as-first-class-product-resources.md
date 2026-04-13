---
id: ADR-044
title: Feature Flags as First-Class Product Resources
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Feature flags control which capabilities are active in a running system without redeployment. In PiCloud, flags are bound to Product versions — a flag targets a version expression, and only Products running a matching version see the flag as active. This enables progressive feature rollout across Product versions with explicit version intent.

**Decision:** Feature flags are a first-class Product resource. A flag declares a name, a version expression, and an enabled state. The platform evaluates flags against the running Product version. Workloads query flags via HTTP or the SDK. The SDK caches flags locally and subscribes to `FeatureFlagChanged` events for invalidation.

**Feature flag resource:**
```bicep
feature-flag 'new-upload-flow' = {
  product: 'photo-app'
  description: 'Enables the redesigned upload flow'
  enabled: true
  version: '= 2'            // exact match
}

feature-flag 'dark-mode' = {
  product: 'photo-app'
  description: 'Dark mode UI'
  enabled: true
  version: '>= 2'           // version 2 and above
}

feature-flag 'legacy-api' = {
  product: 'photo-app'
  description: 'Legacy v1 API compatibility shim'
  enabled: true
  version: '< 2'            // versions before 2
}

feature-flag 'beta-search' = {
  product: 'photo-app'
  description: 'Experimental search'
  enabled: true
  version: '2..4'           // versions 2, 3, and 4 inclusive
}
```

**Version expression operators:**

| Operator | Meaning | Example |
|---|---|---|
| `= N` | Exact version | `= 2` |
| `> N` | Greater than | `> 2` |
| `>= N` | Greater than or equal | `>= 2` |
| `< N` | Less than | `< 2` |
| `<= N` | Less than or equal | `<= 2` |
| `N..M` | Inclusive range | `2..4` |

Version numbers are the integer major version of the Product. `photo-app` version `2.1.0` has major version `2`.

**MVP flag value:** on/off only. Variant flags (percentage rollout, string variants) are a future phase.

**Flag evaluation:**
The platform evaluates a flag as active when:
1. `enabled: true`
2. The running Product version satisfies the version expression

A workload running in `photo-app@2.1.0` asking for `new-upload-flow` (version `= 2`) → **active**.
A workload running in `photo-app@1.5.0` asking for `new-upload-flow` (version `= 2`) → **inactive**.

**HTTP API:**
```
# All flags for the running version (evaluated)
GET https://picloud.local/products/photo-app/flags

# Single flag evaluation
GET https://picloud.local/products/photo-app/flags/new-upload-flow

# Response
{ "name": "new-upload-flow", "active": true, "version": "= 2" }
```

**SDK evaluation model:**
```rust
// Rust SDK
let flags = picloud.flags("photo-app").await?;
if flags.is_active("new-upload-flow") {
    // new flow
}
```

```typescript
// TypeScript SDK
const flags = await picloud.flags("photo-app");
if (flags.isActive("new-upload-flow")) {
    // new flow
}
```

```csharp
// .NET SDK
var flags = await picloud.Flags("photo-app");
if (flags.IsActive("new-upload-flow")) {
    // new flow
}
```

The SDK fetches all flags on startup, caches them locally, and subscribes to `FeatureFlagChanged` events. Flag evaluation is synchronous and in-process after initial load — zero network round-trips in the hot path.

**Live updates:**
When a flag changes (`enabled` toggled, version expression updated), the platform emits `FeatureFlagChanged`. The SDK receives this event, updates its local cache, and the next call to `is_active()` reflects the new state. No restart required.

**RDF representation:**
```turtle
<https://picloud.local/products/photo-app/flags/new-upload-flow>
    a picloud:FeatureFlag ;
    picloud:flagName        "new-upload-flow" ;
    picloud:flagEnabled     true ;
    picloud:flagVersion     "= 2" ;
    picloud:flagDescription "Enables the redesigned upload flow" .
```

**Rationale:**
- Version-bound flags make the intent explicit — "this feature exists from version 2" is a first-class declaration, not a comment
- Binding to Product version means flags are naturally cleaned up — when the minimum supported version exceeds a flag's expression, the flag is dead and should be removed
- SDK-local evaluation with event invalidation gives zero-latency flag checks in the hot path
- On/off MVP is the right starting point — variant flags add complexity that is not needed for Phase 1
- `FeatureFlagChanged` as an event means monitoring products can observe flag lifecycle across the cluster

**Consequences:**
- `feature-flag` is a new Product-scoped resource type
- `FeatureFlagChanged` is a new platform event
- Version expression parsing must handle all six operators and validate at deploy time — invalid expressions are rejected by the platform before the resource is created
- The SDK flag client must know the running Product version to evaluate expressions — this is injected by the platform as an environment variable at workload startup (`PICLOUD_PRODUCT_VERSION`)
- When a Product version changes (upgrade), `FeatureFlagChanged` events are emitted for all flags whose active state changes as a result of the version change