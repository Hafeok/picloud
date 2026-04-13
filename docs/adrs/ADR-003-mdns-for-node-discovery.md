---
id: ADR-003
title: mDNS for Node Discovery
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Nodes need to find each other on the local network. The target environment is a home lab — nodes are on the same broadcast domain, static IPs are undesirable, and there is no infrastructure DNS.

**Decision:** Use mDNS (Multicast DNS) for automatic node discovery. Nodes broadcast their presence on startup and discover peers passively. The platform also advertises `picloud.local` via mDNS, which means external clients (operator laptops, browsers, RDF tools) resolve the cluster domain automatically on any mDNS-capable OS — no DNS configuration required.

**mDNS client support:**
- macOS — native, zero configuration
- Linux — requires `avahi-daemon` (standard on most distributions)
- Windows 10+ — native mDNS resolver

This means external DNS is not a separate concern. The same mDNS mechanism that handles node discovery also handles client-side name resolution for `picloud.local`. One implementation, two purposes.

**Rationale:**
- Zero configuration — nodes join the cluster by powering on
- Works on any local network without infrastructure changes
- `mdns-sd` crate provides a production-quality Rust implementation
- Consistent with the "add a node and capacity grows" user experience

**Constraints:**
- Nodes must be on the same broadcast domain (same L2 network segment)
- mDNS does not work across routers without explicit multicast forwarding
- Not suitable for multi-site or WAN deployments (out of scope per PRD)

**Rejected alternatives:**
- **Static IP configuration** — requires manual intervention on every new node. Violates the zero-configuration goal.
- **Bootstrap token with known seed address** — requires knowing at least one node's address. Adds operational friction.
- **Consul/etcd for discovery** — external infrastructure dependency. Violates single-binary goal.