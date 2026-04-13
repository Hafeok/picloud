---
id: TC-172
title: audience_enforcement
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-051
phase: 1
---

issue a token for `photo-app`. Present the token to `user-service`'s SPARQL endpoint. Assert 403 (wrong audience).