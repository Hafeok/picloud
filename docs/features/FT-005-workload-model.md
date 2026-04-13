---
id: FT-005
title: Workload Model
phase: 1
status: in-progress
depends-on:
- FT-004
- FT-003
adrs:
- ADR-010
- ADR-020
- ADR-022
tests:
- TC-031
- TC-032
- TC-033
- TC-058
- TC-059
- TC-063
- TC-064
- TC-065
- TC-209
- TC-211
domains:
- scheduling
domains-acknowledged: {}
---

PiCloud runs two kinds of workloads: OCI containers and raw binaries. Both are scheduled, monitored, and managed identically by the platform.

### Scheduling

The scheduler assigns workloads to nodes based on available CPU and memory. Scheduling is automatic — operators do not specify which node runs a workload. Constraints (affinity, anti-affinity) are a future phase concern.

When a node fails, its workloads are rescheduled to remaining nodes automatically. The event log records the failure and rescheduling as events, which are projected into the RDF graph.

### OCI containers

Containers are run via an embedded OCI runtime (youki). Images are pulled from any OCI-compatible registry. The platform injects:
- Workload identity credentials
- Secret values (as environment variables or mounted files)
- Volume mounts
- Network configuration

```bicep
container 'api-server' = {
  product: 'photo-app'
  image: 'registry.example.com/photo-api:1.0.0'
  identity: 'api-worker'
  resources: {
    cpu: '500m'
    memory: '512MB'
  }
  mounts: [
    { volume: 'media-store', path: '/data' }
  ]
}
```

### Raw binaries

Binaries are ARM64 executables deployed as platform-managed processes. Useful for native Rust services, scripts, or workloads where container overhead is undesirable. The same identity injection, secret injection, and volume mount model applies.

```bicep
binary 'background-worker' = {
  product: 'photo-app'
  executable: 'worker-arm64'
  identity: 'background-worker-identity'
  resources: {
    cpu: '250m'
    memory: '256MB'
  }
}
```

### Health and restart policy

The platform monitors workload health via process liveness and optional HTTP health endpoints. Failed workloads are restarted according to their declared restart policy. All health state changes are emitted as events and projected into the RDF graph.

---