---
id: TC-134
title: cross_cluster_join_rejection
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-042
phase: 1
---

generate an enrollment token from cluster A. Attempt to use it to join cluster B. Assert a `NodeEnrollmentRejected` event in cluster B's log and the node is not added to cluster B's Raft.