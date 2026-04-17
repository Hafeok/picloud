---
id: TC-334
title: Data CLI exit — data-product list and data-domain list work
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc334_data_cli_exit_data_product_list_and_data_domain_list_work"
validates:
  features: [FT-073]
  adrs: [ADR-056]
phase: 3
last-run: 2026-04-17T10:00:02.229283067+00:00
last-run-duration: 0.6s
---

## Description

Exit-criteria test validating that `picloud data-product list` and
`picloud data-domain list` handle all edge cases correctly:

- **Empty result sets** — both commands return friendly placeholder messages.
- **Multiple entries** — data domains with different sensitivity levels and
  data products with mixed lifecycle statuses are all listed.
- **Nested JSON** — `results.bindings` format is handled alongside flat
  `bindings` format.
- **Missing body structure** — malformed or null JSON returns empty lists.
- **IRI extraction** — steward, product, and domain IRIs are resolved to
  short names; plain names pass through unchanged.
- **URL encoding** — SPARQL queries encode correctly for query parameters.
- **Table formatting** — header, separator, and data rows are present.