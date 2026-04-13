---
id: TC-225
title: Cluster survives one node restart without data loss
type: exit-criteria
status: passing
validates:
  features:
  - FT-014
  adrs:
  - ADR-002
phase: 1
runner: cargo-test
runner-args: "tc225_cluster_survives_one_node_restart_without_data_loss"
last-run: 2026-04-13T20:49:54.762542035+00:00
---

## Description

Validates that a persistent Raft node (sled-backed) retains all committed entries and state machine progress across a full shutdown and restart. Bootstraps a single-node cluster with persistent storage, writes entries through Raft consensus, shuts down cleanly, restarts from the same sled database, and verifies: (1) no re-application of already-committed entries, (2) new writes continue from the persisted offset, (3) the sled database contains all expected state machine metadata.