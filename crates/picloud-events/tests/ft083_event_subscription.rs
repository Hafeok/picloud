/// FT-083 Integration Tests — Event Subscription Resource Type
///
/// Covers:
///   TC-282: Event subscription resource type receives filtered events (scenario)
///   TC-339: Event subscription exit — subscription receives filtered events (exit-criteria)
///
/// Verifies that the event-subscription resource type correctly filters events
/// delivered to subscribers. An EventSubscription declares a source product and
/// event_type — the platform EventLog must deliver only matching events to
/// the subscription's handler. Events from other products or of different types
/// must NOT be delivered.

use std::sync::Arc;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::resources::{EventSubscription, ResourceMeta, ResourceStatus};
use picloud_domain::traits::{EventFilter, EventLog};
use picloud_events::InMemoryEventLog;
use uuid::Uuid;

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

fn make_event(
    event_type: &str,
    product: Option<&str>,
    correlation_id: Uuid,
    payload: serde_json::Value,
) -> EventEnvelope {
    let ib = iri_builder();
    EventEnvelope::new(
        ib.event_schema(event_type, 1),
        event_type,
        ResourceIri::new("https://picloud.local/test").unwrap(),
        product.map(|s| s.to_string()),
        correlation_id,
        payload,
    )
}

/// Helper: build an EventSubscription resource struct.
fn build_subscription(
    subscriber_product: &str,
    subscription_name: &str,
    source_product: &str,
    event_type: &str,
    handler_name: &str,
) -> EventSubscription {
    let ib = iri_builder();
    EventSubscription {
        meta: ResourceMeta {
            iri: ib.resource(subscriber_product, "event-subscriptions", subscription_name),
            resource_type: "EventSubscription".to_string(),
            name: subscription_name.to_string(),
            product: Some(subscriber_product.to_string()),
            status: ResourceStatus::Declared,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            tags: vec![],
        },
        source_product_iri: ib.product(source_product),
        event_type: event_type.to_string(),
        handler_name: handler_name.to_string(),
    }
}

// ============================================================================
// TC-282 — Event subscription resource type receives filtered events
// ============================================================================
/// Scenario test for FT-083: Exercises the full event subscription lifecycle.
///
/// 1. Declare an EventSubscription resource for a specific source product and event_type
/// 2. Subscribe to the event log with a filter matching the subscription's criteria
/// 3. Emit a mix of events — some matching, some from other products, some of
///    different types
/// 4. Assert the subscriber receives ONLY the events matching the filter
/// 5. Verify the EventSubscription resource transitions through its lifecycle
#[tokio::test]
async fn tc282_event_subscription_resource_type_receives_filtered_events() {
    let event_log = Arc::new(InMemoryEventLog::new());
    let _ib = iri_builder();

    // ---- Step 1: Build the EventSubscription resource ----
    // "fulfillment-service" subscribes to "OrderCreated" events from "order-service"
    let subscription = build_subscription(
        "fulfillment-service",
        "order-created-handler",
        "order-service",
        "OrderCreated",
        "order-processor",
    );

    // Verify the resource has expected fields
    assert_eq!(subscription.event_type, "OrderCreated");
    assert_eq!(
        subscription.source_product_iri.as_str(),
        "https://picloud.local/products/order-service"
    );
    assert_eq!(subscription.handler_name, "order-processor");
    assert_eq!(
        subscription.meta.iri.as_str(),
        "https://picloud.local/products/fulfillment-service/event-subscriptions/order-created-handler"
    );

    // ---- Step 2: Emit ResourceDeclared for the subscription ----
    let correlation_id = Uuid::new_v4();
    let declared_event = make_event(
        "ResourceDeclared",
        Some("fulfillment-service"),
        correlation_id,
        serde_json::json!({
            "resource_iri": subscription.meta.iri.as_str(),
            "resource_type": "EventSubscription",
            "product": "fulfillment-service",
            "name": "order-created-handler",
            "source_product": "order-service",
            "event_type": "OrderCreated",
            "handler": "order-processor",
        }),
    );
    event_log.append(declared_event).await.unwrap();

    // ---- Step 3: Subscribe with a filter matching the subscription criteria ----
    // This simulates what the platform does when an EventSubscription becomes Ready:
    // it creates a filtered subscription on the EventLog for the specified event_type
    // scoped to the source product.
    let filter = EventFilter {
        correlation_id: None,
        product: Some("order-service".to_string()),
        event_types: vec!["OrderCreated".to_string()],
    };
    let mut rx = event_log.subscribe(filter).await.unwrap();

    // ---- Step 4: Mark the subscription as Ready ----
    let ready_event = make_event(
        "ResourceReady",
        Some("fulfillment-service"),
        correlation_id,
        serde_json::json!({
            "resource_iri": subscription.meta.iri.as_str(),
        }),
    );
    event_log.append(ready_event).await.unwrap();

    // ---- Step 5: Emit a mix of events from different products and types ----

    // Event 1: OrderCreated from order-service — SHOULD match
    let matching_event_1 = make_event(
        "OrderCreated",
        Some("order-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "order_id": "ORD-001",
            "customer": "alice",
            "total": 99.99,
        }),
    );
    event_log.append(matching_event_1).await.unwrap();

    // Event 2: OrderShipped from order-service — should NOT match (wrong event_type)
    let non_matching_type = make_event(
        "OrderShipped",
        Some("order-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "order_id": "ORD-001",
            "tracking": "TRACK-123",
        }),
    );
    event_log.append(non_matching_type).await.unwrap();

    // Event 3: OrderCreated from inventory-service — should NOT match (wrong product)
    let non_matching_product = make_event(
        "OrderCreated",
        Some("inventory-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "order_id": "ORD-002",
            "customer": "bob",
        }),
    );
    event_log.append(non_matching_product).await.unwrap();

    // Event 4: Platform event (no product) — should NOT match
    let platform_event = make_event(
        "NodeJoined",
        None,
        Uuid::new_v4(),
        serde_json::json!({
            "node_id": Uuid::new_v4().to_string(),
            "node_name": "pi-03",
        }),
    );
    event_log.append(platform_event).await.unwrap();

    // Event 5: Another OrderCreated from order-service — SHOULD match
    let matching_event_2 = make_event(
        "OrderCreated",
        Some("order-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "order_id": "ORD-003",
            "customer": "carol",
            "total": 49.50,
        }),
    );
    event_log.append(matching_event_2).await.unwrap();

    // ---- Step 6: Verify the subscriber received ONLY matching events ----
    let mut received = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
            Ok(Ok(event)) => {
                received.push(event);
            }
            _ => break,
        }
    }

    // Should receive exactly 2 events (both OrderCreated from order-service)
    assert_eq!(
        received.len(),
        2,
        "Subscriber should receive exactly 2 matching events, got {}",
        received.len()
    );

    // Verify all received events are OrderCreated
    for event in &received {
        assert_eq!(
            event.event_type, "OrderCreated",
            "All received events should be OrderCreated, got {}",
            event.event_type
        );
        assert_eq!(
            event.product.as_deref(),
            Some("order-service"),
            "All received events should be from order-service"
        );
    }

    // Verify the specific payloads
    assert_eq!(
        received[0].payload["order_id"], "ORD-001",
        "First event should be ORD-001"
    );
    assert_eq!(
        received[1].payload["order_id"], "ORD-003",
        "Second event should be ORD-003"
    );

    // ---- Step 7: Verify lifecycle events are in the log ----
    let all_events = event_log.events_since(0).await;
    let lifecycle_events: Vec<&EventEnvelope> = all_events
        .iter()
        .filter(|e| e.correlation_id == correlation_id)
        .collect();

    assert_eq!(
        lifecycle_events.len(),
        2,
        "Should have ResourceDeclared + ResourceReady lifecycle events"
    );
    assert_eq!(lifecycle_events[0].event_type, "ResourceDeclared");
    assert_eq!(lifecycle_events[1].event_type, "ResourceReady");

    // ---- Step 8: Verify total event count ----
    // 2 lifecycle + 5 domain events = 7 total
    assert_eq!(
        all_events.len(),
        7,
        "Total log should have 7 events (2 lifecycle + 5 domain)"
    );
}

// ============================================================================
// TC-339 — Event subscription exit — subscription receives filtered events
// ============================================================================
/// Exit-criteria test for FT-083: Minimum bar — an event subscription with a
/// declared event_type filter receives only matching events and rejects
/// non-matching ones. This is the gatekeeper for the feature.
#[tokio::test]
async fn tc339_event_subscription_exit_subscription_receives_filtered_events() {
    let event_log = Arc::new(InMemoryEventLog::new());
    let _ib = iri_builder();

    // ---- Verify EventSubscription resource type has required fields ----
    let subscription = build_subscription(
        "analytics-service",
        "user-events-handler",
        "user-service",
        "UserCreated",
        "analytics-worker",
    );

    // Required fields per ADR-022
    assert!(
        !subscription.event_type.is_empty(),
        "event_type must be set"
    );
    assert!(
        !subscription.source_product_iri.as_str().is_empty(),
        "source_product_iri must be set"
    );
    assert!(
        !subscription.handler_name.is_empty(),
        "handler_name must be set"
    );
    assert_eq!(
        subscription.meta.resource_type, "EventSubscription",
        "resource_type must be EventSubscription"
    );
    assert_eq!(
        subscription.meta.status,
        ResourceStatus::Declared,
        "initial status must be Declared"
    );

    // ---- Create a filtered subscription matching the EventSubscription's criteria ----
    let filter = EventFilter {
        correlation_id: None,
        product: Some("user-service".to_string()),
        event_types: vec!["UserCreated".to_string()],
    };
    let mut rx = event_log.subscribe(filter).await.unwrap();

    // ---- Emit matching event: UserCreated from user-service ----
    let matching = make_event(
        "UserCreated",
        Some("user-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "user_id": "usr-001",
            "email": "alice@example.com",
        }),
    );
    event_log.append(matching).await.unwrap();

    // ---- Emit non-matching events ----
    // Wrong event_type
    let wrong_type = make_event(
        "UserDeleted",
        Some("user-service"),
        Uuid::new_v4(),
        serde_json::json!({ "user_id": "usr-002" }),
    );
    event_log.append(wrong_type).await.unwrap();

    // Wrong product
    let wrong_product = make_event(
        "UserCreated",
        Some("billing-service"),
        Uuid::new_v4(),
        serde_json::json!({ "user_id": "usr-003" }),
    );
    event_log.append(wrong_product).await.unwrap();

    // No product scope
    let no_product = make_event(
        "UserCreated",
        None,
        Uuid::new_v4(),
        serde_json::json!({ "user_id": "usr-004" }),
    );
    event_log.append(no_product).await.unwrap();

    // ---- Emit a second matching event ----
    let matching_2 = make_event(
        "UserCreated",
        Some("user-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "user_id": "usr-005",
            "email": "bob@example.com",
        }),
    );
    event_log.append(matching_2).await.unwrap();

    // ---- Collect filtered events ----
    let mut received = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
            Ok(Ok(event)) => received.push(event),
            _ => break,
        }
    }

    // ---- EXIT CRITERIA: exactly the matching events are delivered ----
    assert_eq!(
        received.len(),
        2,
        "Filtered subscription must deliver exactly 2 matching events, got {}",
        received.len()
    );

    // Both must be UserCreated from user-service
    for event in &received {
        assert_eq!(event.event_type, "UserCreated");
        assert_eq!(event.product.as_deref(), Some("user-service"));
    }

    // Verify payload integrity
    assert_eq!(received[0].payload["user_id"], "usr-001");
    assert_eq!(received[1].payload["user_id"], "usr-005");

    // ---- Verify the full log has all events (nothing lost) ----
    let all_events = event_log.events_since(0).await;
    assert_eq!(
        all_events.len(),
        5,
        "All 5 events should be in the log regardless of filtering"
    );
}
