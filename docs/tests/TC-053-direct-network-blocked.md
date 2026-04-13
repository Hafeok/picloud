---
id: TC-053
title: direct_network_blocked
type: scenario
status: unimplemented
validates:
  features:
  - FT-008
  adrs:
  - ADR-018
phase: 1
---

attempt a direct TCP connection from a container in product A to a container in product B on any port other than the declared ingress. Assert the connection is refused (no route exists).