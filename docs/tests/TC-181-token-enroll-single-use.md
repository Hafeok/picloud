---
id: TC-181
title: token_enroll_single_use
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-053
phase: 1
runner: cargo-test
runner-args: "tc181_token_enroll_single_use"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

generate an enrollment token. Use it once to enroll a node. Assert `NodeEnrolled` event. Attempt to reuse the token on a second node. Assert `NodeEnrollmentRejected` event.