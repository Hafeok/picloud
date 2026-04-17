---
id: TC-196
title: data_domain_declaration
type: scenario
status: passing
validates:
  features:
  - FT-065
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_domain_declaration
last-run: 2026-04-17T09:03:38.439227156+00:00
last-run-duration: 0.6s
---

declare a `data-domain` resource. Assert `DataDomainDeclared` event emitted. Assert the domain appears in the cluster graph with correct `pc:steward`, `pc:sensitivity`, and `pc:description` triples.