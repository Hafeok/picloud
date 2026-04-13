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
---

remove a node via `picloud node remove`. Assert a `NodeRemoved` event, the CRL updated in Raft, and the removed node's subsequent mTLS connections rejected within 5 seconds (one heartbeat cycle).