---
id: TC-344
title: Resource limits exit — CPU and memory limits enforced
type: exit-criteria
status: passing
validates:
  features: [FT-091]
  adrs: []
phase: 4
runner: cargo-test
runner-args: "tc344_resource_limits_exit_cpu_and_memory_limits_enforced"
last-run: 2026-04-15T18:19:30.788440518+00:00
last-run-duration: 0.7s
---

## Description

Exit-criteria test validating the complete resource constraint lifecycle for FT-091:

1. **Validation**: ResourceLimits::validate() accepts valid values (min, max, typical) and rejects invalid values (zero CPU, sub-minimum memory, above-maximum values). Both errors are reported when both fields are invalid.
2. **Scheduler rejection**: ProcessScheduler::schedule() returns InvalidResourceLimits for workloads with out-of-range resource specs.
3. **Helper methods**: cpu_as_fractional_cores(), memory_as_bytes(), cpu_as_cfs_quota_us() produce correct conversions.
4. **Enforcement**: Workloads with valid limits schedule successfully and the spec is preserved in the workload entry.
5. **Defaults**: ResourceLimits::default() and ResourceLimits::none() produce empty, valid limits.
6. **Constants**: MIN/MAX bounds and CFS_PERIOD_US are correct.