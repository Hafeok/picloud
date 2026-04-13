---
id: TC-182
title: token_enroll_expiry
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-053
phase: 1
runner: picloud-test
runner-args: "token-enroll-expiry"
---

generate a token with a 30-second TTL. Wait 45 seconds. Attempt enrollment. Assert `NodeEnrollmentRejected` with an expiry reason.