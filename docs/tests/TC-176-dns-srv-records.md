---
id: TC-176
title: dns_srv_records
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-052
phase: 1
---

query `_sparql._tcp.photo-app.picloud.local`. Assert the SRV record returns the correct host and port for the product's SPARQL endpoint.