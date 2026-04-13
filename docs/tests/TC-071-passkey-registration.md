---
id: TC-071
title: passkey_registration
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-025
phase: 1
runner: cargo-test
runner-args: "tc071_passkey_registration"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

bootstrap a fresh cluster. Complete the WebAuthn registration ceremony using a hardware FIDO2 key. Assert the admin identity is created, the passkey is registered, and no password is present anywhere in the platform event log or RDF graph.