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
runner: cargo-test
runner-args: "tc175_dns_a_records"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

query A records for the cluster root (`picloud.local`), a node hostname, a product hostname, and an ingress hostname. Assert each returns the correct IPv4 address matching the RDF graph.