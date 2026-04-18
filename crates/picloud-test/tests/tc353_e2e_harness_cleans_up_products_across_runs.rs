//! TC-353 regression test — E2E harness cleans up products across runs.
//!
//! Regression guard for the `multi-node-persistence` and
//! `multi-node-replication` scenarios, which both failed on the Pi 5 cluster
//! (2026-04-17) with
//! `apply product failed (409 Conflict): product 'persist-test-e2e' already
//! exists — use picloud resource apply --overwrite or delete first`. The
//! conflicts cascaded into five SKIPs in the E2E run because seeding
//! failed.
//!
//! **Invariant under test:** every scenario in the `picloud-test` harness
//! that seeds a test product must be able to run cleanly regardless of
//! whether a prior run aborted before teardown. A manual workaround
//! (running `picloud resource delete` before the E2E run) must NOT be
//! required.
//!
//! ## Test shape (per TC-353 brief)
//!
//! 1. Build a temporary cluster config pointing at an in-process
//!    `picloud-server`. We wire up an `InMemoryEventLog` and an in-memory
//!    `OxigraphProjector` (with a background projection loop) and serve
//!    them through `PiCloudHttpServer::build_router()` on an ephemeral
//!    loopback port.
//! 2. Pre-seed the event log with a `ProductDeployed` event whose
//!    `product_name` matches the harness's default test-scope identifier
//!    (`persist-test-e2e`). This mirrors the state left behind by a run
//!    that aborted after apply but before teardown.
//! 3. Invoke the harness's product-seeding helper
//!    (`assertions::apply_product_and_wait`) — the same code path used
//!    by the failing scenarios.
//! 4. Assert it returns `Ok(())` with no 409 Conflict surfaced.
//! 5. Repeat with a different scenario's seed name (`repl-test-e2e`) to
//!    confirm the fix works for more than one collision.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::traits::{EventFilter, EventLog, StateProjector};
use picloud_events::InMemoryEventLog;
use picloud_http::PiCloudHttpServer;
use picloud_rdf::OxigraphProjector;
use picloud_test::config::{ClusterConfig, ClusterSection};
use picloud_test::harness::assertions;
use picloud_test::harness::runner::TestContext;
use uuid::Uuid;

/// Spin up an in-process `picloud-server` on an ephemeral loopback port,
/// wired to a shared `InMemoryEventLog` + in-memory `OxigraphProjector`.
///
/// Returns the event log (so the caller can pre-seed it), the bound
/// `SocketAddr`, and a join handle for the server task.
async fn spawn_in_process_server() -> (
    Arc<InMemoryEventLog>,
    Arc<OxigraphProjector>,
    SocketAddr,
    tokio::task::JoinHandle<()>,
) {
    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = Arc::new(
        OxigraphProjector::new().expect("in-memory OxigraphProjector must be constructable"),
    );

    // Background projection loop — subscribe to the event log and project
    // every event into Oxigraph as it arrives. Mirrors the wiring done in
    // `src/main.rs` for the production composition root.
    {
        let event_log: Arc<dyn EventLog> = event_log.clone();
        let projector = projector.clone();
        tokio::spawn(async move {
            let mut rx = match event_log.subscribe(EventFilter::default()).await {
                Ok(rx) => rx,
                Err(_) => return,
            };
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let _ = projector.project(&event).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
    }

    // Build a server with just the bits needed for `/api/apply`,
    // `/api/delete`, and `/graph`.
    let mut server = PiCloudHttpServer::new(
        "127.0.0.1:0".parse().unwrap(),
        ClusterDomain::default(),
    );
    server.event_log = Some(event_log.clone() as Arc<dyn EventLog>);
    server.projector = Some(projector.clone() as Arc<dyn StateProjector>);
    let router = server.build_router();

    // Bind our own listener so we can read the ephemeral port back out.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let bound = listener.local_addr().expect("local_addr");

    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    (event_log, projector, bound, join)
}

/// Build a `TestContext` pointing at the in-process server.
fn test_context_for(addr: SocketAddr) -> TestContext {
    let toml_src = format!(
        r#"
            [cluster]
            domain = "picloud.local"
            http_port = {port}
            tls = false
            base_host = "127.0.0.1"
            platform_version = "test"
        "#,
        port = addr.port()
    );
    // Round-trip through TOML so we exercise the same loader shape the
    // production harness uses.
    let config: ClusterConfig =
        toml::from_str(&toml_src).expect("cluster config must parse");
    assert!(
        matches!(&config.cluster, ClusterSection { .. }),
        "sanity: config parses into the expected shape"
    );
    TestContext::new(config, std::env::temp_dir())
}

/// Pre-seed the event log with a `ProductDeployed` event so that the next
/// apply of a product with the same name would, without the fix, be
/// rejected with 409 Conflict.
async fn seed_lingering_product(log: &InMemoryEventLog, name: &str) {
    let iri_builder = IriBuilder::new(ClusterDomain::default());
    let product_iri = iri_builder.product(name);
    let schema = iri_builder.event_schema("ProductDeployed", 1);
    let envelope = EventEnvelope::new(
        schema,
        "ProductDeployed",
        product_iri.clone(),
        None,
        Uuid::new_v4(),
        serde_json::json!({
            "product_iri": product_iri.as_str(),
            "product_name": name,
            "version": "0.9.0-lingering",
            "data_products": [],
        }),
    );
    log.append(envelope)
        .await
        .expect("pre-seed: append ProductDeployed to in-memory log");

    // Give the background projection loop a moment to project the seed
    // event into Oxigraph. Without this, the first `/api/apply` call can
    // race ahead of the projection and miss the lingering state entirely.
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// Run one "prior run aborted before teardown" cycle:
///   seed a ProductDeployed → call the harness's apply helper →
///   assert it returns Ok with no 409 surfaced.
async fn run_cycle(log: &InMemoryEventLog, ctx: &TestContext, name: &str) {
    seed_lingering_product(log, name).await;

    // Before the fix, this call would return
    //   Err("apply product failed (409 Conflict): product '...' already exists ...")
    // because the lingering ProductDeployed event in the log causes
    // `handle_apply`'s overwrite-protection branch to reject the request.
    let result = assertions::apply_product_and_wait(ctx, name, "1.0.0").await;

    match result {
        Ok(()) => { /* expected — the harness tolerated the collision */ }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("409") || msg.to_lowercase().contains("already exists") {
                panic!(
                    "TC-353 regression: apply_product_and_wait surfaced 409 Conflict for \
                     scenario seed '{name}' — harness must clean up lingering products \
                     (error was: {msg})"
                );
            }
            panic!(
                "TC-353: apply_product_and_wait failed for '{name}' with a non-409 error: {msg}"
            );
        }
    }
}

#[tokio::test]
async fn tc353_e2e_harness_cleans_up_products_across_runs() {
    // Bring up the in-process cluster.
    let (event_log, _projector, addr, _server) = spawn_in_process_server().await;
    let ctx = test_context_for(addr);

    // 1. Seed + apply for the `multi-node-persistence` scenario's default
    //    product name. This was the first scenario to fail on the Pi 5
    //    cluster on 2026-04-17.
    run_cycle(&event_log, &ctx, "persist-test-e2e").await;

    // 2. Repeat for the `multi-node-replication` scenario's default product
    //    name. The TC-353 brief specifically requires that the fix works
    //    for more than one collision — a harness that hard-codes a single
    //    known name to delete would slip through an assertion that only
    //    exercises one seed.
    run_cycle(&event_log, &ctx, "repl-test-e2e").await;
}
