---
id: TC-335
title: Graph isolation exit — cross-product access returns 403
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc335_graph_isolation_exit_cross_product_access_returns_403"
validates:
  features: [FT-074]
  adrs: []
phase: 3
last-run: 2026-04-17T10:07:29.286719746+00:00
last-run-duration: 0.6s
---

## Description

Exit-criteria test that verifies product graph isolation holds across multiple
product boundaries. A non-owner, non-admin user with a token scoped to one
product must receive HTTP 403 Forbidden for every other product's graph endpoint
they attempt to access.

The test issues a single token scoped to "app-alpha" and verifies that accessing
the graph endpoints of "app-beta", "app-gamma", and "app-delta" all return 403.
It also confirms the same token succeeds for the owning product's graph (app-alpha).