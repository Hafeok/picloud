---
id: TC-201
title: cross_product_internal_graph_blocked
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
---

authenticate as a `maps-app` workload identity. Attempt a SPARQL query directly against `https://picloud.local/products/photo-app/graph`. Assert `403 Forbidden`. Assert a `UnauthorisedGraphAccess` event is emitted in the platform log. Repeat with platform-admin identity — assert `200 OK` (admin exemption verified).