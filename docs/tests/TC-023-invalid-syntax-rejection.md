---
id: TC-023
title: invalid_syntax_rejection
type: scenario
status: failing
validates:
  features:
  - FT-001
  adrs:
  - ADR-007
phase: 1
runner: picloud-test
runner-args: "invalid_syntax_rejection"
---

submit `.picloud` files with deliberate syntax errors (missing braces, invalid property names, wrong types). Assert each returns a human-readable error, not a panic or 500.