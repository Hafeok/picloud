//! Event log implementation for picloud-events.
//!
//! Provides an in-memory event log backed by a `Vec<EventEnvelope>` with
//! broadcast-based pub/sub and idempotency deduplication via `idempotency_key`.

use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{broadcast, Mutex as TokioMutex, RwLock};
use tracing::{debug, info};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::EventEnvelope;
use picloud_domain::traits::{EventFilter, EventLog};

/// Default capacity for the broadcast channel.
const DEFAULT_BROADCAST_CAPACITY: usize = 1024;

/// When the log exceeds this many events, compaction is eligible.
const COMPACTION_THRESHOLD: usize = 10_000;

/// Number of most-recent events to retain after compaction.
const COMPACTION_KEEP: usize = 1_000;

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

    /// Return all events starting from the given offset (0-based index).
    ///
    /// This is the catchup mechanism: a projector that has processed events
    /// 0..N calls `events_since(N)` to get everything it missed. A new node
    /// joining the cluster calls `events_since(0)` to replay the full log.
    pub async fn events_since(&self, offset: usize) -> Vec<EventEnvelope> {
        let events = self.events.read().await;
        if offset >= events.len() {
            Vec::new()
        } else {
            events[offset..].to_vec()
        }
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

    async fn events_since(&self, offset: usize) -> Vec<EventEnvelope> {
        let events = self.events.read().await;
        if offset >= events.len() {
            Vec::new()
        } else {
            events[offset..].to_vec()
        }
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

/// Persistent event log that wraps `InMemoryEventLog` and also writes events
/// to a JSON-lines file on disk.
///
/// On construction, if the file exists, all persisted events are replayed into
/// the in-memory log. On append, the event is serialized as a single JSON line
/// and appended to the file before being forwarded to the in-memory log.
///
/// The broadcast channel from the inner log is preserved, so real-time pub/sub
/// works exactly as with `InMemoryEventLog`.
pub struct PersistentEventLog {
    inner: InMemoryEventLog,
    file: TokioMutex<std::fs::File>,
    path: PathBuf,
    /// Number of events that have been compacted away. This offset is added to
    /// logical cursors so that `events_since(N)` remains correct after compaction.
    snapshot_offset: RwLock<usize>,
}

impl PersistentEventLog {
    /// Open (or create) a persistent event log backed by the given file path.
    ///
    /// If the file already exists, all events are replayed into memory.
    /// The parent directory is created if it does not exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PiCloudError::Internal(format!("failed to create event log directory {}: {e}", parent.display())))?;
        }

        let inner = InMemoryEventLog::new();

        // Replay existing events if the file exists.
        if path.exists() {
            let file = std::fs::File::open(&path).map_err(|e| PiCloudError::Internal(format!("failed to open event log file {}: {e}", path.display())))?;
            let reader = std::io::BufReader::new(file);
            let mut replayed = 0u64;

            for (line_no, line) in reader.lines().enumerate() {
                let line = line.map_err(|e| PiCloudError::Internal(format!("failed to read event log line {}: {e}", line_no + 1)))?;
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let event: EventEnvelope =
                    serde_json::from_str(trimmed).map_err(|e| PiCloudError::Internal(format!(
                            "failed to deserialize event at line {}: {e}",
                            line_no + 1
                        )))?;

                // Record idempotency key.
                if let Some(ref key) = event.idempotency_key {
                    // We access the inner fields directly during replay to avoid
                    // broadcasting replayed events (there are no subscribers yet).
                    futures::executor::block_on(async {
                        inner.seen_keys.write().await.insert(key.clone());
                    });
                }
                futures::executor::block_on(async {
                    inner.events.write().await.push(event);
                });
                replayed += 1;
            }
            if replayed > 0 {
                info!(
                    path = %path.display(),
                    events = replayed,
                    "Replayed events from persistent log"
                );
            }
        }

        // Open the file in append mode for subsequent writes.
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| PiCloudError::Internal(format!("failed to open event log file for append {}: {e}", path.display())))?;

        // Load snapshot offset from the sidecar metadata file, if present.
        let meta_path = path.with_extension("jsonl.meta");
        let snapshot_offset = if meta_path.exists() {
            let meta = std::fs::read_to_string(&meta_path).map_err(|e| PiCloudError::Internal(format!("failed to read compaction metadata {}: {e}", meta_path.display())))?;
            meta.trim().parse::<usize>().unwrap_or(0)
        } else {
            0
        };

        Ok(Self {
            inner,
            file: TokioMutex::new(file),
            path,
            snapshot_offset: RwLock::new(snapshot_offset),
        })
    }

    /// Return the number of events currently stored in memory.
    pub async fn len(&self) -> usize {
        self.inner.len().await
    }

    /// Return whether the log is empty.
    pub async fn is_empty(&self) -> bool {
        self.inner.is_empty().await
    }

    /// Return the file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the current snapshot offset (number of events compacted away).
    pub async fn snapshot_offset(&self) -> usize {
        *self.snapshot_offset.read().await
    }

    /// Return the total logical event count (compacted + in-memory).
    pub async fn total_event_count(&self) -> usize {
        *self.snapshot_offset.read().await + self.inner.len().await
    }

    /// Compact the event log using the default policy: if the in-memory log has
    /// more than `COMPACTION_THRESHOLD` events, discard all but the most recent
    /// `COMPACTION_KEEP` events and rewrite the file on disk.
    pub async fn compact_if_needed(&self) -> Result<bool> {
        let current_len = self.inner.len().await;
        if current_len > COMPACTION_THRESHOLD {
            self.compact(current_len - COMPACTION_KEEP).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Compact the event log, discarding the first `discard_count` events from
    /// the in-memory log and rewriting the file on disk.
    ///
    /// The `snapshot_offset` is advanced by `discard_count` so that callers
    /// using logical offsets (e.g. the RDF projector cursor) continue to work
    /// correctly.
    pub async fn compact(&self, discard_count: usize) -> Result<()> {
        let current_len = self.inner.len().await;
        if discard_count == 0 || discard_count > current_len {
            return Err(PiCloudError::Internal(format!(
                "invalid discard_count {discard_count} for log with {current_len} events"
            )));
        }

        info!(
            discard_count,
            current_len,
            path = %self.path.display(),
            "Compacting event log"
        );

        // 1. Drain the first `discard_count` events from the in-memory log.
        let remaining = {
            let mut events = self.inner.events.write().await;
            let remaining: Vec<EventEnvelope> = events.drain(discard_count..).collect();
            *events = remaining.clone();
            remaining
        };

        // 2. Update snapshot_offset.
        {
            let mut offset = self.snapshot_offset.write().await;
            *offset += discard_count;
        }

        // 3. Rewrite the file on disk with only the remaining events.
        {
            let mut file_guard = self.file.lock().await;

            // Write to a temporary file, then rename for atomicity.
            let tmp_path = self.path.with_extension("jsonl.tmp");
            {
                let mut tmp_file = std::fs::File::create(&tmp_path).map_err(|e| {
                    PiCloudError::Internal(format!(
                        "failed to create compaction temp file {}: {e}",
                        tmp_path.display()
                    ))
                })?;
                for event in &remaining {
                    let line = serde_json::to_string(event).map_err(|e| {
                        PiCloudError::Internal(format!(
                            "failed to serialize event during compaction: {e}"
                        ))
                    })?;
                    writeln!(tmp_file, "{}", line).map_err(|e| {
                        PiCloudError::Internal(format!(
                            "failed to write event during compaction: {e}"
                        ))
                    })?;
                }
                tmp_file.flush().map_err(|e| {
                    PiCloudError::Internal(format!("failed to flush compaction temp file: {e}"))
                })?;
            }

            std::fs::rename(&tmp_path, &self.path).map_err(|e| {
                PiCloudError::Internal(format!(
                    "failed to rename compaction temp file: {e}"
                ))
            })?;

            // Write snapshot offset to the metadata sidecar file.
            let meta_path = self.path.with_extension("jsonl.meta");
            let offset_val = *self.snapshot_offset.read().await;
            std::fs::write(&meta_path, offset_val.to_string()).map_err(|e| {
                PiCloudError::Internal(format!(
                    "failed to write compaction metadata: {e}"
                ))
            })?;

            // Re-open the file in append mode.
            let new_file = std::fs::OpenOptions::new()
                .append(true)
                .open(&self.path)
                .map_err(|e| {
                    PiCloudError::Internal(format!(
                        "failed to reopen event log after compaction: {e}"
                    ))
                })?;
            *file_guard = new_file;
        }

        info!(
            remaining = remaining.len(),
            snapshot_offset = *self.snapshot_offset.read().await,
            "Event log compaction complete"
        );

        Ok(())
    }
}

#[async_trait]
impl EventLog for PersistentEventLog {
    async fn append(&self, event: EventEnvelope) -> Result<()> {
        // Check idempotency before writing to disk.
        if let Some(ref key) = event.idempotency_key {
            let seen = self.inner.seen_keys.read().await;
            if seen.contains(key) {
                debug!(
                    idempotency_key = %key,
                    event_id = %event.id,
                    "Duplicate idempotency key — skipping persistent append"
                );
                return Ok(());
            }
        }

        // Serialize and write to disk first (durability before broadcast).
        {
            let line = serde_json::to_string(&event).map_err(|e| PiCloudError::Internal(format!("failed to serialize event {}: {e}", event.id)))?;
            let mut file = self.file.lock().await;
            writeln!(*file, "{}", line).map_err(|e| PiCloudError::Internal(format!("failed to write event to log file: {e}")))?;
            file.flush().map_err(|e| PiCloudError::Internal(format!("failed to flush event log file: {e}")))?;
        }

        // Delegate to in-memory log for broadcast + storage.
        self.inner.append(event).await
    }

    async fn subscribe(
        &self,
        filter: EventFilter,
    ) -> Result<broadcast::Receiver<EventEnvelope>> {
        self.inner.subscribe(filter).await
    }

    async fn events_since(&self, offset: usize) -> Vec<EventEnvelope> {
        let snap = *self.snapshot_offset.read().await;
        // The caller uses logical offsets. Subtract the snapshot offset to get
        // the physical index into the in-memory log.
        let physical = if offset > snap { offset - snap } else { 0 };
        self.inner.events_since(physical).await
    }
}

// ---------------------------------------------------------------------------
// Write-through event log (for Raft replication)
// ---------------------------------------------------------------------------

/// A write callback that receives a serialised event and submits it through
/// an external replication mechanism (e.g. Raft consensus).
///
/// The callback must return `Ok(())` once the event has been durably committed
/// by the replication layer. The local backing store will be populated by the
/// replication layer's apply callback — not by `WriteThroughEventLog` itself.
pub type WriteCallback =
    Arc<dyn Fn(EventEnvelope) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> + Send + Sync>;

/// An `EventLog` implementation that routes writes through an external
/// replication mechanism while delegating reads and subscriptions to a local
/// backing store.
///
/// This is the key piece that enables Raft-based event replication:
///
/// ```text
/// HTTP handler
///   → WriteThroughEventLog.append(event)
///     → write_callback (Raft client_write)
///       → Raft replicates to all nodes
///       → Each node's apply callback appends to its local PersistentEventLog
///         → PersistentEventLog broadcasts to subscribers
/// ```
///
/// `subscribe()` and `events_since()` read from the local backing store,
/// which is populated asynchronously by the Raft apply callback.
pub struct WriteThroughEventLog {
    /// The local backing event log (populated by the Raft apply callback).
    local: Arc<dyn EventLog>,
    /// The write callback that submits events to the replication layer.
    write_cb: WriteCallback,
}

impl WriteThroughEventLog {
    /// Create a new write-through event log.
    ///
    /// - `local`: the local backing store used for reads and subscriptions.
    ///   The replication layer's apply callback must append committed events
    ///   to this same store.
    /// - `write_cb`: called on every `append()` to submit the event to the
    ///   replication layer.
    pub fn new(local: Arc<dyn EventLog>, write_cb: WriteCallback) -> Self {
        Self { local, write_cb }
    }
}

#[async_trait]
impl EventLog for WriteThroughEventLog {
    async fn append(&self, event: EventEnvelope) -> Result<()> {
        // Submit the event to the replication layer (e.g. Raft client_write).
        // The replication layer will call back into the local store once the
        // event is committed across the cluster.
        //
        // If the replication layer fails (e.g. this node is not the Raft
        // leader), fall back to appending directly to the local store.
        // This ensures the platform remains functional even when Raft
        // membership is still being established.
        match (self.write_cb)(event.clone()).await {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    event_type = %event.event_type,
                    "Raft write failed — falling back to local append"
                );
                self.local.append(event).await
            }
        }
    }

    async fn subscribe(
        &self,
        filter: EventFilter,
    ) -> Result<broadcast::Receiver<EventEnvelope>> {
        // Reads come from the local backing store.
        self.local.subscribe(filter).await
    }

    async fn events_since(&self, offset: usize) -> Vec<EventEnvelope> {
        // Reads come from the local backing store.
        self.local.events_since(offset).await
    }
}

// ---------------------------------------------------------------------------
// Product event store
// ---------------------------------------------------------------------------

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

    async fn events_since(&self, offset: usize) -> Vec<EventEnvelope> {
        // Return only events scoped to this product.
        self.inner
            .events_since(offset)
            .await
            .into_iter()
            .filter(|e| e.product.as_deref() == Some(&self.product))
            .collect()
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
            traceparent: None,
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
    async fn test_events_since_full_replay() {
        let log = InMemoryEventLog::new();
        let cid = Uuid::new_v4();

        log.append(make_event("NodeJoined", None, cid)).await.unwrap();
        log.append(make_event("ResourceDeclared", None, cid)).await.unwrap();
        log.append(make_event("ResourceReady", None, cid)).await.unwrap();

        // Full replay from offset 0
        let all = log.events_since(0).await;
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].event_type, "NodeJoined");
        assert_eq!(all[2].event_type, "ResourceReady");
    }

    #[tokio::test]
    async fn test_events_since_partial_replay() {
        let log = InMemoryEventLog::new();
        let cid = Uuid::new_v4();

        log.append(make_event("NodeJoined", None, cid)).await.unwrap();
        log.append(make_event("ResourceDeclared", None, cid)).await.unwrap();
        log.append(make_event("ResourceReady", None, cid)).await.unwrap();

        // Partial replay from offset 2 (already seen first two)
        let missed = log.events_since(2).await;
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].event_type, "ResourceReady");
    }

    #[tokio::test]
    async fn test_events_since_at_end_returns_empty() {
        let log = InMemoryEventLog::new();
        let cid = Uuid::new_v4();

        log.append(make_event("NodeJoined", None, cid)).await.unwrap();

        let missed = log.events_since(1).await;
        assert!(missed.is_empty());

        // Beyond end also returns empty
        let missed = log.events_since(100).await;
        assert!(missed.is_empty());
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

    // ---- PersistentEventLog tests ----

    #[tokio::test]
    async fn persistent_log_creates_file_and_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");

        let log = PersistentEventLog::open(&path).unwrap();
        assert_eq!(log.len().await, 0);

        let cid = Uuid::new_v4();
        log.append(make_event("NodeJoined", None, cid)).await.unwrap();
        log.append(make_event("ResourceReady", None, cid)).await.unwrap();

        assert_eq!(log.len().await, 2);
        assert!(path.exists());

        // File should have 2 lines
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[tokio::test]
    async fn persistent_log_replays_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let cid = Uuid::new_v4();

        // Write events with the first instance.
        {
            let log = PersistentEventLog::open(&path).unwrap();
            log.append(make_event("NodeJoined", None, cid)).await.unwrap();
            log.append(make_event("ResourceDeclared", None, cid)).await.unwrap();
            log.append(make_event("ResourceReady", None, cid)).await.unwrap();
            assert_eq!(log.len().await, 3);
        }

        // Re-open -- events should be replayed from disk.
        {
            let log = PersistentEventLog::open(&path).unwrap();
            assert_eq!(log.len().await, 3);

            let events = log.events_since(0).await;
            assert_eq!(events.len(), 3);
            assert_eq!(events[0].event_type, "NodeJoined");
            assert_eq!(events[1].event_type, "ResourceDeclared");
            assert_eq!(events[2].event_type, "ResourceReady");
        }
    }

    #[tokio::test]
    async fn persistent_log_idempotency_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let cid = Uuid::new_v4();

        let mut event = make_event("NodeJoined", None, cid);
        event.idempotency_key = Some("key-abc".to_string());

        // Write the event.
        {
            let log = PersistentEventLog::open(&path).unwrap();
            log.append(event.clone()).await.unwrap();
            assert_eq!(log.len().await, 1);
        }

        // Re-open and try to append the same event -- should be deduplicated.
        {
            let log = PersistentEventLog::open(&path).unwrap();
            assert_eq!(log.len().await, 1);
            log.append(event.clone()).await.unwrap();
            assert_eq!(log.len().await, 1);
        }
    }

    #[tokio::test]
    async fn persistent_log_broadcast_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");

        let log = PersistentEventLog::open(&path).unwrap();
        let mut rx = log.subscribe(EventFilter::default()).await.unwrap();

        let cid = Uuid::new_v4();
        let event = make_event("NodeJoined", None, cid);
        let event_id = event.id;

        log.append(event).await.unwrap();

        let received = rx.recv().await.expect("should receive event via broadcast");
        assert_eq!(received.id, event_id);
    }

    #[tokio::test]
    async fn persistent_log_events_since() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");

        let log = PersistentEventLog::open(&path).unwrap();
        let cid = Uuid::new_v4();

        log.append(make_event("A", None, cid)).await.unwrap();
        log.append(make_event("B", None, cid)).await.unwrap();
        log.append(make_event("C", None, cid)).await.unwrap();

        let since_1 = log.events_since(1).await;
        assert_eq!(since_1.len(), 2);
        assert_eq!(since_1[0].event_type, "B");
        assert_eq!(since_1[1].event_type, "C");

        let since_3 = log.events_since(3).await;
        assert!(since_3.is_empty());
    }

    #[tokio::test]
    async fn persistent_log_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep").join("nested").join("events.jsonl");

        let log = PersistentEventLog::open(&path).unwrap();
        log.append(make_event("NodeJoined", None, Uuid::new_v4())).await.unwrap();
        assert!(path.exists());
    }

    // ---- Compaction tests ----

    #[tokio::test]
    async fn compaction_reduces_file_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let cid = Uuid::new_v4();

        let log = PersistentEventLog::open(&path).unwrap();
        // Append 100 events.
        for i in 0..100 {
            log.append(make_event(&format!("Event{i}"), None, cid))
                .await
                .unwrap();
        }

        let size_before = std::fs::metadata(&path).unwrap().len();
        assert_eq!(log.len().await, 100);

        // Compact away the first 80 events, keeping the last 20.
        log.compact(80).await.unwrap();

        let size_after = std::fs::metadata(&path).unwrap().len();
        assert!(
            size_after < size_before,
            "file should be smaller after compaction: {size_after} >= {size_before}"
        );

        // The file should now have exactly 20 lines.
        let content = std::fs::read_to_string(&path).unwrap();
        let line_count = content.lines().count();
        assert_eq!(line_count, 20);
    }

    #[tokio::test]
    async fn compaction_events_still_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let cid = Uuid::new_v4();

        let log = PersistentEventLog::open(&path).unwrap();
        for i in 0..50 {
            log.append(make_event(&format!("Event{i}"), None, cid))
                .await
                .unwrap();
        }

        // Compact away the first 30 events.
        log.compact(30).await.unwrap();

        // In-memory log should have 20 events.
        assert_eq!(log.len().await, 20);

        // events_since with logical offset 30 should return 20 events (physical offset 0).
        let events = log.events_since(30).await;
        assert_eq!(events.len(), 20);
        assert_eq!(events[0].event_type, "Event30");
        assert_eq!(events[19].event_type, "Event49");

        // events_since with logical offset 40 should return 10 events.
        let events = log.events_since(40).await;
        assert_eq!(events.len(), 10);
        assert_eq!(events[0].event_type, "Event40");

        // events_since with logical offset 0 (below snapshot) returns all remaining.
        let events = log.events_since(0).await;
        assert_eq!(events.len(), 20);
    }

    #[tokio::test]
    async fn compaction_event_count_correct() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let cid = Uuid::new_v4();

        let log = PersistentEventLog::open(&path).unwrap();
        for i in 0..60 {
            log.append(make_event(&format!("Event{i}"), None, cid))
                .await
                .unwrap();
        }

        assert_eq!(log.total_event_count().await, 60);
        assert_eq!(log.snapshot_offset().await, 0);

        log.compact(40).await.unwrap();

        // In-memory count is 20, but total logical count is still 60.
        assert_eq!(log.len().await, 20);
        assert_eq!(log.snapshot_offset().await, 40);
        assert_eq!(log.total_event_count().await, 60);

        // Append a new event after compaction.
        log.append(make_event("Event60", None, cid)).await.unwrap();
        assert_eq!(log.len().await, 21);
        assert_eq!(log.total_event_count().await, 61);
    }

    #[tokio::test]
    async fn compaction_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let cid = Uuid::new_v4();

        // Write 50 events and compact.
        {
            let log = PersistentEventLog::open(&path).unwrap();
            for i in 0..50 {
                log.append(make_event(&format!("Event{i}"), None, cid))
                    .await
                    .unwrap();
            }
            log.compact(30).await.unwrap();
            assert_eq!(log.len().await, 20);
            assert_eq!(log.snapshot_offset().await, 30);
        }

        // Re-open and verify state is correct.
        {
            let log = PersistentEventLog::open(&path).unwrap();
            assert_eq!(log.len().await, 20);
            assert_eq!(log.snapshot_offset().await, 30);
            assert_eq!(log.total_event_count().await, 50);

            let events = log.events_since(30).await;
            assert_eq!(events.len(), 20);
            assert_eq!(events[0].event_type, "Event30");
        }
    }

    #[tokio::test]
    async fn compaction_invalid_discard_count_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let cid = Uuid::new_v4();

        let log = PersistentEventLog::open(&path).unwrap();
        for i in 0..10 {
            log.append(make_event(&format!("Event{i}"), None, cid))
                .await
                .unwrap();
        }

        // Discarding 0 should fail.
        assert!(log.compact(0).await.is_err());

        // Discarding more than available should fail.
        assert!(log.compact(11).await.is_err());
    }

    // ---- WriteThroughEventLog tests ----

    #[tokio::test]
    async fn write_through_routes_append_to_callback() {
        // The "local" store simulates the backing store that the apply
        // callback would populate after Raft commits the entry.
        let local = Arc::new(InMemoryEventLog::new());
        let local_for_cb = Arc::clone(&local);

        // The write callback simulates Raft: it serialises, then
        // "applies" by appending to the local store (mimicking what the
        // Raft state machine apply callback does).
        let write_cb: WriteCallback = Arc::new(move |event: EventEnvelope| {
            let store = Arc::clone(&local_for_cb);
            Box::pin(async move {
                store.append(event).await
            })
        });

        let wt = WriteThroughEventLog::new(local.clone() as Arc<dyn EventLog>, write_cb);
        let cid = Uuid::new_v4();

        // Subscribe before appending — the subscription reads from the local store.
        let mut rx = wt.subscribe(EventFilter::default()).await.unwrap();

        let event = make_event("NodeJoined", None, cid);
        let event_id = event.id;

        // Append through the write-through log.
        wt.append(event).await.unwrap();

        // The local store should have the event (placed there by the callback).
        assert_eq!(local.len().await, 1);

        // The subscriber should receive the event.
        let received = rx.recv().await.expect("should receive event from local store");
        assert_eq!(received.id, event_id);
    }

    #[tokio::test]
    async fn write_through_events_since_reads_from_local() {
        let local = Arc::new(InMemoryEventLog::new());
        let local_for_cb = Arc::clone(&local);

        let write_cb: WriteCallback = Arc::new(move |event: EventEnvelope| {
            let store = Arc::clone(&local_for_cb);
            Box::pin(async move { store.append(event).await })
        });

        let wt = WriteThroughEventLog::new(local.clone() as Arc<dyn EventLog>, write_cb);
        let cid = Uuid::new_v4();

        wt.append(make_event("A", None, cid)).await.unwrap();
        wt.append(make_event("B", None, cid)).await.unwrap();
        wt.append(make_event("C", None, cid)).await.unwrap();

        let all = wt.events_since(0).await;
        assert_eq!(all.len(), 3);

        let from_1 = wt.events_since(1).await;
        assert_eq!(from_1.len(), 2);
        assert_eq!(from_1[0].event_type, "B");
    }

    #[tokio::test]
    async fn write_through_callback_error_falls_back_to_local() {
        let local = Arc::new(InMemoryEventLog::new());

        // A callback that always fails — simulates Raft rejecting a write
        // (e.g. this node is not the leader). The WriteThroughEventLog
        // should fall back to appending directly to the local store.
        let write_cb: WriteCallback = Arc::new(|_event: EventEnvelope| {
            Box::pin(async {
                Err(PiCloudError::Internal("not the leader".to_string()))
            })
        });

        let wt = WriteThroughEventLog::new(local.clone() as Arc<dyn EventLog>, write_cb);
        let cid = Uuid::new_v4();

        let result = wt.append(make_event("NodeJoined", None, cid)).await;
        // Should succeed via local fallback
        assert!(result.is_ok());

        // The local store should have the event (fallback path).
        assert_eq!(local.len().await, 1);
    }
}
