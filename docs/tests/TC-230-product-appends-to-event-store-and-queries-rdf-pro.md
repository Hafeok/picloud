---
id: TC-230
title: Product appends to event store and queries RDF projection
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc230_product_appends_to_event_store_and_queries_rdf_projection"
validates:
  features: [FT-078]
  adrs: [ADR-032]
phase: 3
last-run: 2026-04-17T10:13:27.429280618+00:00
last-run-duration: 0.5s
---

## Description

Verifies end-to-end that a Product can declare an event-store resource with
aggregate definitions, append events through its ProductEventStore, and have
those events automatically projected into the product's RDF named graph so
that SPARQL queries return the correct projected state.

### Steps

1. Deploy a product ("photo-app" v1.0.0)
2. Declare an EventStore resource ("photos") with two aggregate types (Photo, Album)
3. Mark the EventStore as Ready
4. Verify the EventStore is typed as `picloud:EventStore` in the default RDF graph
5. Verify aggregate types (Photo, Album) are projected as `picloud:aggregateType` triples
6. Verify the EventStore appears in the product's named graph
7. Append 3 product events via ProductEventStore (2 PhotoUploaded, 1 AlbumCreated)
8. Project appended events through the RDF projector
9. Query for PhotoUploaded events — expect 2 results with correct titles
10. Query for AlbumCreated events — expect 1 result with correct title and photo count
11. Query the product's named graph — expect 3 ProductEvent resources
12. Verify product isolation — another product's named graph has zero events
13. Verify ASK query against projected data
14. Verify GROUP BY aggregate type counts (Photo: 2, Album: 1)