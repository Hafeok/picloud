---
id: ADR-058
title: Workload Resource Constraints via cgroup v2
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:dcfa9c87e229c2f3f66550a2e2877f872f476f28c2fd257954d840123ccf7943
---

**Status:** Accepted

**Context:** Without resource constraints, a single misbehaving workload can consume all CPU and memory on a node, starving co-located workloads and platform services. The Raspberry Pi 5 has 4 cores and 16GB RAM — resource exhaustion is a real risk in multi-workload deployments. The platform needs a mechanism to enforce limits that is consistent for both OCI containers and raw binary workloads.

**Decision:** Workloads declare CPU (millicores) and memory (megabytes) constraints in their resource definition. The platform enforces these constraints at runtime via Linux cgroup v2. The scheduler uses declared constraints for placement decisions.

**Enforcement mechanism:**
- **Containers (youki):** Constraints are translated to OCI runtime spec fields, which youki applies via cgroup v2 — `cpu.max` for CPU and `memory.max` for memory
- **Binaries:** The platform creates a cgroup v2 scope for each binary process group and applies the same limits
- Memory limit breaches result in OOM-kill by the kernel; the platform detects this and emits `WorkloadOOMKilled`

**Scheduling integration:**
- The scheduler sums all workload constraints per node and compares against hardware capacity from the RDF graph
- A workload is placed on the node with the most headroom that satisfies its constraints
- Placement fails with `InsufficientCapacity` if no node can satisfy the request

**No constraints = no limits:** If `resources` is omitted, the workload runs without cgroup limits. This is intentional for single-workload nodes. The platform emits a `ResourceWarning` when an unconstrained workload shares a node with other workloads.

**Rationale:**
- cgroup v2 is the standard Linux resource isolation mechanism — no additional runtime dependency
- Unified enforcement for containers and binaries means the scheduler treats them identically
- Millicores and megabytes are the standard units (Kubernetes convention) — operators understand them immediately
- Opt-in constraints (no default limits) avoid surprising operators who deploy a single workload per node

**Rejected alternatives:**
- **cgroup v1** — legacy interface, cgroup v2 is the default on modern kernels and Raspberry Pi OS
- **Process-level ulimits** — cannot enforce CPU time shares and are per-process, not per-workload
- **Mandatory constraints** — would force single-workload nodes to declare limits for no benefit

**Consequences:**
- Node capacity must be accurately reported in the RDF graph (FT-075) for scheduling to work correctly
- The platform binary must have permissions to create cgroup scopes — requires appropriate systemd or root configuration
- OOM-killed workloads follow their declared restart policy — the platform does not distinguish OOM from other failures in restart behavior