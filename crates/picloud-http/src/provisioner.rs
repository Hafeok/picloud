/// Resource Provisioner
///
/// Background task that watches the event log for ResourceDeclared events
/// and provisions resources by calling the appropriate backend trait:
///   - Volumes  -> StorageBackend::allocate_volume()
///   - Containers -> WorkloadScheduler::schedule()
///   - Binaries -> WorkloadScheduler::schedule()
///   - Others (ingress, event-subscription) -> immediate ResourceReady
///
/// After provisioning, emits ResourceReady (success) or ResourceFailed (failure)
/// back into the event log so the RDF projector updates the graph and CLI
/// subscribers see the terminal event.

use std::sync::Arc;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{IriBuilder, ResourceIri};
use picloud_domain::storage::StorageIntent;
use picloud_domain::traits::{EventFilter, EventLog, StorageBackend, WorkloadScheduler, WorkloadSpec};
use picloud_domain::workload::{ContainerSpec, BinarySpec, ResourceLimits, RestartPolicy};
use tracing::{error, info, warn};

/// Configuration for the resource provisioner.
pub struct Provisioner {
    event_log: Arc<dyn EventLog>,
    storage: Arc<dyn StorageBackend>,
    scheduler: Arc<dyn WorkloadScheduler>,
    iri_builder: IriBuilder,
}

impl Provisioner {
    /// Create a new provisioner with all required dependencies.
    pub fn new(
        event_log: Arc<dyn EventLog>,
        storage: Arc<dyn StorageBackend>,
        scheduler: Arc<dyn WorkloadScheduler>,
        iri_builder: IriBuilder,
    ) -> Self {
        Self {
            event_log,
            storage,
            scheduler,
            iri_builder,
        }
    }

    /// Start the provisioner as a background task.
    /// Returns the JoinHandle so the caller can await or abort it.
    pub async fn start(self) -> Result<tokio::task::JoinHandle<()>, picloud_domain::error::PiCloudError> {
        let filter = EventFilter {
            event_types: vec!["ResourceDeclared".to_string()],
            ..Default::default()
        };
        let mut rx = self.event_log.subscribe(filter).await?;
        let event_log = self.event_log.clone();
        let storage = self.storage.clone();
        let scheduler = self.scheduler.clone();
        let iri_builder = self.iri_builder;

        let handle = tokio::spawn(async move {
            info!("Resource provisioner started");
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        provision_resource(
                            &event,
                            &event_log,
                            &storage,
                            &scheduler,
                            &iri_builder,
                        )
                        .await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(
                            skipped = n,
                            "Provisioner lagged — some ResourceDeclared events may have been missed"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Event log closed — provisioner stopping");
                        break;
                    }
                }
            }
        });

        Ok(handle)
    }
}

/// Process a single ResourceDeclared event and provision the resource.
async fn provision_resource(
    event: &EventEnvelope,
    event_log: &Arc<dyn EventLog>,
    storage: &Arc<dyn StorageBackend>,
    scheduler: &Arc<dyn WorkloadScheduler>,
    iri_builder: &IriBuilder,
) {
    let payload = &event.payload;

    let resource_type = match payload.get("resource_type").and_then(|v| v.as_str()) {
        Some(rt) => rt,
        None => {
            warn!(
                event_id = %event.id,
                "ResourceDeclared event missing resource_type in payload — skipping"
            );
            return;
        }
    };

    let resource_iri_str = match payload.get("resource_iri").and_then(|v| v.as_str()) {
        Some(iri) => iri,
        None => {
            warn!(
                event_id = %event.id,
                "ResourceDeclared event missing resource_iri in payload — skipping"
            );
            return;
        }
    };

    let resource_iri = ResourceIri(resource_iri_str.to_string());

    info!(
        resource_type = resource_type,
        resource_iri = resource_iri_str,
        correlation_id = %event.correlation_id,
        "Provisioning resource"
    );

    let result = match resource_type {
        "Volume" => provision_volume(storage, &resource_iri, payload).await,
        "Container" => provision_container(scheduler, &resource_iri, payload).await,
        "Binary" => provision_binary(scheduler, &resource_iri, payload).await,
        "Ingress" | "EventSubscription" => {
            // These resource types don't need backend provisioning — mark ready immediately
            info!(
                resource_type = resource_type,
                resource_iri = resource_iri_str,
                "Resource type requires no provisioning — marking ready"
            );
            Ok(())
        }
        other => {
            warn!(
                resource_type = other,
                resource_iri = resource_iri_str,
                "Unknown resource type — marking ready"
            );
            Ok(())
        }
    };

    // Emit ResourceReady or ResourceFailed
    let (event_type, result_payload) = match result {
        Ok(()) => {
            info!(
                resource_iri = resource_iri_str,
                "Resource provisioned successfully"
            );
            (
                "ResourceReady",
                serde_json::json!({
                    "resource_iri": resource_iri_str,
                }),
            )
        }
        Err(e) => {
            error!(
                resource_iri = resource_iri_str,
                error = %e,
                "Resource provisioning failed"
            );
            (
                "ResourceFailed",
                serde_json::json!({
                    "resource_iri": resource_iri_str,
                    "reason": e.to_string(),
                }),
            )
        }
    };

    let schema = iri_builder.event_schema(event_type, 1);
    let envelope = EventEnvelope::new(
        schema,
        event_type,
        resource_iri,
        event.product.clone(),
        event.correlation_id,
        result_payload,
    );

    if let Err(e) = event_log.append(envelope).await {
        error!(
            event_type = event_type,
            error = %e,
            "Failed to emit provisioning result event"
        );
    }
}

/// Provision a volume by calling StorageBackend::allocate_volume.
async fn provision_volume(
    storage: &Arc<dyn StorageBackend>,
    resource_iri: &ResourceIri,
    payload: &serde_json::Value,
) -> picloud_domain::error::Result<()> {
    let size_gb = payload
        .get("size_gb")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);

    let intent = StorageIntent::default();

    storage.allocate_volume(resource_iri, size_gb, &intent).await?;
    Ok(())
}

/// Provision a container by calling WorkloadScheduler::schedule.
async fn provision_container(
    scheduler: &Arc<dyn WorkloadScheduler>,
    resource_iri: &ResourceIri,
    payload: &serde_json::Value,
) -> picloud_domain::error::Result<()> {
    let image = payload
        .get("image")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let identity = payload
        .get("identity")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let spec = WorkloadSpec::Container(ContainerSpec {
        image,
        identity,
        resources: ResourceLimits {
            cpu_millicores: None,
            memory_mb: None,
        },
        mounts: vec![],
        env: std::collections::HashMap::new(),
        ports: vec![],
        health_check: None,
        restart_policy: RestartPolicy::Always,
    });

    scheduler.schedule(resource_iri, &spec).await?;
    Ok(())
}

/// Provision a binary by calling WorkloadScheduler::schedule.
async fn provision_binary(
    scheduler: &Arc<dyn WorkloadScheduler>,
    resource_iri: &ResourceIri,
    payload: &serde_json::Value,
) -> picloud_domain::error::Result<()> {
    let executable = payload
        .get("executable")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let identity = payload
        .get("identity")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let spec = WorkloadSpec::Binary(BinarySpec {
        executable,
        args: vec![],
        identity,
        resources: ResourceLimits {
            cpu_millicores: None,
            memory_mb: None,
        },
        mounts: vec![],
        env: std::collections::HashMap::new(),
        restart_policy: RestartPolicy::Always,
    });

    scheduler.schedule(resource_iri, &spec).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use picloud_domain::error::Result;
    use picloud_domain::iri::ClusterDomain;
    use picloud_domain::traits::{VolumeHandle, WorkloadHandle, WorkloadStatus};
    use std::sync::Mutex;
    use uuid::Uuid;

    // ---- Mock EventLog ----

    struct MockEventLog {
        appended: Mutex<Vec<EventEnvelope>>,
        tx: tokio::sync::broadcast::Sender<EventEnvelope>,
    }

    impl MockEventLog {
        fn new() -> Self {
            let (tx, _) = tokio::sync::broadcast::channel(64);
            Self {
                appended: Mutex::new(Vec::new()),
                tx,
            }
        }

        fn send(&self, event: EventEnvelope) {
            self.tx.send(event).ok();
        }

        fn appended_events(&self) -> Vec<EventEnvelope> {
            self.appended.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl EventLog for MockEventLog {
        async fn append(&self, event: EventEnvelope) -> Result<()> {
            self.appended.lock().unwrap().push(event);
            Ok(())
        }

        async fn subscribe(
            &self,
            _filter: EventFilter,
        ) -> Result<tokio::sync::broadcast::Receiver<EventEnvelope>> {
            Ok(self.tx.subscribe())
        }
    }

    // ---- Mock StorageBackend ----

    struct MockStorage {
        allocated: Mutex<Vec<String>>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                allocated: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl StorageBackend for MockStorage {
        async fn allocate_volume(
            &self,
            volume_iri: &ResourceIri,
            _size_gb: u64,
            _intent: &StorageIntent,
        ) -> Result<VolumeHandle> {
            self.allocated.lock().unwrap().push(volume_iri.0.clone());
            Ok(VolumeHandle {
                volume_iri: volume_iri.clone(),
                device_path: "/dev/test".to_string(),
                replicated_to: vec![],
            })
        }

        async fn delete_volume(&self, _volume_iri: &ResourceIri) -> Result<()> {
            Ok(())
        }

        async fn available_capacity_gb(&self) -> Result<u64> {
            Ok(100)
        }
    }

    // ---- Mock WorkloadScheduler ----

    struct MockScheduler {
        scheduled: Mutex<Vec<String>>,
    }

    impl MockScheduler {
        fn new() -> Self {
            Self {
                scheduled: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl WorkloadScheduler for MockScheduler {
        async fn schedule(
            &self,
            workload_iri: &ResourceIri,
            _spec: &WorkloadSpec,
        ) -> Result<WorkloadHandle> {
            self.scheduled.lock().unwrap().push(workload_iri.0.clone());
            Ok(WorkloadHandle {
                workload_iri: workload_iri.clone(),
                node_id: Uuid::new_v4(),
                pid: Some(1234),
            })
        }

        async fn stop(&self, _workload_iri: &ResourceIri) -> Result<()> {
            Ok(())
        }

        async fn status(&self, _workload_iri: &ResourceIri) -> Result<WorkloadStatus> {
            Ok(WorkloadStatus::Running)
        }
    }

    // ---- Helper to build a ResourceDeclared event ----

    fn make_resource_declared(
        resource_type: &str,
        resource_iri: &str,
        extra: serde_json::Value,
    ) -> EventEnvelope {
        let iri_builder = IriBuilder::new(ClusterDomain::default());
        let mut payload = serde_json::json!({
            "resource_type": resource_type,
            "resource_iri": resource_iri,
        });
        if let serde_json::Value::Object(map) = extra {
            if let serde_json::Value::Object(ref mut target) = payload {
                target.extend(map);
            }
        }
        EventEnvelope::new(
            iri_builder.event_schema("ResourceDeclared", 1),
            "ResourceDeclared",
            ResourceIri(resource_iri.to_string()),
            Some("test-product".to_string()),
            Uuid::new_v4(),
            payload,
        )
    }

    #[tokio::test]
    async fn provisions_volume_and_emits_ready() {
        let event_log = Arc::new(MockEventLog::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        let event = make_resource_declared(
            "Volume",
            "https://picloud.local/products/test-product/volumes/data",
            serde_json::json!({ "size_gb": 10 }),
        );

        provision_resource(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &iri_builder,
        )
        .await;

        // Storage should have been called
        let allocated = storage.allocated.lock().unwrap();
        assert_eq!(allocated.len(), 1);
        assert_eq!(
            allocated[0],
            "https://picloud.local/products/test-product/volumes/data"
        );

        // ResourceReady should have been emitted
        let appended = event_log.appended_events();
        assert_eq!(appended.len(), 1);
        assert_eq!(appended[0].event_type, "ResourceReady");
        assert_eq!(appended[0].correlation_id, event.correlation_id);
    }

    #[tokio::test]
    async fn provisions_container_and_emits_ready() {
        let event_log = Arc::new(MockEventLog::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        let event = make_resource_declared(
            "Container",
            "https://picloud.local/products/test-product/containers/api",
            serde_json::json!({ "image": "my-image:latest", "identity": "api-id" }),
        );

        provision_resource(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &iri_builder,
        )
        .await;

        let scheduled = scheduler.scheduled.lock().unwrap();
        assert_eq!(scheduled.len(), 1);
        assert_eq!(
            scheduled[0],
            "https://picloud.local/products/test-product/containers/api"
        );

        let appended = event_log.appended_events();
        assert_eq!(appended.len(), 1);
        assert_eq!(appended[0].event_type, "ResourceReady");
    }

    #[tokio::test]
    async fn provisions_binary_and_emits_ready() {
        let event_log = Arc::new(MockEventLog::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        let event = make_resource_declared(
            "Binary",
            "https://picloud.local/products/test-product/binaries/worker",
            serde_json::json!({ "executable": "/usr/bin/worker" }),
        );

        provision_resource(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &iri_builder,
        )
        .await;

        let scheduled = scheduler.scheduled.lock().unwrap();
        assert_eq!(scheduled.len(), 1);

        let appended = event_log.appended_events();
        assert_eq!(appended.len(), 1);
        assert_eq!(appended[0].event_type, "ResourceReady");
    }

    #[tokio::test]
    async fn ingress_emits_ready_immediately() {
        let event_log = Arc::new(MockEventLog::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        let event = make_resource_declared(
            "Ingress",
            "https://picloud.local/products/test-product/ingresses/web",
            serde_json::json!({}),
        );

        provision_resource(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &iri_builder,
        )
        .await;

        // No storage or scheduler calls
        assert!(storage.allocated.lock().unwrap().is_empty());
        assert!(scheduler.scheduled.lock().unwrap().is_empty());

        // But ResourceReady emitted
        let appended = event_log.appended_events();
        assert_eq!(appended.len(), 1);
        assert_eq!(appended[0].event_type, "ResourceReady");
    }

    #[tokio::test]
    async fn event_subscription_emits_ready_immediately() {
        let event_log = Arc::new(MockEventLog::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        let event = make_resource_declared(
            "EventSubscription",
            "https://picloud.local/products/test-product/event-subscriptions/on-order",
            serde_json::json!({}),
        );

        provision_resource(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &iri_builder,
        )
        .await;

        let appended = event_log.appended_events();
        assert_eq!(appended.len(), 1);
        assert_eq!(appended[0].event_type, "ResourceReady");
    }

    #[tokio::test]
    async fn missing_resource_type_skips_event() {
        let event_log = Arc::new(MockEventLog::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        // Event with no resource_type in payload
        let event = EventEnvelope::new(
            iri_builder.event_schema("ResourceDeclared", 1),
            "ResourceDeclared",
            ResourceIri("https://picloud.local/test".to_string()),
            None,
            Uuid::new_v4(),
            serde_json::json!({}),
        );

        provision_resource(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &iri_builder,
        )
        .await;

        // Nothing should be appended
        assert!(event_log.appended_events().is_empty());
    }

    #[tokio::test]
    async fn provisioner_start_and_process_event() {
        let event_log = Arc::new(MockEventLog::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        let provisioner = Provisioner::new(
            event_log.clone(),
            storage.clone(),
            scheduler.clone(),
            iri_builder,
        );

        let handle = provisioner.start().await.expect("provisioner should start");

        // Give the spawned task time to subscribe
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send a ResourceDeclared event through the broadcast channel
        let event = make_resource_declared(
            "Volume",
            "https://picloud.local/products/test-product/volumes/db",
            serde_json::json!({ "size_gb": 5 }),
        );
        event_log.send(event);

        // Give provisioner time to process
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let allocated = storage.allocated.lock().unwrap();
        assert_eq!(allocated.len(), 1);

        let appended = event_log.appended_events();
        assert_eq!(appended.len(), 1);
        assert_eq!(appended[0].event_type, "ResourceReady");

        handle.abort();
    }
}
