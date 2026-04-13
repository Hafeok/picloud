---
id: TC-050
title: jwks_key_rotation
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-017
phase: 1
runner: picloud-test
runner-args: "jwks-key-rotation"
---

trigger key rotation. Assert JWKS endpoint serves both old and new keys during the rotation window. Assert tokens issued under the old key are still valid during the window.