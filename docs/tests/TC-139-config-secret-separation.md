---
id: TC-139
title: config_secret_separation
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-043
phase: 1
runner: cargo-test
runner-args: "config_secret_separation"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

assert that secret values are never stored in the config store. Attempt to set a config entry with the key `password`. Assert the platform rejects any config key flagged as sensitive.