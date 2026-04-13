---
id: TC-298
title: mTLS exit — all node communication encrypted with platform certificates
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc298_mtls"
validates:
  features: [FT-023]
  adrs: [ADR-053]
phase: 1
last-run: 2026-04-13T21:29:54.965924898+00:00
---

## Description

Exit criterion validating that all node-to-node communication uses mutual TLS with platform-issued certificates. Verified by:

1. **Full mTLS round-trip** — Two nodes (node-alpha, node-beta) with platform-issued certs establish an mTLS connection, exchange data, and verify the echoed payload matches.
2. **Config constructibility** — `mtls_server_config` and `mtls_client_config` both produce valid rustls configurations without error.
3. **Certificate properties** — Node certificates carry the correct CN, issuer (PiCloud Platform CA), and SAN (`<node>.picloud.local`).