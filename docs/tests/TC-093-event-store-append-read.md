---
id: TC-093
title: event_store_append_read
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-032
phase: 1
runner: scripts/run-tc.sh
runner-args: "event-store-append-read"
last-run: 2026-04-13T21:37:33.242635225+00:00
---

declare an `event-store` resource with a Photo aggregate. Append 10 `PhotoCreated` events. Read the aggregate stream. Assert all 10 events returned in order with correct payloads.