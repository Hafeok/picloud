---
id: ADR-026
title: Bootstrap Token Exchange and Three-Tier Passkey Recovery
status: accepted
features:
- FT-003
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:e2c162a51ca803a13c4e0b38706de867f597e420a9980a323d302aeb4d429de1
---

**Status:** Accepted

**Context:** Two related problems require a consistent solution: (1) bootstrapping the first admin identity on a fresh cluster, and (2) recovering access when a passkey is lost. Both cases must be solvable without introducing passwords.

**Decision:** A single-use, time-limited token exchange mechanism handles both bootstrap and recovery. Three recovery tiers are defined in order of escalation:

**Bootstrap:** `picloud cluster init` generates a single-use bootstrap token with a 15-minute expiry. The operator opens the platform's enrollment endpoint in a browser and exchanges the token for a WebAuthn registration ceremony. Completing the ceremony creates the first admin identity. The token is invalidated immediately on use or expiry.

**Tier 1 — Admin-initiated reset:** An admin initiates a passkey reset for a user via `picloud identity reset-passkey {name}`. The platform generates a single-use re-enrollment token. The user registers a new authenticator via the enrollment endpoint. The previous passkey is revoked on successful re-enrollment.

**Tier 2 — Backup key enforcement:** Admin accounts must have a minimum of two passkeys registered. The platform enforces this — removing a passkey that would leave an admin with fewer than two is rejected. This ensures admins always have a fallback authenticator, typically a hardware security key stored offline.

**Tier 3 — Physical recovery:** If all admin accounts are inaccessible, an operator with physical access to any cluster node runs `picloud cluster recover` locally (non-network access only). This generates a new bootstrap token, identical in mechanism to the original `cluster init` flow. The recovery event is written to the platform event log as a high-severity audit entry.

**Rationale:**
- Every tier is password-free — recovery tokens are short-lived and single-use, not reusable credentials
- Physical recovery requires physical presence — an attacker cannot trigger recovery remotely
- Backup key enforcement ensures Tier 1 (admin reset) is always available as long as at least one admin is accessible
- The same token exchange mechanism is reused across bootstrap and all recovery tiers — one implementation, multiple use cases
- All recovery operations are auditable events in the platform event log

**Rejected alternatives:**
- **Recovery via password fallback** — reintroduces passwords, contradicting the passkey-only authentication model (ADR-025).
- **Admin-only recovery (no physical tier)** — if all admin accounts are locked out, the cluster becomes permanently inaccessible with no recovery path.