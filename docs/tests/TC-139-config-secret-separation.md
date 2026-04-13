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
---

assert that secret values are never stored in the config store. Attempt to set a config entry with the key `password`. Assert the platform rejects any config key flagged as sensitive.