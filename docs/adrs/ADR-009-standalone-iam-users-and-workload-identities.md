---
id: ADR-009
title: Standalone IAM — Users and Workload Identities
status: accepted
features:
- FT-003
- FT-017
- FT-030
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:19730dd8d50068b83a9aa3b8113159bb92a88d8f72506dccb097321a4e68893a
---

**Status:** Accepted

**Context:** Every operation in PiCloud requires an authenticated identity. Applications built on PiCloud need an IdP for user authentication. Requiring an external system (Authentik, Keycloak, Azure AD) would add infrastructure dependencies and complexity.

**Decision:** PiCloud is a standalone OIDC provider. It manages human identities, workload identities, token issuance, and JWKS. No external IdP integration in MVP. Products act as OIDC App Registrations.

**Rationale:**
- Zero external dependencies — the platform manages its own identity, consistent with single-binary goal
- Every application gets SSO and OIDC for free without additional infrastructure
- Workload identity is native — secrets are injected by the platform, workloads never handle credentials directly
- Platform IAM and application IAM are unified — one identity model for everything

**Consequences:**
- PiCloud must implement OIDC correctly — authorization endpoint, token endpoint, JWKS, refresh tokens
- Token signing keys must be managed by the platform and rotated safely
- This is the most security-critical component of the platform

**Rejected alternatives:**
- **External OIDC provider (Authentik, Keycloak)** — external infrastructure dependency. Requires another service to be running before PiCloud can function.
- **mTLS only (no OIDC)** — sufficient for workload-to-workload but does not cover human authentication or application-level user management.