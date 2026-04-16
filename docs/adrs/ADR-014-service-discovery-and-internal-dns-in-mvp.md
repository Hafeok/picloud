---
id: ADR-014
title: Service Discovery and Internal DNS in MVP
status: accepted
features:
- FT-006
- FT-021
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:c539abf40ecf990d9736da86da4d1924972acf9684242ef378ed2c9b7abcc438
---

**Status:** Accepted

**Context:** Workloads need to find each other by name. Without service discovery, container addresses are ephemeral and workloads must be reconfigured when peers restart or reschedule.

**Decision:** Internal DNS and service discovery are MVP features, not future phases. Every resource that accepts network traffic is automatically registered as `{resource}.{product}.picloud.internal`.

**Rationale:**
- Without service discovery, containers cannot find each other — the platform is not useful
- Internal DNS is a small implementation surface relative to its impact
- Automatic registration means operators never configure DNS manually

**Rejected alternatives:**
- **Deferred to Phase 2** — without service discovery, containers cannot find each other, making the platform unusable for any multi-container product in Phase 1.
- **Manual DNS configuration** — operators configuring DNS entries for every container contradicts the platform's automation-first principle.