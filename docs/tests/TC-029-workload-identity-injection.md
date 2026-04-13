---
id: TC-029
title: workload_identity_injection
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-009
phase: 1
runner: cargo-test
runner-args: "tc029_workload_identity_injection"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

deploy a container with a workload identity. Assert the container process receives an injected credential and can use it to request a token from the IAM endpoint. Assert the token `sub` matches the workload identity IRI.