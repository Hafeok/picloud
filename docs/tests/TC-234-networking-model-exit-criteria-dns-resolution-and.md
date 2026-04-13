---
id: TC-234
title: Networking model exit criteria — DNS resolution and HTTP IRI dereferencing functional
type: exit-criteria
status: unimplemented
validates:
  features: []
  adrs: []
phase: 1
---

⟦Λ:ExitCriteria⟧{
  dns_resolves: dig(product_fqdn).status = NOERROR
  iri_dereferences: GET(product_iri).status = 200
  content_negotiation: GET(product_iri, Accept:"text/turtle").content_type = "text/turtle"
  mdns_fallback: stop(dns_server) → resolve(peer_fqdn).method = mDNS within 10s
  mtls_enforced: GET(product_iri, no_cert).status = 403
}

After deploying a Product with at least one container resource, verify that (1) the product FQDN resolves via the platform DNS server, (2) the product IRI returns RDF when dereferenced via HTTP, (3) content negotiation serves Turtle, JSON-LD, and JSON representations, (4) mDNS fallback activates when the DNS server is unavailable, and (5) unauthenticated requests are rejected with 403.
