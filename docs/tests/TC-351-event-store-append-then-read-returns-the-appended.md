---
id: TC-351
title: Event store append then read returns the appended event within bounded latency
type: scenario
status: passing
validates:
  features:
  - FT-008
  - FT-078
  adrs: []
phase: 4
runner: cargo-test
runner-args: tc351_event_store_append_then_read_returns_event
last-run: 2026-04-18T14:11:22.107011310+00:00
last-run-duration: 2.3s
---

## Description

Regression guard for the `event-store-append-read` E2E scenario, which
failed on the Pi 5 cluster (2026-04-17) with `event evt-<uuid> not found in
event store read response` after a 2010ms duration.

**Invariant under test:** an event appended to a Product event store via
`POST /api/events` must be observable through the read API within a bounded
wait (e.g. 5s, longer than any plausible Raft commit + projection lag).
The 2010ms miss observed suggests either (a) the read API is not waiting
for projection, or (b) the Raft commit path is not honouring strong-read
semantics for the just-written event.

**Shape of the Rust test:**

1. Start a single-node cluster with a Product + event-store resource.
2. Append a uniquely-identified event envelope via the HTTP API.
3. Immediately issue a read-since / get-by-id request for that event,
   polling up to 5s with exponential backoff.
4. Assert the event is returned with the correct payload and schema IRI.
5. Repeat 50 times in sequence to flush out flakiness; all must pass.

If the timing is intentionally eventually-consistent, the test must codify
the documented upper bound rather than relying on hope.