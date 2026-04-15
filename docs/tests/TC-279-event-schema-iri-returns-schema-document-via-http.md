---
id: TC-279
title: Event schema IRI returns schema document via HTTP GET
type: scenario
status: passing
runner: cargo-test
runner-args: "tc279_event_schema_iri_returns_schema_document_via_http_get"
validates:
  features: [FT-079]
  adrs: [ADR-031]
phase: 3
last-run: 2026-04-15T16:57:25.325315271+00:00
last-run-duration: 0.5s
---

## Description

Verifies that platform and product event schema IRIs are dereferenceable via
HTTP GET and return valid JSON Schema documents.

### Steps

1. GET `/schemas/events/ResourceReady/v1` — assert HTTP 200, Content-Type
   application/json, response is valid JSON Schema with correct `$id`,
   `$schema`, `title`, `type`, `properties` (all EventEnvelope fields), and
   `required` array.
2. GET `/products/photo-app/schemas/events/OrderPlaced/v1` — assert HTTP 200,
   `$id` includes product path, `x-picloud-product` extension is present.
3. GET `/schemas/events/ResourceReady/vabc` — assert HTTP 400 for invalid
   version.