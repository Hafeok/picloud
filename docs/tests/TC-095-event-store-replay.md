---
id: TC-095
title: event_store_replay
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-032
phase: 1
runner: scripts/run-tc.sh
runner-args: "event-store-replay"
last-run: 2026-04-13T21:37:33.242635225+00:00
---

deploy a product with a deliberate projector bug that projects incorrect triples. Fix the projector in v2. Deploy v2 and replay the event store. Assert the RDF graph now reflects the correct state.