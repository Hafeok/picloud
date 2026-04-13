---
id: TC-210
title: Cluster survives one node restart without data loss
type: exit-criteria
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-004
  - ADR-002
phase: 1
runner: cargo-test
runner-args: "tc210_cluster_survives_node_restart"
---

## Description

[Describe the test criterion here.]