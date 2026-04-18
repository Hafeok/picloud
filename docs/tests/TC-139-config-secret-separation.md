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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.7s
---

assert that secret values are never stored in the config store. Attempt to set a config entry with the key `password`. Assert the platform rejects any config key flagged as sensitive.