---
id: TC-188
title: capability_implements_shacl_validation
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_implements_shacl_validation"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

deploy a Product that declares `implements: ['gps-to-place@1.0.0']` but whose workload does not subscribe to `CoordinatesReceived`. Assert `resource apply` fails with a SHACL conformance error. Assert no `CapabilityImplementorAdded` event is emitted.