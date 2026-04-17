---
id: FT-075
title: Platform metrics agent — MetricRecorded events at 15s interval per node
phase: 3
status: complete
depends-on: []
adrs:
- ADR-040
- ADR-004
tests:
- TC-229
domains: []
domains-acknowledged: {}
---

## Description

Every node runs a built-in metrics agent as part of the `picloud-server` binary (ADR-040). The agent samples hardware metrics every 15 seconds and emits `MetricRecorded` events to the platform event log.

### Metrics collected per node

| Metric | Unit | Source |
|---|---|---|
| CPU usage (per core + aggregate) | percent | `/proc/stat` |
| Memory used / total | MB | `/proc/meminfo` |
| Disk used / total | GB | NVMe device stats |
| Disk read rate / write rate | MB/s | `/sys/block/*/stat` |
| CPU temperature | °C | `/sys/class/thermal/` |
| Network bytes in / out | bytes | `/sys/class/net/*/statistics/` |

### Event shape

```json
{
  "type": "MetricRecorded",
  "source": "https://picloud.local/nodes/pi-node-01",
  "payload": {
    "node_iri": "https://picloud.local/nodes/pi-node-01",
    "metrics": [
      { "name": "cpu_usage_percent", "value": 42.3, "unit": "percent" },
      { "name": "memory_used_mb", "value": 8192, "unit": "mb" },
      { "name": "cpu_temp_celsius", "value": 58.1, "unit": "celsius" }
    ]
  }
}
```

### RDF projection — latest value only

The projector writes the latest metric values as triples on the node IRI, overwriting previous values:
```turtle
<https://picloud.local/nodes/pi-node-01>
    picloud:cpuUsagePercent 42.3 ;
    picloud:memoryUsedMb 8192 ;
    picloud:memoryTotalMb 16384 ;
    picloud:cpuTempCelsius 58.1 ;
    picloud:metricsUpdatedAt "2025-07-01T12:00:00Z"^^xsd:dateTime .
```

Historical values live in the event log — the graph holds only the current state.

### Inference trigger

`MetricRecorded` events trigger evaluation of alert inference rules (FT-076). Alert rules fire within seconds of a threshold breach.

### Event volume

At 15-second intervals across N nodes, this generates ~4×N events/minute — well within Raft throughput for clusters up to dozens of nodes. The interval is configurable per deployment.
