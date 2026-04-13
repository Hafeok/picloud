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
---

generate an enrollment token. Use it once to enroll a node. Assert `NodeEnrolled` event. Attempt to reuse the token on a second node. Assert `NodeEnrollmentRejected` event.