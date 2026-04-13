---
id: TC-222
title: DNS server failure — fallback to mDNS
type: chaos
status: failing
validates:
  features: []
  adrs: []
phase: 1
runner: picloud-test
runner-args: "dns-server-failure--fallback-to-mdns"
---

⟦Σ:Types⟧{
  Node≜IRI
  ResolutionMethod≜DNS|mDNS
  DiscoveryState≜⟨node:Node, method:ResolutionMethod, peers:Node*⟩
}

⟦Γ:Invariants⟧{
  ∀n:Node: dns_unavailable → n.method = mDNS ∧ |n.peers| > 0
}

⟦Λ:Scenario⟧{
  given≜cluster_init(nodes:3) ∧ dns_server(running)
  when≜stop(dns_server)
  then≜within(10s): ∀n∈nodes: resolve(n, peer_fqdn).method = mDNS
       ∧ cluster_membership(n).size = 3
}

⟦Ε⟧⟨δ≜0.85;φ≜70;τ≜◊?⟩