---
id: TC-074
title: bootstrap_token_single_use
type: scenario
status: unimplemented
validates:
  features:
  - FT-003
  adrs:
  - ADR-026
phase: 1
---

use a bootstrap token to register the first admin. Attempt to reuse the same token. Assert the second use returns 401.