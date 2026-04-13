---
id: TC-031
title: container_schedule
type: scenario
status: failing
validates:
  features:
  - FT-005
  adrs:
  - ADR-010
phase: 1
runner: picloud-test
runner-args: "container_schedule"
---

apply a container resource. Assert `ResourceReady` event emitted, container running (via `youki state`), and RDF graph reflects `picloud:status picloud:Running`.