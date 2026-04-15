/// FT-056 Integration Tests — Group Resource Type with Role Inheritance
///
/// Covers TC-268, TC-325.
/// These tests verify that groups can be assigned roles and that users who
/// are members of a group inherit the group's roles and permissions.

use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{IdentityProvider, TokenExchange};

use picloud_iam::LocalIdentityProvider;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn provider() -> LocalIdentityProvider {
    LocalIdentityProvider::new(b"test-secret-key-for-hmac-signing", ClusterDomain::default())
}

fn identity_iri(name: &str) -> ResourceIri {
    let ib = IriBuilder::new(ClusterDomain::default());
    ib.resource("platform", "identities", name)
}

fn group_iri(name: &str) -> ResourceIri {
    let ib = IriBuilder::new(ClusterDomain::default());
    ib.group(name)
}

// ===========================================================================
// TC-268 — Group with assigned role grants inherited permissions to member users
// ===========================================================================

#[tokio::test]
async fn tc268_group_with_assigned_role_grants_inherited_permissions_to_member_users() {
    let prov = provider();

    // 1. Create an identity (Alice) with no direct roles
    let alice = identity_iri("alice");
    prov.register_identity(alice.clone(), vec![]).await;

    // 2. Create a group "editors" with the "editor" role
    let editors = group_iri("editors");
    prov.create_group(
        editors.clone(),
        "Editors".to_string(),
        Some("Content editors group".to_string()),
        vec!["editor".to_string()],
    )
    .await;

    // 3. Add Alice to the editors group
    prov.add_group_member(&editors, &alice).await.unwrap();

    // 4. Issue a token for Alice — it should include the group-inherited "editor" role
    let token = prov.issue_token(&alice, Some("photo-app")).await.unwrap();
    let validated = prov.validate_token(&token).await.unwrap();
    assert!(
        validated.roles.contains(&"editor".to_string()),
        "Alice should inherit 'editor' role from the editors group, got: {:?}",
        validated.roles
    );

    // 5. Verify resolve_roles also includes group-inherited roles
    let resolved = prov.resolve_roles(&alice, "photo-app").await.unwrap();
    assert!(
        resolved.roles.contains(&"editor".to_string()),
        "resolve_roles should include group-inherited 'editor' role, got: {:?}",
        resolved.roles
    );

    // 6. Verify a user NOT in the group does NOT inherit the role
    let bob = identity_iri("bob");
    prov.register_identity(bob.clone(), vec![]).await;
    let bob_token = prov.issue_token(&bob, Some("photo-app")).await.unwrap();
    let bob_validated = prov.validate_token(&bob_token).await.unwrap();
    assert!(
        !bob_validated.roles.contains(&"editor".to_string()),
        "Bob should NOT have 'editor' role — not a group member, got: {:?}",
        bob_validated.roles
    );

    // 7. Verify that group roles combine with direct roles (no duplicate)
    let charlie = identity_iri("charlie");
    prov.register_identity(
        charlie.clone(),
        vec!["viewer".to_string()],
    )
    .await;
    prov.add_group_member(&editors, &charlie).await.unwrap();

    let charlie_token = prov
        .issue_token(&charlie, Some("photo-app"))
        .await
        .unwrap();
    let charlie_validated = prov.validate_token(&charlie_token).await.unwrap();
    assert!(
        charlie_validated.roles.contains(&"viewer".to_string()),
        "Charlie should have direct 'viewer' role"
    );
    assert!(
        charlie_validated.roles.contains(&"editor".to_string()),
        "Charlie should also inherit 'editor' from group"
    );

    // 8. Verify removing a user from a group removes inherited roles
    prov.remove_group_member(&editors, &alice).await.unwrap();
    let token_after_removal = prov
        .issue_token(&alice, Some("photo-app"))
        .await
        .unwrap();
    let validated_after = prov.validate_token(&token_after_removal).await.unwrap();
    assert!(
        !validated_after.roles.contains(&"editor".to_string()),
        "Alice should no longer have 'editor' after being removed from group, got: {:?}",
        validated_after.roles
    );

    // 9. Verify assigning an additional role to the group propagates to members
    prov.assign_group_role(&editors, "reviewer".to_string())
        .await
        .unwrap();
    let charlie_token2 = prov
        .issue_token(&charlie, Some("photo-app"))
        .await
        .unwrap();
    let charlie_v2 = prov.validate_token(&charlie_token2).await.unwrap();
    assert!(
        charlie_v2.roles.contains(&"editor".to_string()),
        "Charlie should still have 'editor' from group"
    );
    assert!(
        charlie_v2.roles.contains(&"reviewer".to_string()),
        "Charlie should now also have 'reviewer' from group"
    );

    // 10. Verify revoking a role from the group removes it from members
    prov.revoke_group_role(&editors, "editor").await.unwrap();
    let charlie_token3 = prov
        .issue_token(&charlie, Some("photo-app"))
        .await
        .unwrap();
    let charlie_v3 = prov.validate_token(&charlie_token3).await.unwrap();
    assert!(
        !charlie_v3.roles.contains(&"editor".to_string()),
        "Charlie should no longer have 'editor' after it was revoked from group"
    );
    assert!(
        charlie_v3.roles.contains(&"reviewer".to_string()),
        "Charlie should still have 'reviewer' from group"
    );
}

// ===========================================================================
// TC-325 — Groups exit — group role assignment grants inherited permissions
// ===========================================================================

#[tokio::test]
async fn tc325_groups_exit_group_role_assignment_grants_inherited_permissions() {
    let prov = provider();

    // --- Set up: identities, groups, roles ---

    // Create users with various direct roles
    let admin = identity_iri("admin");
    let user1 = identity_iri("user1");
    let user2 = identity_iri("user2");
    let user3 = identity_iri("user3");
    prov.register_identity(admin.clone(), vec!["platform-admin".to_string()])
        .await;
    prov.register_identity(user1.clone(), vec![]).await;
    prov.register_identity(user2.clone(), vec!["viewer".to_string()])
        .await;
    prov.register_identity(user3.clone(), vec![]).await;

    // Create two groups
    let devs = group_iri("developers");
    let ops = group_iri("operations");

    prov.create_group(
        devs.clone(),
        "Developers".to_string(),
        Some("Development team".to_string()),
        vec!["developer".to_string(), "code-reviewer".to_string()],
    )
    .await;

    prov.create_group(
        ops.clone(),
        "Operations".to_string(),
        None,
        vec!["operator".to_string()],
    )
    .await;

    // Add members
    prov.add_group_member(&devs, &user1).await.unwrap();
    prov.add_group_member(&devs, &user2).await.unwrap();
    prov.add_group_member(&ops, &user2).await.unwrap(); // user2 in both groups
    prov.add_group_member(&ops, &user3).await.unwrap();

    // --- Assertions: users inherit correct roles from groups ---

    // user1: member of developers → inherits developer + code-reviewer
    let u1_token = prov.issue_token(&user1, Some("my-app")).await.unwrap();
    let u1 = prov.validate_token(&u1_token).await.unwrap();
    assert!(u1.roles.contains(&"developer".to_string()));
    assert!(u1.roles.contains(&"code-reviewer".to_string()));
    assert!(!u1.roles.contains(&"operator".to_string()), "user1 not in ops");

    // user2: member of developers AND operations, direct role viewer
    let u2_token = prov.issue_token(&user2, Some("my-app")).await.unwrap();
    let u2 = prov.validate_token(&u2_token).await.unwrap();
    assert!(u2.roles.contains(&"viewer".to_string()), "direct role");
    assert!(u2.roles.contains(&"developer".to_string()), "from devs group");
    assert!(u2.roles.contains(&"code-reviewer".to_string()), "from devs group");
    assert!(u2.roles.contains(&"operator".to_string()), "from ops group");

    // user3: member of operations → inherits operator
    let u3_token = prov.issue_token(&user3, Some("my-app")).await.unwrap();
    let u3 = prov.validate_token(&u3_token).await.unwrap();
    assert!(u3.roles.contains(&"operator".to_string()));
    assert!(!u3.roles.contains(&"developer".to_string()), "user3 not in devs");

    // admin: not in any group, only has direct platform-admin role
    let admin_token = prov.issue_token(&admin, Some("my-app")).await.unwrap();
    let admin_v = prov.validate_token(&admin_token).await.unwrap();
    assert!(admin_v.roles.contains(&"platform-admin".to_string()));
    assert!(!admin_v.roles.contains(&"developer".to_string()));
    assert!(!admin_v.roles.contains(&"operator".to_string()));

    // --- Verify resolve_roles includes group inheritance ---
    let u2_resolved = prov.resolve_roles(&user2, "my-app").await.unwrap();
    assert!(u2_resolved.roles.contains(&"viewer".to_string()));
    assert!(u2_resolved.roles.contains(&"developer".to_string()));
    assert!(u2_resolved.roles.contains(&"code-reviewer".to_string()));
    assert!(u2_resolved.roles.contains(&"operator".to_string()));

    // --- Verify issue_token_with_audience includes group roles ---
    let u1_aud_token = prov
        .issue_token_with_audience(
            &user1,
            "https://picloud.local/products/my-app",
            vec!["read".to_string()],
        )
        .await
        .unwrap();
    let u1_aud = prov.validate_token(&u1_aud_token).await.unwrap();
    assert!(u1_aud.roles.contains(&"developer".to_string()));
    assert!(u1_aud.roles.contains(&"code-reviewer".to_string()));

    // --- Dynamic: add role to group, verify it propagates ---
    prov.assign_group_role(&ops, "incident-responder".to_string())
        .await
        .unwrap();
    let u3_token2 = prov.issue_token(&user3, Some("my-app")).await.unwrap();
    let u3_v2 = prov.validate_token(&u3_token2).await.unwrap();
    assert!(u3_v2.roles.contains(&"operator".to_string()));
    assert!(u3_v2.roles.contains(&"incident-responder".to_string()));

    // --- Dynamic: remove member from group, verify role removal ---
    prov.remove_group_member(&devs, &user2).await.unwrap();
    let u2_token2 = prov.issue_token(&user2, Some("my-app")).await.unwrap();
    let u2_v2 = prov.validate_token(&u2_token2).await.unwrap();
    assert!(u2_v2.roles.contains(&"viewer".to_string()), "direct role preserved");
    assert!(!u2_v2.roles.contains(&"developer".to_string()), "no longer in devs");
    assert!(!u2_v2.roles.contains(&"code-reviewer".to_string()), "no longer in devs");
    assert!(u2_v2.roles.contains(&"operator".to_string()), "still in ops");
    assert!(
        u2_v2.roles.contains(&"incident-responder".to_string()),
        "still in ops, gets new role"
    );

    // --- Verify adding duplicate member is idempotent ---
    prov.add_group_member(&ops, &user3).await.unwrap(); // already a member
    let groups_snapshot = prov.roles_from_groups(&user3).await;
    // Count should match unique roles, no duplicates
    let unique_count = {
        let mut roles = groups_snapshot.clone();
        roles.sort();
        roles.dedup();
        roles.len()
    };
    assert_eq!(
        groups_snapshot.len(),
        unique_count,
        "No duplicate roles from duplicate membership"
    );

    // --- Verify error on non-existent group ---
    let fake_group = group_iri("nonexistent");
    let result = prov
        .add_group_member(&fake_group, &user1)
        .await;
    assert!(result.is_err(), "Should fail to add member to non-existent group");
}
