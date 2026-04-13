---
id: ADR-030
title: Platform-Generated CA with BYO-CA Support
status: accepted
features: [FT-006, FT-023]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** All platform communication is TLS — node-to-node mTLS, workload certificates, and HTTPS for the IRI-based resource layer. A CA is required to issue these certificates. External clients need to trust this CA to connect to `picloud.local`.

**Decision:** On `picloud cluster init`, the platform generates its own root CA if none is specified. Operators may optionally provide an external CA (e.g. Smallstep, an existing corporate CA) via configuration. All certificate issuance, rotation, and revocation is managed by the platform regardless of which CA is used.

**Default behaviour — platform-generated CA:**
- `picloud cluster init` generates a root CA keypair, stored encrypted in the platform's secret store and replicated across nodes via Raft
- The CA certificate is exported via `picloud ca export` for installation into client OS trust stores
- Node certificates, workload certificates, and TLS certificates for `picloud.local` are all issued by this CA

**BYO-CA mode:**
- Operator provides a CA certificate and signing key (or an ACME/EST endpoint) in the bootstrap configuration
- The platform uses the provided CA for all certificate issuance
- Useful for integrating with an existing homelab PKI (e.g. Smallstep CA) or corporate PKI

**Rationale:**
- Zero-configuration default — the platform is fully operational without any external PKI
- BYO-CA means operators with existing trust infrastructure (Smallstep, internal CA) don't need to manage two PKIs or distribute a new CA certificate to all their devices
- All certificate lifecycle is platform-managed regardless of CA source — operators never manually issue or rotate certificates

**Rejected alternatives:**
- **External CA required** — adds a mandatory external dependency for TLS, breaking the zero-dependency single-binary model.
- **Self-signed certificates per node** — prevents mutual authentication; nodes cannot verify each other's identity without a shared trust root.

**Consequences:**
- External clients (operator laptops, browsers, RDF tools) must trust the platform CA to connect to `picloud.local` over HTTPS — one-time operation via `picloud ca export`
- In BYO-CA mode, the external CA must be accessible during node join and certificate rotation operations
- The platform CA private key is the most sensitive secret in the cluster — its storage and replication must be treated with the highest security priority