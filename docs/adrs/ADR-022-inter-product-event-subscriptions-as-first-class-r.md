---
id: ADR-022
title: Inter-Product Event Subscriptions as First-Class Resources
status: accepted
features:
- FT-005
- FT-083
- FT-084
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:82d683d794b1df17f4fa7648448410bed4020d1a023095e1a7c09d33938126d8
---

**Status:** Accepted

**Context:** A Product that subscribes to another Product's events needs to declare that dependency somewhere. It could be implicit (subscribe at runtime) or explicit (declared as a resource).

**Decision:** Event subscriptions are declared as `event-subscription` resources in `.picloud` files. The platform provisions and manages the subscription lifecycle. Runtime subscriptions without a resource declaration are not permitted.

**Rationale:**
- All inter-product dependencies are visible in resource files — the dependency graph is auditable and version-controlled
- The platform can enforce that a subscription's source Product and event type exist before provisioning
- Consistent with the IaC-as-only-interface principle — everything exists in a file

**Rejected alternatives:**
- **Runtime subscriptions without resource declaration** — inter-product dependencies become invisible, unauditable, and impossible to validate at deploy time.
- **Implicit subscription by convention** — relies on naming conventions rather than explicit declarations, creating fragile and undiscoverable dependencies.