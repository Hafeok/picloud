---
id: TC-072
title: fido2_cli_auth
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-025
phase: 1
runner: picloud-test
runner-args: "fido2-cli-auth"
---

authenticate the CLI using a FIDO2 hardware key via the device flow. Assert a valid token is issued with the correct `sub` and `iss` claims. Assert the token payload contains no password-derived fields.