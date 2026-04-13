---
id: TC-159
title: ingress_host_routing
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-048
phase: 1
runner: cargo-test
runner-args: "tc159_ingress_host_routing"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

declare an ingress resource with `host: photos.picloud.local`. Send an HTTP request to that host. Assert it is routed to the correct container and the correct response is returned.