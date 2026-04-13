---
id: TC-073
title: webauthn_challenge_replay_rejection
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-025
phase: 1
runner: cargo-test
runner-args: "tc073_webauthn_challenge_replay_rejection"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

capture a WebAuthn challenge response, attempt to replay it. Assert the platform rejects the replayed assertion.