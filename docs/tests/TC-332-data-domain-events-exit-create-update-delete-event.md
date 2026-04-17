---
id: TC-332
title: Data domain events exit — create, update, delete events emitted
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc332_data_domain_events_exit_create_update_delete_events_emitted"
validates:
  features: [FT-071]
  adrs: [ADR-056]
phase: 3
last-run: 2026-04-17T09:58:44.439233298+00:00
last-run-duration: 0.6s
---

## Description

Exit-criteria test for the data domain lifecycle events feature (FT-071).

Verifies the complete lifecycle invariant: a data domain can be created, updated multiple times, and then deleted, with each event correctly mutating the RDF graph. This is the gate criterion — if this passes, the feature is considered complete.

Exercises:
- Two distinct data domains with different sensitivity levels coexist
- Multiple sequential updates to the same data domain (steward reassignment, sensitivity reclassification)
- Deletion of one data domain while the other survives
- Final deletion of the second data domain — catalog is empty
- All mutable fields (steward, sensitivity) are correctly replaced on update (old values removed, new values present)