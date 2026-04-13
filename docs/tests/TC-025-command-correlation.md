---
id: TC-025
title: command_correlation
type: scenario
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-008
phase: 1
runner: cargo-test
runner-args: "tc025_command_correlation"
---

emit `picloud resource apply` with a known correlation ID. Subscribe to the result stream. Assert the terminal event (`ResourceReady` or `ResourceFailed`) carries the same correlation ID and arrives within 30 seconds.