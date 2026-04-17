---
id: TC-201
title: cross_product_internal_graph_blocked
type: scenario
status: passing
validates:
  features:
  - FT-074
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: "tc201_cross_product_internal_graph_blocked"
last-run: 2026-04-17T10:07:29.286719746+00:00
last-run-duration: 0.5s
---

authenticate as a `maps-app` workload identity. Attempt a SPARQL query directly against `https://picloud.local/products/photo-app/graph`. Assert `403 Forbidden`. Assert a `UnauthorisedGraphAccess` event is emitted in the platform log. Repeat with platform-admin identity — assert `200 OK` (admin exemption verified).