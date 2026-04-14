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
use picloud_domain::resources::VolumeType;
use picloud_domain::storage::StorageIntent;
use picloud_domain::traits::{EventFilter, EventLog, SecretStore, StateProjector, StorageBackend, WorkloadScheduler, WorkloadSpec};
use picloud_domain::workload::{ContainerSpec, BinarySpec, EnvValue, PortMapping, ResourceLimits, RestartPolicy, VolumeMount};
use tracing::{error, info, warn};

use crate::implementation::{IngressRoute, IngressTable};
use crate::router::{RouteEntry, SharedRouter};

/// Configuration for the resource provisioner.
pub struct Provisioner {
    event_log: Arc<dyn EventLog>,
    projector: Arc<dyn StateProjector>,
    storage: Arc<dyn StorageBackend>,
    scheduler: Arc<dyn WorkloadScheduler>,
    secret_store: Arc<dyn SecretStore>,
    iri_builder: IriBuilder,
    ingress_routes: Option<IngressTable>,
    /// Native ingress router (ADR-048).
    ingress_router: Option<SharedRouter>,
}

impl Provisioner {
    /// Create a new provisioner with all required dependencies.
    pub fn new(
        event_log: Arc<dyn EventLog>,
        projector: Arc<dyn StateProjector>,
        storage: Arc<dyn StorageBackend>,
        scheduler: Arc<dyn WorkloadScheduler>,
        secret_store: Arc<dyn SecretStore>,
        iri_builder: IriBuilder,
    ) -> Self {
        Self {
            event_log,
            projector,
            storage,
            scheduler,
            secret_store,
            iri_builder,
            ingress_routes: None,
            ingress_router: None,
        }
    }

    /// Set the ingress routing table so the provisioner can register routes
    /// when Ingress resources are declared.
    pub fn with_ingress_table(mut self, table: IngressTable) -> Self {
        self.ingress_routes = Some(table);
        self
    }

    /// Set the native ingress router (ADR-048).
    pub fn with_ingress_router(mut self, router: SharedRouter) -> Self {
        self.ingress_router = Some(router);
        self
    }

    /// Start the provisioner as a background task.
    /// Returns the JoinHandle so the caller can await or abort it.
    pub async fn start(self) -> Result<tokio::task::JoinHandle<()>, picloud_domain::error::PiCloudError> {
        let filter = EventFilter {
            event_types: vec![
                "ResourceDeclared".to_string(),
                "ProductDeleted".to_string(),
                "NodeUnreachable".to_string(),
            ],
            ..Default::default()
        };
        let mut rx = self.event_log.subscribe(filter).await?;
        let event_log = self.event_log.clone();
        let projector = self.projector.clone();
        let storage = self.storage.clone();
        let scheduler = self.scheduler.clone();
        let secret_store = self.secret_store.clone();
        let ingress_routes = self.ingress_routes.clone();
        let ingress_router = self.ingress_router.clone();
        let iri_builder = self.iri_builder;

        let handle = tokio::spawn(async move {
            info!("Resource provisioner started");
            loop {
                match rx.recv().await {
                    Ok(event) => match event.event_type.as_str() {
                        "ResourceDeclared" => {
                            provision_resource(
                                &event,
                                &event_log,
                                &storage,
                                &scheduler,
                                &secret_store,
                                &ingress_routes,
                                &ingress_router,
                                &iri_builder,
                            )
                            .await;
                        }
                        "ProductDeleted" => {
                            cascade_delete_product(
                                &event,
                                &event_log,
                                &projector,
                                &storage,
                                &scheduler,
                                &iri_builder,
                            )
                            .await;
                        }
                        "NodeUnreachable" => {
                            reschedule_workloads_from_failed_node(
                                &event,
                                &event_log,
                                &projector,
                                &iri_builder,
                            )
                            .await;
                        }
                        _ => {}
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(
                            skipped = n,
                            "Provisioner lagged — some events may have been missed"
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
    secret_store: &Arc<dyn SecretStore>,
    ingress_routes: &Option<IngressTable>,
    ingress_router: &Option<SharedRouter>,
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
        "Secret" => provision_secret(secret_store, payload).await,
        "Ingress" => {
            provision_ingress(ingress_routes, ingress_router, resource_iri_str, payload).await
        }
        "EventSubscription" => {
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

    let volume_type = payload
        .get("volume_type")
        .and_then(|v| serde_json::from_value::<VolumeType>(v.clone()).ok())
        .unwrap_or(VolumeType::Mounted);

    let intent = StorageIntent::default();

    storage.allocate_volume(resource_iri, size_gb, &intent, &volume_type).await?;
    Ok(())
}

/// Provision a secret by encrypting and storing it via SecretStore.
async fn provision_secret(
    secret_store: &Arc<dyn SecretStore>,
    payload: &serde_json::Value,
) -> picloud_domain::error::Result<()> {
    let product = payload
        .get("product")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let value = payload
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    secret_store.store_secret(product, name, value).await?;

    info!(
        product = product,
        name = name,
        "Secret encrypted and stored"
    );

    Ok(())
}

/// Provision an ingress by registering the route in both the legacy table
/// and the native ingress router (ADR-048).
async fn provision_ingress(
    ingress_routes: &Option<IngressTable>,
    ingress_router: &Option<SharedRouter>,
    resource_iri_str: &str,
    payload: &serde_json::Value,
) -> picloud_domain::error::Result<()> {
    let path = payload
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("/")
        .to_string();

    let target_port = payload
        .get("port")
        .and_then(|v| v.as_u64())
        .unwrap_or(8080) as u16;

    let product = payload
        .get("product")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let host = payload
        .get("host")
        .and_then(|v| v.as_str())
        .unwrap_or("picloud.local")
        .to_string();

    let internal = payload
        .get("internal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Legacy IngressTable
    if let Some(table) = ingress_routes {
        let route = IngressRoute {
            path: path.clone(),
            target_port,
            product: product.clone(),
        };
        let mut routes = table.write().await;
        routes.insert(resource_iri_str.to_string(), route);
    }

    // ADR-048: Native ingress router
    if let Some(router) = ingress_router {
        let ingress_iri = picloud_domain::iri::ResourceIri::new(resource_iri_str)
            .unwrap_or_else(|_| {
                picloud_domain::iri::ResourceIri::new(format!(
                    "https://picloud.local/products/{product}/ingresses/unknown"
                ))
                .unwrap()
            });

        let entry = RouteEntry {
            path_prefix: path.clone(),
            upstream_addr: "127.0.0.1".to_string(),
            upstream_port: target_port,
            product: product.clone(),
            internal,
            ingress_iri,
        };
        router.upsert_route(host.clone(), entry).await;
    }

    info!(
        resource_iri = resource_iri_str,
        path = %path,
        host = %host,
        target_port = target_port,
        product = %product,
        internal = internal,
        "Ingress route registered"
    );

    Ok(())
}

/// Parse volume mounts from a JSON payload array.
fn parse_mounts(payload: &serde_json::Value) -> Vec<VolumeMount> {
    payload
        .get("mounts")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| serde_json::from_value::<VolumeMount>(m.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Parse environment variables from a JSON payload object.
fn parse_env(payload: &serde_json::Value) -> std::collections::HashMap<String, EnvValue> {
    payload
        .get("env")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| {
                    let env_val = serde_json::from_value::<EnvValue>(v.clone())
                        .unwrap_or_else(|_| EnvValue::Literal(v.as_str().unwrap_or("").to_string()));
                    (k.clone(), env_val)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse resource limits from a JSON payload.
fn parse_resource_limits(payload: &serde_json::Value) -> ResourceLimits {
    ResourceLimits {
        cpu_millicores: payload
            .get("cpu_millicores")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        memory_mb: payload
            .get("memory_mb")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
    }
}

/// Parse port mappings from a JSON payload array.
fn parse_ports(payload: &serde_json::Value) -> Vec<PortMapping> {
    payload
        .get("ports")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| serde_json::from_value::<PortMapping>(p.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Parse string args from a JSON payload array.
fn parse_args(payload: &serde_json::Value) -> Vec<String> {
    payload
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
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
        resources: parse_resource_limits(payload),
        mounts: parse_mounts(payload),
        env: parse_env(payload),
        ports: parse_ports(payload),
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
        args: parse_args(payload),
        identity,
        resources: parse_resource_limits(payload),
        mounts: parse_mounts(payload),
        env: parse_env(payload),
        restart_policy: RestartPolicy::Always,
    });

    scheduler.schedule(resource_iri, &spec).await?;
    Ok(())
}

/// Handle cascading deletion of a product and all its child resources.
///
/// When a ProductDeleted event arrives:
/// 1. Query the RDF graph to find all resources belonging to the product
/// 2. For each child resource: stop workloads, delete volumes
/// 3. Emit ResourceDeleted events for each child so the RDF projector cleans up
async fn cascade_delete_product(
    event: &EventEnvelope,
    event_log: &Arc<dyn EventLog>,
    projector: &Arc<dyn StateProjector>,
    storage: &Arc<dyn StorageBackend>,
    scheduler: &Arc<dyn WorkloadScheduler>,
    iri_builder: &IriBuilder,
) {
    let product_iri_str = match event.payload.get("product_iri").and_then(|v| v.as_str()) {
        Some(iri) => iri,
        None => {
            warn!(
                event_id = %event.id,
                "ProductDeleted event missing product_iri in payload — skipping"
            );
            return;
        }
    };

    let product_name = match &event.product {
        Some(name) => name.clone(),
        None => {
            warn!(
                event_id = %event.id,
                "ProductDeleted event missing product scope — skipping"
            );
            return;
        }
    };

    info!(
        product_iri = product_iri_str,
        correlation_id = %event.correlation_id,
        "Cascading deletion for product"
    );

    // Query the RDF graph for all resources belonging to this product.
    // Resources belonging to a product have IRIs that start with the product IRI.
    let sparql = format!(
        "SELECT ?res ?rtype WHERE {{ \
            ?res <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://picloud.local/ontology#Resource> . \
            ?res <https://picloud.local/ontology#resourceType> ?rtype . \
            FILTER(STRSTARTS(STR(?res), \"{}/\")) \
        }}",
        product_iri_str
    );

    let child_resources = match projector.query(&sparql).await {
        Ok(result) => result.bindings,
        Err(e) => {
            error!(
                product_iri = product_iri_str,
                error = %e,
                "Failed to query child resources for cascading deletion"
            );
            vec![]
        }
    };

    info!(
        product_iri = product_iri_str,
        child_count = child_resources.len(),
        "Found child resources to delete"
    );

    // Process each child resource
    for child in &child_resources {
        let child_iri_str = match child.get("res")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
        {
            Some(iri) => iri,
            None => continue,
        };

        let child_type = child.get("rtype")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");

        let child_iri = ResourceIri(child_iri_str.to_string());

        info!(
            resource_iri = child_iri_str,
            resource_type = child_type,
            "Deleting child resource"
        );

        // Stop workloads or delete volumes based on resource type
        match child_type {
            "Container" | "Binary" => {
                if let Err(e) = scheduler.stop(&child_iri).await {
                    warn!(
                        resource_iri = child_iri_str,
                        error = %e,
                        "Failed to stop workload during cascading deletion — continuing"
                    );
                }
            }
            "Volume" => {
                if let Err(e) = storage.delete_volume(&child_iri).await {
                    warn!(
                        resource_iri = child_iri_str,
                        error = %e,
                        "Failed to delete volume during cascading deletion — continuing"
                    );
                }
            }
            // Ingress, EventSubscription, etc. — no backend cleanup needed
            _ => {}
        }

        // Emit ResourceDeleted event for each child
        let schema = iri_builder.event_schema("ResourceDeleted", 1);
        let envelope = EventEnvelope::new(
            schema,
            "ResourceDeleted",
            child_iri,
            Some(product_name.clone()),
            event.correlation_id,
            serde_json::json!({
                "resource_iri": child_iri_str,
            }),
        );

        if let Err(e) = event_log.append(envelope).await {
            error!(
                resource_iri = child_iri_str,
                error = %e,
                "Failed to emit ResourceDeleted event during cascading deletion"
            );
        }
    }

    info!(
        product_iri = product_iri_str,
        deleted_count = child_resources.len(),
        "Cascading deletion complete"
    );
}

/// Handle workload rescheduling when a node becomes unreachable.
///
/// When a NodeUnreachable event arrives:
/// 1. Extract the failed node's IRI from the event payload
/// 2. Query the RDF graph for workloads that were running on the failed node
/// 3. For each workload, emit a ResourceDeclared event to reschedule it
/// 4. Emit a WorkloadRescheduled event for tracking
async fn reschedule_workloads_from_failed_node(
    event: &EventEnvelope,
    event_log: &Arc<dyn EventLog>,
    projector: &Arc<dyn StateProjector>,
    iri_builder: &IriBuilder,
) {
    let node_iri_str = match event.payload.get("node_iri").and_then(|v| v.as_str()) {
        Some(iri) => iri,
        None => {
            warn!(
                event_id = %event.id,
                "NodeUnreachable event missing node_iri in payload — skipping"
            );
            return;
        }
    };

    let node_name = event
        .payload
        .get("node_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    info!(
        node_iri = node_iri_str,
        node_name = node_name,
        "Handling node failure — querying for affected workloads"
    );

    // Query the RDF graph for workloads that were scheduled on the failed node.
    // Workloads are linked to nodes via picloud:scheduledOn predicate.
    let sparql = format!(
        "SELECT ?workload ?rtype ?product WHERE {{ \
            ?workload <https://picloud.local/ontology#scheduledOn> <{}> . \
            ?workload <https://picloud.local/ontology#resourceType> ?rtype . \
            OPTIONAL {{ ?workload <https://picloud.local/ontology#product> ?product }} \
        }}",
        node_iri_str
    );

    let affected_workloads = match projector.query(&sparql).await {
        Ok(result) => result.bindings,
        Err(e) => {
            error!(
                node_iri = node_iri_str,
                error = %e,
                "Failed to query workloads on failed node"
            );
            return;
        }
    };

    if affected_workloads.is_empty() {
        info!(
            node_iri = node_iri_str,
            "No workloads found on failed node — nothing to reschedule"
        );
        return;
    }

    info!(
        node_iri = node_iri_str,
        workload_count = affected_workloads.len(),
        "Found workloads to reschedule from failed node"
    );

    for workload in &affected_workloads {
        let workload_iri_str = match workload
            .get("workload")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
        {
            Some(iri) => iri,
            None => continue,
        };

        let resource_type = workload
            .get("rtype")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("Container");

        let product = workload
            .get("product")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let workload_iri = ResourceIri(workload_iri_str.to_string());
        let correlation_id = uuid::Uuid::new_v4();

        info!(
            workload_iri = workload_iri_str,
            resource_type = resource_type,
            "Rescheduling workload from failed node"
        );

        // Emit WorkloadRescheduled event for tracking
        let rescheduled_schema = iri_builder.event_schema("WorkloadRescheduled", 1);
        let rescheduled_event = EventEnvelope::new(
            rescheduled_schema,
            "WorkloadRescheduled",
            workload_iri.clone(),
            product.clone(),
            correlation_id,
            serde_json::json!({
                "workload_iri": workload_iri_str,
                "from_node_iri": node_iri_str,
                "reason": format!("Node {} became unreachable", node_name),
            }),
        );

        if let Err(e) = event_log.append(rescheduled_event).await {
            error!(
                workload_iri = workload_iri_str,
                error = %e,
                "Failed to emit WorkloadRescheduled event"
            );
        }

        // Re-emit ResourceDeclared to trigger re-provisioning on a healthy node
        let declared_schema = iri_builder.event_schema("ResourceDeclared", 1);
        let declared_event = EventEnvelope::new(
            declared_schema,
            "ResourceDeclared",
            workload_iri,
            product,
            correlation_id,
            serde_json::json!({
                "resource_iri": workload_iri_str,
                "resource_type": resource_type,
            }),
        );

        if let Err(e) = event_log.append(declared_event).await {
            error!(
                workload_iri = workload_iri_str,
                error = %e,
                "Failed to emit ResourceDeclared event for rescheduling"
            );
        }
    }

    info!(
        node_iri = node_iri_str,
        rescheduled_count = affected_workloads.len(),
        "Workload rescheduling complete for failed node"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use picloud_domain::error::Result;
    use picloud_domain::iri::ClusterDomain;
    use picloud_domain::traits::{QueryResult, SecretStore, VolumeHandle, WorkloadHandle, WorkloadStatus};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    // ---- Mock SecretStore ----

    struct MockSecretStore {
        secrets: Mutex<HashMap<String, String>>,
    }

    impl MockSecretStore {
        fn new() -> Self {
            Self {
                secrets: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl SecretStore for MockSecretStore {
        async fn store_secret(&self, product: &str, name: &str, value: &str) -> Result<()> {
            self.secrets
                .lock()
                .unwrap()
                .insert(format!("{}/{}", product, name), value.to_string());
            Ok(())
        }

        async fn get_secret(&self, product: &str, name: &str) -> Result<String> {
            let key = format!("{}/{}", product, name);
            self.secrets
                .lock()
                .unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| picloud_domain::error::PiCloudError::SecretNotFound {
                    product: product.to_string(),
                    name: name.to_string(),
                })
        }

        async fn delete_secret(&self, product: &str, name: &str) -> Result<()> {
            self.secrets
                .lock()
                .unwrap()
                .remove(&format!("{}/{}", product, name));
            Ok(())
        }
    }

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

        async fn events_since(&self, offset: usize) -> Vec<EventEnvelope> {
            let events = self.appended.lock().unwrap();
            if offset >= events.len() {
                Vec::new()
            } else {
                events[offset..].to_vec()
            }
        }
    }

    // ---- Mock StorageBackend ----

    struct MockStorage {
        allocated: Mutex<Vec<String>>,
        deleted: Mutex<Vec<String>>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                allocated: Mutex::new(Vec::new()),
                deleted: Mutex::new(Vec::new()),
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
            _volume_type: &VolumeType,
        ) -> Result<VolumeHandle> {
            self.allocated.lock().unwrap().push(volume_iri.0.clone());
            Ok(VolumeHandle {
                volume_iri: volume_iri.clone(),
                device_path: "/dev/test".to_string(),
                replicated_to: vec![],
            })
        }

        async fn delete_volume(&self, volume_iri: &ResourceIri) -> Result<()> {
            self.deleted.lock().unwrap().push(volume_iri.0.clone());
            Ok(())
        }

        async fn available_capacity_gb(&self) -> Result<u64> {
            Ok(100)
        }
    }

    // ---- Mock StateProjector ----

    struct MockProjector {
        resources: Mutex<Vec<(String, String)>>, // (iri, type)
    }

    impl MockProjector {
        fn new() -> Self {
            Self {
                resources: Mutex::new(Vec::new()),
            }
        }

        fn add_resource(&self, iri: &str, resource_type: &str) {
            self.resources.lock().unwrap().push((iri.to_string(), resource_type.to_string()));
        }
    }

    #[async_trait]
    impl StateProjector for MockProjector {
        async fn project(&self, _event: &EventEnvelope) -> Result<()> {
            Ok(())
        }

        async fn query(&self, sparql: &str) -> Result<QueryResult> {
            // Return matching resources when query contains STRSTARTS filter
            let resources = self.resources.lock().unwrap();
            let bindings: Vec<serde_json::Value> = resources
                .iter()
                .filter(|(iri, _)| {
                    // Simple check: if the SPARQL contains a product IRI prefix filter,
                    // only return resources whose IRI starts with it
                    let needle = "STRSTARTS(STR(?res), \"";
                    if let Some(start) = sparql.find(needle) {
                        let after = &sparql[start + needle.len()..];
                        if let Some(end) = after.find("\")") {
                            let prefix = &after[..end];
                            return iri.starts_with(prefix);
                        }
                    }
                    true
                })
                .map(|(iri, rtype)| {
                    serde_json::json!({
                        "res": { "type": "uri", "value": iri },
                        "rtype": { "type": "literal", "value": rtype },
                    })
                })
                .collect();
            Ok(QueryResult { bindings })
        }

        async fn query_product(
            &self,
            _product_iri: &ResourceIri,
            _sparql: &str,
        ) -> Result<QueryResult> {
            Ok(QueryResult { bindings: vec![] })
        }
    }

    // ---- Mock WorkloadScheduler ----

    struct MockScheduler {
        scheduled: Mutex<Vec<String>>,
        stopped: Mutex<Vec<String>>,
    }

    impl MockScheduler {
        fn new() -> Self {
            Self {
                scheduled: Mutex::new(Vec::new()),
                stopped: Mutex::new(Vec::new()),
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

        async fn stop(&self, workload_iri: &ResourceIri) -> Result<()> {
            self.stopped.lock().unwrap().push(workload_iri.0.clone());
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
        let secret_store = Arc::new(MockSecretStore::new());
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
            &(secret_store.clone() as Arc<dyn SecretStore>),
            &None,
            &None,
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
        let secret_store = Arc::new(MockSecretStore::new());
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
            &(secret_store.clone() as Arc<dyn SecretStore>),
            &None,
            &None,
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
        let secret_store = Arc::new(MockSecretStore::new());
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
            &(secret_store.clone() as Arc<dyn SecretStore>),
            &None,
            &None,
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
    async fn ingress_emits_ready_and_registers_route() {
        let event_log = Arc::new(MockEventLog::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let secret_store = Arc::new(MockSecretStore::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());
        let ingress_table = crate::new_ingress_table();

        let event = make_resource_declared(
            "Ingress",
            "https://picloud.local/products/test-product/ingresses/web",
            serde_json::json!({
                "path": "/products/test-product/api",
                "port": 8080,
                "product": "test-product"
            }),
        );

        provision_resource(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &(secret_store.clone() as Arc<dyn SecretStore>),
            &Some(ingress_table.clone()),
            &None,
            &iri_builder,
        )
        .await;

        // No storage or scheduler calls
        assert!(storage.allocated.lock().unwrap().is_empty());
        assert!(scheduler.scheduled.lock().unwrap().is_empty());

        // ResourceReady emitted
        let appended = event_log.appended_events();
        assert_eq!(appended.len(), 1);
        assert_eq!(appended[0].event_type, "ResourceReady");

        // Ingress route should be registered in the table
        let routes = ingress_table.read().await;
        assert_eq!(routes.len(), 1);
        let route = routes.values().next().unwrap();
        assert_eq!(route.path, "/products/test-product/api");
        assert_eq!(route.target_port, 8080);
        assert_eq!(route.product, "test-product");
    }

    #[tokio::test]
    async fn event_subscription_emits_ready_immediately() {
        let event_log = Arc::new(MockEventLog::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let secret_store = Arc::new(MockSecretStore::new());
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
            &(secret_store.clone() as Arc<dyn SecretStore>),
            &None,
            &None,
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
        let secret_store = Arc::new(MockSecretStore::new());
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
            &(secret_store.clone() as Arc<dyn SecretStore>),
            &None,
            &None,
            &iri_builder,
        )
        .await;

        // Nothing should be appended
        assert!(event_log.appended_events().is_empty());
    }

    #[tokio::test]
    async fn provisioner_start_and_process_event() {
        let event_log = Arc::new(MockEventLog::new());
        let projector = Arc::new(MockProjector::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let secret_store = Arc::new(MockSecretStore::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        let provisioner = Provisioner::new(
            event_log.clone(),
            projector.clone(),
            storage.clone(),
            scheduler.clone(),
            secret_store.clone(),
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

    // ---- Helper to build a ProductDeleted event ----

    fn make_product_deleted(product_name: &str) -> EventEnvelope {
        let iri_builder = IriBuilder::new(ClusterDomain::default());
        let product_iri = iri_builder.product(product_name);
        EventEnvelope::new(
            iri_builder.event_schema("ProductDeleted", 1),
            "ProductDeleted",
            product_iri.clone(),
            Some(product_name.to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "product_iri": product_iri.as_str(),
            }),
        )
    }

    #[tokio::test]
    async fn cascade_delete_stops_workloads_and_deletes_volumes() {
        let event_log = Arc::new(MockEventLog::new());
        let projector = Arc::new(MockProjector::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        // Register child resources in the mock projector
        projector.add_resource(
            "https://picloud.local/products/test-product/containers/api",
            "Container",
        );
        projector.add_resource(
            "https://picloud.local/products/test-product/volumes/data",
            "Volume",
        );
        projector.add_resource(
            "https://picloud.local/products/test-product/binaries/worker",
            "Binary",
        );
        projector.add_resource(
            "https://picloud.local/products/test-product/ingresses/web",
            "Ingress",
        );

        let event = make_product_deleted("test-product");

        cascade_delete_product(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(projector.clone() as Arc<dyn StateProjector>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &iri_builder,
        )
        .await;

        // Scheduler should have stopped Container and Binary workloads
        let stopped = scheduler.stopped.lock().unwrap();
        assert_eq!(stopped.len(), 2);
        assert!(stopped.contains(&"https://picloud.local/products/test-product/containers/api".to_string()));
        assert!(stopped.contains(&"https://picloud.local/products/test-product/binaries/worker".to_string()));

        // Storage should have deleted the volume
        let deleted = storage.deleted.lock().unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(
            deleted[0],
            "https://picloud.local/products/test-product/volumes/data"
        );

        // ResourceDeleted events should have been emitted for all 4 child resources
        let appended = event_log.appended_events();
        assert_eq!(appended.len(), 4);
        for evt in &appended {
            assert_eq!(evt.event_type, "ResourceDeleted");
            assert_eq!(evt.correlation_id, event.correlation_id);
        }
    }

    #[tokio::test]
    async fn cascade_delete_with_no_children_emits_nothing() {
        let event_log = Arc::new(MockEventLog::new());
        let projector = Arc::new(MockProjector::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        let event = make_product_deleted("empty-product");

        cascade_delete_product(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(projector.clone() as Arc<dyn StateProjector>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &iri_builder,
        )
        .await;

        assert!(event_log.appended_events().is_empty());
        assert!(scheduler.stopped.lock().unwrap().is_empty());
        assert!(storage.deleted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cascade_delete_missing_product_iri_skips() {
        let event_log = Arc::new(MockEventLog::new());
        let projector = Arc::new(MockProjector::new());
        let storage = Arc::new(MockStorage::new());
        let scheduler = Arc::new(MockScheduler::new());
        let iri_builder = IriBuilder::new(ClusterDomain::default());

        // Event without product_iri in payload
        let event = EventEnvelope::new(
            iri_builder.event_schema("ProductDeleted", 1),
            "ProductDeleted",
            ResourceIri("https://picloud.local/test".to_string()),
            Some("test-product".to_string()),
            Uuid::new_v4(),
            serde_json::json!({}),
        );

        cascade_delete_product(
            &event,
            &(event_log.clone() as Arc<dyn EventLog>),
            &(projector.clone() as Arc<dyn StateProjector>),
            &(storage.clone() as Arc<dyn StorageBackend>),
            &(scheduler.clone() as Arc<dyn WorkloadScheduler>),
            &iri_builder,
        )
        .await;

        // Should have skipped — nothing emitted
        assert!(event_log.appended_events().is_empty());
    }
}
