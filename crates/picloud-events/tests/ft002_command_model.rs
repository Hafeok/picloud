/// FT-002 Integration Tests — Command Model
///
/// Covers TC-025, TC-026, TC-027.
/// Tests command correlation, progress streaming, and concurrent commands.

use std::sync::Arc;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
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

// ============================================================================
// TC-025 — command_correlation
// ============================================================================
/// Emit a command with a known correlation ID. Subscribe to the result stream.
/// Assert the terminal event carries the same correlation ID.
#[tokio::test]
async fn tc025_command_correlation() {
    let event_log = Arc::new(InMemoryEventLog::new());
    let correlation_id = Uuid::new_v4();
    let ib = iri_builder();
    let resource_iri = ib.resource("photo-app", "containers", "api-server");

    // Subscribe to events filtered by correlation ID
    let filter = EventFilter {
        correlation_id: Some(correlation_id),
        product: None,
        event_types: vec![],
    };
    let mut rx = event_log.subscribe(filter).await.unwrap();

    // Emit a sequence of events with the same correlation ID
    let declared = make_event(
        "ResourceDeclared",
        Some("photo-app"),
        correlation_id,
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "resource_type": "Container",
            "product": "photo-app",
        }),
    );
    event_log.append(declared).await.unwrap();

    let provisioning = make_event(
        "ResourceProvisioning",
        Some("photo-app"),
        correlation_id,
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "message": "scheduling on node pi-02",
        }),
    );
    event_log.append(provisioning).await.unwrap();

    let ready = make_event(
        "ResourceReady",
        Some("photo-app"),
        correlation_id,
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
        }),
    );
    event_log.append(ready).await.unwrap();

    // Verify we receive all 3 events with matching correlation ID
    let mut received = Vec::new();
    for _ in 0..3 {
        let event = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            rx.recv(),
        )
        .await
        .expect("should receive within timeout")
        .expect("channel should not be closed");

        assert_eq!(
            event.correlation_id, correlation_id,
            "All events should carry the command's correlation ID"
        );
        received.push(event.event_type.clone());
    }

    assert_eq!(received, vec!["ResourceDeclared", "ResourceProvisioning", "ResourceReady"]);

    // The terminal event (ResourceReady) should be the last one
    assert_eq!(received.last().unwrap(), "ResourceReady");
}

// ============================================================================
// TC-026 — progress_streaming
// ============================================================================
/// Apply a multi-resource product. Assert intermediate progress events
/// stream before the terminal event.
#[tokio::test]
async fn tc026_progress_streaming() {
    let event_log = Arc::new(InMemoryEventLog::new());
    let correlation_id = Uuid::new_v4();
    let ib = iri_builder();

    // Subscribe to all events for this correlation
    let filter = EventFilter {
        correlation_id: Some(correlation_id),
        product: None,
        event_types: vec![],
    };
    let mut rx = event_log.subscribe(filter).await.unwrap();

    // Simulate a multi-resource product deployment with progress events
    let product_iri = ib.product("photo-app");
    let container_iri = ib.resource("photo-app", "containers", "api-server");
    let volume_iri = ib.resource("photo-app", "volumes", "media-store");

    let events = vec![
        // Product declared
        make_event("ResourceDeclared", Some("photo-app"), correlation_id, serde_json::json!({
            "resource_iri": product_iri.as_str(),
            "resource_type": "Product",
            "product": "photo-app",
        })),
        // Volume declared
        make_event("ResourceDeclared", Some("photo-app"), correlation_id, serde_json::json!({
            "resource_iri": volume_iri.as_str(),
            "resource_type": "Volume",
            "product": "photo-app",
        })),
        // Volume provisioning (intermediate progress)
        make_event("ResourceProvisioning", Some("photo-app"), correlation_id, serde_json::json!({
            "resource_iri": volume_iri.as_str(),
            "message": "allocating 100GB across 3 nodes",
        })),
        // Volume ready
        make_event("ResourceReady", Some("photo-app"), correlation_id, serde_json::json!({
            "resource_iri": volume_iri.as_str(),
        })),
        // Container declared
        make_event("ResourceDeclared", Some("photo-app"), correlation_id, serde_json::json!({
            "resource_iri": container_iri.as_str(),
            "resource_type": "Container",
            "product": "photo-app",
        })),
        // Container provisioning (intermediate progress)
        make_event("ResourceProvisioning", Some("photo-app"), correlation_id, serde_json::json!({
            "resource_iri": container_iri.as_str(),
            "message": "scheduling on node pi-02",
        })),
        // Container ready (terminal)
        make_event("ResourceReady", Some("photo-app"), correlation_id, serde_json::json!({
            "resource_iri": container_iri.as_str(),
        })),
    ];

    // Emit all events
    for event in &events {
        event_log.append(event.clone()).await.unwrap();
    }

    // Collect received events
    let mut received_types = Vec::new();
    for _ in 0..7 {
        let event = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            rx.recv(),
        )
        .await
        .expect("should receive within timeout")
        .expect("channel should not be closed");

        received_types.push(event.event_type.clone());
    }

    // Assert intermediate progress events appear before terminal events
    let provisioning_positions: Vec<usize> = received_types
        .iter()
        .enumerate()
        .filter(|(_, t)| t.as_str() == "ResourceProvisioning")
        .map(|(i, _)| i)
        .collect();

    let ready_positions: Vec<usize> = received_types
        .iter()
        .enumerate()
        .filter(|(_, t)| t.as_str() == "ResourceReady")
        .map(|(i, _)| i)
        .collect();

    assert!(
        !provisioning_positions.is_empty(),
        "Should have provisioning progress events"
    );
    assert!(
        !ready_positions.is_empty(),
        "Should have ready terminal events"
    );

    // At least one provisioning should appear before the last ready
    let last_ready = *ready_positions.last().unwrap();
    let first_provisioning = *provisioning_positions.first().unwrap();
    assert!(
        first_provisioning < last_ready,
        "Provisioning events should stream before terminal ready events"
    );
}

// ============================================================================
// TC-027 — concurrent_commands
// ============================================================================
/// Emit 10 concurrent commands. Assert all 10 terminal events arrive with
/// matching correlation IDs and no events are cross-contaminated.
#[tokio::test]
async fn tc027_concurrent_commands() {
    let event_log = Arc::new(InMemoryEventLog::new());
    let ib = iri_builder();

    // Generate 10 correlation IDs
    let correlation_ids: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();

    // Subscribe to each correlation ID
    let mut receivers = Vec::new();
    for &cid in &correlation_ids {
        let filter = EventFilter {
            correlation_id: Some(cid),
            product: None,
            event_types: vec![],
        };
        let rx = event_log.subscribe(filter).await.unwrap();
        receivers.push((cid, rx));
    }

    // Emit events concurrently — each command gets 3 events
    let mut handles = Vec::new();
    for (i, &cid) in correlation_ids.iter().enumerate() {
        let log = event_log.clone();
        let ib = iri_builder();
        let handle = tokio::spawn(async move {
            let resource_iri = ib.resource("photo-app", "containers", &format!("svc-{i}"));

            let events = vec![
                make_event("ResourceDeclared", Some("photo-app"), cid, serde_json::json!({
                    "resource_iri": resource_iri.as_str(),
                    "resource_type": "Container",
                    "product": "photo-app",
                    "cmd_index": i,
                })),
                make_event("ResourceProvisioning", Some("photo-app"), cid, serde_json::json!({
                    "resource_iri": resource_iri.as_str(),
                    "message": format!("provisioning svc-{i}"),
                })),
                make_event("ResourceReady", Some("photo-app"), cid, serde_json::json!({
                    "resource_iri": resource_iri.as_str(),
                })),
            ];

            for event in events {
                log.append(event).await.unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all emitters
    for handle in handles {
        handle.await.unwrap();
    }

    // Verify each receiver got exactly 3 events, all with correct correlation ID
    for (cid, mut rx) in receivers {
        let mut count = 0;
        let mut event_types = Vec::new();

        // Drain available events
        loop {
            match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
                Ok(Ok(event)) => {
                    assert_eq!(
                        event.correlation_id, cid,
                        "Event should have correct correlation ID"
                    );
                    event_types.push(event.event_type.clone());
                    count += 1;
                }
                _ => break,
            }
        }

        assert_eq!(count, 3, "Each command should produce exactly 3 events, got {count} for {cid}");
        assert!(
            event_types.contains(&"ResourceDeclared".to_string()),
            "Should have ResourceDeclared"
        );
        assert!(
            event_types.contains(&"ResourceReady".to_string()),
            "Should have terminal ResourceReady"
        );
    }

    // Verify total event count in the log
    assert_eq!(event_log.len().await, 30, "10 commands * 3 events = 30 total events");
}
