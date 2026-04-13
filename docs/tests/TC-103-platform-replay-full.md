---
id: TC-103
title: platform_replay_full
type: scenario
status: failing
validates:
  features:
  - FT-002
  adrs:
  - ADR-035
phase: 1
runner: picloud-test
runner-args: "platform-replay-full"
---

emit 500 known events, record the RDF graph state. Clear Oxigraph. Trigger `picloud cluster replay --from epoch`. Assert the resulting graph is byte-identical to the recorded snapshot.