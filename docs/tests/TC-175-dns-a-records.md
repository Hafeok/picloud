---
id: TC-175
title: dns_a_records
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-052
phase: 1
---

query A records for the cluster root (`picloud.local`), a node hostname, a product hostname, and an ingress hostname. Assert each returns the correct IPv4 address matching the RDF graph.