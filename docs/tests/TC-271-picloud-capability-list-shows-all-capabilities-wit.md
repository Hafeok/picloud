---
id: TC-271
title: picloud capability list shows all capabilities with fulfilment status
type: scenario
status: passing
runner: cargo-test
runner-args: "tc271_picloud_capability_list_shows_all_capabilities_with_fulfilment_status"
validates:
  features: [FT-064]
  adrs: [ADR-055]
phase: 3
last-run: 2026-04-17T08:53:40.499242497+00:00
last-run-duration: 0.5s
---

## Description

Verifies that the `picloud capability list` CLI command correctly constructs a
SPARQL query that fetches all capabilities along with their implementors,
consumers, and fulfilment status. Tests response parsing, fulfilment derivation
(fulfilled iff at least one implementor exists), product name extraction from
IRIs, and table formatting with NAME, VERSION, FULFILLED, IMPLEMENTORS, and
CONSUMERS columns.