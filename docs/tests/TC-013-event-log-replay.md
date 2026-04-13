---
id: TC-013
title: event_log_replay
type: scenario
status: unimplemented
validates:
  features:
  - FT-002
  adrs:
  - ADR-004
phase: 1
---

apply a set of resources, record the RDF graph state via SPARQL, wipe the Oxigraph projection, replay the event log from index 0, assert the resulting graph is byte-identical to the recorded snapshot.