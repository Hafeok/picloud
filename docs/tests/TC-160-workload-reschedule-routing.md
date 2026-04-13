---
id: TC-160
title: workload_reschedule_routing
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-048
phase: 1
runner: cargo-test
runner-args: "tc160_workload_reschedule_routing"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

reschedule the container targeted by an ingress to a different node. Assert HTTP requests continue succeeding within 30 seconds of the `WorkloadRescheduled` event (routing table updated without manual intervention).