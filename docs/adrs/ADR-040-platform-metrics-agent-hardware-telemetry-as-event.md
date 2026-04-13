---
id: ADR-040
title: Platform Metrics Agent — Hardware Telemetry as Events
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Operational alert rules (ADR-038) need hardware metrics — CPU usage, memory usage, disk usage, and CPU temperature — to be present in the RDF graph so SPARQL CONSTRUCT queries can reason over them. These metrics do not arrive naturally as resource lifecycle events. A collection mechanism is needed that is consistent with the platform's event-sourcing model.

**Decision:** Every node runs a platform metrics agent as a built-in capability (not a separate process — it is part of the `picloud-server` binary). The agent samples hardware metrics on a configurable interval (default: 15 seconds) and emits `MetricRecorded` events to the platform event log. These events are projected into the cluster RDF graph as time-stamped metric triples on each node's IRI.

**Metrics collected per node:**
- CPU usage (%) — per core and aggregate
- Memory used / total (MB)
- Disk used / total / read rate / write rate — per NVMe device
- CPU temperature (°C)
- Network bytes in/out per interface

**Event shape:**
```json
{
  "schema": "https://picloud.local/schemas/events/MetricRecorded/v1",
  "type": "MetricRecorded",
  "source": "https://picloud.local/nodes/pi-node-01",
  "payload": {
    "node_iri": "https://picloud.local/nodes/pi-node-01",
    "metrics": [
      { "name": "cpu_usage_percent",     "value": 42.3, "unit": "percent" },
      { "name": "memory_used_mb",        "value": 8192, "unit": "mb" },
      { "name": "memory_total_mb",       "value": 16384, "unit": "mb" },
      { "name": "disk_used_gb",          "value": 312,  "unit": "gb" },
      { "name": "disk_total_gb",         "value": 1000, "unit": "gb" },
      { "name": "cpu_temp_celsius",      "value": 58.1, "unit": "celsius" }
    ]
  }
}
```

**RDF projection — latest value only:**
The projector writes the latest metric values as triples on the node IRI, overwriting previous values. Historical values live in the event log — the graph holds only the current state:
```turtle
<https://picloud.local/nodes/pi-node-01>
    picloud:cpuUsagePercent 42.3 ;
    picloud:memoryUsedMb 8192 ;
    picloud:memoryTotalMb 16384 ;
    picloud:diskUsedGb 312 ;
    picloud:diskTotalGb 1000 ;
    picloud:cpuTempCelsius 58.1 ;
    picloud:metricsUpdatedAt "2025-07-01T12:00:00Z"^^xsd:dateTime .
```

**Product metrics:**
Workloads emit domain metrics (request count, error rate, latency) as events to the product event bus. The platform does not collect these — workloads are responsible for emitting them. The SDK provides helpers for common metric event shapes.

**Rationale:**
- Built into `picloud-server` — no separate agent process, consistent with single-binary model
- Events are the collection mechanism — metrics flow through the same infrastructure as all other platform state
- Latest-value-only projection keeps the graph lean — historical analysis uses event log replay
- 15-second default interval is sufficient for alert rules while not flooding the event log
- `MetricRecorded` events trigger inference rule evaluation (ADR-038) — alert rules fire within seconds of a threshold breach

**Consequences:**
- At 15-second intervals across 5 nodes, `MetricRecorded` generates ~20 events/minute — well within Raft throughput
- The metrics collection interval is configurable per deployment
- Temperature collection requires reading `/sys/class/thermal/` — Linux-specific, consistent with target platform (ADR-004)
- Metric projection overwrites previous triples — the projector must handle this correctly (upsert, not append)