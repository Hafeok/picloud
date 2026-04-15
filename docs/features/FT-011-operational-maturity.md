---
id: FT-011
title: Operational Maturity
phase: 4
status: complete
depends-on:
- FT-004
- FT-002
- FT-005
adrs: []
tests:
- TC-236
- TC-292
domains:
- storage
- consensus
- scheduling
- observability
domains-acknowledged: {}
---

## Description

Operational maturity capabilities that harden the platform for sustained production use.

### Storage tiers

Expand the storage intent model beyond `full-replication` to support additional durability tiers: `quorum`, `local`, `none`, and performance tiers: `fast`, `archive`. The platform selects implementation based on declared intent plus available hardware.

### Workload resource constraints

CPU and memory limits on scheduled workloads. The scheduler respects resource constraints when placing workloads on nodes and rejects workloads that exceed available capacity.

### Node drain and graceful migration

`picloud cluster drain {node}` evacuates all workloads from a node before maintenance. Volumes are re-replicated to surviving nodes. The node remains in the cluster as a learner but accepts no new workloads until uncordoned.

### Event log compaction and snapshotting

The append-only event log grows indefinitely. Compaction produces a snapshot of the current RDF graph state at a given log offset. Events before the snapshot offset can be archived or deleted. Replay from the snapshot offset is equivalent to full replay.

### Platform self-monitoring

The platform monitors itself using its own RDF graph and inference rules. Built-in rules detect degraded replication, lagging projections, and Raft health. AlertFired events surface platform-level issues through the same mechanism as application alerts.

### Multi-node Raft voter tuning

Support for 3- and 5-node voter configurations with automatic learner promotion. The platform recommends voter count based on cluster size and warns when the current configuration cannot survive N failures.
