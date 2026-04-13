---
id: TC-050
title: jwks_key_rotation
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-017
phase: 1
runner: cargo-test
runner-args: "tc050_jwks_key_rotation"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

trigger key rotation. Assert JWKS endpoint serves both old and new keys during the rotation window. Assert tokens issued under the old key are still valid during the window.