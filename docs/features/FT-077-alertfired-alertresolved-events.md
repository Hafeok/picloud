---
id: FT-077
title: AlertFired / AlertResolved events
phase: 3
status: planned
depends-on: []
adrs:
- ADR-041
- ADR-038
tests:
- TC-229
domains: []
domains-acknowledged: {}
---

## Description

Alert state changes emit `AlertFired` and `AlertResolved` events to the platform event log (ADR-041). These events are the subscribable interface for building notification, escalation, or auto-remediation products.

### AlertFired

Emitted when a SPARQL CONSTRUCT inference rule asserts a new `picloud:Alert` triple:
```json
{
  "type": "AlertFired",
  "payload": {
    "alert_type": "HighCpuTemperature",
    "severity": "critical",
    "message": "CPU temperature above 80°C on pi-node-02",
    "resource_iri": "https://picloud.local/nodes/pi-node-02",
    "rule_iri": "https://picloud.local/inference-rules/high-cpu-temp-critical",
    "fired_at": "2025-07-01T12:00:00Z"
  }
}
```

### AlertResolved

Emitted when the condition clears and the alert triple is retracted:
```json
{
  "type": "AlertResolved",
  "payload": {
    "alert_type": "HighCpuTemperature",
    "severity": "critical",
    "resource_iri": "https://picloud.local/nodes/pi-node-02",
    "resolved_at": "2025-07-01T12:05:00Z"
  }
}
```

### Damping

A minimum 60-second hold-off prevents rapid fire/resolve cycles. The same alert on the same resource cannot re-fire within 60 seconds of resolution.

### Subscribability

Any workload with platform-level event permissions can subscribe to `AlertFired` and `AlertResolved`. This is the mechanism for building notification products — the platform provides the signal, products provide the delivery.
