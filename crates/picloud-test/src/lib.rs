//! Library surface of `picloud-test`.
//!
//! The crate is primarily a binary (the integration-test runner driven from
//! `src/main.rs`). Exposing the same modules through a library root lets
//! integration tests under `crates/picloud-test/tests/` reuse the harness
//! helpers (e.g. `apply_product_and_wait`) without duplicating the logic
//! they are trying to guard.
//!
//! TC-353 is the first consumer: it spins up an in-process `picloud-server`,
//! pre-seeds the event log with a product whose name matches the harness's
//! default test-scope identifier, and calls the same seeding helper used by
//! the failing `multi-node-persistence` and `multi-node-replication`
//! scenarios to assert no 409 Conflict is surfaced.

pub mod config;
pub mod harness;
pub mod invariants;
pub mod probes;
pub mod scenarios;
pub mod upgrade;
