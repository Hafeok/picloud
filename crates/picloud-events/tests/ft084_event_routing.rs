/// FT-084 Integration Tests — Platform-managed event routing between Products
///
/// Covers:
///   TC-283: Platform routes events between products via subscription (scenario)
///   TC-340: Event routing exit — platform routes events between products (exit-criteria)
///
/// Verifies that the platform EventRouter correctly routes events between
/// Products based on active EventSubscription resources. When Product A emits
/// an event matching Product B's EventSubscription (source_product + event_type),
/// the router creates a SubscriptionEventRouted event scoped to Product B,
/// preserving the original payload and tracking the subscription + handler.

use std::sync::Arc;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::resources::{EventSubscription, ResourceMeta, ResourceStatus};
use picloud_domain::traits::{EventFilter, EventLog, EventRouter};
use picloud_events::{InMemoryEventLog, PlatformEventRouter};
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
// TC-283 — Platform routes events between products via subscription
// ============================================================================
/// Scenario test for FT-084: Exercises the full platform-managed event routing
/// lifecycle between two Products.
///
/// 1. Create an EventSubscription: fulfillment-service subscribes to
///    "OrderCreated" events from order-service, targeting handler "order-processor"
/// 2. Register the subscription with the PlatformEventRouter
/// 3. Emit a mix of events — some matching, some from other products/types
/// 4. Route each event through the router
/// 5. Verify: only matching events produce SubscriptionEventRouted events
/// 6. Verify: routed events are scoped to the subscriber product
/// 7. Verify: original payload, event type, and handler are preserved
/// 8. Verify: the subscriber can receive routed events via product-scoped filter
/// 9. Verify: unregistering a subscription stops further routing
#[tokio::test]
async fn tc283_platform_routes_events_between_products_via_subscription() {
    let event_log = Arc::new(InMemoryEventLog::new());
    let router = PlatformEventRouter::new(event_log.clone(), ClusterDomain::default());
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

    // ---- Step 2: Register the subscription with the router ----
    router.register_subscription(&subscription).await.unwrap();

    // Verify the subscription is active
    let active = router.active_subscriptions().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].source_product, "order-service");
    assert_eq!(active[0].event_type, "OrderCreated");
    assert_eq!(active[0].subscriber_product, "fulfillment-service");
    assert_eq!(active[0].handler_name, "order-processor");

    // ---- Step 3: Set up a product-scoped subscriber for fulfillment-service ----
    // This simulates what the subscriber product's event store would do:
    // listen for SubscriptionEventRouted events scoped to its product.
    let subscriber_filter = EventFilter {
        correlation_id: None,
        product: Some("fulfillment-service".to_string()),
        event_types: vec!["SubscriptionEventRouted".to_string()],
    };
    let mut subscriber_rx = event_log.subscribe(subscriber_filter).await.unwrap();

    // ---- Step 4: Emit a mix of events and route them ----

    // Event 1: OrderCreated from order-service — SHOULD be routed
    let correlation_1 = Uuid::new_v4();
    let event_1 = make_event(
        "OrderCreated",
        Some("order-service"),
        correlation_1,
        serde_json::json!({
            "order_id": "ORD-001",
            "customer": "alice",
            "total": 99.99,
        }),
    );
    event_log.append(event_1.clone()).await.unwrap();
    let routed_1 = router.route_event(&event_1).await.unwrap();
    assert_eq!(routed_1.len(), 1, "Event 1 should be routed to 1 subscriber");
    assert_eq!(routed_1[0].subscriber_product, "fulfillment-service");
    assert_eq!(routed_1[0].handler_name, "order-processor");

    // Event 2: OrderShipped from order-service — should NOT be routed (wrong event_type)
    let event_2 = make_event(
        "OrderShipped",
        Some("order-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "order_id": "ORD-001",
            "tracking": "TRACK-123",
        }),
    );
    event_log.append(event_2.clone()).await.unwrap();
    let routed_2 = router.route_event(&event_2).await.unwrap();
    assert!(routed_2.is_empty(), "OrderShipped should NOT be routed");

    // Event 3: OrderCreated from inventory-service — should NOT be routed (wrong product)
    let event_3 = make_event(
        "OrderCreated",
        Some("inventory-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "order_id": "ORD-002",
            "customer": "bob",
        }),
    );
    event_log.append(event_3.clone()).await.unwrap();
    let routed_3 = router.route_event(&event_3).await.unwrap();
    assert!(routed_3.is_empty(), "Event from wrong product should NOT be routed");

    // Event 4: Platform event (no product) — should NOT be routed
    let event_4 = make_event(
        "NodeJoined",
        None,
        Uuid::new_v4(),
        serde_json::json!({
            "node_id": Uuid::new_v4().to_string(),
            "node_name": "pi-03",
        }),
    );
    event_log.append(event_4.clone()).await.unwrap();
    let routed_4 = router.route_event(&event_4).await.unwrap();
    assert!(routed_4.is_empty(), "Platform event should NOT be routed");

    // Event 5: Another OrderCreated from order-service — SHOULD be routed
    let correlation_5 = Uuid::new_v4();
    let event_5 = make_event(
        "OrderCreated",
        Some("order-service"),
        correlation_5,
        serde_json::json!({
            "order_id": "ORD-003",
            "customer": "carol",
            "total": 49.50,
        }),
    );
    event_log.append(event_5.clone()).await.unwrap();
    let routed_5 = router.route_event(&event_5).await.unwrap();
    assert_eq!(routed_5.len(), 1, "Event 5 should be routed");

    // ---- Step 5: Verify subscriber receives ONLY routed events ----
    let mut received = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_millis(200), subscriber_rx.recv()).await
        {
            Ok(Ok(event)) => received.push(event),
            _ => break,
        }
    }

    assert_eq!(
        received.len(),
        2,
        "Subscriber should receive exactly 2 SubscriptionEventRouted events, got {}",
        received.len()
    );

    // Verify all received events are SubscriptionEventRouted scoped to fulfillment-service
    for event in &received {
        assert_eq!(event.event_type, "SubscriptionEventRouted");
        assert_eq!(
            event.product.as_deref(),
            Some("fulfillment-service"),
            "Routed events must be scoped to the subscriber product"
        );
    }

    // ---- Step 6: Verify original payload is preserved in routed events ----
    let payload_1: serde_json::Value =
        serde_json::from_value(received[0].payload.clone()).unwrap();
    assert_eq!(payload_1["source_product"], "order-service");
    assert_eq!(payload_1["subscriber_product"], "fulfillment-service");
    assert_eq!(payload_1["handler_name"], "order-processor");
    assert_eq!(payload_1["original_event_type"], "OrderCreated");
    assert_eq!(payload_1["original_payload"]["order_id"], "ORD-001");
    assert_eq!(payload_1["original_payload"]["customer"], "alice");
    assert_eq!(payload_1["original_payload"]["total"], 99.99);

    let payload_2: serde_json::Value =
        serde_json::from_value(received[1].payload.clone()).unwrap();
    assert_eq!(payload_2["original_payload"]["order_id"], "ORD-003");
    assert_eq!(payload_2["original_payload"]["customer"], "carol");

    // ---- Step 7: Verify subscription IRI is tracked ----
    assert_eq!(
        payload_1["subscription_iri"],
        "https://picloud.local/products/fulfillment-service/event-subscriptions/order-created-handler"
    );

    // ---- Step 8: Verify the full event log ----
    let all_events = event_log.events_since(0).await;
    // 5 original events + 2 routed events = 7 total
    assert_eq!(
        all_events.len(),
        7,
        "Event log should have 7 events (5 original + 2 routed), got {}",
        all_events.len()
    );

    // Count routed events
    let routed_count = all_events
        .iter()
        .filter(|e| e.event_type == "SubscriptionEventRouted")
        .count();
    assert_eq!(routed_count, 2, "Should have 2 routed events in the log");

    // ---- Step 9: Unregister subscription and verify routing stops ----
    router
        .unregister_subscription(&subscription.meta.iri)
        .await
        .unwrap();

    let active = router.active_subscriptions().await.unwrap();
    assert!(active.is_empty(), "No active subscriptions after unregister");

    // Emit another matching event — should NOT be routed
    let event_6 = make_event(
        "OrderCreated",
        Some("order-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "order_id": "ORD-004",
            "customer": "dave",
        }),
    );
    event_log.append(event_6.clone()).await.unwrap();
    let routed_6 = router.route_event(&event_6).await.unwrap();
    assert!(
        routed_6.is_empty(),
        "No events should be routed after unregister"
    );

    // Event log should have 8 events (7 + 1 new original, no new routed)
    let final_events = event_log.events_since(0).await;
    assert_eq!(final_events.len(), 8);
    let final_routed_count = final_events
        .iter()
        .filter(|e| e.event_type == "SubscriptionEventRouted")
        .count();
    assert_eq!(
        final_routed_count, 2,
        "Still only 2 routed events (no new routing after unregister)"
    );

    // ---- Step 10: Multiple subscriptions from different products ----
    // Register a second subscriber for the same source event
    let sub2 = build_subscription(
        "analytics-service",
        "order-analytics",
        "order-service",
        "OrderCreated",
        "analytics-worker",
    );
    router.register_subscription(&subscription).await.unwrap(); // re-register first
    router.register_subscription(&sub2).await.unwrap();

    let active = router.active_subscriptions().await.unwrap();
    assert_eq!(active.len(), 2, "Should have 2 active subscriptions");

    // Emit an event — should be routed to BOTH subscribers
    let event_7 = make_event(
        "OrderCreated",
        Some("order-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "order_id": "ORD-005",
            "customer": "eve",
        }),
    );
    let routed_7 = router.route_event(&event_7).await.unwrap();
    assert_eq!(
        routed_7.len(),
        2,
        "Event should be routed to both subscribers"
    );

    let subscriber_products: Vec<&str> = routed_7.iter().map(|r| r.subscriber_product.as_str()).collect();
    assert!(subscriber_products.contains(&"fulfillment-service"));
    assert!(subscriber_products.contains(&"analytics-service"));
}

// ============================================================================
// TC-340 — Event routing exit — platform routes events between products
// ============================================================================
/// Exit-criteria test for FT-084: Minimum bar — the platform correctly routes
/// events from a source product to a subscriber product via EventSubscription.
/// This is the gatekeeper for the feature.
#[tokio::test]
async fn tc340_event_routing_exit_platform_routes_events_between_products() {
    let event_log = Arc::new(InMemoryEventLog::new());
    let router = PlatformEventRouter::new(event_log.clone(), ClusterDomain::default());
    let _ib = iri_builder();

    // ---- EXIT CRITERION 1: Register a valid EventSubscription ----
    let subscription = build_subscription(
        "notification-service",
        "user-signup-handler",
        "user-service",
        "UserCreated",
        "email-sender",
    );
    router.register_subscription(&subscription).await.unwrap();

    // Verify subscription is active
    let active = router.active_subscriptions().await.unwrap();
    assert_eq!(active.len(), 1);

    // ---- EXIT CRITERION 2: Matching event is routed ----
    let correlation = Uuid::new_v4();
    let source_event = make_event(
        "UserCreated",
        Some("user-service"),
        correlation,
        serde_json::json!({
            "user_id": "usr-001",
            "email": "alice@example.com",
            "name": "Alice",
        }),
    );

    let routed = router.route_event(&source_event).await.unwrap();
    assert_eq!(
        routed.len(),
        1,
        "Matching event must be routed to exactly 1 subscriber"
    );

    // ---- EXIT CRITERION 3: Routed event is scoped to subscriber product ----
    let all_events = event_log.events_since(0).await;
    assert_eq!(all_events.len(), 1, "One routed event should be in the log");

    let routed_event = &all_events[0];
    assert_eq!(routed_event.event_type, "SubscriptionEventRouted");
    assert_eq!(
        routed_event.product.as_deref(),
        Some("notification-service"),
        "Routed event must be scoped to the subscriber product"
    );

    // ---- EXIT CRITERION 4: Original payload is preserved verbatim ----
    let payload = &routed_event.payload;
    assert_eq!(payload["source_product"], "user-service");
    assert_eq!(payload["subscriber_product"], "notification-service");
    assert_eq!(payload["handler_name"], "email-sender");
    assert_eq!(payload["original_event_type"], "UserCreated");
    assert_eq!(payload["original_event_id"], source_event.id.to_string());
    assert_eq!(payload["original_payload"]["user_id"], "usr-001");
    assert_eq!(payload["original_payload"]["email"], "alice@example.com");
    assert_eq!(payload["original_payload"]["name"], "Alice");

    // ---- EXIT CRITERION 5: Non-matching events are NOT routed ----
    // Wrong event type from same product
    let wrong_type = make_event(
        "UserDeleted",
        Some("user-service"),
        Uuid::new_v4(),
        serde_json::json!({ "user_id": "usr-002" }),
    );
    let no_route_1 = router.route_event(&wrong_type).await.unwrap();
    assert!(no_route_1.is_empty(), "Wrong event type must not be routed");

    // Same event type from wrong product
    let wrong_product = make_event(
        "UserCreated",
        Some("billing-service"),
        Uuid::new_v4(),
        serde_json::json!({ "user_id": "usr-003" }),
    );
    let no_route_2 = router.route_event(&wrong_product).await.unwrap();
    assert!(no_route_2.is_empty(), "Wrong product must not be routed");

    // Platform event (no product scope)
    let platform = make_event(
        "UserCreated",
        None,
        Uuid::new_v4(),
        serde_json::json!({ "user_id": "usr-004" }),
    );
    let no_route_3 = router.route_event(&platform).await.unwrap();
    assert!(no_route_3.is_empty(), "Unscoped event must not be routed");

    // Verify no additional routed events were created
    let final_events = event_log.events_since(0).await;
    assert_eq!(
        final_events.len(),
        1,
        "Still only 1 routed event in the log (non-matching events produced no routing)"
    );

    // ---- EXIT CRITERION 6: Subscriber can receive routed events via product filter ----
    // Subscribe to fulfillment-service's event stream
    let sub_filter = EventFilter {
        correlation_id: None,
        product: Some("notification-service".to_string()),
        event_types: vec!["SubscriptionEventRouted".to_string()],
    };
    let mut rx = event_log.subscribe(sub_filter).await.unwrap();

    // Route another matching event
    let source_event_2 = make_event(
        "UserCreated",
        Some("user-service"),
        Uuid::new_v4(),
        serde_json::json!({
            "user_id": "usr-005",
            "email": "bob@example.com",
            "name": "Bob",
        }),
    );
    let routed_2 = router.route_event(&source_event_2).await.unwrap();
    assert_eq!(routed_2.len(), 1);

    // Subscriber should receive exactly 1 event
    let mut received = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
            Ok(Ok(event)) => received.push(event),
            _ => break,
        }
    }
    assert_eq!(
        received.len(),
        1,
        "Subscriber must receive exactly 1 routed event via product filter"
    );
    assert_eq!(received[0].event_type, "SubscriptionEventRouted");
    assert_eq!(
        received[0].product.as_deref(),
        Some("notification-service")
    );
    assert_eq!(received[0].payload["original_payload"]["user_id"], "usr-005");

    // ---- EXIT CRITERION 7: Subscription IRI is tracked in routed events ----
    assert_eq!(
        received[0].payload["subscription_iri"],
        subscription.meta.iri.as_str(),
        "Routed event must reference the subscription IRI"
    );
}
