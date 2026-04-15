---
id: TC-322
title: SPARQL auth exit — unauthenticated requests rejected with 401
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc322_sparql_auth_exit_unauthenticated_requests_rejected_with_401"
validates:
  features: [FT-052]
  adrs: []
phase: 1
last-run: 2026-04-15T13:00:07.742391449+00:00
last-run-duration: 0.5s
---

## Description

When IAM is configured on the cluster, any HTTP request to the product
SPARQL endpoint (`/products/{name}/graph`) that does **not** carry a valid
`Authorization: Bearer <token>` header must be rejected with:

- HTTP **401 Unauthorized**
- A `WWW-Authenticate: Bearer realm="picloud"` response header (RFC 6750)
- A JSON body containing an `error` field that mentions "authentication"

This is the minimum exit criterion for FT-052 — the SPARQL endpoint must
never leak graph data to unauthenticated callers.