/// FT-055 Integration Tests — Universal tagging: TagAdded/TagRemoved events, SPARQL-queryable
///
/// Covers:
///   TC-228: Inference rule automatically assigns user to group on tag change

use std::sync::Arc;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{EventLog, StateProjector};
use picloud_events::InMemoryEventLog;
use picloud_http::inference::{InferenceEngine, LoadedRule};
use picloud_rdf::OxigraphProjector;
use uuid::Uuid;

const PICLOUD_NS: &str = "https://picloud.local/ontology#";

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

fn make_event(
    event_type: &str,
    product: Option<&str>,
    payload: serde_json::Value,
) -> EventEnvelope {
    let ib = iri_builder();
    EventEnvelope::new(
        ib.event_schema(event_type, 1),
        event_type,
        ResourceIri::new("https://picloud.local/test").unwrap(),
        product.map(|s| s.to_string()),
        Uuid::new_v4(),
        payload,
    )
}

fn make_identity_created(name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let identity_iri = ib.resource("platform", "identities", name);
    make_event(
        "IdentityCreated",
        None,
        serde_json::json!({
            "identity_iri": identity_iri.as_str(),
            "identity_type": "Human",
            "name": name,
        }),
    )
}

fn make_tag_added(resource_iri: &str, key: &str, value: &str) -> EventEnvelope {
    make_event(
        "TagAdded",
        None,
        serde_json::json!({
            "resource_iri": resource_iri,
            "key": key,
            "value": value,
        }),
    )
}

fn make_tag_removed(resource_iri: &str, key: &str, value: &str) -> EventEnvelope {
    make_event(
        "TagRemoved",
        None,
        serde_json::json!({
            "resource_iri": resource_iri,
            "key": key,
            "value": value,
        }),
    )
}

// ============================================================================
// TC-228 — Inference rule automatically assigns user to group on tag change
// ============================================================================
/// End-to-end integration test verifying the full tag → inference → group
/// membership pipeline:
///
/// 1. Create a user identity in the RDF graph
/// 2. Register an inference rule: "users tagged dept=engineering → member of eng-group"
/// 3. Add the matching tag to the user via TagAdded event
/// 4. Evaluate the inference rule (triggered by TagAdded)
/// 5. Verify GroupMembershipChanged event is emitted with action=added
/// 6. Project the GroupMembershipChanged event into the graph
/// 7. Verify group membership is SPARQL-queryable
/// 8. Remove the tag via TagRemoved event
/// 9. Re-evaluate the rule and verify retraction
/// 10. Verify group membership is removed from the graph
#[tokio::test]
async fn tc228_inference_rule_automatically_assigns_user_to_group_on_tag_change() {
    let ib = iri_builder();
    let projector = Arc::new(OxigraphProjector::new().unwrap());
    let event_log = Arc::new(InMemoryEventLog::new());

    // Create the inference engine with the real projector and event log
    let engine = InferenceEngine::new(
        projector.clone() as Arc<dyn StateProjector>,
        event_log.clone() as Arc<dyn EventLog>,
        iri_builder(),
    );

    // --- Step 1: Create a user identity ---
    let user_iri = ib.resource("platform", "identities", "alice");
    projector
        .project(&make_identity_created("alice"))
        .await
        .unwrap();

    // Verify the identity exists in the graph
    let ask = format!(
        "ASK {{ <{iri}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{PICLOUD_NS}Identity> }}",
        iri = user_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "User alice should exist as Identity in the graph"
    );

    // --- Step 2: Define the group IRI and register an inference rule ---
    let group_iri = "https://picloud.local/groups/engineering";

    // The inference rule: any Identity with tag dept=engineering becomes a member
    // of the engineering group.
    //
    // The CONSTRUCT template uses ?s ?p ?o which is what the inference engine
    // expects (it transforms CONSTRUCT to SELECT ?s ?p ?o).
    // The WHERE clause BINDs the group IRI, hasMember predicate, and matched user.
    let construct_query = format!(
        r#"CONSTRUCT {{ ?s ?p ?o }}
        WHERE {{
            ?user <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{PICLOUD_NS}Identity> .
            ?user <{PICLOUD_NS}tag> ?tagNode .
            ?tagNode <{PICLOUD_NS}tagKey> "dept" .
            ?tagNode <{PICLOUD_NS}tagValue> "engineering" .
            BIND(<{group_iri}> AS ?s)
            BIND(<{PICLOUD_NS}hasMember> AS ?p)
            BIND(?user AS ?o)
        }}"#
    );

    let rule = LoadedRule {
        iri: ResourceIri::new("https://picloud.local/inference-rules/eng-group-rule").unwrap(),
        name: "eng-group-rule".to_string(),
        scope: "platform".to_string(),
        trigger: "event".to_string(),
        trigger_events: vec!["TagAdded".to_string(), "TagRemoved".to_string()],
        reconciliation: true,
        construct_query,
    };

    engine.register_rule(rule.clone()).await;

    // --- Step 3: Add tag dept=engineering to user alice ---
    let tag_event = make_tag_added(user_iri.as_str(), "dept", "engineering");
    projector.project(&tag_event).await.unwrap();

    // Verify the tag is in the graph
    let tag_ask = format!(
        r#"ASK {{
            <{iri}> <{PICLOUD_NS}tag> ?t .
            ?t <{PICLOUD_NS}tagKey> "dept" .
            ?t <{PICLOUD_NS}tagValue> "engineering" .
        }}"#,
        iri = user_iri.as_str()
    );
    let result = projector.query(&tag_ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Tag dept=engineering should be queryable on alice"
    );

    // --- Step 4: Evaluate the inference rule (triggered by TagAdded) ---
    let (assertions, retractions) = engine.evaluate_rule(&rule).await.unwrap();
    assert_eq!(
        assertions, 1,
        "Should detect one new assertion (alice → engineering group)"
    );
    assert_eq!(retractions, 0, "Should have no retractions on first evaluation");

    // --- Step 5: Verify GroupMembershipChanged event was emitted ---
    let events = event_log.events_since(0).await;
    let membership_events: Vec<&EventEnvelope> = events
        .iter()
        .filter(|e| e.event_type == "GroupMembershipChanged")
        .collect();
    assert_eq!(
        membership_events.len(),
        1,
        "Should have emitted exactly one GroupMembershipChanged event"
    );
    assert_eq!(
        membership_events[0].payload["group_iri"], group_iri,
        "GroupMembershipChanged should reference the engineering group"
    );
    assert_eq!(
        membership_events[0].payload["member_iri"],
        user_iri.as_str(),
        "GroupMembershipChanged should reference alice"
    );
    assert_eq!(
        membership_events[0].payload["action"], "added",
        "Action should be 'added'"
    );

    // Also verify InferenceRuleEvaluated event
    let rule_events: Vec<&EventEnvelope> = events
        .iter()
        .filter(|e| e.event_type == "InferenceRuleEvaluated")
        .collect();
    assert_eq!(
        rule_events.len(),
        1,
        "Should have emitted InferenceRuleEvaluated"
    );
    assert_eq!(rule_events[0].payload["assertions"], 1);
    assert_eq!(rule_events[0].payload["retractions"], 0);

    // --- Step 6: Project GroupMembershipChanged into the graph ---
    // In production, the event loop does this. In the test, we do it manually.
    projector
        .project(membership_events[0])
        .await
        .unwrap();

    // --- Step 7: Verify group membership is SPARQL-queryable ---
    let membership_ask = format!(
        "ASK {{ <{group_iri}> <{PICLOUD_NS}hasMember> <{user}> }}",
        user = user_iri.as_str()
    );
    let result = projector.query(&membership_ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "alice should be a member of the engineering group after inference"
    );

    // Verify the group was typed as Group
    let group_type_ask = format!(
        "ASK {{ <{group_iri}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{PICLOUD_NS}Group> }}"
    );
    let result = projector.query(&group_type_ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Engineering group should be typed as picloud:Group"
    );

    // Query all members of the engineering group
    let members_query = format!(
        "SELECT ?member WHERE {{ <{group_iri}> <{PICLOUD_NS}hasMember> ?member }}"
    );
    let result = projector.query(&members_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "Engineering group should have exactly one member"
    );
    assert_eq!(
        result.bindings[0]["member"]["value"].as_str().unwrap(),
        user_iri.as_str(),
        "The sole member should be alice"
    );

    // --- Step 8: Add a second user with the same tag ---
    let user2_iri = ib.resource("platform", "identities", "bob");
    projector
        .project(&make_identity_created("bob"))
        .await
        .unwrap();
    projector
        .project(&make_tag_added(user2_iri.as_str(), "dept", "engineering"))
        .await
        .unwrap();

    // Evaluate rule again — should detect bob as a new assertion
    let (assertions, retractions) = engine.evaluate_rule(&rule).await.unwrap();
    assert_eq!(assertions, 1, "Should detect bob as a new group member");
    assert_eq!(retractions, 0, "alice should still be a member (no retraction)");

    // Project the new membership event
    let events_after = event_log.events_since(0).await;
    let new_membership: Vec<&EventEnvelope> = events_after
        .iter()
        .filter(|e| e.event_type == "GroupMembershipChanged" && e.payload["action"] == "added")
        .collect();
    // Find the bob membership event (should be the last one)
    let bob_event = new_membership
        .iter()
        .find(|e| e.payload["member_iri"] == user2_iri.as_str())
        .expect("Should have a GroupMembershipChanged for bob");
    projector.project(bob_event).await.unwrap();

    // Verify both members are now in the group
    let members_query = format!(
        "SELECT ?member WHERE {{ <{group_iri}> <{PICLOUD_NS}hasMember> ?member }} ORDER BY ?member"
    );
    let result = projector.query(&members_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "Engineering group should now have two members"
    );

    // --- Step 9: Remove tag from alice via TagRemoved ---
    let remove_event = make_tag_removed(user_iri.as_str(), "dept", "engineering");
    projector.project(&remove_event).await.unwrap();

    // Verify the tag was removed from the graph
    let tag_ask = format!(
        r#"ASK {{
            <{iri}> <{PICLOUD_NS}tag> ?t .
            ?t <{PICLOUD_NS}tagKey> "dept" .
            ?t <{PICLOUD_NS}tagValue> "engineering" .
        }}"#,
        iri = user_iri.as_str()
    );
    let result = projector.query(&tag_ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "Tag dept=engineering should be removed from alice"
    );

    // Re-evaluate the rule — should detect alice retraction
    let (assertions, retractions) = engine.evaluate_rule(&rule).await.unwrap();
    assert_eq!(assertions, 0, "No new assertions");
    assert_eq!(
        retractions, 1,
        "Should detect alice was removed from the group (tag removed)"
    );

    // Verify the removal event was emitted
    let all_events = event_log.events_since(0).await;
    let removed_events: Vec<&EventEnvelope> = all_events
        .iter()
        .filter(|e| {
            e.event_type == "GroupMembershipChanged"
                && e.payload["action"] == "removed"
                && e.payload["member_iri"] == user_iri.as_str()
        })
        .collect();
    assert_eq!(
        removed_events.len(),
        1,
        "Should have emitted a GroupMembershipChanged with action=removed for alice"
    );

    // Project the removal into the graph
    projector.project(removed_events[0]).await.unwrap();

    // --- Step 10: Verify alice is no longer a member ---
    let result = projector.query(&membership_ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "alice should no longer be a member of the engineering group"
    );

    // Bob should still be a member
    let bob_ask = format!(
        "ASK {{ <{group_iri}> <{PICLOUD_NS}hasMember> <{user}> }}",
        user = user2_iri.as_str()
    );
    let result = projector.query(&bob_ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "bob should still be a member of the engineering group"
    );

    // Verify the group now has exactly one member (bob only)
    let result = projector.query(&members_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "Engineering group should have only one member after alice's tag was removed"
    );
    assert_eq!(
        result.bindings[0]["member"]["value"].as_str().unwrap(),
        user2_iri.as_str(),
        "The remaining member should be bob"
    );
}
