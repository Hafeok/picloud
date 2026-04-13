---
id: TC-173
title: token_exchange_on_behalf_of
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-051
phase: 1
runner: cargo-test
runner-args: "tc173_token_exchange_on_behalf_of"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

execute RFC 8693 token exchange: `photo-app` acts on behalf of Alice against `user-service`. Assert the new token has `aud: user-service`, `sub: alice`, and an `act` claim containing `photo-app`.