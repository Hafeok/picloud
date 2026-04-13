---
id: TC-159
title: ingress_host_routing
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-048
phase: 1
runner: picloud-test
runner-args: "ingress-host-routing"
---

declare an ingress resource with `host: photos.picloud.local`. Send an HTTP request to that host. Assert it is routed to the correct container and the correct response is returned.