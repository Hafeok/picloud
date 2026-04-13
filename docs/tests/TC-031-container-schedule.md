---
id: TC-031
title: container_schedule
type: scenario
status: passing
validates:
  features:
  - FT-005
  adrs:
  - ADR-010
phase: 1
runner: scripts/run-tc.sh
runner-args: "container_schedule"
last-run: 2026-04-13T19:48:54.098720974+00:00
---

apply a container resource. Assert `ResourceReady` event emitted, container running (via `youki state`), and RDF graph reflects `picloud:status picloud:Running`.