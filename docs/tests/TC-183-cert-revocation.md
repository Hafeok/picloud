---
id: TC-183
title: cert_revocation
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-053
phase: 1
runner: cargo-test
runner-args: "tc183_cert_revocation"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

remove a node via `picloud node remove`. Assert a `NodeRemoved` event, the CRL updated in Raft, and the removed node's subsequent mTLS connections rejected within 5 seconds (one heartbeat cycle).