---
id: TC-273
title: Freshness monitor emits DataProductStale when maxAge exceeded
type: scenario
status: passing
validates:
  features:
  - FT-068
  adrs:
  - ADR-056
phase: 3
runner: cargo-test
runner-args: "tc273_freshness_monitor_emits_data_product_stale_when_max_age_exceeded"
last-run: 2026-04-15T14:41:42.335607333+00:00
last-run-duration: 0.5s
---

## Description

Declare a data product with a 5-minute maxAge SLO. Project a DataProductRefreshed event with a lastRefreshed timestamp 10 minutes in the past (well beyond the SLO). Run the freshness monitor and verify it emits a Breach action with the correct data product IRI, maxAge, and actual age. Run the monitor a second time and verify it does NOT emit a duplicate breach (de-duplication via internal state tracking).