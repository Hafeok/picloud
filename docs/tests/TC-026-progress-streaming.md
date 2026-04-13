---
id: TC-026
title: progress_streaming
type: scenario
status: failing
validates:
  features:
  - FT-002
  adrs:
  - ADR-008
phase: 1
runner: picloud-test
runner-args: "progress_streaming"
---

apply a multi-resource product. Assert that intermediate progress events (`ResourceDeclared`, `ResourceProvisioning`) stream to the CLI before the terminal event.