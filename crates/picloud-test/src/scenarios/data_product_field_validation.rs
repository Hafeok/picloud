//! Scenario: data_product_field_validation (ADR-056)
//!
//! Attempt to declare a `data-product` missing each mandatory field in turn
//! (`triggers`, `maxAge`, `domain`, `shapes`/`ontology`). Assert each attempt is
//! rejected at `resource apply` with a specific validation error. Assert no
//! partial resource state is created in the cluster graph.
//! Requires a live cluster — stub created for catalogue sync.

#[ignore = "requires live cluster with capability/data product support"]
#[test]
fn data_product_field_validation() {
    // TODO: implement per ADR-056 test coverage section
}
