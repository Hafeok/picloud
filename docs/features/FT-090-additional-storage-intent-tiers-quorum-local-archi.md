---
id: FT-090
title: Additional storage intent tiers (quorum, local, archive, fast)
phase: 4
status: planned
depends-on: []
adrs:
- ADR-057
- ADR-024
tests:
- TC-286
- TC-343
domains: []
domains-acknowledged: {}
---

## Description

Extends the storage intent model (ADR-024) with additional durability and performance tiers beyond the MVP `full-replication` tier. Products declare storage requirements semantically and the platform translates intent into implementation.

### Durability tiers

| Tier | Behaviour | Use case |
|---|---|---|
| `full-replication` | Replicated to every node (existing MVP tier) | Maximum durability — default |
| `quorum` | Replicated to a majority of nodes | Databases, event stores — survives minority node failure with lower storage overhead |
| `local` | Single node, no replication | Ephemeral caches, scratch space — fast, no durability guarantee |
| `none` | Ephemeral, lost on container restart | Temporary files, build artifacts — zero persistence |

### Performance tiers

| Tier | Behaviour | Use case |
|---|---|---|
| `standard` | Balanced read/write (existing MVP tier) | General workloads — default |
| `fast` | Optimized for low-latency random I/O | Databases, RDF stores — prioritizes IOPS |
| `archive` | Optimized for sequential write, infrequent read | Media storage, logs — prioritizes throughput over latency |

### Resource syntax

```bicep
volume 'cache' = {
  product: 'photo-app'
  size: '10GB'
  storageIntent: {
    durability: 'local'
    performance: 'fast'
  }
}

volume 'media-archive' = {
  product: 'photo-app'
  size: '500GB'
  storageIntent: {
    durability: 'quorum'
    performance: 'archive'
  }
}
```

### Platform behaviour

- The scheduler selects placement nodes based on available NVMe capacity, current I/O load, and the declared performance tier
- `quorum` replication uses the same block replication infrastructure as `full-replication` but targets `⌈N/2⌉ + 1` nodes instead of all N
- `local` volumes are pinned to the node where the workload runs — if the workload migrates (FT-092), the volume does not follow
- `none` volumes are backed by tmpfs and are destroyed on container restart
- Existing volumes with `full-replication` continue to work unchanged — no migration required
- Tier changes on existing volumes are rejected — operators must create a new volume and migrate data manually

### Events

- `VolumeAllocated` payload includes `durability_tier` and `performance_tier` fields
- Tier metadata is projected into the RDF graph as `picloud:durabilityTier` and `picloud:performanceTier` triples on the volume IRI
