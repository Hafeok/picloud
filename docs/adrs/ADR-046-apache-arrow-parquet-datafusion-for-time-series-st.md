---
id: ADR-046
title: Apache Arrow + Parquet + DataFusion for Time-Series Storage
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Raw OTel spans, metrics, and logs are high-volume, high-cardinality, time-bounded data. They cannot go into the Raft event log (volume) or Oxigraph (cardinality). A dedicated time-series storage layer is needed that is pure Rust, embeds into `picloud-server`, runs on ARM64, and supports efficient time-range queries for the aggregator and for operator inspection.

**Decision:** Raw OTel data is stored as Apache Parquet files on each node's NVMe. Apache Arrow is the in-memory columnar format. DataFusion provides SQL query execution over the Parquet files. All three crates are pure Rust and compile to ARM64 with no external dependencies.

### Storage layout

```
/home/ubuntu/picloud/data/telemetry/
├── traces/
│   ├── 2025-07-01T00/    ← hourly partitions
│   │   ├── part-0001.parquet
│   │   └── part-0002.parquet
│   └── 2025-07-01T01/
├── metrics/
│   ├── 2025-07-01T00/
│   └── 2025-07-01T01/
└── logs/
    ├── 2025-07-01T00/
    └── 2025-07-01T01/
```

Partitioned by hour. Each partition is one or more Parquet files, rotated when they reach a configurable size (default: 128MB). Old partitions are deleted by a retention policy (default: 7 days for traces, 30 days for metrics, 7 days for logs).

### Parquet schema — traces

```
trace_id:       Utf8
span_id:        Utf8
parent_span_id: Utf8 (nullable)
operation_name: Utf8
service_name:   Utf8
product:        Utf8 (nullable)
node_id:        Utf8
start_time:     Timestamp(Nanosecond)
end_time:       Timestamp(Nanosecond)
duration_ms:    Float64
status:         Utf8   (ok | error | unset)
attributes:     Utf8   (JSON)
```

### Parquet schema — metrics

```
timestamp:      Timestamp(Nanosecond)
resource_iri:   Utf8
metric_name:    Utf8
metric_value:   Float64
unit:           Utf8
product:        Utf8 (nullable)
node_id:        Utf8
attributes:     Utf8   (JSON)
```

### Parquet schema — logs

```
timestamp:      Timestamp(Nanosecond)
trace_id:       Utf8 (nullable)
span_id:        Utf8 (nullable)
severity:       Utf8
body:           Utf8
service_name:   Utf8
product:        Utf8 (nullable)
node_id:        Utf8
attributes:     Utf8  (JSON)
```

### Querying via DataFusion

The platform exposes a OTLP-compatible query API and a SQL endpoint over DataFusion:

```bash
# CLI query
picloud telemetry query \
  --signal traces \
  --from "2025-07-01T00:00:00Z" \
  --to   "2025-07-01T01:00:00Z" \
  --sql  "SELECT operation_name, AVG(duration_ms) FROM traces
          WHERE product = 'photo-app'
          GROUP BY operation_name
          ORDER BY AVG(duration_ms) DESC"
```

### Aggregator reads from Parquet

The metric aggregator (ADR-045) queries Parquet files via DataFusion every 15 seconds to compute summaries:

```sql
SELECT
  resource_iri,
  AVG(metric_value)                                    AS avg_value,
  PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY metric_value) AS p95_value
FROM metrics
WHERE metric_name = 'http.request.duration'
  AND timestamp > NOW() - INTERVAL '15 seconds'
GROUP BY resource_iri
```

Results become `MetricRecorded` events → Oxigraph → alert rules.

### Retention policy

Configurable per signal type. Default:

| Signal | Retention |
|---|---|
| Traces | 7 days |
| Metrics | 30 days |
| Logs | 7 days |

A background task runs hourly, deletes partition directories older than the retention window.

### Why not Delta Lake

Delta Lake is built on Parquet and adds ACID transactions, schema evolution, and time-travel. These are valuable for analytical workloads but add a Spark or DuckDB dependency for writes. A future Rust-native Delta Lake implementation would be a natural evolution of this storage layer — the Parquet files produced here would be compatible with Delta Lake with the addition of a transaction log.

**Rationale:**
- Pure Rust — arrow, parquet, datafusion crates all compile to ARM64 with no external dependencies (ADR-001)
- Single binary stays intact — no separate time-series daemon
- Columnar Parquet is highly efficient for time-range and aggregation queries over metric data
- DataFusion SQL is accessible to LLMs and operators without specialised knowledge
- Hourly partitioning means retention cleanup is O(1) — delete a directory, no compaction needed
- Parquet is self-describing and portable — files can be analysed off-node with any Arrow-compatible tool
- Natural upgrade path to Delta Lake when a Rust-native implementation is available

**Consequences:**
- `picloud-storage` gains a `TelemetryStore` implementation backed by Parquet
- The telemetry store is local to each node — not distributed across the cluster
- For cluster-wide telemetry queries, the aggregated summaries in Oxigraph are the right layer — raw Parquet queries are per-node
- Write throughput must be benchmarked on Pi5 NVMe — Parquet writes are batched, not per-span
- The `arrow`, `parquet`, and `datafusion` crates add significant compile time — acceptable given the capability they provide