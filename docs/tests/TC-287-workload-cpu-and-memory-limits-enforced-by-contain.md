---
id: TC-287
title: Workload CPU and memory limits enforced by container runtime
type: scenario
status: passing
validates:
  features: [FT-091]
  adrs: []
phase: 4
runner: cargo-test
runner-args: "tc287_workload_cpu_and_memory_limits_enforced_by_container_runtime"
last-run: 2026-04-15T18:19:30.788440518+00:00
last-run-duration: 0.7s
---

## Description

Schedule workloads with CPU and memory limits and verify the runtime enforces them.

**Binary workloads**: RLIMIT_AS (address space / memory) and RLIMIT_CPU (cumulative CPU seconds) are set via pre_exec hooks. On Linux, /proc/<pid>/limits is read to confirm the limits were applied.

**Container workloads**: Resource limits are passed through to the OCI runtime — for youki via the OCI spec resources block (memory.limit, cpu.quota/period), for podman/docker via --memory and --cpus CLI flags.

**Phases**:
1. Binary workload with both CPU and memory limits — verify RLIMIT_AS and RLIMIT_CPU in /proc
2. Binary workload with memory-only limit — verify RLIMIT_AS
3. Binary workload with CPU-only limit — verify RLIMIT_CPU
4. Container workload with limits (simulated runtime) — verify scheduling succeeds
5. Workload with no limits — verify scheduling still works