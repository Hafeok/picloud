---
id: TC-208
title: data-domain
type: scenario
status: passing
validates:
  features:
  - FT-065
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_domain_declaration
last-run: 2026-04-17T09:03:38.439227156+00:00
last-run-duration: 0.7s
---

Declare a `data-domain` resource with steward, sensitivity, and description fields. Assert the domain appears in the cluster RDF graph with correct triples. Assert it has a dereferenceable IRI. Assert a second `data-domain` with the same name is rejected as a duplicate. Assert the domain cannot be deleted while a data product is assigned to it.

This test validates the data-domain lifecycle end-to-end, complementing TC-196 (declaration event) and TC-205 (deletion guard).