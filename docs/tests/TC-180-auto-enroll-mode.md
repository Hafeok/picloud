---
id: TC-180
title: auto_enroll_mode
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-053
phase: 1
runner: cargo-test
runner-args: "tc180_auto_enroll_mode"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

configure a cluster in auto-enroll mode. Power on a new node. Assert `NodeEnrolled` event within 30 seconds of mDNS discovery, correct node certificate issued, node participates in Raft.