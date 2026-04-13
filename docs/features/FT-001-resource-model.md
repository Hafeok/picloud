---
id: FT-001
title: Resource Model
phase: 1
status: complete
depends-on: []
adrs:
- ADR-007
- ADR-015
- ADR-016
tests:
- TC-022
- TC-023
- TC-024
- TC-042
- TC-043
- TC-044
- TC-045
- TC-046
- TC-047
- TC-209
domains:
- api
- data-model
domains-acknowledged: {}
---

Everything in PiCloud is a resource. Resources are typed, versioned, named, and IAM-governed. There is no operation in the platform that does not create, update, or delete a resource.

### Resource addressing

Every resource in PiCloud has a canonical IRI that is both its unique identifier and its network location. IRIs follow a path-based hierarchy rooted at the cluster domain:

```
https://picloud.local/nodes/{node-name}
https://picloud.local/products/{product-name}
https://picloud.local/products/{product-name}/{resource-type}/{resource-name}
```

Examples:
```
https://picloud.local/nodes/pi-node-01
https://picloud.local/products/photo-app
https://picloud.local/products/photo-app/containers/api-server
https://picloud.local/products/photo-app/volumes/media-store
https://picloud.local/products/photo-app/identities/api-worker
https://picloud.local/products/photo-app/event-subscriptions/user-service-user-created
https://picloud.local/products/photo-app/graph
https://picloud.local/products/photo-app/ontology
https://picloud.local/products/photo-app/events
```

Every IRI is dereferenceable over HTTPS. The platform serves RDF representations at every resource IRI via HTTP content negotiation (`text/turtle`, `application/ld+json`, `application/json`). Resource IRIs are stable — they do not change when workloads reschedule to different nodes.

### Resource types

**Platform-scoped resources** (exist outside any Product):
- `node` — a cluster member
- `identity` — a human user
- `group` — a collection of identities sharing roles — membership managed by inference rules
- `role` — a platform-level RBAC role
- `inference-rule` — a SPARQL CONSTRUCT query that derives new graph facts, manages group membership, or fires alerts
- `capability` — a named, versioned interface contract (ontology + SHACL shapes + event schema); the operational sharing primitive — behaviour, not data (see ADR-054)
- `data-domain` — a governance namespace grouping related data products across Products; declares a steward identity, sensitivity classification, and domain-level SHACL constraints enforced at `resource apply` time (see ADR-055)

**Product-scoped resources**:
- `product` — the deployment unit itself, declares version, metadata, capability relationships (`implements`, `capabilities`), and data product dependencies (`dataProducts`)
- `container` — an OCI container workload
- `binary` — a raw executable workload
- `volume` — a block storage allocation with declared storage intent
- `rdf-store` — a managed Oxigraph instance with IAM-gated SPARQL endpoint; internal to the Product — not directly queryable by other Products (see ADR-055)
- `identity` — a workload identity (service account)
- `event-subscription` — a declared subscription to another Product's event stream
- `ontology` — a `.ttl` or `.shacl` file bound to the Product version
- `inference-rule` — a product-scoped SPARQL CONSTRUCT rule for alerts and derived facts
- `config` — typed key-value configuration store with tags, live-reloaded via events
- `feature-flag` — version-bound on/off flag, SDK-evaluated with event invalidation
- `data-product` — a versioned, published projection of selected domain data into a separate named graph; the analytical sharing primitive — knowledge, not behaviour (see ADR-055)

### Resource definition syntax

Resources are declared in `.picloud` files using a Bicep-inspired syntax:

```bicep
product 'photo-app' = {
  version:    '1.0.0'
  description: 'Photo sharing application'
  implements: ['gps-to-place@1.0.0']   // fulfils a capability contract
}

volume 'media-store' = {
  product: 'photo-app'
  storageIntent: {
    durability: 'full-replication'
    performance: 'standard'
  }
  size: '100GB'
}

container 'api-server' = {
  product: 'photo-app'
  image: 'photo-api:1.0.0'
  identity: 'api-worker'
  mounts: [
    { volume: 'media-store', path: '/data' }
  ]
  env: {
    DB_URL: secret('db-connection')
  }
}

event-subscription 'user-created' = {
  product: 'photo-app'
  source:  'user-service@1.0.0'
  event:   'UserCreated'
  handler: 'api-server'
}

// Cluster-scoped capability — the operational sharing primitive
capability 'gps-to-place' = {
  version:     '1.0.0'
  description: 'Translates GPS coordinates to a named place with confidence score'
  ontology:    './capabilities/gps-to-place.ttl'
  shapes:      './capabilities/gps-to-place.shacl'
  events: {
    input:  'CoordinatesReceived'
    output: 'PlaceResolved'
  }
}

// Cluster-scoped data domain — the governance grouping for analytical data
data-domain 'geospatial' = {
  description: 'Location and mapping data products across the cluster'
  steward:     'identity/alice'
  sensitivity: 'internal'
}

// Product-scoped data product — the analytical sharing primitive
// Published into a separate named graph; never exposes the internal graph directly
data-product 'photo-locations' = {
  product:     'photo-app'
  domain:      'geospatial'
  version:     '1.0.0'
  description: 'Geo-tagged photo locations aggregated by resolved place'
  ontology:    './data-products/photo-locations.ttl'
  shapes:      './data-products/photo-locations.shacl'
  projection:  './data-products/photo-locations.rq'   // SPARQL CONSTRUCT over internal graph
  freshness: {
    maxAge:   '15m'
    triggers: ['PlaceResolved', 'PhotoDeleted']       // capability output drives refresh
  }
  access: {
    visibility: 'cluster'
    roles:      ['data-consumer']
  }
}

// maps-app declares dependencies on both the capability and the data product
// In both cases the dependency is on the contract, not on photo-app directly
product 'maps-app' = {
  version:      '1.0.0'
  capabilities: [
    { capability: 'gps-to-place', minVersion: '1.0.0' }
  ]
  dataProducts: [
    { source: 'photo-app/photo-locations', minVersion: '1.0.0' }
  ]
}
```

### Dependency resolution

The platform resolves resource dependencies before provisioning. A container that references a volume will not start until the volume is provisioned. Dependencies are declared implicitly through resource references — there is no explicit `dependsOn` syntax.

### Resource lifecycle events

Every resource state change emits an event to the platform event log:

- `ResourceDeclared` — resource definition received
- `ResourceProvisioning` — platform is actively provisioning
- `ResourceReady` — resource is available
- `ResourceFailed` — provisioning failed, reason attached
- `ResourceDeleted` — resource and all its data removed

These events are projected into the RDF graph and are subscribable by any workload with appropriate IAM permissions.

---