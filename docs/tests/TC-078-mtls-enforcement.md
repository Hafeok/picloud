---
id: TC-078
title: mtls_enforcement
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-027
phase: 1
runner: cargo-test
runner-args: "tc078_mtls_enforcement"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

attempt to connect to the platform API with no client certificate: assert TLS handshake fails with `certificate_required` alert. Attempt with a self-signed cert not issued by the cluster CA: assert rejection. Connect with a valid platform-issued certificate: assert 200.