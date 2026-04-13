---
id: TC-075
title: bootstrap_token_expiry
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-026
phase: 1
---

generate a bootstrap token with a 1-minute TTL. Wait 90 seconds. Attempt to use the token. Assert rejection with a clear expiry error.