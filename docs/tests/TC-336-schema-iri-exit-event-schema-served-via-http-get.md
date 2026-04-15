---
id: TC-336
title: Schema IRI exit — event schema served via HTTP GET
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc336_schema_iri_exit_event_schema_served_via_http_get"
validates:
  features: [FT-079]
  adrs: [ADR-031, ADR-032]
phase: 3
last-run: 2026-04-15T16:57:25.325315271+00:00
last-run-duration: 0.6s
---

## Description

Exit criteria: every schema IRI referenced by an EventEnvelope is
dereferenceable via HTTP GET and returns a valid JSON Schema document.

### Checks

1. Multiple platform event types (ResourceReady, NodeJoined, ProductDeployed,
   LeaderElected, ResourceFailed) all resolve to HTTP 200 with valid JSON
   Schema and correct `$id`.
2. Product event schemas across different products (photo-app, analytics) all
   resolve with correct `x-picloud-product` extension and product `const`
   constraint.
3. Version parameter produces distinct `$id` values (v1 ≠ v2).
4. `$schema` always references JSON Schema 2020-12.
5. `Content-Type` is always `application/json`.
6. Invalid versions return HTTP 400 for both platform and product schemas.