---
id: TC-177
title: dns_txt_records
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-052
phase: 1
runner: cargo-test
runner-args: "tc177_dns_txt_records"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

query `photo-app.picloud.local` TXT record. Assert it contains the ontology IRI and product version.