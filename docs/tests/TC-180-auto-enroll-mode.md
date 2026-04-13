---
id: TC-180
title: auto_enroll_mode
type: scenario
status: unimplemented
validates:
  features:
  - FT-006
  adrs:
  - ADR-053
phase: 1
---

configure a cluster in auto-enroll mode. Power on a new node. Assert `NodeEnrolled` event within 30 seconds of mDNS discovery, correct node certificate issued, node participates in Raft.