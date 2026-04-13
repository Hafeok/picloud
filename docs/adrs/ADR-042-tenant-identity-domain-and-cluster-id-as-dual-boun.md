---
id: ADR-042
title: Tenant Identity — Domain and Cluster ID as Dual Boundary
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** PiCloud is designed to run as a single tenant by default (`picloud.local`) but must support multiple isolated tenants — either on the same network or across different networks. Two clusters on the same local network must not accidentally merge, even if misconfigured. The tenant boundary must be both human-readable and cryptographically enforced.

**Decision:** Every PiCloud cluster has two identifiers established at `cluster init`:

1. **Cluster domain** — the human-readable tenant identity. Defaults to `picloud.local`. Configurable at init time. All IRIs, mDNS advertisements, and TLS certificates are scoped to this domain.

2. **Cluster ID** — a UUID generated at `cluster init` and stored in the cluster's Raft state. Cryptographically bound to the cluster CA. All node-join bootstrap tokens are signed by the cluster CA and carry the cluster ID. A node cannot join a cluster unless its bootstrap token was issued by that cluster's CA.

**The dual boundary:**
- The domain prevents accidental mDNS cross-discovery — a node advertising `company-a.local` is invisible to a node listening for `company-b.local`
- The cluster ID + CA prevents deliberate or accidental cross-join — even if two clusters share a domain name, a node cannot join without a valid bootstrap token from that cluster's CA

**Installation:**
```bash
# Default tenant
picloud cluster init

# Named tenant
picloud cluster init --domain acme.local

# Custom domain (external CA, BYO-CA mode — ADR-030)
picloud cluster init --domain cloud.acme.com --ca-cert ./acme-ca.pem
```

**Cluster identity stored in Raft:**
```rust
pub struct ClusterIdentity {
    pub cluster_id: Uuid,
    pub domain: ClusterDomain,
    pub created_at: DateTime<Utc>,
    /// Fingerprint of the cluster CA — all node certs must chain to this
    pub ca_fingerprint: String,
}
```

**mDNS scoping:**
Nodes advertise their cluster domain as the mDNS service type. Discovery filters strictly by service type — a node only responds to discovery from peers advertising the same domain. Two clusters on the same network are mutually invisible.

**Node join validation:**
When a node attempts to join:
1. It presents a bootstrap token
2. The cluster leader verifies the token was signed by the cluster CA (CA fingerprint match)
3. The leader verifies the cluster ID in the token matches the cluster's own cluster ID
4. Only then is the node admitted to Raft

A node that passes mDNS discovery but fails token validation is rejected and logged as a `NodeJoinRejected` event.

**IRI namespace:**
The cluster domain is the root of the IRI namespace. Every resource IRI is scoped to the domain, which is scoped to the cluster:
```
https://picloud.local/...     ← default tenant
https://acme.local/...        ← named tenant
https://cloud.acme.com/...    ← custom domain tenant
```

**Future multi-tenancy:**
When running multiple tenants, each cluster is fully independent — separate event log, separate RDF graph, separate IAM, separate storage pool. There is no cross-tenant resource sharing. This is consistent with ADR-028 (low coupling) applied at the cluster level.

**Rationale:**
- Domain alone is insufficient — two operators could accidentally use the same `.local` name on the same network and partially merge clusters
- Cluster ID alone is insufficient — it is not human-readable, making operations error-prone
- The dual boundary provides defence in depth: human-readable discrimination via domain, cryptographic enforcement via cluster CA
- Defaulting to `picloud.local` means zero configuration for the common single-tenant home lab case
- The cluster identity is established at `cluster init` and never changes — it is permanent for the lifetime of the cluster

**Consequences:**
- `ClusterIdentity` must be the first thing written to Raft state on `cluster init` — before any other operation
- The cluster domain must be embedded in the cluster CA certificate (SAN field) — this is how mTLS clients verify they are talking to the right cluster
- Changing a cluster's domain after init is not supported — the domain is part of the cluster's cryptographic identity
- `picloud cluster init` output must clearly display the cluster ID, domain, and CA fingerprint so operators can verify they are managing the right cluster