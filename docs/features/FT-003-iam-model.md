---
id: FT-003
title: IAM Model
phase: 1
status: complete
depends-on:
- FT-002
adrs:
- ADR-009
- ADR-017
- ADR-025
- ADR-026
- ADR-027
- ADR-051
tests:
- TC-028
- TC-029
- TC-030
- TC-048
- TC-049
- TC-050
- TC-051
- TC-071
- TC-072
- TC-073
- TC-074
- TC-075
- TC-076
- TC-077
- TC-078
- TC-079
- TC-080
- TC-171
- TC-172
- TC-173
- TC-174
- TC-212
domains:
- iam
- security
domains-acknowledged: {}
---

PiCloud is a full OIDC provider. It issues tokens, manages identities, and enforces authorization for both platform operations and application-level authentication. There is no external IdP dependency.

### Identity types

**Human identities** — users who interact with the cluster via the CLI or via applications built on PiCloud. Authenticated via OIDC flows. Assigned platform-level roles and/or Product-level roles.

**Workload identities** — service accounts assigned to containers and binaries. The platform automatically injects credentials into workloads at runtime. Workloads never handle secrets directly — they exchange their injected identity token for scoped access tokens.

### Product as App Registration

Every Product acts as an OIDC App Registration. When a user authenticates against a Product-hosted application:

1. The application redirects to the PiCloud OIDC authorization endpoint
2. The user authenticates against their platform identity
3. PiCloud issues a token scoped to that Product with the user's roles within that Product
4. The application validates the token against PiCloud's JWKS endpoint

This means every application built on PiCloud gets SSO, token management, and user management for free.

### IAM scopes

**Platform scope** — governs access to cluster operations: node management, Product creation, platform identity management.

**Product scope** — governs access to a Product's resources and determines what roles a user has within an application built on that Product.

A user can have different roles in different Products. Platform administrators are not automatically administrators of all Products.

### RBAC

Roles are declared as resources. Permissions are additive. Every API operation on every resource requires an explicit permission check against the caller's identity token.

```bicep
role 'photo-viewer' = {
  product: 'photo-app'
  permissions: [
    'photo-app/containers/api-server:read'
    'photo-app/rdf-store/graph:query'
  ]
}
```

### Authentication — Passkeys and FIDO2

Human authentication in PiCloud uses passkeys and FIDO2 exclusively. There are no passwords. This applies to all human identity flows — platform administration, application login via OIDC, and CLI authentication.

**Browser-based flows** use the WebAuthn API. The platform's OIDC authorization endpoint initiates a WebAuthn ceremony. The user completes authentication with their platform authenticator (Touch ID, Face ID, hardware security key).

**CLI authentication** supports two modes:
- **Device flow** — the CLI initiates a device authorization flow, the operator completes passkey authentication in a browser on any device, the CLI polls for completion and receives a token
- **FIDO2 directly in terminal** — for operators with a hardware security key (YubiKey), FIDO2 assertion can be completed directly in the terminal without a browser

**App Registrations** (OAuth machine flows) use client ID and client secret as normal. Passkeys apply to human identities only — machine-to-machine authentication uses mTLS workload certificates and OAuth client credentials.

### Bootstrap

On a fresh cluster with no identities, `picloud cluster init` generates a one-time bootstrap token. The operator opens the platform's enrollment endpoint in a browser, exchanges the token for a passkey registration ceremony, and completes FIDO2 enrollment. This creates the first admin identity with the registered passkey bound to it. The bootstrap token is single-use and expires after 15 minutes.

### Passkey recovery

PiCloud enforces a three-tier recovery model:

**Tier 1 — Admin-initiated reset.** An admin can initiate a passkey reset for any user. The platform generates a single-use re-enrollment token (same mechanism as bootstrap) and the user registers a new authenticator. The previous passkey is revoked.

**Tier 2 — Backup key enforcement.** Admin accounts are required to have a minimum of two passkeys registered. The platform enforces this constraint — an admin cannot remove a passkey if it would leave them with fewer than two registered. Backup keys are typically a hardware security key stored offline.

**Tier 3 — Physical recovery.** If all admin accounts are inaccessible, an operator with physical access to any cluster node can run `picloud cluster recover` to generate a new bootstrap token. This requires local non-network access to the node and is logged as a high-severity event in the platform event log. This mirrors the original `cluster init` flow.

### Secret management

Secrets are first-class resources. They are encrypted at rest, replicated across the cluster, and injected into workloads by the platform. Workloads never see secret values directly in their resource definitions — they reference secrets by name and the platform handles injection.

```bicep
container 'api-server' = {
  env: {
    DB_PASSWORD: secret('db-password')
  }
}
```

---