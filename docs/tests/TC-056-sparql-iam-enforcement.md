---
id: TC-056
title: sparql_iam_enforcement
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-019
phase: 1
runner: scripts/run-tc.sh
runner-args: "sparql-iam-enforcement"
last-run: 2026-04-17T13:58:22.735189315+00:00
last-run-duration: 0.0s
---

query the product SPARQL endpoint with no token (assert 401), with an expired token (assert 401), with a token for a different product (assert 403), and with a valid scoped token (assert 200).