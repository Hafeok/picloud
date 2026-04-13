---
id: ADR-051
title: Product IAM — Roles, Custom Claims, Scopes, and Audience
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Products act as OIDC App Registrations (ADR-017). A token issued by the platform for a product must carry the roles, permissions, and custom claims specific to that product. Without roles and scopes, the token is structurally valid but semantically empty. Without audience validation, tokens can be reused across products. Four capabilities are needed: role definitions with inheritance, custom static claims, product-defined OAuth scopes, and audience-bound tokens.

**Decision:** Products declare roles, scopes, and custom claims as resources in their `.picloud` files. The platform IAM engine resolves roles (including inheritance via OWL subclass inference), evaluates scope-to-claim mappings, and issues tokens with product-scoped audience. Three token flows are supported: user authentication, on-behalf-of (user delegating to a product acting against another product), and M2M client credentials.

### Token anatomy

Every token issued for a product carries:

```json
{
  "iss": "https://picloud.local",
  "aud": "https://picloud.local/products/photo-app",
  "sub": "https://picloud.local/platform/identities/alice",
  "exp": 1735689600,
  "iat": 1735686000,
  "scope": "photos:read photos:write",
  "roles": ["editor"],
  "permissions": ["photos:read", "photos:write", "albums:manage"],
  "department": "engineering"
}
```

- `iss` — always the platform IRI (cluster domain)
- `aud` — the product IRI. A token for `photo-app` is rejected by `user-service`
- `sub` — the user's platform identity IRI
- `scope` — space-separated OAuth scopes granted in this token
- `roles` — product roles assigned to this user
- `permissions` — flattened permission set from all assigned roles
- Custom claims — static key-value pairs declared on roles or scopes

### Role declaration

```bicep
role "viewer" = {
  product:     "photo-app"
  description: "Can view photos and albums"
  permissions: [
    "photos:read"
    "albums:read"
  ]
  claims: {
    "access_level": "read-only"
  }
}

role "editor" = {
  product:     "photo-app"
  description: "Can view and manage photos"
  inherits:    "viewer"        // inherits all viewer permissions and claims
  permissions: [
    "photos:write"
    "albums:manage"
  ]
  claims: {
    "access_level": "read-write"
  }
}

role "admin" = {
  product:     "photo-app"
  description: "Full product access"
  inherits:    "editor"        // transitive — inherits viewer and editor
  permissions: [
    "photos:delete"
    "albums:delete"
    "users:manage"
  ]
  claims: {
    "access_level": "admin"
  }
}
```

**Role inheritance** uses `rdfs:subClassOf` in the RDF graph — the OWL inference engine (ADR-039) resolves the full permission set transitively. `admin` inherits `editor` which inherits `viewer` — token issuance reads the inferred permission closure, not just the declared permissions.

### Scope declaration

```bicep
scope "photos:read" = {
  product:     "photo-app"
  description: "Read access to photos and albums"
  claims: {
    "photos_access": "read"
  }
  permissions: ["photos:read", "albums:read"]
}

scope "photos:write" = {
  product:     "photo-app"
  description: "Write access to photos and albums"
  claims: {
    "photos_access": "write"
  }
  permissions: ["photos:read", "photos:write", "albums:manage"]
}
```

Scopes and roles both contribute claims to the token. When a scope and a role declare the same claim key, the role value wins — roles are more specific.

### Token flows

**Flow 1 — User authentication (standard OIDC)**

User authenticates with passkey → platform issues token scoped to the product:

```
User → OIDC authorization endpoint
     → passkey authentication
     → platform resolves user's roles in this product
     → platform resolves requested scopes
     → token issued with aud = product IRI
```

**Flow 2 — On-behalf-of (RFC 8693 token exchange)**

`photo-app` needs to call `user-service` on Alice's behalf. Alice has already authenticated against `photo-app`:

```
photo-app → POST /token
  grant_type: urn:ietf:params:oauth:grant-type:token-exchange
  subject_token: <alice's photo-app token>
  audience: https://picloud.local/products/user-service
  scope: users:read

Platform:
  1. Validates subject_token (aud = photo-app ✓)
  2. Checks photo-app has permission to act on behalf of users in user-service
  3. Resolves Alice's roles in user-service
  4. Issues new token:
     aud: user-service
     sub: alice
     act: { sub: photo-app }    ← actor claim — who is acting on Alice's behalf
     scope: users:read
```

The `act` claim preserves the full delegation chain — `user-service` knows both that Alice authorised the request and that `photo-app` is acting for her.

**Flow 3 — M2M client credentials**

A container in `photo-app` calls `user-service`'s SPARQL endpoint using its workload identity:

```
photo-app/api-server → POST /token
  grant_type: client_credentials
  client_id: photo-app
  client_secret: <app registration secret>
  scope: users:read
  audience: https://picloud.local/products/user-service

Platform:
  1. Validates client credentials (App Registration)
  2. Checks photo-app M2M permissions for user-service
  3. Issues token:
     aud: user-service
     sub: https://picloud.local/products/photo-app
     scope: users:read
```

M2M tokens have `sub` set to the product IRI, not a user IRI. `user-service` can distinguish M2M from delegated user access by checking `sub` type.

### M2M permission declaration

Products declare which other products they allow M2M access from:

```bicep
m2m-permission "allow-photo-app-read" = {
  product:      "user-service"
  client:       "photo-app"
  scopes:       ["users:read"]
  description:  "photo-app may read user profiles via M2M"
}
```

This resource must exist in `user-service`'s deployment before `photo-app` can request M2M tokens. This is consistent with ADR-022 (inter-product dependencies are declared resources) and ADR-028 (low coupling enforced structurally).

### Audience validation in the SDK

The SDK validates `aud` automatically on every incoming token:

```rust
// Rust SDK — token validation
let claims = picloud.iam().validate_token(token, expected_audience)?;
// Fails if aud != https://picloud.local/products/user-service
```

```typescript
// TypeScript SDK
const claims = await picloud.iam().validateToken(token, expectedAudience);
```

```csharp
// .NET SDK
var claims = await picloud.Iam().ValidateTokenAsync(token, expectedAudience);
```

### RDF representation

```turtle
<https://picloud.local/products/photo-app/roles/editor>
    a pc:Role ;
    pc:product    <https://picloud.local/products/photo-app> ;
    rdfs:subClassOf <https://picloud.local/products/photo-app/roles/viewer> ;
    pc:permission "photos:write" ;
    pc:permission "albums:manage" ;
    pc:claim [ pc:claimKey "access_level" ; pc:claimValue "read-write" ] .
```

Role inheritance is `rdfs:subClassOf` — the OWL inference engine materialises the full permission closure automatically. Token issuance queries the inferred graph, not the raw triples.

**Rationale:**
- Audience binding (`aud`) prevents token reuse across products — a fundamental JWT security property that is cheap to implement and expensive to lack
- Role inheritance via `rdfs:subClassOf` reuses the inference engine already in the platform — no custom inheritance logic
- On-behalf-of (RFC 8693) is the standard OAuth pattern for delegated access — no proprietary token exchange mechanism needed
- M2M client credentials are standard OAuth — workloads already have App Registration credentials (ADR-017)
- M2M permission declarations are resources in the target product — consistent with ADR-022, target product controls who can access it
- Static custom claims cover 90% of real use cases without the token issuance latency of dynamic SPARQL claims (dynamic claims are Phase 3)
- Custom scopes give API consumers a standard OAuth surface for requesting specific access

**Consequences:**
- `role`, `scope`, and `m2m-permission` are new product-scoped resource types
- Token issuance in `picloud-iam` must query the inferred RDF graph for the full permission closure
- `picloud-iam` must implement RFC 8693 token exchange endpoint
- The SDK `validateToken` method must check `aud` — this is the most critical SDK method from a security perspective
- Role inheritance creates a dependency ordering problem at deployment — if `editor` inherits `viewer`, `viewer` must exist before `editor` is created. The platform resolves this via the dependency graph at deploy time.
- M2M permission resources must exist in the target product before M2M tokens can be issued — cross-product declaration, target wins