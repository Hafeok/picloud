//! Process-based workload scheduler
//!
//! Phase 1 implementation that tracks workloads in-memory with simulated PIDs.
//! Actual youki/OCI container integration is future work.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use uuid::Uuid;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{WorkloadHandle, WorkloadScheduler, WorkloadSpec, WorkloadStatus};

/// Internal record of a scheduled workload.
struct WorkloadEntry {
    #[allow(dead_code)]
    workload_iri: String,
    #[allow(dead_code)]
    spec: WorkloadSpec,
    status: WorkloadStatus,
    #[allow(dead_code)]
    node_id: Uuid,
    #[allow(dead_code)]
    pid: Option<u32>,
    #[allow(dead_code)]
    started_at: DateTime<Utc>,
}

/// A workload scheduler that tracks workloads in-memory.
///
/// In Phase 1 this simulates scheduling by recording entries and assigning
/// fake PIDs. Real process/container management will be added later.
pub struct ProcessScheduler {
    node_id: Uuid,
    workloads: RwLock<HashMap<String, WorkloadEntry>>,
    iri_builder: IriBuilder,
    next_pid: AtomicU32,
}

impl ProcessScheduler {
    /// Create a new scheduler for the given node.
    pub fn new(node_id: Uuid, domain: ClusterDomain) -> Self {
        Self {
            node_id,
            workloads: RwLock::new(HashMap::new()),
            iri_builder: IriBuilder::new(domain),
            next_pid: AtomicU32::new(10000),
        }
    }

    /// Returns a reference to the IRI builder.
    pub fn iri_builder(&self) -> &IriBuilder {
        &self.iri_builder
    }
}

#[async_trait]
impl WorkloadScheduler for ProcessScheduler {
    async fn schedule(
        &self,
        workload_iri: &ResourceIri,
        spec: &WorkloadSpec,
    ) -> Result<WorkloadHandle> {
        let key = workload_iri.as_str().to_string();
        let mut workloads = self.workloads.write().await;

        if workloads.contains_key(&key) {
            return Err(PiCloudError::ResourceAlreadyExists { iri: key });
        }

        let pid = self.next_pid.fetch_add(1, Ordering::Relaxed);

        tracing::info!(
            workload_iri = %workload_iri,
            node_id = %self.node_id,
            pid = pid,
            "Scheduled workload"
        );

        let entry = WorkloadEntry {
            workload_iri: key.clone(),
            spec: spec.clone(),
            status: WorkloadStatus::Running,
            node_id: self.node_id,
            pid: Some(pid),
            started_at: Utc::now(),
        };

        workloads.insert(key, entry);

        Ok(WorkloadHandle {
            workload_iri: workload_iri.clone(),
            node_id: self.node_id,
            pid: Some(pid),
        })
    }

    async fn stop(&self, workload_iri: &ResourceIri) -> Result<()> {
        let key = workload_iri.as_str().to_string();
        let mut workloads = self.workloads.write().await;

        let entry = workloads
            .get_mut(&key)
            .ok_or_else(|| PiCloudError::ResourceNotFound { iri: key.clone() })?;

        entry.status = WorkloadStatus::Stopped;

        tracing::info!(
            workload_iri = %workload_iri,
            "Stopped workload"
        );

        Ok(())
    }

    async fn status(&self, workload_iri: &ResourceIri) -> Result<WorkloadStatus> {
        let key = workload_iri.as_str().to_string();
        let workloads = self.workloads.read().await;

        let entry = workloads
            .get(&key)
            .ok_or_else(|| PiCloudError::ResourceNotFound { iri: key.clone() })?;

        Ok(entry.status.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::workload::{BinarySpec, ResourceLimits, RestartPolicy};
    use std::collections::HashMap;

    fn test_scheduler() -> ProcessScheduler {
        ProcessScheduler::new(Uuid::new_v4(), ClusterDomain::default())
    }

    fn test_iri() -> ResourceIri {
        ResourceIri::new("https://picloud.local/products/test-app/containers/web").unwrap()
    }

    fn test_spec() -> WorkloadSpec {
        WorkloadSpec::Binary(BinarySpec {
            executable: "/usr/bin/test".to_string(),
            args: vec![],
            identity: "test-identity".to_string(),
            resources: ResourceLimits {
                cpu_millicores: Some(500),
                memory_mb: Some(256),
            },
            mounts: vec![],
            env: HashMap::new(),
            restart_policy: RestartPolicy::Never,
        })
    }

    #[tokio::test]
    async fn schedule_creates_a_workload() {
        let scheduler = test_scheduler();
        let iri = test_iri();
        let spec = test_spec();

        let handle = scheduler.schedule(&iri, &spec).await.unwrap();

        assert_eq!(handle.workload_iri, iri);
        assert!(handle.pid.is_some());
    }

    #[tokio::test]
    async fn stop_changes_status() {
        let scheduler = test_scheduler();
        let iri = test_iri();
        let spec = test_spec();

        scheduler.schedule(&iri, &spec).await.unwrap();
        scheduler.stop(&iri).await.unwrap();

        let status = scheduler.status(&iri).await.unwrap();
        assert!(matches!(status, WorkloadStatus::Stopped));
    }

    #[tokio::test]
    async fn status_returns_running_after_schedule() {
        let scheduler = test_scheduler();
        let iri = test_iri();
        let spec = test_spec();

        scheduler.schedule(&iri, &spec).await.unwrap();

        let status = scheduler.status(&iri).await.unwrap();
        assert!(matches!(status, WorkloadStatus::Running));
    }

    #[tokio::test]
    async fn scheduling_duplicate_returns_error() {
        let scheduler = test_scheduler();
        let iri = test_iri();
        let spec = test_spec();

        scheduler.schedule(&iri, &spec).await.unwrap();
        let result = scheduler.schedule(&iri, &spec).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::ResourceAlreadyExists { .. }
        ));
    }

    #[tokio::test]
    async fn stopping_unknown_workload_returns_error() {
        let scheduler = test_scheduler();
        let iri = test_iri();

        let result = scheduler.stop(&iri).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::ResourceNotFound { .. }
        ));
    }
}
