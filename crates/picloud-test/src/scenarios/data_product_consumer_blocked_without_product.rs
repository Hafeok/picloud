//! Scenario: data_product_consumer_blocked_without_product (ADR-056)
//!
//! Attempt to deploy a consumer Product with a `dataProducts` dependency on a
//! data product that does not exist. Assert `resource apply` fails with a
//! `DataProductNotFound` error. Assert the consumer Product is not deployed.
//! Requires a live cluster — stub created for catalogue sync.

#[ignore = "requires live cluster with capability/data product support"]
#[test]
fn data_product_consumer_blocked_without_product() {
    // TODO: implement per ADR-056 test coverage section
}
