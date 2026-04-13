---
id: TC-071
title: passkey_registration
type: scenario
status: unimplemented
validates:
  features:
  - FT-003
  adrs:
  - ADR-025
phase: 1
---

bootstrap a fresh cluster. Complete the WebAuthn registration ceremony using a hardware FIDO2 key. Assert the admin identity is created, the passkey is registered, and no password is present anywhere in the platform event log or RDF graph.