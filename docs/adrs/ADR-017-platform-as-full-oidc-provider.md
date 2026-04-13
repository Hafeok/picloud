---
id: ADR-017
title: Platform as Full OIDC Provider
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Applications built on PiCloud need user authentication. The platform manages identities (ADR-009). Extending the platform to a full OIDC provider means applications never need an external IdP.

**Decision:** PiCloud implements the OIDC authorization code flow. It exposes an authorization endpoint, token endpoint, and JWKS endpoint. Products act as OIDC clients (App Registrations). Users authenticate against their platform identity and receive Product-scoped tokens.

**Rationale:**
- Applications get SSO for free — no Keycloak, no Authentik, no Auth0 required
- The identity model is unified — the same identity a user uses for `picloud` CLI is the identity they use for applications
- Product-scoped tokens mean a user's permissions within an application are distinct from their platform permissions

**Security requirements:**
- Token signing keys are stored in the platform's encrypted secret store
- Key rotation must not invalidate active sessions (JWKS must serve both old and new keys during rotation)
- All OIDC endpoints must be served over TLS