---
id: ADR-047
title: Volume Snapshots and Offsite Backup as Storage Intent Primitives
status: accepted
features:
- FT-004
- FT-033
- FT-034
- FT-035
- FT-036
- FT-037
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:7cf430cd7aae734f494bc9f17f74ebab87630d9f2fa2120c024406e35767e54a
---

**Status:** Accepted

**Context:** Replication across cluster nodes (ADR-013, ADR-024) protects against hardware failure. It does not protect against accidental deletion, data corruption, logical failures, or physical disasters affecting all nodes simultaneously (fire, flood, theft). For irreplaceable data — family photos, personal documents, application state — point-in-time snapshots and offsite backup are essential additional layers.

**The three failure scenarios and their mitigations:**

| Scenario | Replication | Snapshots | Offsite |
|---|---|---|---|
| Node hardware failure | ✓ | ✓ | ✓ |
| Accidental deletion | ✗ | ✓ | ✓ |
| Data corruption / bug | ✗ | ✓ | ✓ |
| Total cluster loss (fire/flood/theft) | ✗ | ✗ | ✓ |

**Decision:** Volume snapshots and offsite backup are first-class storage intent primitives declared in the volume resource definition. Snapshots are stored on a local NAS (fast recovery). Offsite backup targets S3-compatible endpoints (disaster recovery). Both are configured declaratively — the platform manages scheduling, retention, and transfer.

### Volume declaration with snapshots and backup

```bicep
volume 'family-photos' = {
  product: 'photo-app'
  size: '500GB'
  storageIntent: {
    durability:  'full-replication'
    performance: 'standard'
    snapshots: {
      enabled:  true
      schedule: 'daily'           // hourly | daily | weekly
      storage:  secret('nas-snapshot-config')
      retention: {
        daily:   30               // keep 30 daily snapshots
        weekly:  26               // keep 26 weekly snapshots
        monthly: 0                // 0 = keep forever
      }
    }
    offsite: {
      enabled:   true
      target:    secret('s3-backup-config')
      frequency: 'daily'          // daily | weekly
      encryption: true            // always encrypt before upload
    }
  }
}
```

### Snapshot storage — local NAS

Snapshots are point-in-time, immutable copies of a volume stored on a local NAS. The NAS is referenced via a secret containing connection details (NFS mount path or SMB share). Snapshots are not stored on cluster NVMe — this preserves the full NVMe capacity for live data.

**Snapshot secret format:**
```json
{
  "type":   "nfs",
  "host":   "192.168.1.200",
  "path":   "/volume1/picloud-snapshots",
  "options": "vers=4,rsize=1048576,wsize=1048576"
}
```

**Snapshot naming convention:**
```
{volume-name}/{product}/{date}T{time}Z.snapshot
family-photos/photo-app/2025-07-01T02:00:00Z.snapshot
```

**Snapshot schedule:**
The platform runs a snapshot job according to the declared schedule. Snapshots are crash-consistent — the volume is quiesced briefly during the snapshot operation. The snapshot job emits `SnapshotCreated` and `SnapshotFailed` events.

**Retention enforcement:**
After each snapshot, the platform evaluates retention policy and deletes snapshots outside the policy window. Deletion emits `SnapshotDeleted` events. The retention policy is evaluated per category:
- `daily: 30` — keep the most recent 30 daily snapshots
- `weekly: 26` — keep the most recent 26 weekly snapshots (Sunday snapshots are promoted to weekly)
- `monthly: 0` — keep all monthly snapshots forever (first snapshot of each month promoted to monthly)

**Recovery:**
```bash
# List available snapshots for a volume
picloud volume snapshots family-photos

# Restore a volume to a point in time
picloud volume restore family-photos \
  --snapshot "2025-07-01T02:00:00Z" \
  --target family-photos-restored
```

### Offsite backup — S3-compatible endpoint

Offsite backup uploads encrypted volume data to any S3-compatible endpoint. Recommended providers for home use: Backblaze B2 (cheapest per GB), Cloudflare R2 (no egress fees), or a self-hosted MinIO instance at a family member's location.

**S3 backup secret format:**
```json
{
  "type":     "s3",
  "endpoint": "https://s3.us-west-000.backblazeb2.com",
  "bucket":   "picloud-backup-emil",
  "region":   "us-west-000",
  "access_key_id":     "...",
  "secret_access_key": "..."
}
```

**Encryption:** All data is encrypted client-side before upload using a platform-managed key stored in the cluster's secret store. The S3 provider never sees plaintext data. The encryption key is itself backed up to the NAS snapshot store — losing the key means losing the backup.

**Backup format:** Volumes are uploaded as chunked, deduplicated, compressed archives. Chunks that have not changed since the last backup are not re-uploaded. This makes incremental backups efficient even for large volumes.

**Backup schedule:**
The platform runs offsite backup jobs according to the declared frequency. Backup jobs emit `BackupStarted`, `BackupCompleted`, and `BackupFailed` events. Backup duration and bytes transferred are recorded in the platform metrics (ADR-040) and queryable via the RDF graph.

**Recovery from offsite:**
```bash
# List available offsite backups
picloud volume backups family-photos --offsite

# Restore from offsite (slower — downloads from S3)
picloud volume restore family-photos \
  --offsite \
  --date "2025-07-01" \
  --target family-photos-restored
```

### Snapshot and backup status in the RDF graph

Current snapshot and backup state is projected into the RDF graph:

```turtle
<https://picloud.local/products/photo-app/volumes/family-photos>
    a picloud:Volume ;
    picloud:lastSnapshotAt     "2025-07-01T02:00:00Z"^^xsd:dateTime ;
    picloud:lastSnapshotStatus "success" ;
    picloud:snapshotCount      47 ;
    picloud:lastBackupAt       "2025-07-01T03:00:00Z"^^xsd:dateTime ;
    picloud:lastBackupStatus   "success" ;
    picloud:lastBackupSizeGb   312.4 .
```

This means alert rules can fire on backup failures:

```bicep
inference-rule 'backup-failed-alert' = {
  scope: 'platform'
  trigger: 'event'
  trigger-events: ['BackupFailed']
  construct: '''
    CONSTRUCT {
      ?volume a picloud:Alert ;
              picloud:alertType     "BackupFailed" ;
              picloud:alertSeverity "critical" ;
              picloud:alertMessage  "Offsite backup failed — data at risk" ;
              picloud:alertResource ?volume .
    }
    WHERE {
      ?volume a picloud:Volume ;
              picloud:lastBackupStatus "failed" .
    }
  '''
}
```

**Rationale:**
- Snapshots and backup are declared in the volume resource — versioned, auditable, consistent with IaC-as-only-interface (ADR-010)
- NAS for snapshots keeps recovery fast and local — no internet dependency for common recovery scenarios
- S3 for offsite keeps disaster recovery simple — any S3-compatible provider works, including self-hosted
- Client-side encryption before upload means the backup is secure regardless of provider security posture
- Separating snapshot storage from cluster NVMe preserves full cluster storage capacity for live data
- Backup failures emit events and fire alert rules — operators are notified before they discover data loss the hard way
- Secrets for NAS and S3 credentials follow the existing secret injection model (ADR-009) — no new credential management needed

**Rejected alternatives:**
- **External backup tools (Restic, Velero)** — adds external dependencies and operates outside the platform's event log, making backup state unauditable.
- **No built-in backup** — unacceptable for a platform that promises durability; operators would need to build backup infrastructure from scratch.

**Consequences:**
- `picloud-storage` gains NFS/SMB mount capability for snapshot storage
- `picloud-storage` gains an S3-compatible client (`aws-sdk-s3` or `opendal` crate) for offsite backup
- The encryption key for S3 backups must be backed up — losing it means losing all offsite backups. The platform should warn loudly if the encryption key has no backup.
- Snapshot quiescing requires coordination with the workload — containers receive `SIGTSTP` during snapshot, `SIGCONT` after. Duration should be milliseconds.
- A volume with both snapshots and offsite backup enabled uses three storage locations: cluster NVMe (live), NAS (snapshots), S3 (offsite). All three are declared in one resource definition.