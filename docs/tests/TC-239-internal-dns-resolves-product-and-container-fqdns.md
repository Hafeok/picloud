---
id: TC-239
title: Internal DNS resolves product and container FQDNs from any node
type: scenario
status: passing
runner: cargo-test
runner-args: "tc239_internal_dns_resolves_product_and_container_fqdns"
validates:
  features: [FT-021]
  adrs: []
phase: 1
last-run: 2026-04-13T21:16:32.238106612+00:00
---

## Description

Verify that the internal DNS resolver correctly resolves both product FQDNs
(e.g., `photo-app.picloud.local`) and container FQDNs
(e.g., `api-server.photo-app.picloud.local`) from any node in the cluster.

The test exercises:
- Product FQDN A record resolution via SPARQL-backed resolver
- Container FQDN A record resolution for multiple containers in the same product
- Identical resolution results from independent resolver instances (any-node property)
- InMemoryDnsRegistry registration and resolution for product and container IRIs
- Negative test: unregistered resources return ResourceNotFound