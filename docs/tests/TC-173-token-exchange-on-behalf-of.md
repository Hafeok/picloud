---
id: TC-173
title: token_exchange_on_behalf_of
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-051
phase: 1
runner: picloud-test
runner-args: "token-exchange-on-behalf-of"
---

execute RFC 8693 token exchange: `photo-app` acts on behalf of Alice against `user-service`. Assert the new token has `aud: user-service`, `sub: alice`, and an `act` claim containing `photo-app`.