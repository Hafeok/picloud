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
runner: cargo-test
runner-args: "tc172_audience_enforcement"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

issue a token for `photo-app`. Present the token to `user-service`'s SPARQL endpoint. Assert 403 (wrong audience).