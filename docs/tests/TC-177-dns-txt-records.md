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
---

query `photo-app.picloud.local` TXT record. Assert it contains the ontology IRI and product version.