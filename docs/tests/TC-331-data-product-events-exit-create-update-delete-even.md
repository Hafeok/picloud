---
id: TC-331
title: Data product events exit — create, update, delete events emitted
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc331_data_product_events_exit_create_update_delete_events_emitted"
validates:
  features: [FT-070]
  adrs: [ADR-056]
phase: 3
last-run: 2026-04-15T15:53:52.149111234+00:00
last-run-duration: 0.5s
---

## Description

Exit-criteria test for data product lifecycle events (FT-070). Verifies the complete
lifecycle invariant: multiple data products can be created, updated sequentially
(version bumps, domain reassignment, SLO changes), and selectively deleted while
sibling data products remain unaffected. Tests two data products in the same owning
product, multiple sequential updates, and isolated deletion. This is the gate
criterion — if this passes, FT-070 is considered complete.