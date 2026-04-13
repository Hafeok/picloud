---
id: TC-073
title: webauthn_challenge_replay_rejection
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-025
phase: 1
runner: picloud-test
runner-args: "webauthn-challenge-replay-rejection"
---

capture a WebAuthn challenge response, attempt to replay it. Assert the platform rejects the replayed assertion.