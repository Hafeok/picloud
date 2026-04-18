---
id: TC-357
title: Event store read with limit returns the most recent events per product
type: scenario
status: passing
runner: cargo-test
runner-args: "tc357_event_store_read_limit_returns_latest"
validates:
  features:
  - FT-008
  - FT-078
  adrs:
  - ADR-032
phase: 3
last-run: 2026-04-18T15:53:49.091301469+00:00
last-run-duration: 0.7s
---

## Description

Regression guard for the `event-store-append-read` E2E scenario, which
failed on the Pi 5 cluster (2026-04-18) with
`event evt-<uuid> not found in event store read response` after 2011ms.

TC-351 (in-memory, 50 iterations, `limit=100`) passes cleanly because each
iteration starts with an empty `InMemoryEventLog` — so the newly-appended
event is the only product event present and it naturally fits inside the
`limit`. On the live cluster the event log already contains hundreds of
historical product events from prior runs, and the E2E scenario uses
`limit=10`. The just-appended event therefore falls past the truncation
boundary and is never returned.

The root cause is in `crates/picloud-http/src/implementation.rs` around
line 3560 (`handle_event_store_api_read`):

```rust
let all_events = event_log.events_since(0).await;
let product_events: Vec<_> = all_events
    .into_iter()
    .filter(|e| e.product.as_deref() == Some(product.as_str()))
    .take(limit)
    .collect();
```

`take(limit)` yields the **oldest** N product events, not the most
recent. Clients asking `events?limit=10` after appending receive the
first ten events the store ever saw, which is the opposite of what any
event-log consumer expects.

**Invariant under test:** with a persistent event log that already
contains more than `limit` events for a Product, appending a new event and
reading `GET /api/event-store/:product/events?limit=N` MUST return the
new event within the slice (most-recent-first semantics, or any
well-defined semantics that guarantees visibility of a just-appended
event).

**Shape of the Rust test:**

1. Build a router backed by an `EventLog` pre-seeded with at least
   `2 * limit` `PhotoCreated` events for `picloud-test` spread across
   multiple aggregate IDs, so the store is already "fat".
2. Append a new `PhotoCreated` event with a unique `eventId`.
3. GET `/api/event-store/picloud-test/events?limit=10`.
4. Assert the newly-appended `eventId` is present in the response. If the
   endpoint guarantees most-recent-first, also assert it appears at index
   0.
5. Repeat with `limit=1` to make the boundary failure explicit: a single
   append followed by a one-element read MUST yield the just-appended
   event, not the very first event ever recorded.

This test will red until the read handler changes its ordering/truncation
strategy (or grows a `since=` / `after_event_id=` filter and the scenario
is updated to use it).