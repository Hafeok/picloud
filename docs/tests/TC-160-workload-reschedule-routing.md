---
id: TC-160
title: workload_reschedule_routing
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-048
phase: 1
runner: picloud-test
runner-args: "workload-reschedule-routing"
---

reschedule the container targeted by an ingress to a different node. Assert HTTP requests continue succeeding within 30 seconds of the `WorkloadRescheduled` event (routing table updated without manual intervention).