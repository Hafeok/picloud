---
id: TC-064
title: undeclared_subscription_rejection
type: scenario
status: failing
validates:
  features:
  - FT-005
  adrs:
  - ADR-022
phase: 1
runner: picloud-test
runner-args: "undeclared-subscription-rejection"
---

attempt to subscribe to a product's events at runtime via the SDK without a declared `event-subscription` resource. Assert the platform returns 403.