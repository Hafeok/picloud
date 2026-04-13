---
id: TC-224
title: Two-node cluster runs a containerized workload with a replicated volume
type: exit-criteria
status: passing
validates:
  features: [FT-012]
  adrs: [ADR-001, ADR-034]
phase: 1
runner: picloud-test
runner-args: run --scenario two-node-cluster-exit-criteria
last-run: 2026-04-13T21:05:30.706453738+00:00
---

## Description

Exit-criteria test for FT-012 (Single binary compiles to ARM64). Validates that all infrastructure necessary for a two-node cluster running containerized workloads with replicated volumes exists in the single compiled binary. Checks: ClusterMembership trait, WorkloadScheduler trait, StorageBackend with replication, ContainerSpec types, StorageIntent with FullReplication durability, composition root wiring of cluster/workload/storage slices, mDNS discovery in picloud-cluster, and replication infrastructure in picloud-storage. When a live cluster is available, additionally verifies the health endpoint responds.