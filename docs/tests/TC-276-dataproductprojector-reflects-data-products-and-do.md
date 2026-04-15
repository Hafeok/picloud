---
id: TC-276
title: DataProductProjector reflects data products and domains in RDF graph
type: scenario
status: passing
runner: cargo-test
runner-args: "tc276_dataproductprojector_reflects_data_products_and_domains_in_rdf_graph"
validates:
  features: [FT-072]
  adrs: [ADR-056]
phase: 3
last-run: 2026-04-15T16:06:53.719194487+00:00
last-run-duration: 0.7s
---

## Description

Scenario test verifying that the DataProductProjector builds a complete RDF graph
reflecting data products, domains, producers, and freshness:

1. Declares two data domains (governance, analytics) with stewards and sensitivity.
   Verifies both are discoverable as `pc:DataDomain` via SPARQL.

2. Deploys two products (reporting-app, ml-pipeline) and declares data products in
   each, linked to different domains. Verifies:
   - Each data product has `pc:producedBy` → owning product (producer link)
   - Each data product has `pc:belongsToDomain` → domain (domain membership)
   - Metadata (name, version, maxAge) is projected correctly

3. Emits `DataProductRefreshed` for one data product. Verifies freshness metadata:
   `pc:lastRefreshed` (xsd:dateTime) and `pc:tripleCount` (xsd:unsignedLong) are
   present; status transitions to "ready" (NamedNode + statusLabel).

4. Emits a second `DataProductRefreshed`. Verifies old freshness values are replaced —
   only the latest refresh metadata remains (no stale duplicates).

5. Verifies cross-product, cross-domain SPARQL discoverability: a single query finds
   all data products across both products and both domains. Queries by producer and
   by domain return the expected subsets.