---
id: ADR-052
title: Integrated DNS Server — Authoritative for Tenant Domain
status: accepted
features:
- FT-006
- FT-021
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:a897b4248132426e71bee4c014878fb32b326a48b99e515bf0ebc63229fb9ecd
---

**Status:** Accepted

**Context:** Every workload, ingress hostname, node, and product in PiCloud has a canonical IRI and a known network address — all of it already projected into the Oxigraph RDF graph. Clients on the local network need to resolve these hostnames without manual DNS record management. The platform is the authoritative source of truth for its own domain — the DNS server is just a query interface over data that already exists.

**Decision:** Every `picloud-server` node runs an authoritative DNS server on port 53 (UDP and TCP). It is authoritative for the cluster's tenant domain only (e.g. `picloud.local` or a custom domain configured at `cluster init`). It answers queries from the RDF graph. It does not recurse, forward, or resolve external names. External DNS resolution is delegated to the operator's existing infrastructure (Pi-hole + Unbound in the reference setup). Clients are configured to forward the tenant domain to any PiCloud node — one conditional forwarding rule in Pi-hole.

### Integration with existing DNS infrastructure

```
Client device
  → Pi-hole + Unbound (handles all external resolution)
    → *.picloud.local → forwarded to PiCloud DNS (192.168.x.x:53)
    → everything else → resolved normally via Unbound

# Pi-hole conditional forwarding — one rule:
picloud.local → 192.168.1.101  # any cluster node
```

PiCloud DNS only ever answers for its own domain. Pi-hole never needs to know about PiCloud internals.

### Records served

**A records** — IPv4 address for a hostname:

| Query | Answer | Source in graph |
|---|---|---|
| `picloud.local` | Cluster leader ingress IP | `pc:isLeader true` node |
| `pi-node-01.picloud.local` | Node IP | `pc:nodeAddress` on `pc:Node` |
| `photo-app.picloud.local` | Product ingress IP | `pc:ingressAddress` on `pc:Product` |
| `photos.picloud.local` | Ingress target IP | `pc:hostname` on `pc:Ingress` |
| `staging.photo-api.picloud.local` | Staging ingress IP | Ephemeral ingress resource |

**SRV records** — service discovery by type:

| Query | Answer |
|---|---|
| `_sparql._tcp.photo-app.picloud.local` | SPARQL endpoint port and host |
| `_events._tcp.photo-app.picloud.local` | Event stream SSE endpoint |
| `_https._tcp.picloud.local` | Cluster HTTPS ingress |

**TXT records** — semantic metadata for a service:

| Query | Answer |
|---|---|
| `photo-app.picloud.local` | `"ontology=https://picloud.local/products/photo-app/ontology version=1.0.0"` |
| `picloud.local` | `"cluster-id={uuid} platform-version=0.1.0"` |

**PTR records** — reverse DNS (IP → hostname):
Registered for node addresses and ingress IPs so tools like `nmap` and `traceroute` show meaningful names.

**NXDOMAIN** — for any hostname not found in the graph. No fallthrough, no recursion.

### Query model

Every DNS query resolves in two steps:

1. **Cache lookup** — in-memory cache keyed by `(qtype, qname)`. If present and not expired, return immediately.
2. **Graph query** — if cache miss, query Oxigraph with a SPARQL SELECT. Cache the result with TTL = 30 seconds.

```sparql
# A record lookup for an ingress hostname
SELECT ?address WHERE {
  {
    # Ingress hostname match
    ?ingress a pc:Ingress ;
             pc:hostname "{qname}" ;
             pc:targetAddress ?address .
  } UNION {
    # Node hostname match
    ?node a pc:Node ;
          pc:hostname "{qname}" ;
          pc:nodeAddress ?address .
  } UNION {
    # Product hostname match
    ?product a pc:Product ;
             pc:hostname "{qname}" ;
             pc:ingressAddress ?address .
  }
}
LIMIT 1
```

### TTL and cache invalidation

**TTL: 30 seconds** — short enough that clients re-query frequently, long enough to avoid hammering Oxigraph on every request.

**Event-driven cache invalidation** — the DNS server subscribes to platform events and invalidates affected cache entries immediately, without waiting for TTL expiry:

| Event | Cache entries invalidated |
|---|---|
| `WorkloadRescheduled` | All A records for that workload's hostname |
| `IngressCreated` | New entry added immediately |
| `IngressUpdated` | A, SRV, TXT records for that ingress hostname |
| `IngressDeleted` | Entry removed, subsequent queries return NXDOMAIN |
| `NodeJoined` | New PTR and A record for node hostname |
| `NodeLeft` | A and PTR records for that node removed |
| `ProductDeployed` | TXT record updated with new version |
| `StagingDeploymentReady` | Ephemeral staging A record added |
| `StagingTeardownCompleted` | Ephemeral staging A record removed |

This means workload reschedules are visible to DNS clients within seconds — the TTL is a fallback, not the primary invalidation mechanism.

### Multi-node consistency

Every node runs its own DNS server with its own in-memory cache. Caches are not synchronised across nodes — each node independently queries Oxigraph, which is consistent across the cluster via Raft. Since all nodes read from the same RDF graph, responses are consistent. Cache entries expire and refresh independently on each node within the 30-second TTL window.

Clients can point at any node's IP as their DNS server. If a node goes down, Pi-hole's conditional forwarding retries against another node (standard DNS retry behaviour).

### Implementation in picloud-network

The DNS server lives in `picloud-network` — the crate already responsible for mDNS, TLS, and certificate management.

```
picloud-network/src/
├── dns/
│   ├── server.rs      — UDP/TCP listener on port 53, query dispatch
│   ├── resolver.rs    — cache lookup → SPARQL query → response assembly
│   ├── cache.rs       — in-memory TTL cache with event-driven invalidation
│   ├── records.rs     — A, SRV, TXT, PTR record construction from RDF data
│   └── events.rs      — platform event subscription, cache invalidation
```

**DNS library:** `hickory-dns` (formerly trust-dns) — pure Rust, actively maintained, supports authoritative server mode, compiles to ARM64.

### Pi-hole configuration

One conditional forwarding rule points the tenant domain at any cluster node:

```
# Pi-hole Admin → Settings → DNS → Conditional Forwarding
Domain: picloud.local
DNS Server: 192.168.1.101  # any node IP — others used as fallback
```

For clusters with a custom domain at init time:
```
Domain: acme.local
DNS Server: 192.168.1.101
```

No other Pi-hole configuration needed. Pi-hole continues to handle all external resolution, ad blocking, and DHCP as before.

### Rationale
- The RDF graph already contains every hostname and address — DNS is a read-only projection of existing data, not a new data store
- Authoritative-only design keeps the implementation minimal — no recursive resolver, no upstream forwarder, no root hint management
- Delegating external resolution to Pi-hole + Unbound respects the operator's existing investment and keeps concerns separated
- Event-driven cache invalidation means workload reschedules are visible to clients within seconds without requiring zero-TTL records
- `hickory-dns` is the only pure Rust DNS library with authoritative server support — consistent with ADR-001
- Every node runs DNS independently — no single point of failure, no leader election needed for DNS

**Rejected alternatives:**
- **External DNS server (CoreDNS, BIND)** — adds an external dependency; DNS records would not be automatically derived from the RDF graph.
- **hosts file management** — does not scale, cannot serve dynamic records, and requires manual updates on every client machine.

**Consequences:**
- `picloud-network` gains a `dns/` module
- `hickory-dns` is added as a workspace dependency
- Port 53 must be open on all cluster nodes (added to `deploy/setup-node.sh`)
- The platform ontology gains `pc:hostname` as a property on `pc:Ingress`, `pc:Node`, and `pc:Product`
- `picloud cluster init` output should include the conditional forwarding rule to paste into Pi-hole
- DNS queries are logged as OTel spans — slow Oxigraph queries surface in telemetry