---
id: ADR-010
title: OCI Containers and Raw Binaries as Workload Primitives
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Workloads need to be schedulable on any node. OCI containers are the standard packaging format. Raw binaries are needed for native Rust services and lightweight workloads where container overhead is undesirable.

**Decision:** PiCloud supports two workload primitives: OCI containers (via youki) and raw ARM64 binaries. Both receive the same identity injection, secret injection, volume mount, and networking treatment.

**Rationale:**
- OCI containers are the standard — any existing containerized workload runs without modification
- Raw binaries enable PiCloud's own internal services to be deployed as Platform workloads (dogfooding)
- youki is a pure Rust OCI runtime — consistent with ADR-001, no external runtime dependency
- Unified resource model means containers and binaries are interchangeable from the scheduler's perspective

**Rejected alternatives:**
- **VMs** — too heavyweight for Pi5 hardware. Not suitable for the target environment.
- **WebAssembly** — interesting but tooling is immature for production workloads. Future consideration.
- **Containers only** — excludes lightweight native workloads and makes dogfooding harder.