---
id: TC-231
title: Data product declared, projection rebuilt on trigger event, second product queries it
type: exit-criteria
status: passing
validates:
  features: [FT-066]
  adrs: [ADR-056]
phase: 3
runner: cargo-test
runner-args: "tc231_data_product_declared_projection_rebuilt_on_trigger_event_second_product_queries_it"
last-run: 2026-04-15T14:24:34.239051194+00:00
last-run-duration: 1.0s
---

## Description

Deploy product A ("photo-app") with photo-location triples in its operational graph. Declare a data product ("photo-locations") scoped to product A with a SPARQL CONSTRUCT projection. Trigger the projection and verify the data product's own named graph is populated. Deploy product B ("maps-app") and query the data product — assert it can read the projected triples. Update the source data, re-trigger, and verify the atomic swap replaces stale data with fresh results visible to product B.