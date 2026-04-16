---
id: FT-076
title: Built-in platform alert rules (CPU temp, memory, disk, node health, workload failure)
phase: 3
status: complete
depends-on: []
adrs:
- ADR-041
- ADR-040
tests:
- TC-123
- TC-124
- TC-125
- TC-126
- TC-127
- TC-128
- TC-129
- TC-130
- TC-131
- TC-132
- TC-229
domains: []
domains-acknowledged: {}
---

## Description

The platform ships built-in alert rules as SPARQL CONSTRUCT inference rules (ADR-041). These rules evaluate on `MetricRecorded` events and assert `picloud:Alert` triples when thresholds are breached. Alert lifecycle (fired/resolved) is automatic.

### Built-in platform alert rules

| Rule | Threshold | Severity |
|---|---|---|
| High CPU temperature | > 80°C | critical |
| High CPU temperature | > 70°C | warning |
| High memory usage | > 90% | critical |
| High memory usage | > 80% | warning |
| High disk usage | > 90% | critical |
| Node unreachable | Raft heartbeat missed | critical |
| Workload failed | `ResourceStatus = Failed` | critical |

### Example rule (CPU temperature)

```sparql
CONSTRUCT {
  ?node a picloud:Alert ;
        picloud:alertType "HighCpuTemperature" ;
        picloud:alertSeverity "critical" ;
        picloud:alertMessage "CPU temperature above 80°C" ;
        picloud:alertResource ?node .
}
WHERE {
  ?node a picloud:Node ;
        picloud:cpuTempCelsius ?temp .
  FILTER(?temp > 80.0)
}
```

### Alert lifecycle

- When the CONSTRUCT produces a new `picloud:Alert` triple → `AlertFired` event
- When the condition clears and the triple is retracted → `AlertResolved` event
- Resolution is automatic — when CPU temperature drops below 80°C, the CONSTRUCT no longer matches, the triple is retracted, and `AlertResolved` fires

### Custom alert rules

Products can declare their own alert rules as `inference-rule` resources. Any CONSTRUCT query that produces `picloud:Alert` triples is an alert rule.

### Querying active alerts

```bash
picloud graph query --sparql "SELECT ?resource ?type ?severity WHERE { ?a a picloud:Alert ; picloud:alertResource ?resource ; picloud:alertType ?type ; picloud:alertSeverity ?severity . }"
```

### No built-in notification

Alerts are events. The platform does not deliver notifications (Slack, email, PagerDuty). Products built on PiCloud subscribe to `AlertFired`/`AlertResolved` and handle delivery.
