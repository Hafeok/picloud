//! Platform-managed event routing between Products (ADR-022, FT-084).
//!
//! The `PlatformEventRouter` watches for events that match active
//! `EventSubscription` resources and re-emits them as
//! `SubscriptionEventRouted` events scoped to the subscriber product.
//! This enables hermetic inter-product communication: Products never call
//! each other directly — the platform mediates all cross-product event flow.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::{EventEnvelope, SubscriptionEventRoutedPayload};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::resources::EventSubscription;
use picloud_domain::traits::{
    ActiveSubscriptionInfo, EventLog, EventRouter, RoutedEventInfo,
};
use tokio::sync::RwLock;

/// Platform-managed event router that forwards events between Products
/// based on active EventSubscription resources.
///
/// # How it works
///
/// 1. When a Product's EventSubscription resource becomes `Ready`, the
///    composition root calls `register_subscription()`.
/// 2. When any event is appended to the log, the composition root calls
///    `route_event()` to check it against all active subscriptions.
/// 3. For each matching subscription (source product + event_type), the
///    router creates a `SubscriptionEventRouted` event scoped to the
///    subscriber product and appends it to the event log.
/// 4. The subscriber product picks up routed events via its product-scoped
///    event stream (e.g., `ProductEventStore`).
pub struct PlatformEventRouter {
    event_log: Arc<dyn EventLog>,
    cluster_domain: ClusterDomain,
    /// Active subscriptions indexed for fast lookup.
    subscriptions: RwLock<Vec<RegisteredSubscription>>,
}

/// Internal representation of a registered subscription.
#[derive(Debug, Clone)]
struct RegisteredSubscription {
    /// IRI of the EventSubscription resource
    subscription_iri: ResourceIri,
    /// Source product name (extracted from source_product_iri)
    source_product: String,
    /// Event type to match
    event_type: String,
    /// Subscriber product name
    subscriber_product: String,
    /// Handler name in the subscriber product
    handler_name: String,
}

impl PlatformEventRouter {
    /// Create a new platform event router.
    pub fn new(event_log: Arc<dyn EventLog>, cluster_domain: ClusterDomain) -> Self {
        Self {
            event_log,
            cluster_domain,
            subscriptions: RwLock::new(Vec::new()),
        }
    }

    /// Extract the product name from a product IRI.
    /// e.g., "https://picloud.local/products/order-service" -> "order-service"
    fn extract_product_name(product_iri: &ResourceIri) -> String {
        product_iri
            .as_str()
            .rsplit("/products/")
            .next()
            .and_then(|s| s.split('/').next())
            .unwrap_or(product_iri.as_str())
            .to_string()
    }
}

#[async_trait]
impl EventRouter for PlatformEventRouter {
    async fn register_subscription(&self, subscription: &EventSubscription) -> Result<()> {
        let source_product = Self::extract_product_name(&subscription.source_product_iri);
        let subscriber_product = subscription
            .meta
            .product
            .clone()
            .ok_or_else(|| PiCloudError::EventRoutingFailed {
                subscription: subscription.meta.iri.as_str().to_string(),
                reason: "EventSubscription must belong to a product".to_string(),
            })?;

        let registered = RegisteredSubscription {
            subscription_iri: subscription.meta.iri.clone(),
            source_product: source_product.clone(),
            event_type: subscription.event_type.clone(),
            subscriber_product: subscriber_product.clone(),
            handler_name: subscription.handler_name.clone(),
        };

        let mut subs = self.subscriptions.write().await;

        // Prevent duplicate registrations
        if subs.iter().any(|s| s.subscription_iri.as_str() == subscription.meta.iri.as_str()) {
            debug!(
                subscription = %subscription.meta.iri.as_str(),
                "Subscription already registered, skipping"
            );
            return Ok(());
        }

        debug!(
            subscription = %subscription.meta.iri.as_str(),
            source = %source_product,
            event_type = %subscription.event_type,
            subscriber = %subscriber_product,
            handler = %subscription.handler_name,
            "Registered event subscription for cross-product routing"
        );

        subs.push(registered);
        Ok(())
    }

    async fn unregister_subscription(&self, subscription_iri: &ResourceIri) -> Result<()> {
        let mut subs = self.subscriptions.write().await;
        let before = subs.len();
        subs.retain(|s| s.subscription_iri.as_str() != subscription_iri.as_str());
        let removed = before - subs.len();

        if removed == 0 {
            return Err(PiCloudError::ResourceNotFound {
                iri: subscription_iri.as_str().to_string(),
            });
        }

        debug!(
            subscription = %subscription_iri.as_str(),
            "Unregistered event subscription"
        );
        Ok(())
    }

    async fn route_event(&self, event: &EventEnvelope) -> Result<Vec<RoutedEventInfo>> {
        let subs = self.subscriptions.read().await;
        let mut routed = Vec::new();

        // Match against the event's product scope and type
        let event_product = match &event.product {
            Some(p) => p.as_str(),
            None => return Ok(routed), // Platform events are not routed via subscriptions
        };

        for sub in subs.iter() {
            if sub.source_product == event_product && sub.event_type == event.event_type {
                let iri_builder = IriBuilder::new(self.cluster_domain.clone());

                let routed_payload = SubscriptionEventRoutedPayload {
                    subscription_iri: sub.subscription_iri.clone(),
                    source_product: sub.source_product.clone(),
                    subscriber_product: sub.subscriber_product.clone(),
                    handler_name: sub.handler_name.clone(),
                    original_event_type: event.event_type.clone(),
                    original_event_id: event.id,
                    original_payload: event.payload.clone(),
                };

                let routed_event = EventEnvelope::new(
                    iri_builder.event_schema("SubscriptionEventRouted", 1),
                    "SubscriptionEventRouted",
                    event.source.clone(),
                    Some(sub.subscriber_product.clone()),
                    event.correlation_id,
                    serde_json::to_value(&routed_payload).map_err(|e| {
                        PiCloudError::EventRoutingFailed {
                            subscription: sub.subscription_iri.as_str().to_string(),
                            reason: format!("failed to serialize routed payload: {e}"),
                        }
                    })?,
                );

                debug!(
                    subscription = %sub.subscription_iri.as_str(),
                    source = %sub.source_product,
                    subscriber = %sub.subscriber_product,
                    handler = %sub.handler_name,
                    original_event = %event.event_type,
                    "Routing event to subscriber product"
                );

                self.event_log.append(routed_event).await.map_err(|e| {
                    PiCloudError::EventRoutingFailed {
                        subscription: sub.subscription_iri.as_str().to_string(),
                        reason: format!("failed to append routed event: {e}"),
                    }
                })?;

                routed.push(RoutedEventInfo {
                    subscription_iri: sub.subscription_iri.clone(),
                    subscriber_product: sub.subscriber_product.clone(),
                    handler_name: sub.handler_name.clone(),
                });
            }
        }

        Ok(routed)
    }

    async fn active_subscriptions(&self) -> Result<Vec<ActiveSubscriptionInfo>> {
        let subs = self.subscriptions.read().await;
        Ok(subs
            .iter()
            .map(|s| ActiveSubscriptionInfo {
                subscription_iri: s.subscription_iri.clone(),
                source_product: s.source_product.clone(),
                event_type: s.event_type.clone(),
                subscriber_product: s.subscriber_product.clone(),
                handler_name: s.handler_name.clone(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryEventLog;
    use picloud_domain::iri::ClusterDomain;
    use picloud_domain::resources::{ResourceMeta, ResourceStatus};

    fn iri_builder() -> IriBuilder {
        IriBuilder::new(ClusterDomain::default())
    }

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

    #[tokio::test]
    async fn register_and_route_event() {
        let event_log = Arc::new(InMemoryEventLog::new());
        let router = PlatformEventRouter::new(
            event_log.clone(),
            ClusterDomain::default(),
        );

        let sub = build_subscription(
            "fulfillment",
            "order-handler",
            "orders",
            "OrderCreated",
            "processor",
        );
        router.register_subscription(&sub).await.unwrap();

        let ib = iri_builder();
        let event = EventEnvelope::new(
            ib.event_schema("OrderCreated", 1),
            "OrderCreated",
            ResourceIri::new("https://picloud.local/test").unwrap(),
            Some("orders".to_string()),
            uuid::Uuid::new_v4(),
            serde_json::json!({"order_id": "ORD-001"}),
        );

        let routed = router.route_event(&event).await.unwrap();
        assert_eq!(routed.len(), 1);
        assert_eq!(routed[0].subscriber_product, "fulfillment");
        assert_eq!(routed[0].handler_name, "processor");

        // Verify the routed event was appended to the log
        let all = event_log.events_since(0).await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].event_type, "SubscriptionEventRouted");
        assert_eq!(all[0].product.as_deref(), Some("fulfillment"));
    }

    #[tokio::test]
    async fn no_routing_for_non_matching_events() {
        let event_log = Arc::new(InMemoryEventLog::new());
        let router = PlatformEventRouter::new(
            event_log.clone(),
            ClusterDomain::default(),
        );

        let sub = build_subscription(
            "fulfillment",
            "order-handler",
            "orders",
            "OrderCreated",
            "processor",
        );
        router.register_subscription(&sub).await.unwrap();

        let ib = iri_builder();

        // Wrong event type
        let event = EventEnvelope::new(
            ib.event_schema("OrderShipped", 1),
            "OrderShipped",
            ResourceIri::new("https://picloud.local/test").unwrap(),
            Some("orders".to_string()),
            uuid::Uuid::new_v4(),
            serde_json::json!({}),
        );
        let routed = router.route_event(&event).await.unwrap();
        assert!(routed.is_empty());

        // Wrong product
        let event = EventEnvelope::new(
            ib.event_schema("OrderCreated", 1),
            "OrderCreated",
            ResourceIri::new("https://picloud.local/test").unwrap(),
            Some("inventory".to_string()),
            uuid::Uuid::new_v4(),
            serde_json::json!({}),
        );
        let routed = router.route_event(&event).await.unwrap();
        assert!(routed.is_empty());

        // Platform event (no product)
        let event = EventEnvelope::new(
            ib.event_schema("NodeJoined", 1),
            "NodeJoined",
            ResourceIri::new("https://picloud.local/test").unwrap(),
            None,
            uuid::Uuid::new_v4(),
            serde_json::json!({}),
        );
        let routed = router.route_event(&event).await.unwrap();
        assert!(routed.is_empty());

        // No events should be in the log
        assert!(event_log.events_since(0).await.is_empty());
    }

    #[tokio::test]
    async fn unregister_stops_routing() {
        let event_log = Arc::new(InMemoryEventLog::new());
        let router = PlatformEventRouter::new(
            event_log.clone(),
            ClusterDomain::default(),
        );

        let sub = build_subscription(
            "fulfillment",
            "order-handler",
            "orders",
            "OrderCreated",
            "processor",
        );
        router.register_subscription(&sub).await.unwrap();

        // Unregister
        router.unregister_subscription(&sub.meta.iri).await.unwrap();

        let ib = iri_builder();
        let event = EventEnvelope::new(
            ib.event_schema("OrderCreated", 1),
            "OrderCreated",
            ResourceIri::new("https://picloud.local/test").unwrap(),
            Some("orders".to_string()),
            uuid::Uuid::new_v4(),
            serde_json::json!({"order_id": "ORD-001"}),
        );

        let routed = router.route_event(&event).await.unwrap();
        assert!(routed.is_empty());
        assert!(event_log.events_since(0).await.is_empty());
    }
}
