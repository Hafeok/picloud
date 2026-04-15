---
id: TC-275
title: Data domain lifecycle events emitted on create, update, delete
type: scenario
status: passing
runner: cargo-test
runner-args: "tc275_data_domain_lifecycle_events_emitted_on_create_update_delete"
validates:
  features: [FT-071]
  adrs: [ADR-056]
phase: 3
last-run: 2026-04-15T16:00:55.097586476+00:00
last-run-duration: 0.6s
---

## Description

Scenario test for the data domain lifecycle events feature (FT-071).

Verifies that the three core lifecycle events (DataDomainDeclared, DataDomainUpdated, DataDomainDeleted) are properly emitted and projected into the RDF graph:

1. **Create** — Declare a data domain with steward and sensitivity classification. Verify it is projected as a `pc:DataDomain` with correct metadata (name, steward, sensitivity, status="declared").
2. **Update** — Emit a `DataDomainUpdated` event that reassigns the steward and reclassifies the sensitivity. Verify the RDF graph reflects both changes atomically — old values removed, new values present, immutable fields (type, name, status) unchanged.
3. **Delete** — Emit a `DataDomainDeleted` event. Verify all triples about the data domain are removed from every graph.
4. **Full lifecycle** — The same IRI transitions from declared → updated → deleted with correct state at each stage.