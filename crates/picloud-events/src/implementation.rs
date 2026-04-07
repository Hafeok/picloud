//! Event log implementation for picloud-events.
//!
//! Provides an in-memory event log backed by a `Vec<EventEnvelope>` with
//! broadcast-based pub/sub and idempotency deduplication via `idempotency_key`.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info};

use picloud_domain::error::Result;
use picloud_domain::events::EventEnvelope;
use picloud_domain::traits::{EventFilter, EventLog};

/// Default capacity for the broadcast channel.
const DEFAULT_BROADCAST_CAPACITY: usize = 1024;

/// In-memory event log backed by a `Vec<EventEnvelope>` and a tokio broadcast
/// channel for real-time subscriptions.
pub struct InMemoryEventLog {
    /// The append-only event log.
    events: RwLock<Vec<EventEnvelope>>,
    /// Broadcast sender for pub/sub.
    sender: broadcast::Sender<EventEnvelope>,
    /// Set of seen idempotency keys for deduplication.
    seen_keys: RwLock<HashSet<String>>,
}

impl InMemoryEventLog {
    /// Create a new in-memory event log with the default broadcast capacity.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BROADCAST_CAPACITY)
    }

    /// Create a new in-memory event log with a custom broadcast channel capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            events: RwLock::new(Vec::new()),
            sender,
            seen_keys: RwLock::new(HashSet::new()),
        }
    }

    /// Return the number of events currently stored.
    pub async fn len(&self) -> usize {
        self.events.read().await.len()
    }

    /// Return whether the log is empty.
    pub async fn is_empty(&self) -> bool {
        self.events.read().await.is_empty()
    }
}

impl Default for InMemoryEventLog {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventLog for InMemoryEventLog {
    async fn append(&self, event: EventEnvelope) -> Result<()> {
        // Idempotency check: if the event has a key we've already seen, skip it.
        if let Some(ref key) = event.idempotency_key {
            let seen = self.seen_keys.read().await;
            if seen.contains(key) {
                debug!(
                    idempotency_key = %key,
                    event_id = %event.id,
                    "Duplicate idempotency key — skipping append"
                );
                return Ok(());
            }
        }

        // Record the idempotency key before appending.
        if let Some(ref key) = event.idempotency_key {
            self.seen_keys.write().await.insert(key.clone());
        }

        info!(
            event_id = %event.id,
            event_type = %event.event_type,
            correlation_id = %event.correlation_id,
            product = ?event.product,
            "Appending event to log"
        );

        // Broadcast to subscribers (ignore error when there are no receivers).
        let _ = self.sender.send(event.clone());

        // Append to the persistent log.
        self.events.write().await.push(event);

        Ok(())
    }

    async fn subscribe(
        &self,
        filter: EventFilter,
    ) -> Result<broadcast::Receiver<EventEnvelope>> {
        debug!(?filter, "Creating new event subscription");

        // If the filter is empty (no criteria), return the raw receiver.
        if filter.correlation_id.is_none()
            && filter.product.is_none()
            && filter.event_types.is_empty()
        {
            return Ok(self.sender.subscribe());
        }

        // For filtered subscriptions, we create a forwarding channel that only
        // sends events matching the filter.
        let (filtered_tx, filtered_rx) = broadcast::channel(DEFAULT_BROADCAST_CAPACITY);
        let mut upstream_rx = self.sender.subscribe();

        tokio::spawn(async move {
            loop {
                match upstream_rx.recv().await {
                    Ok(event) => {
                        if matches_filter(&event, &filter) {
                            // Stop forwarding if no receivers remain.
                            if filtered_tx.send(event).is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(skipped = n, "Filtered subscriber lagged — skipped events");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });

        Ok(filtered_rx)
    }
}

/// Check whether an event matches the given filter criteria.
fn matches_filter(event: &EventEnvelope, filter: &EventFilter) -> bool {
    if let Some(cid) = &filter.correlation_id {
        if event.correlation_id != *cid {
            return false;
        }
    }

    if let Some(product) = &filter.product {
        match &event.product {
            Some(ep) if ep == product => {}
            _ => return false,
        }
    }

    if !filter.event_types.is_empty() && !filter.event_types.contains(&event.event_type) {
        return false;
    }

    true
}

/// A per-product event store that scopes all operations to a single product.
///
/// This is a Phase 3 feature that wraps an `InMemoryEventLog` (or any `EventLog`
/// implementation) and automatically sets/filters the product field.
pub struct ProductEventStore {
    /// The product name this store is scoped to.
    product: String,
    /// The underlying event log.
    inner: Arc<dyn EventLog>,
}

impl ProductEventStore {
    /// Create a new product-scoped event store.
    pub fn new(product: impl Into<String>, inner: Arc<dyn EventLog>) -> Self {
        Self {
            product: product.into(),
            inner,
        }
    }

    /// Return the product name this store is scoped to.
    pub fn product(&self) -> &str {
        &self.product
    }
}

#[async_trait]
impl EventLog for ProductEventStore {
    async fn append(&self, mut event: EventEnvelope) -> Result<()> {
        // Enforce product scope: override the product field.
        event.product = Some(self.product.clone());
        self.inner.append(event).await
    }

    async fn subscribe(
        &self,
        mut filter: EventFilter,
    ) -> Result<broadcast::Receiver<EventEnvelope>> {
        // Enforce product scope: always filter by this product.
        filter.product = Some(self.product.clone());
        self.inner.subscribe(filter).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use picloud_domain::events::EventEnvelope;
    use picloud_domain::iri::ResourceIri;
    use uuid::Uuid;

    /// Helper to build a test event with sensible defaults.
    fn make_event(event_type: &str, product: Option<&str>, correlation_id: Uuid) -> EventEnvelope {
        EventEnvelope {
            id: Uuid::new_v4(),
            schema: ResourceIri::new(format!(
                "https://picloud.local/schemas/events/{event_type}/v1"
            ))
            .expect("valid schema IRI"),
            event_type: event_type.to_string(),
            timestamp: chrono::Utc::now(),
            source: ResourceIri::new("https://picloud.local/test").expect("valid source IRI"),
            product: product.map(|s| s.to_string()),
            correlation_id,
            idempotency_key: None,
            payload: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn test_append_and_subscribe_round_trip() {
        let log = InMemoryEventLog::new();
        let correlation_id = Uuid::new_v4();

        // Subscribe before appending.
        let mut rx = log
            .subscribe(EventFilter::default())
            .await
            .expect("subscribe should succeed");

        let event = make_event("NodeJoined", None, correlation_id);
        let event_id = event.id;

        log.append(event).await.expect("append should succeed");

        let received = rx.recv().await.expect("should receive event");
        assert_eq!(received.id, event_id);
        assert_eq!(received.event_type, "NodeJoined");

        // Also verify the log stores the event.
        assert_eq!(log.len().await, 1);
    }

    #[tokio::test]
    async fn test_idempotency_deduplication() {
        let log = InMemoryEventLog::new();
        let correlation_id = Uuid::new_v4();

        let mut event1 = make_event("ResourceReady", None, correlation_id);
        event1.idempotency_key = Some("key-123".to_string());

        let mut event2 = make_event("ResourceReady", None, correlation_id);
        event2.idempotency_key = Some("key-123".to_string());

        log.append(event1).await.expect("first append should succeed");
        log.append(event2)
            .await
            .expect("second append should succeed (idempotent)");

        // Only one event should be stored.
        assert_eq!(log.len().await, 1);
    }

    #[tokio::test]
    async fn test_filter_by_correlation_id() {
        let log = InMemoryEventLog::new();
        let target_cid = Uuid::new_v4();
        let other_cid = Uuid::new_v4();

        let filter = EventFilter {
            correlation_id: Some(target_cid),
            ..Default::default()
        };

        let mut rx = log.subscribe(filter).await.expect("subscribe should succeed");

        // Give the spawned filter task a moment to start.
        tokio::task::yield_now().await;

        let event_match = make_event("ResourceReady", None, target_cid);
        let event_no_match = make_event("ResourceReady", None, other_cid);

        log.append(event_match.clone()).await.unwrap();
        log.append(event_no_match).await.unwrap();

        let received = rx.recv().await.expect("should receive matching event");
        assert_eq!(received.correlation_id, target_cid);

        // The non-matching event should not appear. Use a short timeout.
        let result = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(
            result.is_err(),
            "should not receive event with different correlation_id"
        );
    }

    #[tokio::test]
    async fn test_filter_by_product() {
        let log = InMemoryEventLog::new();
        let cid = Uuid::new_v4();

        let filter = EventFilter {
            product: Some("photo-app".to_string()),
            ..Default::default()
        };

        let mut rx = log.subscribe(filter).await.expect("subscribe should succeed");

        tokio::task::yield_now().await;

        let match_event = make_event("ProductDeployed", Some("photo-app"), cid);
        let no_match_event = make_event("ProductDeployed", Some("other-app"), cid);
        let no_product_event = make_event("NodeJoined", None, cid);

        log.append(match_event.clone()).await.unwrap();
        log.append(no_match_event).await.unwrap();
        log.append(no_product_event).await.unwrap();

        let received = rx.recv().await.expect("should receive matching event");
        assert_eq!(received.product.as_deref(), Some("photo-app"));

        let result = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(
            result.is_err(),
            "should not receive events from other products"
        );
    }

    #[tokio::test]
    async fn test_filter_by_event_types() {
        let log = InMemoryEventLog::new();
        let cid = Uuid::new_v4();

        let filter = EventFilter {
            event_types: vec!["NodeJoined".to_string(), "NodeLeft".to_string()],
            ..Default::default()
        };

        let mut rx = log.subscribe(filter).await.expect("subscribe should succeed");

        tokio::task::yield_now().await;

        log.append(make_event("NodeJoined", None, cid)).await.unwrap();
        log.append(make_event("ResourceReady", None, cid)).await.unwrap();
        log.append(make_event("NodeLeft", None, cid)).await.unwrap();

        let first = rx.recv().await.expect("should receive NodeJoined");
        assert_eq!(first.event_type, "NodeJoined");

        let second = rx.recv().await.expect("should receive NodeLeft");
        assert_eq!(second.event_type, "NodeLeft");

        let result = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(result.is_err(), "should not receive ResourceReady");
    }

    #[tokio::test]
    async fn test_product_event_store_sets_product() {
        let inner = Arc::new(InMemoryEventLog::new());
        let store = ProductEventStore::new("photo-app", inner.clone());

        let event = make_event("ResourceDeclared", None, Uuid::new_v4());
        assert!(event.product.is_none());

        store.append(event).await.unwrap();

        // The inner log should have the event with product set.
        let events = inner.events.read().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].product.as_deref(), Some("photo-app"));
    }

    #[tokio::test]
    async fn test_product_event_store_filters_subscribe() {
        let inner = Arc::new(InMemoryEventLog::new());
        let store = ProductEventStore::new("photo-app", inner.clone());

        // Subscribe via the product store (should auto-filter by product).
        let mut rx = store
            .subscribe(EventFilter::default())
            .await
            .expect("subscribe should succeed");

        tokio::task::yield_now().await;

        // Append via the inner log with different products.
        let cid = Uuid::new_v4();
        inner
            .append(make_event("ResourceReady", Some("photo-app"), cid))
            .await
            .unwrap();
        inner
            .append(make_event("ResourceReady", Some("other-app"), cid))
            .await
            .unwrap();

        let received = rx.recv().await.expect("should receive photo-app event");
        assert_eq!(received.product.as_deref(), Some("photo-app"));

        let result = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(result.is_err(), "should not receive other-app event");
    }

    #[tokio::test]
    async fn test_events_without_idempotency_key_always_append() {
        let log = InMemoryEventLog::new();
        let cid = Uuid::new_v4();

        // Two events with no idempotency key should both be stored.
        log.append(make_event("NodeJoined", None, cid)).await.unwrap();
        log.append(make_event("NodeJoined", None, cid)).await.unwrap();

        assert_eq!(log.len().await, 2);
    }
}
