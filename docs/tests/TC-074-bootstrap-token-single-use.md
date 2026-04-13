---
id: TC-074
title: bootstrap_token_single_use
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-026
phase: 1
runner: cargo-test
runner-args: "tc074_bootstrap_token_single_use"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

use a bootstrap token to register the first admin. Attempt to reuse the same token. Assert the second use returns 401.