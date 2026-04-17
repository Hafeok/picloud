---
id: TC-277
title: picloud data-product list and data-domain list return expected entries
type: scenario
status: passing
runner: cargo-test
runner-args: "tc277_picloud_data_product_list_and_data_domain_list_return_expected_entries"
validates:
  features: [FT-073]
  adrs: [ADR-056]
phase: 3
last-run: 2026-04-17T10:00:02.229283067+00:00
last-run-duration: 0.5s
---

## Description

Verifies that the `picloud data-product list` and `picloud data-domain list`
CLI commands produce the expected output for representative inputs. Tests:

1. **SPARQL query construction** — the data-domain query selects DataDomain type
   with name, steward, and sensitivity fields; the data-product query selects
   DataProduct type with name, product, domain, version, and status fields;
   both queries include ORDER BY clauses.
2. **Response parsing** — SPARQL JSON bindings are parsed into structured rows
   with IRI paths resolved to short names (steward, product, domain).
3. **Table formatting** — output includes all required column headers and
   correctly renders row data.