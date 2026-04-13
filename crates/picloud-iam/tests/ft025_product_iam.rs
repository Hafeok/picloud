/// FT-025 Integration Tests — Product-scoped IAM and role assignment
///
/// Covers TC-227: User authenticates against Product-hosted application via OIDC.
///
/// Validates the full OIDC authentication flow for a product-hosted application:
///
/// 1. **Setup** — Register a human identity with platform roles, register a passkey,
///    and create a Product app registration (OIDC client).
/// 2. **OIDC Discovery** — Verify the OIDC discovery document advertises the
///    authorization_code grant type and correct endpoints.
/// 3. **Authorization Code Flow** — User authenticates via passkey, then the platform
///    issues an authorization code for the product's OIDC client. The code is exchanged
///    for a product-scoped access token.
/// 4. **Token Validation** — The access token carries the correct audience (product IRI),
///    scopes, and RBAC roles. Audience enforcement ensures the token is only valid for
///    the intended product.
/// 5. **Token Exchange (RFC 8693)** — A platform token is exchanged for a product-scoped
///    token via the on-behalf-of flow, with the original identity preserved as the `actor`.
/// 6. **Client Credentials with Audience** — M2M token issuance targets a specific product
///    audience, and M2M permission enforcement blocks unauthorized cross-product access.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::hmac;

use picloud_domain::identity::{
    AuthenticationResponse, M2mPermission, RegistrationResponse,
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

fn product_iri(name: &str) -> ResourceIri {
    let ib = IriBuilder::new(ClusterDomain::default());
    ib.product(name)
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
// TC-227 — User authenticates against Product-hosted application via OIDC
// ===========================================================================

/// Validates the full OIDC authentication flow for a product-hosted application:
/// passkey login → authorization code → product-scoped token → audience validation
/// → token exchange → M2M with audience enforcement.
#[tokio::test]
async fn tc227_user_authenticates_against_product_hosted_application_via_oidc() {
    let prov = provider();

    // ── Step 1: Setup — register identity, passkey, and product app ──

    let alice = iri("alice");
    prov.register_identity(
        alice.clone(),
        vec!["editor".to_string(), "user".to_string()],
    )
    .await;

    let alice_pk = register_passkey(&prov, &alice, "alice-key-1").await;

    // Register an OIDC app (client) for "photo-app"
    let photo_app_iri = product_iri("photo-app");
    let app_reg = prov
        .register_app(
            &photo_app_iri,
            vec!["https://photo-app.picloud.local/callback".to_string()],
            vec!["openid".to_string(), "photos:read".to_string(), "photos:write".to_string()],
        )
        .await
        .unwrap();

    let client_id = app_reg.client_id.clone();
    let client_secret = app_reg.client_secret_hash.clone(); // plaintext at creation time

    assert!(!client_id.is_empty(), "client_id must be assigned");
    assert!(!client_secret.is_empty(), "client_secret must be returned at creation");
    assert_eq!(app_reg.product_iri, photo_app_iri, "app must be bound to product");

    // ── Step 2: OIDC Discovery — verify discovery document ──

    let discovery = prov.oidc_discovery().await.unwrap();
    assert_eq!(discovery.issuer, "https://picloud.local");
    assert!(
        discovery.grant_types_supported.contains(&"authorization_code".to_string()),
        "discovery must advertise authorization_code grant, got: {:?}",
        discovery.grant_types_supported
    );
    assert!(
        discovery.grant_types_supported.contains(&"client_credentials".to_string()),
        "discovery must advertise client_credentials grant"
    );
    assert!(
        !discovery.authorization_endpoint.is_empty(),
        "authorization_endpoint must be present"
    );
    assert!(
        !discovery.token_endpoint.is_empty(),
        "token_endpoint must be present"
    );

    // JWKS endpoint returns signing key metadata
    let jwks = prov.jwks().await.unwrap();
    assert!(
        !jwks.keys.is_empty(),
        "JWKS must contain at least one signing key"
    );
    assert_eq!(jwks.keys[0].alg, "HS256");

    // ── Step 3: Authorization Code Flow ──
    // User authenticates via passkey first
    let _user_token = authenticate_hmac(&prov, &alice, "alice-key-1", &alice_pk).await;

    // Platform issues an authorization code for the product's OIDC client
    let auth_code = prov
        .begin_authorization_code(
            &client_id,
            "https://photo-app.picloud.local/callback",
            Some("openid photos:read"),
            &alice,
        )
        .await
        .unwrap();

    assert!(!auth_code.is_empty(), "authorization code must be non-empty");

    // Exchange authorization code for product-scoped token
    let token_response = prov
        .exchange_authorization_code(
            &auth_code,
            &client_id,
            "https://photo-app.picloud.local/callback",
        )
        .await
        .unwrap();

    assert_eq!(token_response.token_type, "Bearer");
    assert!(token_response.expires_in > 0, "token must have positive TTL");
    assert!(!token_response.access_token.is_empty(), "access_token must be present");

    // Scopes should reflect what was requested in the authorization
    assert!(
        token_response.scope.is_some(),
        "token response should include granted scopes"
    );
    let granted_scopes = token_response.scope.as_ref().unwrap();
    assert!(
        granted_scopes.contains("openid"),
        "granted scopes should include 'openid', got: {}",
        granted_scopes
    );
    assert!(
        granted_scopes.contains("photos:read"),
        "granted scopes should include 'photos:read', got: {}",
        granted_scopes
    );

    // ── Step 4: Token Validation — audience, scopes, roles ──

    let validated = prov.validate_token(&token_response.access_token).await.unwrap();

    // Subject is alice
    assert_eq!(
        validated.identity_iri, alice,
        "token subject must be alice"
    );

    // Product is photo-app
    assert_eq!(
        validated.product,
        Some("photo-app".to_string()),
        "token must be scoped to photo-app"
    );

    // Audience must reference the product
    assert!(
        validated.audience.is_some(),
        "product-scoped token must have an audience"
    );
    let audience = validated.audience.as_ref().unwrap();
    assert!(
        audience.contains("photo-app"),
        "audience must reference photo-app, got: {}",
        audience
    );
    assert!(
        audience.starts_with("https://picloud.local/products/"),
        "audience must be a full product IRI, got: {}",
        audience
    );

    // Scopes in the validated token
    assert!(
        validated.scopes.contains(&"openid".to_string()),
        "validated token must carry openid scope, got: {:?}",
        validated.scopes
    );

    // Roles from the identity are carried through
    assert!(
        validated.roles.contains(&"editor".to_string()),
        "token should carry editor role from identity, got: {:?}",
        validated.roles
    );
    assert!(
        validated.roles.contains(&"user".to_string()),
        "token should carry user role from identity, got: {:?}",
        validated.roles
    );

    // ── Step 5: Token Exchange (RFC 8693) — on-behalf-of flow ──

    // Get a platform token for alice first
    let platform_token = prov.issue_token(&alice, None).await.unwrap();

    // Exchange platform token for a product-scoped token
    let exchanged = prov
        .token_exchange(
            &platform_token,
            Some("https://picloud.local/products/photo-app"),
            Some("photos:read"),
        )
        .await
        .unwrap();

    assert_eq!(exchanged.token_type, "Bearer");
    assert!(!exchanged.access_token.is_empty());

    let exchanged_validated = prov.validate_token(&exchanged.access_token).await.unwrap();
    assert_eq!(
        exchanged_validated.identity_iri, alice,
        "exchanged token subject must still be alice"
    );
    assert!(
        exchanged_validated.audience.is_some(),
        "exchanged token must have audience"
    );
    assert!(
        exchanged_validated
            .audience
            .as_ref()
            .unwrap()
            .contains("photo-app"),
        "exchanged token audience must reference photo-app"
    );

    // ── Step 6: Client Credentials with Audience + M2M Enforcement ──

    // Register a second product app for billing-app
    let billing_iri = product_iri("billing-app");
    let billing_app = prov
        .register_app(
            &billing_iri,
            vec!["https://billing.picloud.local/callback".to_string()],
            vec!["billing:read".to_string()],
        )
        .await
        .unwrap();

    let billing_client_id = billing_app.client_id.clone();
    let billing_secret = billing_app.client_secret_hash.clone();

    // Self-scoped client credentials token works without M2M
    let self_token = prov
        .client_credentials_token(&billing_client_id, &billing_secret, Some("billing:read"))
        .await
        .unwrap();
    assert!(!self_token.access_token.is_empty());
    assert_eq!(self_token.token_type, "Bearer");

    let self_validated = prov.validate_token(&self_token.access_token).await.unwrap();
    assert!(
        self_validated.audience.is_some(),
        "self-scoped client creds token should have audience"
    );
    assert!(
        self_validated
            .audience
            .as_ref()
            .unwrap()
            .contains("billing-app"),
        "self-scoped token audience should reference billing-app"
    );

    // Cross-product access WITHOUT M2M permission should fail
    let cross_result = prov
        .client_credentials_token_with_audience(
            &billing_client_id,
            &billing_secret,
            Some("photos:read"),
            Some("https://picloud.local/products/photo-app"),
        )
        .await;
    assert!(
        cross_result.is_err(),
        "cross-product client_credentials without M2M permission must fail"
    );

    // Register M2M permission: billing-app may access photo-app with photos:read
    prov.register_m2m_permission(M2mPermission {
        name: "billing-reads-photos".to_string(),
        product: "photo-app".to_string(),
        client: "billing-app".to_string(),
        scopes: vec!["photos:read".to_string()],
        description: Some("Billing reads photo metadata for invoicing".to_string()),
    })
    .await;

    // Now cross-product access with M2M permission should succeed
    let m2m_token = prov
        .client_credentials_token_with_audience(
            &billing_client_id,
            &billing_secret,
            Some("photos:read"),
            Some("https://picloud.local/products/photo-app"),
        )
        .await
        .unwrap();

    assert!(!m2m_token.access_token.is_empty());

    let m2m_validated = prov.validate_token(&m2m_token.access_token).await.unwrap();
    assert!(
        m2m_validated
            .audience
            .as_ref()
            .unwrap()
            .contains("photo-app"),
        "M2M token audience must reference the target product photo-app"
    );

    // M2M with unauthorized scope should still fail
    let bad_scope_result = prov
        .client_credentials_token_with_audience(
            &billing_client_id,
            &billing_secret,
            Some("photos:write"),
            Some("https://picloud.local/products/photo-app"),
        )
        .await;
    assert!(
        bad_scope_result.is_err(),
        "M2M with unauthorized scope must fail"
    );
}
