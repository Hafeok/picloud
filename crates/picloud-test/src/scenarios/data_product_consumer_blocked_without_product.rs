//! Scenario: data_product_consumer_blocked_without_product (ADR-056 / FT-069)
//!
//! Attempt to deploy a consumer Product with a `dataProducts` dependency on a
//! data product that does not exist. Assert `resource apply` fails with a
//! `DataProductNotFound` error. Assert the consumer Product is not deployed.
//!
//! The fully-integrated HTTP-layer test lives in
//! `picloud-http/tests/ft069_data_product_consumer_validation.rs` — this stub
//! remains as a placeholder for the optional live-cluster variant that runs
//! against a real `picloud-server` instance.

#[ignore = "live-cluster variant — the integrated test runs under picloud-http"]
#[test]
fn data_product_consumer_blocked_without_product_live_cluster() {
    // Placeholder for the live-cluster variant. The scenario is covered end to
    // end by the integration test in picloud-http which exercises the same
    // HTTP apply handler and enforces the same invariant.
}
