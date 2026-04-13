---
id: TC-088
title: content_negotiation
type: scenario
status: unimplemented
validates:
  features: []
  adrs:
  - ADR-029
phase: 1
---

GET a resource IRI with `Accept: text/turtle`, then with `Accept: application/ld+json`, then with `Accept: application/json`. Assert correct Content-Type in each response and that the body is valid for the declared type.