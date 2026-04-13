---
id: TC-078
title: mtls_enforcement
type: scenario
status: unimplemented
validates:
  features:
  - FT-003
  adrs:
  - ADR-027
phase: 1
---

attempt to connect to the platform API with no client certificate: assert TLS handshake fails with `certificate_required` alert. Attempt with a self-signed cert not issued by the cluster CA: assert rejection. Connect with a valid platform-issued certificate: assert 200.