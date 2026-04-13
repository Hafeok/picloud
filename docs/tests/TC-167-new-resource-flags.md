---
id: TC-167
title: new_resource_flags
type: scenario
status: unimplemented
validates:
  features:
  - FT-007
  adrs:
  - ADR-050
phase: 1
---

run `picloud new container` with all required flags specified. Assert a `.picloud` file is generated, is valid (auto-validation passes), and the content matches the specified flags.