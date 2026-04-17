---
id: FT-059
title: capability resource type — cluster-scoped interface contract with ontology, SHACL shapes, and declared event types
phase: 3
status: complete
depends-on: []
adrs:
- ADR-055
- ADR-029
tests:
- TC-232
domains: []
domains-acknowledged: {}
---

## Description

A `capability` is a cluster-scoped resource type representing a named, versioned interface contract (ADR-055). A capability declares an event schema (input/output events), an ontology, and SHACL shapes — but has no workload, no container, no code. It is a pure contract.

### Resource syntax

```bicep
capability 'gps-to-place' = {
  version: '1.0.0'
  description: 'Translates GPS coordinates to a named place with confidence score'
  ontology: './capabilities/gps-to-place.ttl'
  shapes: './capabilities/gps-to-place.shacl'
  events: {
    input: 'CoordinatesReceived'
    output: 'PlaceResolved'
  }
}
```

### Validation rules (at `resource apply` time)

1. Must declare at least one `input` event and one `output` event
2. Must declare `ontology` or `shapes` (or both) — the contract must be expressed in the type system
3. Cannot be deleted while any Product declares a `capabilities` dependency on it

### RDF projection

```turtle
<https://picloud.local/capabilities/gps-to-place>
    a picloud:Capability ;
    picloud:version "1.0.0" ;
    picloud:inputEvent "CoordinatesReceived" ;
    picloud:outputEvent "PlaceResolved" ;
    picloud:ontology <https://picloud.local/capabilities/gps-to-place/ontology> .
```

### Relationship to Products

- Products declare `implements` to fulfil a capability (FT-060)
- Products declare `capabilities` to consume a capability (FT-061)
- Consumers bind to the capability contract, never to a specific implementing Product
