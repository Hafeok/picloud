---
id: TC-187
title: capability_declaration
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_declaration"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
failure-message: "No matching test function found (0 tests ran)"
---

declare a `capability` resource via `picloud resource apply`. Assert a `CapabilityDeclared` event is emitted. Assert the capability appears in the cluster RDF graph as a `pc:Capability` node with correct `pc:version`, `pc:inputEvent`, and `pc:outputEvent` triples.