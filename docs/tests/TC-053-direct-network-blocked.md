---
id: TC-053
title: direct_network_blocked
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-018
phase: 1
runner: scripts/run-tc.sh
runner-args: "direct-network-blocked"
last-run: 2026-04-18T13:20:29.293271188+00:00
last-run-duration: 0.0s
---

attempt a direct TCP connection from a container in product A to a container in product B on any port other than the declared ingress. Assert the connection is refused (no route exists).