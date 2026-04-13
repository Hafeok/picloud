---
id: TC-196
title: data_domain_declaration
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: "data_domain_declaration"
---

declare a `data-domain` resource. Assert `DataDomainDeclared` event emitted. Assert the domain appears in the cluster graph with correct `pc:steward`, `pc:sensitivity`, and `pc:description` triples.