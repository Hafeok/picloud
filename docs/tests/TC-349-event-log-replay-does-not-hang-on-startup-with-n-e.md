---
id: TC-349
title: Event log replay does not hang on startup with N existing events
type: scenario
status: passing
runner: cargo-test
runner-args: "tc349_event_log_replay_does_not_hang_on_startup"
validates:
  features: [FT-015, FT-093]
  adrs: []
phase: 4
last-run: 2026-04-17T14:26:56.569025843+00:00
last-run-duration: 1.4s
---

## Description

Regression guard against a startup hang observed in release builds on the Pi 5
cluster (node3, 2026-04-17).

**Observed symptom:** with `/var/lib/picloud/events` containing ~387 NDJSON
events from prior runs, `picloud-server` logs `Cluster membership started`,
then spins at 100% CPU indefinitely, never binding port 7443. A freshly wiped
event log on the same binary starts cleanly and reaches `PiCloud server
ready` within ~5s.

**Invariant under test:** server startup must reach HTTP listener within a
bounded time for any well-formed event log of N entries (for N up to at least
10000). Startup must not spin, infinite-loop, or hang on replay.

**Shape of the Rust test:**

1. Construct a PersistentEventLog pre-populated with N synthetic
   EventEnvelope entries (various event types including NodeJoined,
   LeaderElected, MetricRecorded).
2. Open it via `PersistentEventLog::open`, measure wall-clock time.
3. Assert open completes in < 2s for N in {0, 100, 1000, 10000}.
4. (Integration variant) spawn `picloud-server` against the pre-populated
   directory with a test timeout; assert the HTTP port binds within 15s.

**Reproduction:** see `events.bak.1776426409` / `raft.bak.1776426409` on
node3 — restoring those directories reproduces the hang.