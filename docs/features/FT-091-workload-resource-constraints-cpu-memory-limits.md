---
id: FT-091
title: Workload resource constraints (CPU/memory limits)
phase: 4
status: planned
depends-on: []
adrs:
- ADR-058
- ADR-010
tests:
- TC-287
- TC-344
domains: []
domains-acknowledged: {}
---

## Description

Workloads declare CPU and memory constraints that the scheduler enforces. Constraints govern both placement decisions (will the workload fit on a node?) and runtime enforcement (prevent a workload from consuming more than its allocation).

### Resource syntax

```bicep
container 'api-server' = {
  product: 'photo-app'
  image: 'photo-api:1.0.0'
  resources: {
    cpu: '500m'       # 500 millicores (0.5 CPU cores)
    memory: '512MB'   # 512 megabytes
  }
}

binary 'background-worker' = {
  product: 'photo-app'
  executable: 'worker-arm64'
  resources: {
    cpu: '250m'
    memory: '256MB'
  }
}
```

### Scheduling

- The scheduler sums resource constraints for all workloads assigned to each node and compares against the node's available capacity
- A workload is placed on the node with the most available headroom that satisfies its constraints
- If no node has sufficient capacity, `resource apply` fails with `InsufficientCapacity` and a `ResourceFailed` event is emitted
- Node capacity is derived from hardware metrics (FT-075) projected into the RDF graph — `picloud:cpuCoresTotal` and `picloud:memoryTotalMb` triples on the node IRI

### Runtime enforcement

- **Containers (youki):** CPU limits are enforced via cgroup v2 `cpu.max` and memory limits via `memory.max`. The OCI runtime spec is generated with the declared constraints.
- **Binaries:** CPU and memory limits are enforced via cgroup v2 applied to the process group. The platform creates a cgroup scope for each binary workload.
- A workload exceeding its memory limit is OOM-killed by the kernel. The platform detects the kill, emits a `WorkloadOOMKilled` event, and applies the restart policy.

### Defaults

- If `resources` is omitted, the workload has **no constraints** — it can use all available node resources. This is intentional for single-workload-per-node deployments.
- The platform emits a `ResourceWarning` event at deploy time if a workload has no resource constraints and shares a node with other workloads.

### RDF projection

Resource constraints are projected into the RDF graph:
```turtle
<https://picloud.local/products/photo-app/containers/api-server>
    picloud:cpuLimitMillicores 500 ;
    picloud:memoryLimitMb 512 .
```

Node-level allocation summaries are maintained:
```turtle
<https://picloud.local/nodes/pi-node-01>
    picloud:cpuAllocatedMillicores 1500 ;
    picloud:memoryAllocatedMb 2048 .
```
