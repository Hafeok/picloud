---
id: ADR-055
title: Capability as a First-Class Interface Contract
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** As the number of Products in a PiCloud cluster grows, shared functionality emerges naturally. The naive solution — letting one Product depend directly on another that happens to implement the functionality — violates the stable dependency principle. A dependency on `photo-app` for GPS resolution means consumers are coupled to `photo-app`'s deployment lifecycle, versioning, and ownership. If `photo-app` is deprecated, every consumer breaks. If the GPS logic needs to evolve, the photo-app team must accommodate all consumers' timelines.

The root problem is that PiCloud lacked a way to express a named, versioned *interface contract* independently of any *implementation*. Products are hermetically sealed deployment units — they are the wrong primitive for expressing a shared capability that multiple Products may implement over time.

**Decision:** Introduce `capability` as a first-class, cluster-scoped resource type. A capability is a pure interface contract: a named, versioned declaration of an event schema and SHACL shapes that describes a service interaction. A capability has no workload, no container, no code. It declares only the contract.

Products separately declare `implements` (they fulfil a capability's contract) and `capabilities` (they depend on a capability being available in the cluster). Consumers bind to the capability, not to any specific Product.

**Resource definition:**

```bicep
// Cluster-scoped — no product: field
capability 'gps-to-place' = {
  version: '1.0.0'
  description: 'Translates GPS coordinates to a named place with confidence score'
  ontology: './capabilities/gps-to-place.ttl'
  shapes:   './capabilities/gps-to-place.shacl'
  events: {
    input:  'CoordinatesReceived'
    output: 'PlaceResolved'
  }
}

// Photo-app is the first implementor — it does not own the capability
product 'photo-app' = {
  version: '2.0.0'
  implements: ['gps-to-place@1.0.0']
}

// Maps-app depends on the capability, not on photo-app
product 'maps-app' = {
  version: '1.0.0'
  capabilities: [
    { capability: 'gps-to-place', minVersion: '1.0.0' }
  ]
}
```

**Enforcement rules (applied at `resource apply` time):**

1. A `capability` must declare at least one `input` event and one `output` event — capabilities with no declared interface are rejected.
2. A `capability` must declare either `ontology` or `shapes` (or both) — the contract must be expressed in the type system, not just in prose.
3. A Product declaring `implements` must have an `event-subscription` to the capability's `input` event type and must emit the capability's `output` event type — the platform validates structural conformance against the declared SHACL shapes at deploy time.
4. A Product declaring a `capabilities` dependency will fail `resource apply` if no Product in the cluster currently declares `implements` for that capability at the required `minVersion`.
5. A `capability` cannot be deleted while any Product declares a `capabilities` dependency on it — cascading deletion is blocked.
6. Dependency direction is enforced: a Product that `implements` a capability cannot also declare a `capabilities` dependency on a capability it does not itself implement.

**Capability lifecycle events:**

- `CapabilityDeclared` — capability resource received and validated
- `CapabilityReady` — at least one implementing Product is deployed and structurally conformant
- `CapabilityImplementorAdded` — a new Product declares `implements` for an existing capability
- `CapabilityImplementorRemoved` — an implementing Product is removed
- `CapabilityUnfulfilled` — no implementing Product exists; all consumers receive this event
- `CapabilityDeleted` — capability removed (only when no consumers exist)

**Routing:**

The platform resolves which Product currently implements the required capability and routes the `input` event to that Product's event bus. If multiple Products implement the same capability, the platform selects the implementor with the highest version that satisfies the consumer's `minVersion` constraint. One active implementor per capability in Phase 1.

**Rationale:**
- Separating interface (capability) from implementation (Product) is the structural enforcement of the stable dependency principle — consumers never take a direct dependency on a mutable deployment unit
- Capabilities are expressed in the platform's native type system (RDF ontology + SHACL) — no separate contract registry is needed
- The `CapabilityUnfulfilled` event gives consumers observable signal when their dependency is broken
- Requiring at least one implementor before a consumer can deploy prevents consumers from deploying against phantom contracts

**Rejected alternatives:**
- **Direct product-to-product dependencies** — creates tight coupling between deployment lifecycles.
- **Capability as a sub-resource of the implementing Product** — embeds the same ownership problem one level down.
- **Shared library / SDK** — moves the contract into code, loses platform-level validation and discoverability.
- **Service mesh with named service contracts** — adds sidecar network layer, violates single-binary goal.

**Consequences:**
- `capability` is a new cluster-scoped resource type
- The `product` resource gains `implements` and `capabilities` fields
- `resource apply` gains a capability resolution validation step
- `picloud capability list` surfaces all capabilities, implementors, consumers, and fulfilment status