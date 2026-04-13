---
id: ADR-053
title: Node Certificate Issuance and Enrollment
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Every node in a PiCloud cluster communicates over mTLS (ADR-027). A new node needs a certificate signed by the cluster CA to participate. The CA private key lives in the cluster — only the cluster can issue node certificates. This creates a bootstrap problem: a node needs a cert to join, but needs to join to get a cert.

Two operational contexts have different security requirements:

- **Home lab / trusted network** — the network is the trust boundary. Auto-enrolling any node that appears on the network is acceptable and eliminates operational friction entirely.
- **Secured environment** — network presence alone is not sufficient. A token must be pre-provisioned to authorise each new node.

**Decision:** PiCloud supports two enrollment modes configured at `cluster init`. Both use the same two-phase join and the same CA infrastructure. The mode is a cluster-wide setting — it applies to all nodes.

### The two-phase join (both modes)

**Phase 1 — Pre-auth enrollment (plain TLS, no client cert required)**

The cluster leader exposes a dedicated enrollment endpoint at `https://picloud.local/enroll`. This endpoint accepts plain TLS (server cert only — clients present no client cert). It does exactly one thing: issue node certificates.

```
New node (no cert yet)
  → discovers cluster via mDNS
  → generates ephemeral keypair locally
  → POST https://picloud.local/enroll
      { csr: <DER-encoded CSR>, token: <enrollment_token | null> }
  → cluster validates (mode-dependent — see below)
  → cluster CA signs CSR with node identity
  → returns signed certificate + cluster CA certificate
  → node stores cert and CA cert on disk
  → enrollment token invalidated (token mode only)
```

**Phase 2 — Full join (cert in hand)**

```
New node (cert issued)
  → connects to leader via mTLS ✓
  → presents cluster CA cert for server verification ✓
  → Raft join proceeds ✓
  → NodeJoined event emitted ✓
```

### Mode A — Auto-enroll (default)

Any node that discovers the cluster via mDNS and presents a valid CSR receives a certificate. No token required. Network presence is the authorisation.

```bash
picloud cluster init --domain picloud.local
# Auto-enroll is the default — no additional flags needed
```

**Security model:** The local network is the trust boundary. Any device on the network can join the cluster. Suitable when the network is controlled (home lab, dedicated VLAN, isolated switch).

**Safeguard:** Even in auto-enroll mode, the cluster ID and CA fingerprint are checked on every subsequent connection. A rogue node that somehow gets a cert can only participate if it also passes Raft membership — which requires the existing cluster to accept it. The cluster can revoke a node certificate at any time via `picloud node remove`.

### Mode B — Token enrollment

A node must present a valid enrollment token to receive a certificate. Tokens are single-use, time-limited, and issued by an existing cluster admin.

```bash
picloud cluster init --domain acme.local --enrollment-mode token
```

**Generating an enrollment token:**
```bash
picloud node enrollment-token --ttl 2h
→ Token: picloud-enroll-a3f9b2c1d4e5f6...
→ Expires: 2025-07-01T14:00:00Z
→ Single use — invalidated after first use
```

**Distributing the token to a new node:**
The token is placed in the node's config before boot. Two delivery mechanisms:

```bash
# Option 1 — environment variable in systemd service override
sudo systemctl edit picloud
# Add:
[Service]
Environment=PICLOUD_ENROLLMENT_TOKEN=picloud-enroll-a3f9b2c1...

# Option 2 — config file
echo "enrollment_token = picloud-enroll-a3f9b2c1..." \
  > /home/ubuntu/picloud/config.toml
```

On startup, `picloud-server` reads the token, uses it once to enroll, then removes it from config. The token is never stored after use.

### CA architecture

**The CA lives in the cluster, replicated via Raft.**

The CA private key is generated at `cluster init`, encrypted at rest with the cluster's master key, and stored in Raft state. Every node has a copy of the encrypted CA key — if the leader fails, the new leader has the key and can continue issuing certificates immediately.

The CA certificate is embedded in the cluster identity (ADR-042) alongside the cluster ID. Every node knows the CA certificate at join time — it is returned in the enrollment response.

**Certificate lifetime:**
- Node certificates: 90 days, auto-renewed 7 days before expiry
- Workload certificates: 24 hours, auto-renewed 1 hour before expiry
- Ingress/TLS certificates: 90 days, auto-renewed 14 days before expiry

**Auto-renewal:** The platform tracks certificate expiry in the RDF graph. An inference rule (ADR-038) fires an `AlertFired` event when any certificate is within its renewal window. The certificate management component in `picloud-network` subscribes to this event and initiates renewal automatically.

### Certificate revocation

When a node is removed from the cluster (`picloud node remove`):
1. `NodeRemoved` event emitted
2. Node's certificate added to an in-memory CRL (Certificate Revocation List) stored in Raft state
3. All other nodes refresh their CRL from Raft state
4. The removed node's mTLS connections are rejected within one Raft heartbeat cycle

### Enrollment endpoint security

The `/enroll` endpoint is the most sensitive surface in the platform:

- Served over TLS with the cluster's CA certificate — clients can verify they are talking to the legitimate cluster
- Rate limited — maximum 5 enrollment attempts per minute per IP
- In auto-enroll mode: logs every enrollment as a `NodeEnrolled` event with the node's address
- In token mode: token is single-use and time-limited
- CSR validation: the CSR must contain only the node's hostname in the Subject — no wildcard SANs, no IP SANs other than the node's own address
- Enrollment is always logged as a platform event — `NodeEnrolled` or `NodeEnrollmentRejected`

### Node identity in certificates

Every node certificate carries:
```
Subject: CN=pi-node-01.picloud.local
SAN: DNS:pi-node-01.picloud.local, IP:192.168.1.101
Issuer: CN=PiCloud CA, O=picloud.local, cluster-id={uuid}
```

The cluster ID is embedded in the Issuer — a certificate issued by a different cluster (different cluster ID) is rejected even if it chains to the same CA.

### Implementation in picloud-network

```
picloud-network/src/
├── ca/
│   ├── mod.rs         — CA module root
│   ├── authority.rs   — CA key management, certificate signing
│   ├── enrollment.rs  — enrollment endpoint handler, CSR validation
│   ├── renewal.rs     — certificate expiry tracking, auto-renewal
│   └── revocation.rs  — CRL management, Raft-replicated
└── dns/               — (existing)
```

**Crates used:**
- `rcgen` — pure Rust certificate generation and CSR handling (already in workspace)
- `x509-parser` — certificate parsing and validation (already in workspace)
- `rustls` — TLS configuration (already in workspace)

No new dependencies required.

### CLI commands

```bash
# Generate enrollment token (token mode only)
picloud node enrollment-token --ttl 2h

# List active enrollment tokens
picloud node enrollment-tokens

# Revoke an enrollment token
picloud node revoke-token <token-id>

# Remove a node and revoke its certificate
picloud node remove pi-node-05

# List all node certificates and their expiry
picloud node certs

# Manually trigger certificate renewal for a node
picloud node renew-cert pi-node-01
```

### Configuration at cluster init

```bash
# Auto-enroll (default — for trusted networks)
picloud cluster init --domain picloud.local

# Token enrollment (for secured environments)
picloud cluster init --domain acme.local --enrollment-mode token

# Token enrollment with BYO CA
picloud cluster init \
  --domain acme.local \
  --enrollment-mode token \
  --ca-cert ./ca.pem \
  --ca-key  ./ca-key.pem
```

**Rationale:**
- Two modes with a clear default eliminates friction for the primary use case (home lab) while making the secure path available without custom implementation
- Same two-phase join in both modes means one code path, one security model — only the authorisation check differs
- CA in Raft state means no single point of failure for certificate issuance — any node that becomes leader can immediately issue certs
- All enrollment events in the platform log — `NodeEnrolled`, `NodeEnrollmentRejected` — mean the cluster always knows who joined and when
- `rcgen` and `x509-parser` are already in the workspace — zero new dependencies
- Auto-renewal via inference rules and event subscriptions means certificate expiry is handled the same way as any other platform alert — consistently and observably

**Consequences:**
- `picloud-network` gains a `ca/` module
- The enrollment endpoint must be started before Raft join — it is the first HTTP endpoint brought up at node startup
- In auto-enroll mode, the cluster should log a prominent warning at init time so operators know the security model
- Certificate expiry tracking adds `pc:certExpiresAt` and `pc:certFingerprint` to the node's RDF triples
- The master key used to encrypt the CA private key at rest must be derived from the cluster ID — losing the cluster ID means losing access to the CA