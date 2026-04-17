---
id: TC-265
title: SPARQL endpoint rejects unauthenticated and unauthorized requests
type: scenario
status: passing
runner: cargo-test
runner-args: "tc265_sparql_endpoint_rejects_unauthenticated_and_unauthorized_requests"
validates:
  features: [FT-052]
  adrs: []
phase: 1
last-run: 2026-04-17T06:59:07.452918442+00:00
last-run-duration: 0.6s
---

## Description

Full scenario test for FT-052 IAM-gated SPARQL endpoint per Product.
Exercises four access-control scenarios against `/products/{name}/graph`:

1. **No token** → HTTP 401 (unauthenticated)
2. **Invalid / expired token** → HTTP 401 (bad credentials)
3. **Valid token, wrong audience** → HTTP 403 (unauthorized for this product)
4. **Valid token, correct audience** → request proceeds (not 401/403)

Uses `LocalIdentityProvider` to issue real HMAC-signed tokens with
controlled audience claims.