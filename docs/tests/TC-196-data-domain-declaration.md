---
id: TC-196
title: data_domain_declaration
type: scenario
status: failing
validates:
  features:
  - FT-065
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_domain_declaration
last-run: 2026-04-15T14:29:59.558362753+00:00
last-run-duration: 0.8s
failure-message: "No matching test function found (0 tests ran)"
---

declare a `data-domain` resource. Assert `DataDomainDeclared` event emitted. Assert the domain appears in the cluster graph with correct `pc:steward`, `pc:sensitivity`, and `pc:description` triples.