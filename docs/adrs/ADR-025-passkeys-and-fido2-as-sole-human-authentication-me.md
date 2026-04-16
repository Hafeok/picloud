---
id: ADR-025
title: Passkeys and FIDO2 as Sole Human Authentication Mechanism
status: accepted
features:
- FT-003
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:a21e7f8e10a275565d06830f712409edede420c9572b18ab4cf333cbb145a3b4
---

**Status:** Accepted

**Context:** PiCloud is a full OIDC provider (ADR-017) and must authenticate human users. Traditional OIDC implementations use username and password. Passwords introduce credential storage risk, password reset complexity, and phishing vulnerability.

**Decision:** Human authentication uses passkeys (WebAuthn) and FIDO2 exclusively. There are no passwords in the platform. This applies to all human-facing flows: CLI authentication, platform administration, and application login via OIDC.

**Authentication modes:**
- **Browser-based** — WebAuthn ceremony via the platform's OIDC authorization endpoint. Works with any platform authenticator (Touch ID, Face ID, Windows Hello, hardware security key).
- **CLI device flow** — CLI initiates device authorization flow, operator completes passkey authentication in a browser on any device, CLI polls for token.
- **CLI FIDO2 direct** — for operators with a hardware security key, FIDO2 assertion completes directly in the terminal without a browser.

**Machine flows are unaffected:** App Registrations (OAuth client credentials) use client ID and client secret. mTLS certificates serve as workload identity credentials. Passkeys apply to human identities only.

**Rationale:**
- Eliminates password storage entirely — no credential database to breach
- Passkeys are phishing-resistant by construction — the credential is bound to the origin
- FIDO2 hardware key support means the platform works in fully headless, air-gapped environments
- Passkeys are now supported natively on all major platforms and browsers
- Consistent with a forward-looking security model — passwords are a solved problem we choose not to have

**Consequences:**
- The platform must implement the WebAuthn relying party correctly — ceremony initiation, challenge verification, authenticator registration
- Every human identity has one or more passkeys registered. Recovery uses a three-tier model: admin-initiated reset, enforced backup keys for admins, and physical node recovery as last resort (see ADR-026)
- Admin accounts are required to have a minimum of two passkeys registered — the platform enforces this constraint
- CLI device flow requires the platform to serve a browser-accessible enrollment page — this is the only browser-facing surface in Phase 1 CLI usage

**Rejected alternatives:**
- **Username and password** — credential storage risk, phishing risk, password reset complexity. Not acceptable.
- **SSH keys only** — suitable for CLI but does not cover browser-based OIDC flows for applications.
- **TOTP/OTP** — second factor only, still requires a primary credential. Adds complexity without eliminating passwords.