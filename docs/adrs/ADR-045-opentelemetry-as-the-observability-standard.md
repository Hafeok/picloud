---
id: ADR-045
title: OpenTelemetry as the Observability Standard
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Both the platform and Products need observability — traces, metrics, and logs. The standard for this is OpenTelemetry (OTel). The platform must produce OTel signals for its own operations and provide a path for workloads to emit theirs. OTel data must feed the alert system without overwhelming the Raft-replicated event log or Oxigraph.

**Decision:** OTel is the observability standard for the platform and all Products. The platform runs an OTel event stream — a high-throughput, non-Raft-replicated channel separate from the platform event log. Raw OTel flows through this stream to subscribers. A platform aggregator samples it every 15 seconds, computes summaries, and emits `MetricRecorded` events into the Raft log. Those summaries land in Oxigraph and feed the alert inference rules (ADR-041). Raw OTel spans and metrics are stored in the time-series layer (ADR-046).

### Signal coverage

All three OTel signals are in scope from day one:

- **Traces** — every CLI command produces a root span. Platform operations (Raft append, RDF projection, workload scheduling, inference rule evaluation) produce child spans. Workload traces are correlated to platform traces via W3C trace context propagation.
- **Metrics** — hardware metrics (ADR-040) and workload-emitted domain metrics flow through the OTel metrics pipeline. Aggregated summaries feed Oxigraph.
- **Logs** — structured logs from `picloud-server` and workloads are emitted as OTel log records with trace context attached.

### The OTel event stream

A dedicated pub/sub channel inside `picloud-server` — not Raft-replicated, not written to the event log. High throughput, bounded buffer, drop-on-overflow for non-critical signals. Subscribers register at runtime:

```
OTel signal produced
  → OTel event stream (in-process pub/sub)
    → Time-series store (ADR-046)       ← raw spans, metrics, logs
    → Platform aggregator               ← every 15s
      → MetricRecorded event            ← Raft log → Oxigraph → alerts
    → External OTel exporter (optional) ← OTLP to Grafana, Tempo, etc.
```

### CLI trace propagation

Every CLI command creates a root OTel span. The correlation ID on the command event carries the trace context. Platform operations that process that command create child spans under the same trace. The result is a complete trace from CLI invocation through Raft append, projection, scheduling, and workload startup.

```
picloud resource apply ./photo-app/
└── [trace] resource.apply
    ├── [span] raft.append
    ├── [span] rdf.project
    └── [span] workload.schedule
        └── [span] container.start
```

### Workload OTel configuration

The platform injects OTel configuration into every workload as environment variables at startup:

```
OTEL_SERVICE_NAME=photo-app.api-server
OTEL_SERVICE_VERSION=2.1.0
OTEL_EXPORTER_OTLP_ENDPOINT=https://picloud.local/otel
OTEL_RESOURCE_ATTRIBUTES=picloud.product=photo-app,picloud.node=pi-node-01
```

Workloads configure their OTel SDK using these variables — no hardcoding. Additional configuration can be set per-workload in the resource file or via the SDK at startup:

```bicep
container 'api-server' = {
  product: 'photo-app'
  otel: {
    traces: true
    metrics: true
    logs: true
    sampleRate: 1.0
  }
}
```

### Trace correlation — platform to workload

When a platform event causes a workload to receive traffic (e.g. `ResourceReady` triggers a health check, or an event subscription delivers an event), the platform attaches W3C trace context headers. The workload's OTel SDK picks these up automatically and creates child spans under the platform's trace. This gives end-to-end traces from CLI command through platform operations through workload execution.

### Aggregation into Oxigraph

The platform aggregator reads from the OTel stream every 15 seconds and computes per-resource summaries:

- Request rate (req/s)
- Error rate (%)
- P50/P95/P99 latency (ms)
- Active span count

These are written as `MetricRecorded` events — identical in structure to hardware metrics (ADR-040). Inference rules treat them identically. An alert rule for "product error rate above 5%" uses the same SPARQL CONSTRUCT pattern as a CPU temperature alert.

### External OTel export

Operators can configure an OTLP endpoint to forward raw OTel data to external systems (Grafana, Tempo, Jaeger, Prometheus). This is optional — the platform works without it. When configured, the external exporter is a subscriber on the OTel event stream.

```bicep
# Platform-level config
otel-export 'grafana' = {
  endpoint: 'https://grafana.acme.local:4317'
  protocol: 'grpc'
  signals: ['traces', 'metrics', 'logs']
}
```

**Rationale:**
- OTel is the industry standard — workloads instrumented with any OTel SDK work out of the box
- Separating the OTel stream from the Raft event log prevents high-volume telemetry from starving platform operations
- Aggregation before writing to Oxigraph solves the cardinality problem — the graph holds current-state summaries, not individual spans
- Injecting OTel config as environment variables means workloads need zero platform-specific code to be observable
- W3C trace context propagation is standard — no custom headers, any OTel SDK handles it
- Unifying hardware metrics (ADR-040) and product metrics at the `MetricRecorded` event level means one alert rule syntax for all metric types

**Consequences:**
- `picloud-http` must serve an OTLP endpoint at `https://picloud.local/otel` — workloads export here
- The OTel event stream is a new in-process component — not Raft-replicated, bounded buffer, not persistent
- Raw OTel data is stored in the time-series layer (ADR-046) — not in the event log
- The aggregator must handle metric cardinality carefully — aggregate by resource IRI, not by individual request
- `PICLOUD_PRODUCT_VERSION` (ADR-044) and OTel resource attributes are injected together at workload startup