/// FT-017 Integration Tests — Basic IAM: human identities, workload identities,
/// RBAC, token issuance
///
/// Covers TC-238, TC-295.
/// These tests verify the end-to-end flow: passkey authentication produces a
/// token that carries RBAC roles, and that token can be validated to enforce
/// scoped access.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::hmac;

use picloud_domain::identity::{
    AuthenticationResponse, RegistrationResponse,
};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::IdentityProvider;

use picloud_iam::LocalIdentityProvider;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn provider() -> LocalIdentityProvider {
    LocalIdentityProvider::new(b"test-secret-key-for-hmac-signing", ClusterDomain::default())
}

fn iri(name: &str) -> ResourceIri {
    let ib = IriBuilder::new(ClusterDomain::default());
    ib.resource("platform", "identities", name)
}

fn random_bytes(len: usize) -> Vec<u8> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..len).map(|_| rng.gen()).collect()
}

/// Register a passkey via the full ceremony and return the credential key bytes.
async fn register_passkey(
    prov: &LocalIdentityProvider,
    identity_iri: &ResourceIri,
    cred_id: &str,
) -> Vec<u8> {
    let (challenge_id, _opts) = prov.begin_registration(identity_iri).await.unwrap();
    let pk = random_bytes(32);
    let resp = RegistrationResponse {
        credential_id: cred_id.to_string(),
        public_key: URL_SAFE_NO_PAD.encode(&pk),
        attestation: None,
        aaguid: Some("test-aaguid".to_string()),
        display_name: Some(format!("Key {cred_id}")),
    };
    prov.complete_registration(&challenge_id, resp).await.unwrap();
    pk
}

/// Perform HMAC-based authentication and return the token.
async fn authenticate_hmac(
    prov: &LocalIdentityProvider,
    identity_iri: &ResourceIri,
    cred_id: &str,
    public_key: &[u8],
) -> String {
    let (challenge_id, opts) = prov.begin_authentication(identity_iri).await.unwrap();
    let challenge_bytes = URL_SAFE_NO_PAD.decode(&opts.challenge).unwrap();
    let sig_key = hmac::Key::new(hmac::HMAC_SHA256, public_key);
    let sig = hmac::sign(&sig_key, &challenge_bytes);
    let signature = URL_SAFE_NO_PAD.encode(sig.as_ref());

    let resp = AuthenticationResponse {
        credential_id: cred_id.to_string(),
        signature,
        authenticator_data: None,
        client_data_json: None,
        signature_format: "hmac".to_string(),
    };
    prov.complete_authentication(&challenge_id, resp).await.unwrap()
}

// ===========================================================================
// TC-238 — IAM token issued after passkey authentication grants RBAC-scoped
//          access
// ===========================================================================

/// Verifies the complete flow: register identity with RBAC roles, register a
/// passkey, authenticate via passkey, receive a token, and validate that the
/// token carries the correct RBAC roles and scoped permissions.
#[tokio::test]
async fn tc238_iam_token_issued_after_passkey_authentication_grants_rbac_scoped_access() {
    let prov = provider();

    // --- Set up two identities with different RBAC roles ---
    let admin = iri("admin-user");
    let viewer = iri("viewer-user");

    prov.register_identity(admin.clone(), vec!["admin".to_string(), "user".to_string()])
        .await;
    prov.register_identity(viewer.clone(), vec!["viewer".to_string()])
        .await;

    // --- Register passkeys for both ---
    let admin_pk = register_passkey(&prov, &admin, "admin-key-1").await;
    let viewer_pk = register_passkey(&prov, &viewer, "viewer-key-1").await;

    // --- Authenticate admin via passkey ---
    let admin_token = authenticate_hmac(&prov, &admin, "admin-key-1", &admin_pk).await;
    assert!(!admin_token.is_empty(), "Admin token must not be empty");

    // --- Authenticate viewer via passkey ---
    let viewer_token = authenticate_hmac(&prov, &viewer, "viewer-key-1", &viewer_pk).await;
    assert!(!viewer_token.is_empty(), "Viewer token must not be empty");

    // --- Validate admin token carries admin+user roles ---
    let admin_validated = prov.validate_token(&admin_token).await.unwrap();
    assert_eq!(admin_validated.identity_iri, admin);
    assert!(
        admin_validated.roles.contains(&"admin".to_string()),
        "Admin token must contain 'admin' role, got: {:?}",
        admin_validated.roles
    );
    assert!(
        admin_validated.roles.contains(&"user".to_string()),
        "Admin token must contain 'user' role, got: {:?}",
        admin_validated.roles
    );

    // --- Validate viewer token carries only viewer role ---
    let viewer_validated = prov.validate_token(&viewer_token).await.unwrap();
    assert_eq!(viewer_validated.identity_iri, viewer);
    assert_eq!(viewer_validated.roles, vec!["viewer"]);
    assert!(
        !viewer_validated.roles.contains(&"admin".to_string()),
        "Viewer must not have admin role"
    );

    // --- Product-scoped token carries audience ---
    let product_token = prov
        .issue_token(&admin, Some("photo-app"))
        .await
        .unwrap();
    let product_validated = prov.validate_token(&product_token).await.unwrap();
    assert_eq!(product_validated.identity_iri, admin);
    assert_eq!(product_validated.product, Some("photo-app".to_string()));
    assert!(
        product_validated.audience.is_some(),
        "Product-scoped token must have audience"
    );
    assert!(
        product_validated
            .audience
            .as_ref()
            .unwrap()
            .contains("photo-app"),
        "Audience must reference the product"
    );

    // --- Tokens are distinct per identity ---
    assert_ne!(
        admin_token, viewer_token,
        "Different identities must produce different tokens"
    );
}

// ===========================================================================
// TC-295 — IAM exit — passkey login, token issuance, RBAC enforcement
//          functional
// ===========================================================================

/// Exit criteria: verifies that all three pillars — passkey login, token
/// issuance, and RBAC enforcement — work together end-to-end. This is the
/// gate check before FT-017 can be marked complete.
#[tokio::test]
async fn tc295_iam_exit_passkey_login_token_issuance_rbac_enforcement_functional() {
    let prov = provider();
    let ib = IriBuilder::new(ClusterDomain::default());

    // -----------------------------------------------------------------------
    // 1. PASSKEY LOGIN — human identity authenticates via passkey
    // -----------------------------------------------------------------------
    let human = iri("exit-criteria-user");
    prov.register_identity(
        human.clone(),
        vec!["operator".to_string(), "user".to_string()],
    )
    .await;

    // Register passkey and authenticate
    let pk = register_passkey(&prov, &human, "exit-key-1").await;
    let token = authenticate_hmac(&prov, &human, "exit-key-1", &pk).await;
    assert!(!token.is_empty(), "Passkey login must produce a token");

    // -----------------------------------------------------------------------
    // 2. TOKEN ISSUANCE — tokens carry correct claims
    // -----------------------------------------------------------------------
    let validated = prov.validate_token(&token).await.unwrap();
    assert_eq!(validated.identity_iri, human);
    assert!(validated.roles.contains(&"operator".to_string()));
    assert!(validated.roles.contains(&"user".to_string()));

    // Platform-scoped token (no product)
    let platform_token = prov.issue_token(&human, None).await.unwrap();
    let platform_validated = prov.validate_token(&platform_token).await.unwrap();
    assert_eq!(platform_validated.identity_iri, human);
    assert!(platform_validated.product.is_none());

    // Product-scoped token
    let product_token = prov
        .issue_token(&human, Some("my-product"))
        .await
        .unwrap();
    let product_validated = prov.validate_token(&product_token).await.unwrap();
    assert_eq!(product_validated.product, Some("my-product".to_string()));
    assert!(product_validated.audience.is_some());

    // -----------------------------------------------------------------------
    // 3. RBAC ENFORCEMENT — different roles yield different access
    // -----------------------------------------------------------------------
    let restricted = iri("restricted-user");
    prov.register_identity(restricted.clone(), vec!["readonly".to_string()])
        .await;
    let restricted_pk = register_passkey(&prov, &restricted, "restricted-key-1").await;
    let restricted_token =
        authenticate_hmac(&prov, &restricted, "restricted-key-1", &restricted_pk).await;

    let restricted_validated = prov.validate_token(&restricted_token).await.unwrap();
    assert_eq!(restricted_validated.roles, vec!["readonly"]);
    assert!(
        !restricted_validated.roles.contains(&"operator".to_string()),
        "Restricted user must not have operator role"
    );
    assert!(
        !restricted_validated.roles.contains(&"admin".to_string()),
        "Restricted user must not have admin role"
    );

    // Verify role separation: the two tokens carry different RBAC scopes
    assert_ne!(validated.roles, restricted_validated.roles);

    // -----------------------------------------------------------------------
    // 4. WORKLOAD IDENTITY — workload certs are issued correctly
    // -----------------------------------------------------------------------
    let workload_iri = ib.resource("my-product", "containers", "api-server");
    prov.register_identity(workload_iri.clone(), vec!["workload".to_string()])
        .await;

    let cert = prov.issue_workload_certificate(&workload_iri).await.unwrap();
    assert!(!cert.certificate_pem.is_empty());
    assert!(!cert.private_key_pem.is_empty());
    assert!(cert.certificate_pem.contains("BEGIN CERTIFICATE"));
    assert!(cert.private_key_pem.contains("BEGIN PRIVATE KEY"));

    // Workload token issuance
    let workload_token = prov.issue_token(&workload_iri, Some("my-product")).await.unwrap();
    let workload_validated = prov.validate_token(&workload_token).await.unwrap();
    assert_eq!(workload_validated.roles, vec!["workload"]);
    assert_eq!(
        workload_validated.product,
        Some("my-product".to_string())
    );

    // -----------------------------------------------------------------------
    // 5. TOKEN EXPIRY — expired tokens are rejected
    // -----------------------------------------------------------------------
    let short_prov = LocalIdentityProvider::with_ttl(
        b"test-secret-key-for-hmac-signing",
        ClusterDomain::default(),
        -1, // already expired
    );
    let ephemeral = iri("ephemeral-user");
    short_prov
        .register_identity(ephemeral.clone(), vec!["user".to_string()])
        .await;
    let expired_token = short_prov.issue_token(&ephemeral, None).await.unwrap();
    let result = short_prov.validate_token(&expired_token).await;
    assert!(result.is_err(), "Expired token must be rejected");
}
