---
id: TC-177
title: dns_txt_records
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-052
phase: 1
runner: picloud-test
runner-args: "dns-txt-records"
---

query `photo-app.picloud.local` TXT record. Assert it contains the ontology IRI and product version.