---
id: ADR-017
title: Platform as Full OIDC Provider
status: accepted
features:
- FT-003
- FT-026
- FT-027
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:5d9d65b9c55c91ba29e8e6cc26d2f2e81d99b18878cb1bcd59c3fbebbb9919b3
---

**Status:** Accepted

**Context:** Applications built on PiCloud need user authentication. The platform manages identities (ADR-009). Extending the platform to a full OIDC provider means applications never need an external IdP.

**Decision:** PiCloud implements the OIDC authorization code flow. It exposes an authorization endpoint, token endpoint, and JWKS endpoint. Products act as OIDC clients (App Registrations). Users authenticate against their platform identity and receive Product-scoped tokens.

**Rationale:**
- Applications get SSO for free — no Keycloak, no Authentik, no Auth0 required
- The identity model is unified — the same identity a user uses for `picloud` CLI is the identity they use for applications
- Product-scoped tokens mean a user's permissions within an application are distinct from their platform permissions

**Rejected alternatives:**
- **External IdP only (Keycloak, Auth0)** — adds an external dependency to a platform designed for zero external dependencies; breaks the single-binary model.
- **Simple token-based auth without OIDC** — non-standard, requires every application to implement custom auth logic, and prevents interoperability with standard OIDC clients.

**Security requirements:**
- Token signing keys are stored in the platform's encrypted secret store
- Key rotation must not invalidate active sessions (JWKS must serve both old and new keys during rotation)
- All OIDC endpoints must be served over TLS