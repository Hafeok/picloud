---
id: TC-045
title: product_full_lifecycle
type: scenario
status: unimplemented
validates:
  features:
  - FT-001
  adrs:
  - ADR-016
phase: 1
---

apply a product with container, volume, and identity. Assert `ProductReady` event. Delete the product. Assert `ProductDeleted` event and all child resources removed from the RDF graph within 60 seconds.