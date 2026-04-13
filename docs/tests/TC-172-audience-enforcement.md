---
id: TC-172
title: audience_enforcement
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-051
phase: 1
runner: picloud-test
runner-args: "audience-enforcement"
---

issue a token for `photo-app`. Present the token to `user-service`'s SPARQL endpoint. Assert 403 (wrong audience).