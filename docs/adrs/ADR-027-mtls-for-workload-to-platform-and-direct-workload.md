---
id: ADR-027
title: mTLS for Workload-to-Platform and Direct Workload-to-SPARQL Communication
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Workloads need authenticated, encrypted communication with the platform event bus and with other Products' SPARQL endpoints. Two routing options exist: all traffic via platform, or direct connections where appropriate.

**Decision:** Two mTLS patterns are used:

1. **Workload → platform event bus** — routed via the platform. The platform enforces IAM on every event operation and maintains the full audit trail. Transport is mTLS with platform-issued workload certificates.

2. **Workload → product SPARQL endpoint** — direct connection from the querying workload to the target Product's endpoint over mTLS. The platform issues certificates to both parties at workload startup. IAM is enforced at the SPARQL endpoint by validating the caller's workload certificate against the platform's CA and checking the caller's permissions.

**Certificate lifecycle:** The platform's built-in CA issues certificates to workloads at runtime as part of workload startup. Certificates are short-lived and rotated automatically. Workloads never handle certificate generation — the platform injects them.

**Rationale:**
- Events are fire-and-forget — platform mediation adds audit trail and IAM enforcement with minimal latency cost
- SPARQL queries are request-response — the extra platform hop adds latency and creates a platform bottleneck for what could be a high-frequency read pattern
- Direct mTLS for SPARQL maintains security (mutual authentication, IAM at endpoint) without sacrificing performance
- All certificates are platform-issued — no external PKI, no operator certificate management

**Rejected alternatives:**
- **All traffic via platform** — creates a platform bottleneck for SPARQL queries. High-frequency graph reads would saturate the platform's routing layer.
- **Direct connections without mTLS** — unacceptable. All workload communication must be mutually authenticated and encrypted.