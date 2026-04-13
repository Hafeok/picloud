---
id: TC-187
title: capability_declaration
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
---

declare a `capability` resource via `picloud resource apply`. Assert a `CapabilityDeclared` event is emitted. Assert the capability appears in the cluster RDF graph as a `pc:Capability` node with correct `pc:version`, `pc:inputEvent`, and `pc:outputEvent` triples.