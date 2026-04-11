//! Scenario: cross_product_internal_graph_blocked (ADR-056)
//!
//! Authenticate as a consumer workload identity. Attempt a SPARQL query directly
//! against another Product's internal graph. Assert `403 Forbidden`. Assert a
//! `UnauthorisedGraphAccess` event is emitted. Repeat with platform-admin
//! identity and assert `200 OK` (admin exemption verified).
//! Requires a live cluster — stub created for catalogue sync.

#[ignore = "requires live cluster with capability/data product support"]
#[test]
fn cross_product_internal_graph_blocked() {
    // TODO: implement per ADR-056 test coverage section
}
