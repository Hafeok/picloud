---
id: TC-046
title: cascading_delete
type: scenario
status: failing
validates:
  features:
  - FT-001
  adrs:
  - ADR-016
phase: 1
runner: picloud-test
runner-args: "cascading-delete"
---

apply a product with 5 child resources. Delete the product. Assert all 5 child resource IRIs return SPARQL `ASK { ?s ?p ?o }` = false within 60 seconds.