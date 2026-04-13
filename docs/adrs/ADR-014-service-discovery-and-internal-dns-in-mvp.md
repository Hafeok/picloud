---
id: ADR-014
title: Service Discovery and Internal DNS in MVP
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Workloads need to find each other by name. Without service discovery, container addresses are ephemeral and workloads must be reconfigured when peers restart or reschedule.

**Decision:** Internal DNS and service discovery are MVP features, not future phases. Every resource that accepts network traffic is automatically registered as `{resource}.{product}.picloud.internal`.

**Rationale:**
- Without service discovery, containers cannot find each other — the platform is not useful
- Internal DNS is a small implementation surface relative to its impact
- Automatic registration means operators never configure DNS manually