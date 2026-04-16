---
id: ADR-015
title: Imperative API with Idempotent Execution
status: accepted
features:
- FT-001
- FT-007
- FT-022
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:4df0ebd799f40bc3da7ffb4c30bf7c183267ce3037d8f973fa4bee42d9297220
---

**Status:** Accepted

**Context:** Two approaches exist for IaC execution: declarative-convergent (platform continuously reconciles desired vs actual state, like Kubernetes) and imperative (operator runs a command, it executes once). Declarative requires a reconciliation loop and continuous state comparison. Imperative is simpler but risks partial application on failure.

**Decision:** The API is imperative from the operator's perspective — `picloud resource apply` deploys what is declared. Internally, every operation is idempotent via client-generated idempotency keys.

**Rationale:**
- No background reconciliation loop — the platform only acts when commanded
- Simpler implementation — no desired-state vs actual-state diffing engine required
- Idempotency via keys means re-running `apply` on unchanged files is safe and produces no effect
- This is how Azure ARM works — Bicep/ARM feels imperative but deployments are idempotent
- Failure recovery is explicit — the operator reruns `apply`, the platform deduplicates

**Consequences:**
- Drift detection (platform state diverges from declared files) is not automatic — the operator is responsible for reapplying when drift occurs
- A future `picloud resource diff` command could surface drift on demand

**Rejected alternatives:**
- **Declarative-convergent (Kubernetes model)** — requires a reconciliation loop, desired-state storage, and a diffing engine. Significant complexity for a system that prioritises simplicity.