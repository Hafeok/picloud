---
id: TC-182
title: token_enroll_expiry
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-053
phase: 1
runner: cargo-test
runner-args: "tc182_token_enroll_expiry"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

generate a token with a 30-second TTL. Wait 45 seconds. Attempt enrollment. Assert `NodeEnrollmentRejected` with an expiry reason.