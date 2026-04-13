---
id: TC-095
title: event_store_replay
type: scenario
status: failing
validates:
  features:
  - FT-008
  adrs:
  - ADR-032
phase: 1
runner: picloud-test
runner-args: "event-store-replay"
---

deploy a product with a deliberate projector bug that projects incorrect triples. Fix the projector in v2. Deploy v2 and replay the event store. Assert the RDF graph now reflects the correct state.