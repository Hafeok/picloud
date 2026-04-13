---
id: TC-241
title: mTLS rejects connections without platform-issued certificates
type: scenario
status: passing
runner: cargo-test
runner-args: "tc241_mtls_rejects"
validates:
  features: [FT-023]
  adrs: [ADR-053]
phase: 1
last-run: 2026-04-13T21:29:54.965924898+00:00
---

## Description

Validates that the mTLS server configuration rejects TLS connections from clients that do not present a valid platform-issued certificate. Two sub-scenarios are tested:

1. **No client certificate** — A client connects with plain TLS (no client cert). The mTLS server rejects the handshake.
2. **Foreign CA certificate** — A client presents a certificate signed by a different (rogue) CA. The mTLS server rejects the handshake because the client cert is not signed by the platform CA.