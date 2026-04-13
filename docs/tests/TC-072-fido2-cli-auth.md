---
id: TC-072
title: fido2_cli_auth
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-025
phase: 1
runner: cargo-test
runner-args: "tc072_fido2_cli_auth"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

authenticate the CLI using a FIDO2 hardware key via the device flow. Assert a valid token is issued with the correct `sub` and `iss` claims. Assert the token payload contains no password-derived fields.