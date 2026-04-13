---
id: TC-029
title: workload_identity_injection
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-009
phase: 1
runner: picloud-test
runner-args: "workload-identity-injection"
---

deploy a container with a workload identity. Assert the container process receives an injected credential and can use it to request a token from the IAM endpoint. Assert the token `sub` matches the workload identity IRI.