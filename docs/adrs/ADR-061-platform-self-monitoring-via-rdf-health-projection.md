---
id: ADR-061
title: Platform Self-Monitoring via RDF Health Projection
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:77a7ff2fb5aee22dfbd33eb068bf8399bae1baf7ec3d7b98d5b270557344b9f6
---

**Status:** Accepted

**Context:** The platform collects hardware metrics per node (ADR-040) and projects them into the RDF graph. But the platform itself has health dimensions beyond hardware — Raft consensus health, RDF projection lag, workload scheduling state, event log integrity. Operators need a single query surface to understand whether the platform is healthy, not just whether the hardware is healthy.

**Decision:** The platform performs periodic self-monitoring health checks and projects the results into the RDF graph via `SelfMonitoringCheckCompleted` events. Each event carries an overall node health status and a list of individual check results.

**Health checks:**
| Check | What it measures | Healthy condition |
|---|---|---|
| `raft_health` | Raft heartbeat responsiveness | Leader reachable within timeout |
| `projection_lag` | Gap between latest log entry and latest projected event | Lag < 100 events |
| `workload_state` | All workloads on this node in `Running` state | No `Failed` workloads |
| `storage_health` | NVMe device responsiveness | Read/write latency within threshold |
| `event_log_integrity` | Log file and sidecar consistency | Offsets match, no corruption detected |

**Overall status derivation:**
- `healthy` — all checks pass
- `degraded` — one or more checks report warnings
- `unhealthy` — one or more checks report failures

**RDF projection:**
```turtle
<https://picloud.local/nodes/pi-node-01>
    picloud:selfMonitoringStatus "healthy" ;
    picloud:selfMonitoringCheckedAt "2025-07-01T12:00:00Z"^^xsd:dateTime ;
    picloud:hasHealthCheck [
        picloud:checkName "raft_health" ;
        picloud:checkStatus "healthy" ;
        picloud:checkMessage "Leader reachable in 12ms"
    ] ;
    picloud:hasHealthCheck [
        picloud:checkName "projection_lag" ;
        picloud:checkStatus "healthy" ;
        picloud:checkMessage "Lag: 3 events"
    ] .
```

**Query surface:** Operators can query full platform health via a single SPARQL query:
```sparql
SELECT ?node ?status ?checkedAt WHERE {
  ?node a picloud:Node ;
        picloud:selfMonitoringStatus ?status ;
        picloud:selfMonitoringCheckedAt ?checkedAt .
}
```

**Integration with alerts (ADR-041):** Self-monitoring results trigger inference rules. Built-in alert rules fire `AlertFired` when a node transitions to `degraded` or `unhealthy`.

**Rationale:**
- Platform health in the RDF graph means operators use the same query interface for hardware metrics, workload state, and platform health — one model for everything
- Event-driven projection is consistent with the platform's architecture — health state is an event like everything else
- Individual check results give operators actionable detail, not just a binary healthy/unhealthy signal

**Rejected alternatives:**
- **External health check endpoint only (HTTP /healthz)** — does not integrate with the RDF graph, inference rules, or alert system
- **Hardware metrics as sole health signal** — misses platform-specific health dimensions like projection lag and Raft health
- **Log-based health monitoring** — requires operators to parse logs instead of querying structured data

**Consequences:**
- Health check frequency must be tuned to avoid excessive event volume — default 60-second interval
- The platform must handle the bootstrapping problem — self-monitoring cannot run until the event log and RDF projection are operational
- Health checks that themselves depend on the subsystem they're monitoring (e.g., checking projection lag requires the projector to be working) need timeout-based fallback