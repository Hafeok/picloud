---
id: TC-328
title: Capability list exit — all capabilities shown with fulfilment status
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc328_capability_list_exit_all_capabilities_shown_with_fulfilment_status"
validates:
  features: [FT-064]
  adrs: [ADR-055]
phase: 3
last-run: 2026-04-17T08:53:40.499242497+00:00
last-run-duration: 0.5s
---

## Description

Exit-criteria test validating that `picloud capability list` handles all edge
cases: empty result sets, multiple capabilities with mixed fulfilment states,
multiple implementors and consumers per capability, nested SPARQL JSON response
format (`results.bindings`), malformed response bodies, product name extraction
from full IRIs and plain names, SPARQL query well-formedness (GROUP BY, ORDER BY,
OPTIONAL), URL-encoding round-trip safety, and table formatting with separator
lines and correct column alignment.