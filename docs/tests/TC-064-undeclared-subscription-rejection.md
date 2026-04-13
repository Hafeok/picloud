---
id: TC-064
title: undeclared_subscription_rejection
type: scenario
status: passing
validates:
  features:
  - FT-005
  adrs:
  - ADR-022
phase: 1
runner: scripts/run-tc.sh
runner-args: "undeclared-subscription-rejection"
last-run: 2026-04-13T19:48:54.098720974+00:00
---

attempt to subscribe to a product's events at runtime via the SDK without a declared `event-subscription` resource. Assert the platform returns 403.