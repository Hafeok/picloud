---
id: FT-004
title: Storage Model
phase: 1
status: in-progress
depends-on:
- FT-002
adrs:
- ADR-011
- ADR-012
- ADR-013
- ADR-024
- ADR-047
tests:
- TC-034
- TC-035
- TC-036
- TC-037
- TC-038
- TC-039
- TC-068
- TC-069
- TC-070
- TC-153
- TC-154
- TC-155
- TC-156
- TC-157
- TC-158
- TC-209
- TC-220
- TC-221
domains:
- storage
domains-acknowledged: {}
---

### Block storage

Every node contributes its NVMe to a cluster-wide block storage pool. The platform manages allocation, replication, and placement. Operators never interact with individual disks.

Products declare storage intent — not storage implementation:

```bicep
volume 'media-store' = {
  product: 'photo-app'
  size: '100GB'
  storageIntent: {
    durability: 'full-replication'   // replicate to all available nodes
    performance: 'standard'          // balanced read/write
  }
}
```

**Durability tiers (MVP):**
- `full-replication` — data is replicated to every node. Maximum durability. Default for MVP.

**Snapshots — local NAS (ADR-047):**
Point-in-time immutable copies of a volume stored on a local NAS. Fast recovery from accidental deletion, corruption, or logical failures without internet dependency. Retention policy controls how many daily, weekly, and monthly snapshots are kept. Snapshots are stored separately from cluster NVMe to preserve live storage capacity.

**Offsite backup — S3-compatible (ADR-047):**
Encrypted, deduplicated, incremental backups to any S3-compatible endpoint (Backblaze B2, Cloudflare R2, self-hosted MinIO). Protects against total cluster loss — fire, flood, or theft. Data is encrypted client-side before upload. The platform manages scheduling, chunking, deduplication, and retention.

**The three layers together:**
```
live data:   cluster NVMe  (full-replication across nodes)
snapshots:   local NAS     (fast recovery, no internet)
offsite:     S3 endpoint   (disaster recovery, survives total cluster loss)
```

Replication protects against hardware failure. Snapshots protect against accidental deletion and logical failures. Offsite protects against physical disasters. All three are declared in a single volume resource definition.

**Durability tiers (future phases):**
- `quorum` — replicated to a majority of nodes
- `local` — single node, no replication, for ephemeral or cache workloads
- `none` — ephemeral storage, lost on container restart

**Performance tiers (future phases):**
- `fast` — optimized for low-latency read/write (e.g. databases)
- `standard` — balanced
- `archive` — optimized for sequential write, infrequent read

### Volume types

**Mounted volumes** — presented as a filesystem path inside a container or binary. The platform handles mount lifecycle.

**Raw block devices** — presented as a raw block device. For workloads that manage their own filesystem (e.g. databases).

### RDF graph storage

Each Product that declares an `rdf-store` resource gets a dedicated Oxigraph instance. The instance is:
- Backed by a platform-managed block volume with `full-replication` durability
- IAM-gated — all SPARQL queries and updates require a valid identity token
- Accessible via a SPARQL 1.1 endpoint scoped to the Product
- Automatically backed by the platform event log — graph mutations are events

The cluster-level RDF graph (platform state) is a separate Oxigraph instance managed by the platform, not accessible to workloads directly.

---