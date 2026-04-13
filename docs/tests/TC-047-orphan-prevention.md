---
id: TC-047
title: orphan_prevention
type: scenario
status: unimplemented
validates:
  features:
  - FT-001
  adrs:
  - ADR-016
phase: 1
---

delete a product. Query the RDF graph for any resource whose IRI contains the deleted product's path. Assert the result set is empty.