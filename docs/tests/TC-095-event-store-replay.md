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
last-run: 2026-04-17T15:53:27.871008242+00:00
last-run-duration: 0.0s
---

deploy a product with a deliberate projector bug that projects incorrect triples. Fix the projector in v2. Deploy v2 and replay the event store. Assert the RDF graph now reflects the correct state.