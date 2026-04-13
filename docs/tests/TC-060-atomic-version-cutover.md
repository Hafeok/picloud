---
id: TC-060
title: atomic_version_cutover
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-021
phase: 1
---

deploy product v1, then apply v2. Monitor the RDF graph and the product's ingress throughout the upgrade. Assert there is no window where both v1 and v2 containers are simultaneously tagged `picloud:Running` under the product IRI.