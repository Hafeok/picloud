//! Raft consensus implementation for PiCloud using openraft.
//!
//! Defines the openraft type configuration, in-memory log storage,
//! state machine (applies committed entries to the event log),
//! and HTTP-based network transport between nodes.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::sync::Arc;

use openraft::storage::{LogFlushed, LogState, RaftLogStorage, RaftStateMachine};
use openraft::{
    BasicNode, Config, Entry, LogId, RaftLogReader, RaftSnapshotBuilder,
    Snapshot, SnapshotMeta, StorageError, StoredMembership, Vote,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info};

// ---------------------------------------------------------------------------
// Type configuration
// ---------------------------------------------------------------------------

openraft::declare_raft_types!(
    /// PiCloud Raft type configuration.
    pub PiCloudTypeConfig:
        D            = ClientRequest,
        R            = ClientResponse,
        NodeId       = u64,
        Node         = BasicNode,
        Entry        = Entry<PiCloudTypeConfig>,
        SnapshotData = Cursor<Vec<u8>>,
        AsyncRuntime = openraft::TokioRuntime,
        Responder    = openraft::impls::OneshotResponder<PiCloudTypeConfig>,
);

/// The Raft handle type used throughout PiCloud.
pub type PiCloudRaft = openraft::Raft<PiCloudTypeConfig>;

// ---------------------------------------------------------------------------
// Application data types
// ---------------------------------------------------------------------------

/// Data submitted by a client write request — a serialised EventEnvelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientRequest {
    /// JSON-serialised `EventEnvelope`.
    pub event_json: String,
}

/// Response returned by the state machine after applying an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientResponse {
    /// Sequence number assigned to the applied event.
    pub index: u64,
}

// ---------------------------------------------------------------------------
// Log storage (in-memory)
// ---------------------------------------------------------------------------

/// In-memory Raft log storage.
///
/// Stores log entries, the current vote, and snapshot metadata in memory.
/// Production deployments would persist to disk; this is sufficient for MVP.
#[derive(Debug)]
pub struct MemLogStore {
    inner: Arc<RwLock<MemLogStoreInner>>,
}

#[derive(Debug, Default)]
struct MemLogStoreInner {
    last_purged_log_id: Option<LogId<u64>>,
    log: BTreeMap<u64, Entry<PiCloudTypeConfig>>,
    vote: Option<Vote<u64>>,
    committed: Option<LogId<u64>>,
}

impl MemLogStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MemLogStoreInner::default())),
        }
    }
}

impl Default for MemLogStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MemLogStore {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

// -- RaftLogReader ----------------------------------------------------------

impl RaftLogReader<PiCloudTypeConfig> for MemLogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<PiCloudTypeConfig>>, StorageError<u64>> {
        let inner = self.inner.read().await;
        let entries: Vec<_> = inner
            .log
            .range(range)
            .map(|(_, e)| e.clone())
            .collect();
        Ok(entries)
    }
}

// -- RaftLogStorage ---------------------------------------------------------

impl RaftLogStorage<PiCloudTypeConfig> for MemLogStore {
    type LogReader = Self;

    async fn get_log_state(&mut self) -> Result<LogState<PiCloudTypeConfig>, StorageError<u64>> {
        let inner = self.inner.read().await;
        let last_log_id = inner.log.values().last().map(|e| e.log_id.clone());
        Ok(LogState {
            last_purged_log_id: inner.last_purged_log_id.clone(),
            last_log_id: last_log_id.or_else(|| inner.last_purged_log_id.clone()),
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &Vote<u64>) -> Result<(), StorageError<u64>> {
        let mut inner = self.inner.write().await;
        inner.vote = Some(vote.clone());
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<u64>>, StorageError<u64>> {
        let inner = self.inner.read().await;
        Ok(inner.vote.clone())
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<u64>>,
    ) -> Result<(), StorageError<u64>> {
        let mut inner = self.inner.write().await;
        inner.committed = committed;
        Ok(())
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<u64>>, StorageError<u64>> {
        let inner = self.inner.read().await;
        Ok(inner.committed.clone())
    }

    async fn append<I>(
        &mut self,
        entries: I,
        callback: LogFlushed<PiCloudTypeConfig>,
    ) -> Result<(), StorageError<u64>>
    where
        I: IntoIterator<Item = Entry<PiCloudTypeConfig>> + Send,
        I::IntoIter: Send,
    {
        let mut inner = self.inner.write().await;
        for entry in entries {
            inner.log.insert(entry.log_id.index, entry);
        }
        // In-memory: data is immediately "flushed".
        callback.log_io_completed(Ok(()));
        Ok(())
    }

    async fn truncate(&mut self, log_id: LogId<u64>) -> Result<(), StorageError<u64>> {
        let mut inner = self.inner.write().await;
        let keys: Vec<u64> = inner
            .log
            .range(log_id.index..)
            .map(|(k, _)| *k)
            .collect();
        for k in keys {
            inner.log.remove(&k);
        }
        Ok(())
    }

    async fn purge(&mut self, log_id: LogId<u64>) -> Result<(), StorageError<u64>> {
        let mut inner = self.inner.write().await;
        let keys: Vec<u64> = inner
            .log
            .range(..=log_id.index)
            .map(|(k, _)| *k)
            .collect();
        for k in keys {
            inner.log.remove(&k);
        }
        inner.last_purged_log_id = Some(log_id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Log storage (sled-backed, persistent)
// ---------------------------------------------------------------------------

/// Persistent Raft log storage backed by sled.
///
/// Uses two sled trees:
/// - `raft_log` — maps big-endian u64 index to JSON-serialized `Entry`
/// - `raft_meta` — stores vote, committed, last_purged_log_id under string keys
#[derive(Debug, Clone)]
pub struct SledLogStore {
    db: sled::Db,
    log_tree: sled::Tree,
    meta_tree: sled::Tree,
}

impl SledLogStore {
    pub fn new(db: sled::Db) -> Result<Self, sled::Error> {
        let log_tree = db.open_tree("raft_log")?;
        let meta_tree = db.open_tree("raft_meta")?;
        Ok(Self {
            db,
            log_tree,
            meta_tree,
        })
    }

    fn index_key(index: u64) -> [u8; 8] {
        index.to_be_bytes()
    }

    fn sled_to_storage_error(e: sled::Error) -> StorageError<u64> {
        StorageError::from_io_error(
            openraft::ErrorSubject::Store,
            openraft::ErrorVerb::Read,
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        )
    }

    fn json_to_storage_error(e: serde_json::Error) -> StorageError<u64> {
        StorageError::from_io_error(
            openraft::ErrorSubject::Store,
            openraft::ErrorVerb::Read,
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        )
    }
}

impl RaftLogReader<PiCloudTypeConfig> for SledLogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<PiCloudTypeConfig>>, StorageError<u64>> {
        // Iterate all entries in the log tree (sorted by big-endian key)
        // and filter by the requested range. Since keys are big-endian u64,
        // sled's BTree ordering matches numeric ordering.
        let mut entries = Vec::new();
        for item in self.log_tree.iter() {
            let (key_bytes, val) = item.map_err(Self::sled_to_storage_error)?;
            if key_bytes.len() == 8 {
                let idx = u64::from_be_bytes(key_bytes.as_ref().try_into().unwrap());
                if range.contains(&idx) {
                    let entry: Entry<PiCloudTypeConfig> =
                        serde_json::from_slice(&val).map_err(Self::json_to_storage_error)?;
                    entries.push(entry);
                }
            }
        }
        Ok(entries)
    }
}

impl RaftLogStorage<PiCloudTypeConfig> for SledLogStore {
    type LogReader = Self;

    async fn get_log_state(&mut self) -> Result<LogState<PiCloudTypeConfig>, StorageError<u64>> {
        let last_purged: Option<LogId<u64>> = self
            .meta_tree
            .get(b"last_purged_log_id")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| serde_json::from_slice(&v))
            .transpose()
            .map_err(Self::json_to_storage_error)?;

        let last_log_id = self
            .log_tree
            .last()
            .map_err(Self::sled_to_storage_error)?
            .map(|(_k, v)| {
                let entry: Entry<PiCloudTypeConfig> = serde_json::from_slice(&v)
                    .map_err(Self::json_to_storage_error)
                    .unwrap();
                entry.log_id
            });

        Ok(LogState {
            last_purged_log_id: last_purged.clone(),
            last_log_id: last_log_id.or_else(|| last_purged.clone()),
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &Vote<u64>) -> Result<(), StorageError<u64>> {
        let val = serde_json::to_vec(vote).map_err(Self::json_to_storage_error)?;
        self.meta_tree
            .insert(b"vote", val)
            .map_err(Self::sled_to_storage_error)?;
        self.db.flush_async().await.map_err(|e| {
            Self::sled_to_storage_error(e)
        })?;
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<u64>>, StorageError<u64>> {
        self.meta_tree
            .get(b"vote")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| serde_json::from_slice(&v).map_err(Self::json_to_storage_error))
            .transpose()
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<u64>>,
    ) -> Result<(), StorageError<u64>> {
        match committed {
            Some(ref c) => {
                let val = serde_json::to_vec(c).map_err(Self::json_to_storage_error)?;
                self.meta_tree
                    .insert(b"committed", val)
                    .map_err(Self::sled_to_storage_error)?;
            }
            None => {
                self.meta_tree
                    .remove(b"committed")
                    .map_err(Self::sled_to_storage_error)?;
            }
        }
        Ok(())
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<u64>>, StorageError<u64>> {
        self.meta_tree
            .get(b"committed")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| serde_json::from_slice(&v).map_err(Self::json_to_storage_error))
            .transpose()
    }

    async fn append<I>(
        &mut self,
        entries: I,
        callback: LogFlushed<PiCloudTypeConfig>,
    ) -> Result<(), StorageError<u64>>
    where
        I: IntoIterator<Item = Entry<PiCloudTypeConfig>> + Send,
        I::IntoIter: Send,
    {
        for entry in entries {
            let key = Self::index_key(entry.log_id.index);
            let val = serde_json::to_vec(&entry).map_err(Self::json_to_storage_error)?;
            self.log_tree
                .insert(key, val)
                .map_err(Self::sled_to_storage_error)?;
        }
        let db = self.db.clone();
        tokio::task::spawn(async move {
            let result = db.flush_async().await.map(|_| ()).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            });
            callback.log_io_completed(result);
        });
        Ok(())
    }

    async fn truncate(&mut self, log_id: LogId<u64>) -> Result<(), StorageError<u64>> {
        let start = Self::index_key(log_id.index);
        let keys: Vec<sled::IVec> = self
            .log_tree
            .range(start..)
            .filter_map(|r| r.ok().map(|(k, _)| k))
            .collect();
        for k in keys {
            self.log_tree
                .remove(&k)
                .map_err(Self::sled_to_storage_error)?;
        }
        Ok(())
    }

    async fn purge(&mut self, log_id: LogId<u64>) -> Result<(), StorageError<u64>> {
        let end = Self::index_key(log_id.index);
        let keys: Vec<sled::IVec> = self
            .log_tree
            .range(..=end)
            .filter_map(|r| r.ok().map(|(k, _)| k))
            .collect();
        for k in keys {
            self.log_tree
                .remove(&k)
                .map_err(Self::sled_to_storage_error)?;
        }
        let val = serde_json::to_vec(&log_id).map_err(Self::json_to_storage_error)?;
        self.meta_tree
            .insert(b"last_purged_log_id", val)
            .map_err(Self::sled_to_storage_error)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// State machine (sled-backed, persistent)
// ---------------------------------------------------------------------------

/// Persistent Raft state machine backed by sled.
///
/// Stores applied state in the sled database. Uses the same `apply_callback`
/// pattern as `MemStateMachine` so the composition root can wire it to the
/// platform event log.
pub struct SledStateMachine {
    db: sled::Db,
    meta_tree: sled::Tree,
    apply_callback: Option<ApplyCallback>,
}

impl std::fmt::Debug for SledStateMachine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SledStateMachine")
            .field("has_callback", &self.apply_callback.is_some())
            .finish()
    }
}

impl SledStateMachine {
    pub fn new(db: sled::Db, apply_callback: Option<ApplyCallback>) -> Result<Self, sled::Error> {
        let meta_tree = db.open_tree("sm_meta")?;
        Ok(Self {
            db,
            meta_tree,
            apply_callback,
        })
    }

    fn sled_to_storage_error(e: sled::Error) -> StorageError<u64> {
        StorageError::from_io_error(
            openraft::ErrorSubject::Store,
            openraft::ErrorVerb::Read,
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        )
    }

    fn json_to_storage_error(e: serde_json::Error) -> StorageError<u64> {
        StorageError::from_io_error(
            openraft::ErrorSubject::Store,
            openraft::ErrorVerb::Read,
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        )
    }

    fn get_applied_count(&self) -> Result<u64, StorageError<u64>> {
        self.meta_tree
            .get(b"sm_count")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| serde_json::from_slice(&v).map_err(Self::json_to_storage_error))
            .transpose()
            .map(|o| o.unwrap_or(0))
    }

    fn set_applied_count(&self, count: u64) -> Result<(), StorageError<u64>> {
        let val = serde_json::to_vec(&count).map_err(Self::json_to_storage_error)?;
        self.meta_tree
            .insert(b"sm_count", val)
            .map_err(Self::sled_to_storage_error)?;
        Ok(())
    }
}

impl RaftStateMachine<PiCloudTypeConfig> for SledStateMachine {
    type SnapshotBuilder = Self;

    async fn applied_state(
        &mut self,
    ) -> Result<
        (
            Option<LogId<u64>>,
            StoredMembership<u64, BasicNode>,
        ),
        StorageError<u64>,
    > {
        let last_applied: Option<LogId<u64>> = self
            .meta_tree
            .get(b"sm_last_applied")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| serde_json::from_slice(&v).map_err(Self::json_to_storage_error))
            .transpose()?;

        let membership: StoredMembership<u64, BasicNode> = self
            .meta_tree
            .get(b"sm_membership")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| serde_json::from_slice(&v).map_err(Self::json_to_storage_error))
            .transpose()?
            .unwrap_or_default();

        Ok((last_applied, membership))
    }

    async fn apply<I>(
        &mut self,
        entries: I,
    ) -> Result<Vec<ClientResponse>, StorageError<u64>>
    where
        I: IntoIterator<Item = Entry<PiCloudTypeConfig>> + Send,
        I::IntoIter: Send,
    {
        let mut applied_count = self.get_applied_count()?;
        let mut responses = Vec::new();

        for entry in entries {
            // Save last_applied_log
            let val =
                serde_json::to_vec(&entry.log_id).map_err(Self::json_to_storage_error)?;
            self.meta_tree
                .insert(b"sm_last_applied", val)
                .map_err(Self::sled_to_storage_error)?;

            match entry.payload {
                openraft::EntryPayload::Blank => {
                    responses.push(ClientResponse {
                        index: applied_count,
                    });
                }
                openraft::EntryPayload::Normal(req) => {
                    applied_count += 1;
                    debug!(index = applied_count, "Sled state machine applying entry");

                    if let Some(ref cb) = self.apply_callback {
                        cb(&req.event_json);
                    }

                    responses.push(ClientResponse {
                        index: applied_count,
                    });
                }
                openraft::EntryPayload::Membership(mem) => {
                    let stored =
                        StoredMembership::new(Some(entry.log_id.clone()), mem);
                    let val =
                        serde_json::to_vec(&stored).map_err(Self::json_to_storage_error)?;
                    self.meta_tree
                        .insert(b"sm_membership", val)
                        .map_err(Self::sled_to_storage_error)?;
                    responses.push(ClientResponse {
                        index: applied_count,
                    });
                }
            }
        }

        self.set_applied_count(applied_count)?;
        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        SledStateMachine {
            db: self.db.clone(),
            meta_tree: self.meta_tree.clone(),
            apply_callback: self.apply_callback.clone(),
        }
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<u64>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<u64, BasicNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<u64>> {
        // Save last_applied_log
        if let Some(ref log_id) = meta.last_log_id {
            let val = serde_json::to_vec(log_id).map_err(Self::json_to_storage_error)?;
            self.meta_tree
                .insert(b"sm_last_applied", val)
                .map_err(Self::sled_to_storage_error)?;
        }

        // Save membership
        let val =
            serde_json::to_vec(&meta.last_membership).map_err(Self::json_to_storage_error)?;
        self.meta_tree
            .insert(b"sm_membership", val)
            .map_err(Self::sled_to_storage_error)?;

        // Save snapshot data and meta
        let data = snapshot.into_inner();
        self.meta_tree
            .insert(b"sm_snapshot_data", data)
            .map_err(Self::sled_to_storage_error)?;
        let meta_val = serde_json::to_vec(meta).map_err(Self::json_to_storage_error)?;
        self.meta_tree
            .insert(b"sm_snapshot_meta", meta_val)
            .map_err(Self::sled_to_storage_error)?;

        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<PiCloudTypeConfig>>, StorageError<u64>> {
        let meta: Option<SnapshotMeta<u64, BasicNode>> = self
            .meta_tree
            .get(b"sm_snapshot_meta")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| serde_json::from_slice(&v).map_err(Self::json_to_storage_error))
            .transpose()?;

        match meta {
            Some(meta) => {
                let data = self
                    .meta_tree
                    .get(b"sm_snapshot_data")
                    .map_err(Self::sled_to_storage_error)?
                    .map(|v| v.to_vec())
                    .unwrap_or_default();
                Ok(Some(Snapshot {
                    meta,
                    snapshot: Box::new(Cursor::new(data)),
                }))
            }
            None => Ok(None),
        }
    }
}

impl RaftSnapshotBuilder<PiCloudTypeConfig> for SledStateMachine {
    async fn build_snapshot(
        &mut self,
    ) -> Result<Snapshot<PiCloudTypeConfig>, StorageError<u64>> {
        let data = self
            .meta_tree
            .get(b"sm_snapshot_data")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| v.to_vec())
            .unwrap_or_default();

        let last_applied: Option<LogId<u64>> = self
            .meta_tree
            .get(b"sm_last_applied")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| serde_json::from_slice(&v).map_err(Self::json_to_storage_error))
            .transpose()?;

        let membership: StoredMembership<u64, BasicNode> = self
            .meta_tree
            .get(b"sm_membership")
            .map_err(Self::sled_to_storage_error)?
            .map(|v| serde_json::from_slice(&v).map_err(Self::json_to_storage_error))
            .transpose()?
            .unwrap_or_default();

        let snapshot_id = last_applied
            .as_ref()
            .map(|id| format!("{}-{}", id.leader_id, id.index))
            .unwrap_or_else(|| "empty".to_string());

        let meta = SnapshotMeta {
            last_log_id: last_applied,
            last_membership: membership,
            snapshot_id,
        };

        // Persist snapshot meta
        let meta_val = serde_json::to_vec(&meta).map_err(Self::json_to_storage_error)?;
        self.meta_tree
            .insert(b"sm_snapshot_meta", meta_val)
            .map_err(Self::sled_to_storage_error)?;

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data)),
        })
    }
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// Callback invoked when a Raft entry is committed.
///
/// This allows wiring the state machine to the platform event log
/// without making picloud-cluster depend on picloud-events.
pub type ApplyCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// In-memory Raft state machine.
///
/// When entries are committed (applied), the state machine invokes an optional
/// callback with the serialised event JSON. The composition root hooks this up
/// to the `InMemoryEventLog`.
pub struct MemStateMachine {
    inner: Arc<RwLock<MemStateMachineInner>>,
    apply_callback: Option<ApplyCallback>,
}

impl std::fmt::Debug for MemStateMachine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemStateMachine")
            .field("has_callback", &self.apply_callback.is_some())
            .finish()
    }
}

#[derive(Debug, Default)]
struct MemStateMachineInner {
    last_applied_log: Option<LogId<u64>>,
    last_membership: StoredMembership<u64, BasicNode>,
    /// Simple count of applied entries, used as the response index.
    applied_count: u64,
    /// Snapshot: serialised applied events (JSON lines).
    snapshot_data: Vec<u8>,
    snapshot_meta: Option<SnapshotMeta<u64, BasicNode>>,
}

impl MemStateMachine {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MemStateMachineInner::default())),
            apply_callback: None,
        }
    }

    /// Set a callback that is invoked for every committed `ClientRequest`.
    pub fn with_apply_callback(mut self, cb: ApplyCallback) -> Self {
        self.apply_callback = Some(cb);
        self
    }
}

impl Default for MemStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl RaftStateMachine<PiCloudTypeConfig> for MemStateMachine {
    type SnapshotBuilder = Self;

    async fn applied_state(
        &mut self,
    ) -> Result<
        (
            Option<LogId<u64>>,
            StoredMembership<u64, BasicNode>,
        ),
        StorageError<u64>,
    > {
        let inner = self.inner.read().await;
        Ok((
            inner.last_applied_log.clone(),
            inner.last_membership.clone(),
        ))
    }

    async fn apply<I>(
        &mut self,
        entries: I,
    ) -> Result<Vec<ClientResponse>, StorageError<u64>>
    where
        I: IntoIterator<Item = Entry<PiCloudTypeConfig>> + Send,
        I::IntoIter: Send,
    {
        let mut inner = self.inner.write().await;
        let mut responses = Vec::new();

        for entry in entries {
            inner.last_applied_log = Some(entry.log_id.clone());

            match entry.payload {
                openraft::EntryPayload::Blank => {
                    responses.push(ClientResponse { index: inner.applied_count });
                }
                openraft::EntryPayload::Normal(req) => {
                    inner.applied_count += 1;
                    let idx = inner.applied_count;
                    debug!(index = idx, "State machine applying entry");

                    if let Some(ref cb) = self.apply_callback {
                        cb(&req.event_json);
                    }

                    responses.push(ClientResponse { index: idx });
                }
                openraft::EntryPayload::Membership(mem) => {
                    inner.last_membership =
                        StoredMembership::new(Some(entry.log_id.clone()), mem);
                    responses.push(ClientResponse { index: inner.applied_count });
                }
            }
        }

        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        MemStateMachine {
            inner: Arc::clone(&self.inner),
            apply_callback: self.apply_callback.clone(),
        }
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<u64>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<u64, BasicNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<u64>> {
        let mut inner = self.inner.write().await;
        inner.last_applied_log = meta.last_log_id.clone();
        inner.last_membership = meta.last_membership.clone();
        inner.snapshot_data = snapshot.into_inner();
        inner.snapshot_meta = Some(meta.clone());
        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<PiCloudTypeConfig>>, StorageError<u64>> {
        let inner = self.inner.read().await;
        match &inner.snapshot_meta {
            Some(meta) => Ok(Some(Snapshot {
                meta: meta.clone(),
                snapshot: Box::new(Cursor::new(inner.snapshot_data.clone())),
            })),
            None => Ok(None),
        }
    }
}

impl RaftSnapshotBuilder<PiCloudTypeConfig> for MemStateMachine {
    async fn build_snapshot(
        &mut self,
    ) -> Result<Snapshot<PiCloudTypeConfig>, StorageError<u64>> {
        let mut inner = self.inner.write().await;
        let data = inner.snapshot_data.clone();
        let last_applied = inner.last_applied_log.clone();
        let membership = inner.last_membership.clone();

        let snapshot_id = last_applied
            .as_ref()
            .map(|id| format!("{}-{}", id.leader_id, id.index))
            .unwrap_or_else(|| "empty".to_string());

        let meta = SnapshotMeta {
            last_log_id: last_applied,
            last_membership: membership,
            snapshot_id,
        };
        inner.snapshot_meta = Some(meta.clone());

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data)),
        })
    }
}

// ---------------------------------------------------------------------------
// Network — HTTP-based RPCs
// ---------------------------------------------------------------------------

/// TLS configuration for Raft inter-node RPCs (mTLS).
///
/// When present, the `HttpNetworkFactory` will use HTTPS with mutual TLS
/// for all Raft RPC calls. The CA cert validates peer servers, and the
/// client cert+key authenticates this node to peers.
#[derive(Clone)]
pub struct RaftTlsConfig {
    pub ca_cert_pem: String,
    pub client_cert_pem: String,
    pub client_key_pem: String,
}

/// Factory that creates network connections to peer Raft nodes.
pub struct HttpNetworkFactory {
    client: reqwest::Client,
    use_tls: bool,
}

impl HttpNetworkFactory {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            use_tls: false,
        }
    }

    /// Create a factory with mTLS enabled for all Raft RPCs.
    pub fn with_tls(tls_config: RaftTlsConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let ca_cert = reqwest::Certificate::from_pem(tls_config.ca_cert_pem.as_bytes())?;

        let mut id_pem = tls_config.client_cert_pem.as_bytes().to_vec();
        id_pem.push(b'\n');
        id_pem.extend_from_slice(tls_config.client_key_pem.as_bytes());
        let identity = reqwest::Identity::from_pem(&id_pem)?;

        let client = reqwest::Client::builder()
            .add_root_certificate(ca_cert)
            .identity(identity)
            .build()?;

        Ok(Self {
            client,
            use_tls: true,
        })
    }
}

impl Default for HttpNetworkFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl openraft::RaftNetworkFactory<PiCloudTypeConfig> for HttpNetworkFactory {
    type Network = HttpNetwork;

    async fn new_client(&mut self, _target: u64, node: &BasicNode) -> Self::Network {
        HttpNetwork {
            addr: node.addr.clone(),
            client: self.client.clone(),
            use_tls: self.use_tls,
        }
    }
}

/// A single network connection to a peer Raft node, sending RPCs over HTTP.
pub struct HttpNetwork {
    addr: String,
    client: reqwest::Client,
    use_tls: bool,
}

impl HttpNetwork {
    /// Build the base URL for a Raft RPC, using HTTPS when TLS is enabled.
    fn base_url(&self, path: &str) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{scheme}://{}/{path}", self.addr)
    }
}

impl openraft::RaftNetwork<PiCloudTypeConfig> for HttpNetwork {
    async fn append_entries(
        &mut self,
        rpc: openraft::raft::AppendEntriesRequest<PiCloudTypeConfig>,
        _option: openraft::network::RPCOption,
    ) -> Result<
        openraft::raft::AppendEntriesResponse<u64>,
        openraft::error::RPCError<
            u64,
            BasicNode,
            openraft::error::RaftError<u64>,
        >,
    > {
        let url = self.base_url("raft/append");
        let resp = self
            .client
            .post(&url)
            .json(&rpc)
            .send()
            .await
            .map_err(|e| new_unreachable(e.to_string()))?;

        let body = resp
            .json()
            .await
            .map_err(|e| new_unreachable(e.to_string()))?;
        Ok(body)
    }

    async fn install_snapshot(
        &mut self,
        rpc: openraft::raft::InstallSnapshotRequest<PiCloudTypeConfig>,
        _option: openraft::network::RPCOption,
    ) -> Result<
        openraft::raft::InstallSnapshotResponse<u64>,
        openraft::error::RPCError<
            u64,
            BasicNode,
            openraft::error::RaftError<u64, openraft::error::InstallSnapshotError>,
        >,
    > {
        let url = self.base_url("raft/snapshot");
        let resp = self
            .client
            .post(&url)
            .json(&rpc)
            .send()
            .await
            .map_err(|e| new_unreachable(e.to_string()))?;

        let body = resp
            .json()
            .await
            .map_err(|e| new_unreachable(e.to_string()))?;
        Ok(body)
    }

    async fn vote(
        &mut self,
        rpc: openraft::raft::VoteRequest<u64>,
        _option: openraft::network::RPCOption,
    ) -> Result<
        openraft::raft::VoteResponse<u64>,
        openraft::error::RPCError<
            u64,
            BasicNode,
            openraft::error::RaftError<u64>,
        >,
    > {
        let url = self.base_url("raft/vote");
        let resp = self
            .client
            .post(&url)
            .json(&rpc)
            .send()
            .await
            .map_err(|e| new_unreachable(e.to_string()))?;

        let body = resp
            .json()
            .await
            .map_err(|e| new_unreachable(e.to_string()))?;
        Ok(body)
    }
}

/// Helper to create an `Unreachable` RPC error.
fn new_unreachable<E: std::error::Error + 'static>(
    msg: String,
) -> openraft::error::RPCError<u64, BasicNode, E> {
    openraft::error::RPCError::Unreachable(openraft::error::Unreachable::new(&std::io::Error::new(
        std::io::ErrorKind::ConnectionRefused,
        msg,
    )))
}

// ---------------------------------------------------------------------------
// Raft node builder
// ---------------------------------------------------------------------------

/// Create and initialise a Raft node.
///
/// `node_id` is a `u64` derived from the UUID (we use the lower 64 bits).
/// `addr` is the `host:port` this node listens on for Raft RPCs.
///
/// If `members` is `Some`, the node will be initialised as the founding member
/// of a new cluster. For subsequent nodes that join, pass `None` and use
/// `add_learner` / `change_membership` on the leader instead.
pub async fn create_raft_node(
    node_id: u64,
    _addr: &str,
    apply_callback: Option<ApplyCallback>,
    members: Option<BTreeMap<u64, BasicNode>>,
) -> Result<PiCloudRaft, Box<dyn std::error::Error>> {
    create_raft_node_with_tls(node_id, _addr, apply_callback, members, None).await
}

/// Create and initialise a Raft node with optional mTLS for inter-node RPCs.
pub async fn create_raft_node_with_tls(
    node_id: u64,
    _addr: &str,
    apply_callback: Option<ApplyCallback>,
    members: Option<BTreeMap<u64, BasicNode>>,
    tls_config: Option<RaftTlsConfig>,
) -> Result<PiCloudRaft, Box<dyn std::error::Error>> {
    let config = Config {
        cluster_name: "picloud".to_string(),
        heartbeat_interval: 500,
        election_timeout_min: 1500,
        election_timeout_max: 3000,
        ..Default::default()
    };
    let config = Arc::new(config);

    let log_store = MemLogStore::new();
    let mut sm = MemStateMachine::new();
    if let Some(cb) = apply_callback {
        sm = sm.with_apply_callback(cb);
    }
    let network = match tls_config {
        Some(tls) => HttpNetworkFactory::with_tls(tls)?,
        None => HttpNetworkFactory::new(),
    };

    let raft = PiCloudRaft::new(node_id, config, network, log_store, sm).await?;

    if let Some(members) = members {
        info!(node_id, "Initializing Raft cluster with members");
        raft.initialize(members).await?;
    }

    Ok(raft)
}

/// Create and initialise a persistent Raft node backed by sled.
///
/// `db_path` is the directory where sled stores its data files.
/// The log store and state machine are both persisted, so state survives restarts.
///
/// If `members` is `Some`, the node will be initialised as the founding member
/// of a new cluster. For subsequent nodes that join, pass `None`.
pub async fn create_persistent_raft_node(
    node_id: u64,
    _addr: &str,
    apply_callback: Option<ApplyCallback>,
    members: Option<BTreeMap<u64, BasicNode>>,
    db_path: &std::path::Path,
) -> Result<PiCloudRaft, Box<dyn std::error::Error>> {
    create_persistent_raft_node_with_tls(node_id, _addr, apply_callback, members, db_path, None).await
}

/// Create and initialise a persistent Raft node with optional mTLS.
pub async fn create_persistent_raft_node_with_tls(
    node_id: u64,
    _addr: &str,
    apply_callback: Option<ApplyCallback>,
    members: Option<BTreeMap<u64, BasicNode>>,
    db_path: &std::path::Path,
    tls_config: Option<RaftTlsConfig>,
) -> Result<PiCloudRaft, Box<dyn std::error::Error>> {
    let config = Config {
        cluster_name: "picloud".to_string(),
        heartbeat_interval: 500,
        election_timeout_min: 1500,
        election_timeout_max: 3000,
        ..Default::default()
    };
    let config = Arc::new(config);

    let db = sled::open(db_path)?;
    let log_store = SledLogStore::new(db.clone())?;
    let state_machine = SledStateMachine::new(db, apply_callback)?;
    let network = match tls_config {
        Some(tls) => HttpNetworkFactory::with_tls(tls)?,
        None => HttpNetworkFactory::new(),
    };

    let raft = PiCloudRaft::new(node_id, config, network, log_store, state_machine).await?;

    if let Some(members) = members {
        info!(node_id, "Initializing persistent Raft cluster with members");
        // `initialize` returns an error if the store is already initialized
        // (e.g. restart from an existing sled directory). That is a benign
        // no-op for us — the persisted state is authoritative — so we log
        // and continue instead of propagating. This is the fix for the
        // TC-358 symptom "not allowed to initialize due to current raft
        // state" observed on restart.
        if let Err(e) = raft.initialize(members).await {
            let msg = e.to_string();
            if msg.contains("not allowed to initialize") || msg.contains("NotAllowed") {
                info!(
                    node_id,
                    "Raft store already initialized — preserving persisted state"
                );
            } else {
                return Err(Box::new(e));
            }
        }
    }

    Ok(raft)
}

/// Derive a stable `u64` node ID from a UUID by taking the lower 64 bits.
pub fn node_id_from_uuid(uuid: &uuid::Uuid) -> u64 {
    let bytes = uuid.as_bytes();
    u64::from_be_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ])
}

// ---------------------------------------------------------------------------
// Raft RPC HTTP handlers (axum)
// ---------------------------------------------------------------------------

/// Build an axum `Router` with the three Raft RPC endpoints.
///
/// The returned router must be merged into the main HTTP server router
/// in the composition root (`main.rs`).
pub fn raft_rpc_router(raft: Arc<PiCloudRaft>) -> axum::Router {
    use axum::{extract::State, routing::post, Json, Router};

    #[derive(Clone)]
    struct RaftState {
        raft: Arc<PiCloudRaft>,
    }

    async fn handle_vote(
        State(state): State<RaftState>,
        Json(rpc): Json<openraft::raft::VoteRequest<u64>>,
    ) -> Result<Json<openraft::raft::VoteResponse<u64>>, axum::http::StatusCode> {
        state
            .raft
            .vote(rpc)
            .await
            .map(Json)
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
    }

    async fn handle_append(
        State(state): State<RaftState>,
        Json(rpc): Json<openraft::raft::AppendEntriesRequest<PiCloudTypeConfig>>,
    ) -> Result<
        Json<openraft::raft::AppendEntriesResponse<u64>>,
        axum::http::StatusCode,
    > {
        state
            .raft
            .append_entries(rpc)
            .await
            .map(Json)
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
    }

    async fn handle_snapshot(
        State(state): State<RaftState>,
        Json(rpc): Json<openraft::raft::InstallSnapshotRequest<PiCloudTypeConfig>>,
    ) -> Result<
        Json<openraft::raft::InstallSnapshotResponse<u64>>,
        axum::http::StatusCode,
    > {
        state
            .raft
            .install_snapshot(rpc)
            .await
            .map(Json)
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
    }

    Router::new()
        .route("/raft/vote", post(handle_vote))
        .route("/raft/append", post(handle_append))
        .route("/raft/snapshot", post(handle_snapshot))
        .with_state(RaftState { raft })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn test_single_node_elects_leader() {
        let node_id: u64 = 1;
        let addr = "127.0.0.1:21001";

        let mut members = BTreeMap::new();
        members.insert(node_id, BasicNode::new(addr));

        let raft = create_raft_node(node_id, addr, None, Some(members))
            .await
            .expect("failed to create raft node");

        // Wait for the single-node cluster to elect itself leader.
        // openraft is fast with a single node — typically < 100ms.
        let mut metrics_rx = raft.metrics();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let m = metrics_rx.borrow_and_update().clone();
                if m.current_leader == Some(node_id) {
                    break;
                }
                metrics_rx.changed().await.unwrap();
            }
        })
        .await
        .expect("single-node cluster did not elect leader within 5s");

        let leader = raft.current_leader().await;
        assert_eq!(leader, Some(node_id));

        // Write an entry and confirm it is applied.
        let resp = raft
            .client_write(ClientRequest {
                event_json: r#"{"test": true}"#.to_string(),
            })
            .await
            .expect("client_write failed");
        assert_eq!(resp.data.index, 1);

        raft.shutdown().await.expect("shutdown failed");
    }

    #[test]
    fn test_node_id_from_uuid() {
        let uuid = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let nid = node_id_from_uuid(&uuid);
        // Lower 8 bytes of UUID: a716-446655440000
        assert!(nid > 0);
    }
}
