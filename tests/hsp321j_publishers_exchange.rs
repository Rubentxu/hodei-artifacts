//! HSP-321 tramo j (HSP-152): the OIDC exchange of the trusted publishers,
//! against an in-process OIDC issuer (own JWKS, signed tokens, controllable).
//!
//! Runs without Dgraph: the registry is the in-memory one and the JWKS goes
//! through the REAL egress client (`ProxyFetcher`) against the fake issuer.
//! Every security control has a test that fails when the control is removed.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use hodei_artifacts_api::proxy::egress::{AllowedHost, EgressPolicy};
use hodei_artifacts_api::proxy::fetch::{FetchLimits, ProxyFetcher};
use hodei_artifacts_api::publishers::{
    ExchangeError, GateError, InMemoryPublisherRegistry, PublisherRegistry, PublisherSettings,
    PublisherSpec, PublishersService, TrustedPublisher,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};

const SECRET: &str = "publisher-credential-secret-0123456789abcdef";
const AUDIENCE: &str = "hodei-artifacts";
const SUBJECT: &str = "repo:acme/app:ref:refs/heads/main";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/hsp152/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

fn pem_of(name: &str) -> String {
    let key: Value = serde_json::from_str(&fixture(name)).unwrap();
    key["pem_lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the fake issuer serves at `/jwks`.
#[derive(Clone)]
struct Issuer {
    body: Arc<Mutex<(u16, String)>>,
    hits: Arc<AtomicUsize>,
    redirected_hits: Arc<AtomicUsize>,
    port: u16,
}

impl Issuer {
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    fn serve(&self, status: u16, body: String) {
        *self.body.lock().unwrap() = (status, body);
    }
    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

async fn jwks(State(issuer): State<Issuer>) -> axum::response::Response {
    issuer.hits.fetch_add(1, Ordering::SeqCst);
    let (status, body) = issuer.body.lock().unwrap().clone();
    (StatusCode::from_u16(status).unwrap(), body).into_response()
}

async fn redirect() -> axum::response::Response {
    (StatusCode::FOUND, [(header::LOCATION, "/landing")]).into_response()
}

async fn slow(State(issuer): State<Issuer>) -> axum::response::Response {
    issuer.hits.fetch_add(1, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    (StatusCode::OK, fixture("issuer-jwks-1.json")).into_response()
}

async fn landing(State(issuer): State<Issuer>) -> axum::response::Response {
    issuer.redirected_hits.fetch_add(1, Ordering::SeqCst);
    (StatusCode::OK, fixture("issuer-jwks-1.json")).into_response()
}

async fn start_issuer() -> Issuer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let issuer = Issuer {
        body: Arc::new(Mutex::new((200, fixture("issuer-jwks-1.json")))),
        hits: Arc::new(AtomicUsize::new(0)),
        redirected_hits: Arc::new(AtomicUsize::new(0)),
        port,
    };
    let app = axum::Router::new()
        .route("/jwks", axum::routing::get(jwks))
        .route("/slow", axum::routing::get(slow))
        .route("/redirect", axum::routing::get(redirect))
        .route("/landing", axum::routing::get(landing))
        .with_state(issuer.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.ok() });
    issuer
}

fn fetcher_for(port: u16) -> ProxyFetcher {
    let policy = EgressPolicy::new(vec![AllowedHost {
        host: "127.0.0.1".into(),
        port: Some(port),
        allow_private: true,
        allow_http: true,
    }])
    .unwrap();
    ProxyFetcher::new(
        policy,
        FetchLimits {
            connect_timeout: Duration::from_secs(2),
            total_timeout: Duration::from_secs(5),
            max_bytes: 1 << 20,
        },
        4,
    )
}

fn record(issuer: &Issuer) -> TrustedPublisher {
    TrustedPublisher {
        id: "ci-main".into(),
        account: "acme".into(),
        repository: "releases".into(),
        issuer: issuer.url(),
        audience: AUDIENCE.into(),
        jwks_url: format!("{}/jwks", issuer.url()),
        subject: SUBJECT.into(),
        claims: BTreeMap::from([
            ("repository".to_string(), "acme/app".to_string()),
            ("ref".to_string(), "refs/heads/main".to_string()),
            ("workflow".to_string(), "release.yml".to_string()),
        ]),
        created_by: "hrn:hodei:iam::acme:User/admin".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
    }
}

struct Rig {
    service: PublishersService,
    registry: Arc<InMemoryPublisherRegistry>,
    issuer: Issuer,
}

async fn rig_with(settings: PublisherSettings) -> Rig {
    let issuer = start_issuer().await;
    let registry = Arc::new(InMemoryPublisherRegistry::default());
    registry.put(&record(&issuer)).await.unwrap();
    let service =
        PublishersService::new(registry.clone(), fetcher_for(issuer.port), SECRET, settings)
            .unwrap();
    Rig {
        service,
        registry,
        issuer,
    }
}

async fn rig() -> Rig {
    rig_with(PublisherSettings::default()).await
}

static JTI: AtomicUsize = AtomicUsize::new(0);

fn claims(issuer: &str) -> Value {
    let n = now();
    json!({
        "iss": issuer, "aud": AUDIENCE, "sub": SUBJECT,
        "iat": n, "nbf": n, "exp": n + 300,
        "jti": format!("jti-{}", JTI.fetch_add(1, Ordering::SeqCst)),
        "repository": "acme/app", "ref": "refs/heads/main", "workflow": "release.yml"
    })
}

fn sign_with(key: &str, kid: &str, body: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_string());
    encode(
        &header,
        body,
        &EncodingKey::from_rsa_pem(pem_of(key).as_bytes()).unwrap(),
    )
    .unwrap()
}

fn sign(body: &Value) -> String {
    sign_with("issuer-key-1.json", "issuer-key-1", body)
}

async fn exchange(rig: &Rig, token: &str) -> Result<(), ExchangeError> {
    rig.service
        .exchange(token, "acme", "releases")
        .await
        .map(|_| ())
}

fn denied(result: Result<(), ExchangeError>, reason: &str) {
    match result {
        Err(ExchangeError::Denied(found)) => assert_eq!(found, reason),
        other => panic!("expected Denied({reason}), got {other:?}"),
    }
}

// ---- happy path --------------------------------------------------------

#[tokio::test]
async fn a_valid_token_earns_a_minimal_short_credential_bound_to_the_repository() {
    let rig = rig().await;
    let token = sign(&claims(&rig.issuer.url()));
    let exchange = rig
        .service
        .exchange(&token, "acme", "releases")
        .await
        .expect("the exchange succeeds");
    assert_eq!(exchange.account, "acme");
    assert_eq!(exchange.repository, "releases");
    assert!(exchange.expires_in <= 900 && exchange.expires_in > 0);
    // No secret of the OIDC token travels inside the credential.
    for part in token.split('.') {
        assert!(!exchange.credential.contains(part));
    }
    let verified = rig
        .service
        .verify_credential(&exchange.credential)
        .await
        .expect("a live credential");
    assert_eq!(verified.claims.acts, vec!["upload".to_string()]);
    assert!(verified.may_upload_to("releases"));
    assert!(!verified.may_upload_to("other"));
    assert!(
        verified
            .claims
            .sub
            .starts_with("hrn:hodei:iam::acme:Workload/")
    );
    let context = rig.service.context_of(&exchange.credential).await.unwrap();
    assert_eq!(context.account().as_str(), "acme");
}

// ---- signature and algorithm -------------------------------------------

#[tokio::test]
async fn a_token_signed_by_another_key_is_denied() {
    let rig = rig().await;
    // Same kid, other private key: only the signature differs.
    let token = sign_with(
        "issuer-key-2.json",
        "issuer-key-1",
        &claims(&rig.issuer.url()),
    );
    denied(exchange(&rig, &token).await, "token_not_trusted");
}

#[tokio::test]
async fn the_algorithm_allow_list_refuses_none_and_hmac() {
    let rig = rig().await;
    let body = claims(&rig.issuer.url());
    // HS256 signed with the public modulus as the secret (algorithm confusion).
    let jwks: Value = serde_json::from_str(&fixture("issuer-jwks-1.json")).unwrap();
    let n = jwks["keys"][0]["n"].as_str().unwrap().to_string();
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("issuer-key-1".into());
    let hs = encode(&header, &body, &EncodingKey::from_secret(n.as_bytes())).unwrap();
    denied(exchange(&rig, &hs).await, "algorithm_not_allowed");

    // alg none, hand made.
    use base64::Engine as _;
    let b64 = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
    let none = format!(
        "{}.{}.",
        b64(&json!({"alg": "none", "typ": "JWT"})),
        b64(&body)
    );
    assert!(matches!(
        exchange(&rig, &none).await,
        Err(ExchangeError::Denied(_))
    ));
    assert_eq!(rig.issuer.hits(), 0, "no key was even fetched for them");
}

// ---- iss, aud, time -----------------------------------------------------

#[tokio::test]
async fn a_token_of_another_issuer_or_audience_is_denied() {
    let rig = rig().await;
    let mut body = claims(&rig.issuer.url());
    body["iss"] = json!("http://evil.example");
    denied(exchange(&rig, &sign(&body)).await, "no_trusted_publisher");

    let mut body = claims(&rig.issuer.url());
    body["aud"] = json!("someone-else");
    denied(exchange(&rig, &sign(&body)).await, "token_not_trusted");
}

#[tokio::test]
async fn expired_not_yet_valid_stale_or_future_tokens_are_denied() {
    let rig = rig().await;
    let n = now();
    let mut expired = claims(&rig.issuer.url());
    expired["exp"] = json!(n - 3600);
    denied(exchange(&rig, &sign(&expired)).await, "token_not_trusted");

    let mut early = claims(&rig.issuer.url());
    early["nbf"] = json!(n + 3600);
    denied(exchange(&rig, &sign(&early)).await, "token_not_trusted");

    // Still unexpired, but minted too long ago for an exchange.
    let mut stale = claims(&rig.issuer.url());
    stale["iat"] = json!(n - 7200);
    stale["exp"] = json!(n + 600);
    denied(exchange(&rig, &sign(&stale)).await, "token_not_trusted");

    let mut future = claims(&rig.issuer.url());
    future["iat"] = json!(n + 3600);
    future["nbf"] = json!(n);
    denied(exchange(&rig, &sign(&future)).await, "token_not_trusted");

    let mut no_iat = claims(&rig.issuer.url());
    no_iat.as_object_mut().unwrap().remove("iat");
    denied(exchange(&rig, &sign(&no_iat)).await, "token_not_trusted");
}

// ---- claims bind to the registry, never to the token --------------------

#[tokio::test]
async fn claims_that_do_not_match_the_registry_record_are_denied() {
    let rig = rig().await;
    for (claim, value) in [
        ("sub", json!("repo:acme/other:ref:refs/heads/main")),
        ("repository", json!("acme/other")),
        ("ref", json!("refs/heads/evil")),
        ("workflow", json!("evil.yml")),
        ("workflow", json!(["release.yml"])),
    ] {
        let mut body = claims(&rig.issuer.url());
        body[claim] = value;
        denied(exchange(&rig, &sign(&body)).await, "token_not_trusted");
    }
    // A claim the record pins but the token lacks.
    let mut body = claims(&rig.issuer.url());
    body.as_object_mut().unwrap().remove("workflow");
    denied(exchange(&rig, &sign(&body)).await, "token_not_trusted");
}

#[tokio::test]
async fn the_token_cannot_choose_its_repository_or_account() {
    let rig = rig().await;
    // The token claims whatever it likes about the target: it only matters
    // what the registry says about the repository the caller names.
    let mut body = claims(&rig.issuer.url());
    body["hodei_repository"] = json!("private");
    body["account"] = json!("other");
    let token = sign(&body);
    for (account, repository) in [
        ("acme", "private"),
        ("other", "releases"),
        ("acme", "Releases"),
    ] {
        let result = rig.service.exchange(&token, account, repository).await;
        assert!(
            matches!(result, Err(ExchangeError::Denied("no_trusted_publisher"))),
            "{account}/{repository}: {result:?}"
        );
    }
    assert!(
        rig.service
            .exchange(&token, "acme", "releases")
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn a_malformed_target_is_a_bad_request() {
    let rig = rig().await;
    let token = sign(&claims(&rig.issuer.url()));
    for (account, repository) in [("", "releases"), ("acme", "../x"), ("ac me", "r")] {
        assert_eq!(
            rig.service
                .exchange(&token, account, repository)
                .await
                .unwrap_err(),
            ExchangeError::BadRequest
        );
    }
}

// ---- JWKS through the egress, fail closed --------------------------------

#[tokio::test]
async fn an_unreachable_or_broken_jwks_fails_closed() {
    let rig = rig_with(PublisherSettings {
        jwks_refetch_cooldown: Duration::ZERO,
        ..PublisherSettings::default()
    })
    .await;
    let token = || sign(&claims(&rig.issuer.url()));
    rig.issuer.serve(500, "boom".into());
    assert!(matches!(
        exchange(&rig, &token()).await,
        Err(ExchangeError::Unavailable("jwks_unavailable"))
    ));
    rig.issuer.serve(200, "not json".into());
    assert!(matches!(
        exchange(&rig, &token()).await,
        Err(ExchangeError::Unavailable("jwks_unavailable"))
    ));
    rig.issuer.serve(200, json!({"keys": []}).to_string());
    denied(exchange(&rig, &token()).await, "token_not_trusted");
}

#[tokio::test]
async fn the_jwks_is_never_fetched_through_a_redirect() {
    let rig = rig().await;
    let mut record = record(&rig.issuer);
    record.jwks_url = format!("{}/redirect", rig.issuer.url());
    rig.registry.put(&record).await.unwrap();
    let token = sign(&claims(&rig.issuer.url()));
    assert!(matches!(
        exchange(&rig, &token).await,
        Err(ExchangeError::Unavailable("jwks_unavailable"))
    ));
    assert_eq!(rig.issuer.redirected_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_jwks_outside_the_egress_allow_list_is_never_contacted() {
    let rig = rig().await;
    let spare = start_issuer().await;
    let mut record = record(&rig.issuer);
    // Another loopback port: not in the administrator's allow-list.
    record.jwks_url = format!("{}/jwks", spare.url());
    rig.registry.put(&record).await.unwrap();
    let token = sign(&claims(&rig.issuer.url()));
    assert!(matches!(
        exchange(&rig, &token).await,
        Err(ExchangeError::Unavailable("jwks_unavailable"))
    ));
    assert_eq!(spare.hits(), 0);
}

#[tokio::test]
async fn there_is_no_stale_key_fallback() {
    let rig = rig_with(PublisherSettings {
        jwks_ttl: Duration::ZERO,
        ..PublisherSettings::default()
    })
    .await;
    exchange(&rig, &sign(&claims(&rig.issuer.url())))
        .await
        .expect("the keys were obtained");
    rig.issuer.serve(503, "down".into());
    assert!(matches!(
        exchange(&rig, &sign(&claims(&rig.issuer.url()))).await,
        Err(ExchangeError::Unavailable("jwks_unavailable"))
    ));
}

#[tokio::test]
async fn keys_are_cached_and_an_unknown_kid_does_not_stampede_the_issuer() {
    let rig = rig().await;
    for _ in 0..3 {
        exchange(&rig, &sign(&claims(&rig.issuer.url())))
            .await
            .unwrap();
    }
    assert_eq!(rig.issuer.hits(), 1, "one fetch serves the cache window");
    // A kid the issuer never published, many times: refetch is rate limited.
    let rogue = sign_with(
        "issuer-key-2.json",
        "issuer-key-2",
        &claims(&rig.issuer.url()),
    );
    for _ in 0..5 {
        denied(exchange(&rig, &rogue).await, "token_not_trusted");
    }
    assert_eq!(rig.issuer.hits(), 1);
}

#[tokio::test]
async fn key_rotation_is_picked_up_and_the_retired_key_stops_working() {
    let rig = rig_with(PublisherSettings {
        jwks_refetch_cooldown: Duration::ZERO,
        ..PublisherSettings::default()
    })
    .await;
    exchange(&rig, &sign(&claims(&rig.issuer.url())))
        .await
        .unwrap();
    // The issuer rotates: only key 2 is published from now on.
    rig.issuer.serve(200, fixture("issuer-jwks-2.json"));
    let rotated = sign_with(
        "issuer-key-2.json",
        "issuer-key-2",
        &claims(&rig.issuer.url()),
    );
    exchange(&rig, &rotated)
        .await
        .expect("the new kid is fetched");
    // The retired kid is no longer published: it cannot exchange.
    denied(
        exchange(&rig, &sign(&claims(&rig.issuer.url()))).await,
        "token_not_trusted",
    );
}

// ---- replay --------------------------------------------------------------

#[tokio::test]
async fn a_token_exchanges_once() {
    let rig = rig().await;
    let token = sign(&claims(&rig.issuer.url()));
    exchange(&rig, &token).await.unwrap();
    denied(exchange(&rig, &token).await, "replayed");
}

#[tokio::test]
async fn a_token_without_jti_is_denied() {
    let rig = rig().await;
    let mut body = claims(&rig.issuer.url());
    body.as_object_mut().unwrap().remove("jti");
    denied(exchange(&rig, &sign(&body)).await, "jti_missing");
}

#[tokio::test]
async fn an_unsigned_token_cannot_burn_a_jti() {
    let rig = rig().await;
    let body = claims(&rig.issuer.url());
    let forged = sign_with("issuer-key-2.json", "issuer-key-1", &body);
    denied(exchange(&rig, &forged).await, "token_not_trusted");
    // The honest token with the same jti still works.
    exchange(&rig, &sign(&body)).await.unwrap();
}

#[tokio::test]
async fn a_full_replay_cache_fails_closed_instead_of_forgetting() {
    let rig = rig_with(PublisherSettings {
        replay_capacity: 1,
        ..PublisherSettings::default()
    })
    .await;
    let first = sign(&claims(&rig.issuer.url()));
    exchange(&rig, &first).await.unwrap();
    assert!(matches!(
        exchange(&rig, &sign(&claims(&rig.issuer.url()))).await,
        Err(ExchangeError::Unavailable("replay_cache_full"))
    ));
    // And the first is still remembered.
    denied(exchange(&rig, &first).await, "replayed");
}

// ---- the credential -------------------------------------------------------

async fn credential(rig: &Rig) -> String {
    rig.service
        .exchange(&sign(&claims(&rig.issuer.url())), "acme", "releases")
        .await
        .unwrap()
        .credential
}

#[tokio::test]
async fn the_credential_is_short_lived_and_capped() {
    let rig = rig_with(PublisherSettings {
        credential_ttl: Duration::from_secs(1),
        ..PublisherSettings::default()
    })
    .await;
    let credential = credential(&rig).await;
    assert!(rig.service.verify_credential(&credential).await.is_ok());
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert_eq!(
        rig.service
            .verify_credential(&credential)
            .await
            .unwrap_err(),
        GateError::Invalid
    );
    // An operator cannot configure a long-lived credential.
    let long = rig_with(PublisherSettings {
        credential_ttl: Duration::from_secs(86_400),
        ..PublisherSettings::default()
    })
    .await;
    let exchange = long
        .service
        .exchange(&sign(&claims(&long.issuer.url())), "acme", "releases")
        .await
        .unwrap();
    assert!(exchange.expires_in <= 900);
}

#[tokio::test]
async fn a_tampered_or_foreign_credential_is_not_a_credential() {
    let rig = rig().await;
    let credential = credential(&rig).await;
    // Another deployment's secret signs a credential for the same scope.
    let other = PublishersService::new(
        rig.registry.clone(),
        fetcher_for(rig.issuer.port),
        "another-secret-another-secret-another-x",
        PublisherSettings::default(),
    )
    .unwrap();
    assert!(other.verify_credential(&credential).await.is_err());
    let mut tampered = credential.clone();
    tampered.pop();
    tampered.push('x');
    assert!(rig.service.verify_credential(&tampered).await.is_err());
    // The OIDC token is not a publisher credential either.
    assert!(
        rig.service
            .verify_credential(&sign(&claims(&rig.issuer.url())))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn removing_the_record_revokes_the_credential_at_once() {
    let rig = rig().await;
    let credential = credential(&rig).await;
    assert!(rig.service.verify_credential(&credential).await.is_ok());
    assert!(
        rig.registry
            .delete("acme", "releases", "ci-main")
            .await
            .unwrap()
    );
    assert_eq!(
        rig.service
            .verify_credential(&credential)
            .await
            .unwrap_err(),
        GateError::Invalid
    );
}

// ---- the registry validates what an administrator writes ------------------

#[test]
fn a_record_cannot_be_written_with_wildcards_reserved_claims_or_bad_urls() {
    let good = || PublisherSpec {
        issuer: "https://token.actions.example".into(),
        audience: "hodei".into(),
        jwks_url: "https://token.actions.example/jwks".into(),
        subject: "repo:acme/app:ref:refs/heads/main".into(),
        claims: BTreeMap::new(),
    };
    assert!(good().validate("ci-main").is_ok());
    assert!(good().validate("bad id").is_err());
    let mut spec = good();
    spec.subject = "repo:acme/*".into();
    assert!(spec.validate("x").is_err());
    let mut spec = good();
    spec.claims.insert("aud".into(), "x".into());
    assert!(spec.validate("x").is_err());
    let mut spec = good();
    spec.claims.insert("ref".into(), "refs/heads/*".into());
    assert!(spec.validate("x").is_err());
    let mut spec = good();
    spec.jwks_url = "ftp://x/jwks".into();
    assert!(spec.validate("x").is_err());
    let mut spec = good();
    spec.jwks_url = "https://token.actions.example/jwks?n=1".into();
    assert!(spec.validate("x").is_err());
    let mut spec = good();
    spec.issuer = "https://user:pw@x.example".into();
    assert!(spec.validate("x").is_err());
    assert!(
        PublishersService::new(
            Arc::new(InMemoryPublisherRegistry::default()),
            fetcher_for(1),
            "short",
            PublisherSettings::default()
        )
        .is_err()
    );
}

#[test]
fn the_credential_secret_never_shows_in_the_debug_of_the_configuration() {
    let mut config = hodei_artifacts_api::AppConfig::default();
    config.security.publisher_credential_secret = Some(SECRET.to_string());
    let text = format!("{:?}", config.security);
    assert!(!text.contains(SECRET), "{text}");
    assert!(text.contains("<redacted>"));
}

#[test]
fn the_bundle_must_declare_the_management_action_when_publishers_are_mounted() {
    use hodei_artifacts_api::actions::check_publishers_declared;
    let community = std::fs::read_to_string(format!(
        "{}/config/community/schema.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let schema = cedar_policy::Schema::from_json_str(&community).unwrap();
    check_publishers_declared(&schema).expect("the community bundle declares it");
    let without = community.replace("ManageTrustedPublishers", "ManageSomethingElse");
    let schema = cedar_policy::Schema::from_json_str(&without).unwrap();
    assert!(check_publishers_declared(&schema).is_err());
}

#[tokio::test]
async fn the_real_user_authenticator_rejects_a_publisher_credential() {
    use hodei_artifacts_api::auth::JwtAuthenticator;
    use hodei_authz::{Credentials, PrincipalAuthenticator};
    let rig = rig().await;
    let credential = credential(&rig).await;
    // The worst case: the user authenticator is configured with the SAME
    // secret, and the issuer and audience the credential carries.
    let user = JwtAuthenticator::new(
        SECRET.to_string(),
        "hodei-publishers".to_string(),
        "hodei-artifacts".to_string(),
    );
    assert!(
        user.authenticate(&Credentials::bearer(&credential))
            .await
            .is_err(),
        "a publisher credential is not a user credential"
    );
}

#[tokio::test]
async fn a_configured_leeway_is_capped() {
    let rig = rig_with(PublisherSettings {
        leeway: Duration::from_secs(86_400),
        ..PublisherSettings::default()
    })
    .await;
    let mut expired = claims(&rig.issuer.url());
    expired["exp"] = json!(now() - 3600);
    denied(exchange(&rig, &sign(&expired)).await, "token_not_trusted");
}

// ---- F1: a failing or slow issuer cannot be used to stall the service ------

#[tokio::test]
async fn repeated_jwks_failures_cost_one_fetch_per_cooldown() {
    // ttl zero: the keys are always expired, so every request wants a fetch.
    let rig = rig_with(PublisherSettings {
        jwks_ttl: Duration::ZERO,
        exchange_burst: 1000,
        ..PublisherSettings::default()
    })
    .await;
    rig.issuer.serve(500, "down".into());
    for _ in 0..8 {
        assert!(matches!(
            exchange(&rig, &sign(&claims(&rig.issuer.url()))).await,
            Err(ExchangeError::Unavailable(_))
        ));
    }
    assert_eq!(
        rig.issuer.hits(),
        1,
        "the failure is cached for the cooldown"
    );
}

#[tokio::test]
async fn concurrent_requests_share_one_fetch() {
    let rig = Arc::new(
        rig_with(PublisherSettings {
            exchange_burst: 1000,
            ..PublisherSettings::default()
        })
        .await,
    );
    rig.issuer.serve(500, "down".into());
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let rig = rig.clone();
        let token = sign(&claims(&rig.issuer.url()));
        tasks.push(tokio::spawn(async move { exchange(&rig, &token).await }));
    }
    for task in tasks {
        assert!(task.await.unwrap().is_err());
    }
    assert_eq!(rig.issuer.hits(), 1);
}

#[tokio::test]
async fn a_slow_issuer_does_not_delay_a_healthy_one() {
    let slow_issuer = start_issuer().await;
    let healthy = start_issuer().await;
    let registry = Arc::new(InMemoryPublisherRegistry::default());
    let mut slow_record = record(&slow_issuer);
    slow_record.repository = "slow".into();
    slow_record.jwks_url = format!("{}/slow", slow_issuer.url());
    registry.put(&slow_record).await.unwrap();
    registry.put(&record(&healthy)).await.unwrap();
    let policy = EgressPolicy::new(
        [slow_issuer.port, healthy.port]
            .into_iter()
            .map(|port| AllowedHost {
                host: "127.0.0.1".into(),
                port: Some(port),
                allow_private: true,
                allow_http: true,
            })
            .collect(),
    )
    .unwrap();
    let fetcher = ProxyFetcher::new(
        policy,
        FetchLimits {
            connect_timeout: Duration::from_secs(2),
            total_timeout: Duration::from_secs(5),
            max_bytes: 1 << 20,
        },
        4,
    );
    let service = Arc::new(
        PublishersService::new(registry, fetcher, SECRET, PublisherSettings::default()).unwrap(),
    );
    let stalled = {
        let service = service.clone();
        let token = sign(&claims(&slow_issuer.url()));
        tokio::spawn(async move { service.exchange(&token, "acme", "slow").await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let started = std::time::Instant::now();
    service
        .exchange(&sign(&claims(&healthy.url())), "acme", "releases")
        .await
        .expect("the healthy issuer exchanges");
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "the healthy issuer waited {:?} behind the slow one",
        started.elapsed()
    );
    assert!(stalled.await.unwrap().is_ok());
}

// ---- F2: cheap checks first, bounded rate ----------------------------------

struct Counting {
    inner: InMemoryPublisherRegistry,
    lists: AtomicUsize,
}

#[async_trait::async_trait]
impl PublisherRegistry for Counting {
    async fn put(
        &self,
        p: &TrustedPublisher,
    ) -> Result<(), hodei_artifacts_api::publishers::RegistryError> {
        self.inner.put(p).await
    }
    async fn delete(
        &self,
        a: &str,
        r: &str,
        i: &str,
    ) -> Result<bool, hodei_artifacts_api::publishers::RegistryError> {
        self.inner.delete(a, r, i).await
    }
    async fn list(
        &self,
        a: &str,
        r: &str,
    ) -> Result<Vec<TrustedPublisher>, hodei_artifacts_api::publishers::RegistryError> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.inner.list(a, r).await
    }
}

#[tokio::test]
async fn an_unparsable_or_disallowed_token_costs_no_registry_read_and_no_budget() {
    let issuer = start_issuer().await;
    let registry = Arc::new(Counting {
        inner: InMemoryPublisherRegistry::default(),
        lists: AtomicUsize::new(0),
    });
    registry.put(&record(&issuer)).await.unwrap();
    let service = PublishersService::new(
        registry.clone(),
        fetcher_for(issuer.port),
        SECRET,
        PublisherSettings {
            exchange_rate_per_sec: 1,
            exchange_burst: 2,
            ..PublisherSettings::default()
        },
    )
    .unwrap();
    let mut hs = Header::new(Algorithm::HS256);
    hs.kid = Some("k".into());
    let hs_token = encode(&hs, &claims(&issuer.url()), &EncodingKey::from_secret(b"x")).unwrap();
    for _ in 0..50 {
        for token in ["garbage", "a.b.c", hs_token.as_str(), ""] {
            assert!(matches!(
                service.exchange(token, "acme", "releases").await,
                Err(ExchangeError::Denied(_))
            ));
        }
    }
    assert_eq!(registry.lists.load(Ordering::SeqCst), 0);
    // The budget is intact: the honest token still gets through.
    service
        .exchange(&sign(&claims(&issuer.url())), "acme", "releases")
        .await
        .expect("garbage did not drain the budget");
}

#[tokio::test]
async fn the_exchange_rate_is_bounded() {
    let rig = rig_with(PublisherSettings {
        exchange_rate_per_sec: 1,
        exchange_burst: 3,
        ..PublisherSettings::default()
    })
    .await;
    let mut limited = 0;
    for _ in 0..10 {
        if matches!(
            rig.service
                .exchange(&sign(&claims(&rig.issuer.url())), "acme", "releases")
                .await,
            Err(ExchangeError::RateLimited)
        ) {
            limited += 1;
        }
    }
    assert!(limited >= 5, "only {limited} were limited");
}

// ---- F4 / F5 / F6 / F8 ------------------------------------------------------

#[tokio::test]
async fn a_huge_exp_cannot_overflow_the_replay_cache() {
    let rig = rig().await;
    for _ in 0..3 {
        let mut body = claims(&rig.issuer.url());
        body["exp"] = json!(u64::MAX);
        exchange(&rig, &sign(&body)).await.expect("no overflow");
    }
}

#[tokio::test]
async fn a_recreated_record_with_other_content_does_not_resurrect_the_credential() {
    let rig = rig().await;
    let credential = credential(&rig).await;
    let mut changed = record(&rig.issuer);
    changed.subject = "repo:acme/other:ref:refs/heads/main".into();
    rig.registry.put(&changed).await.unwrap();
    assert_eq!(
        rig.service
            .verify_credential(&credential)
            .await
            .unwrap_err(),
        GateError::Invalid
    );
    // Same id, other claims: also invalid. Same content again: valid again.
    let mut claims_changed = record(&rig.issuer);
    claims_changed
        .claims
        .insert("ref".into(), "refs/heads/dev".into());
    rig.registry.put(&claims_changed).await.unwrap();
    assert!(rig.service.verify_credential(&credential).await.is_err());
    rig.registry.put(&record(&rig.issuer)).await.unwrap();
    assert!(rig.service.verify_credential(&credential).await.is_ok());
}

#[tokio::test]
async fn a_registry_that_may_hide_records_is_not_decided_on() {
    let rig = rig().await;
    for n in 0..40 {
        let mut extra = record(&rig.issuer);
        extra.id = format!("extra-{n}");
        rig.registry.put(&extra).await.unwrap();
    }
    assert!(matches!(
        exchange(&rig, &sign(&claims(&rig.issuer.url()))).await,
        Err(ExchangeError::Unavailable("registry_oversized"))
    ));
}

#[tokio::test]
async fn a_token_with_a_crit_header_is_refused_before_anything_else() {
    use base64::Engine as _;
    let rig = rig().await;
    let b64 = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
    let token = format!(
        "{}.{}.AAAA",
        b64(&json!({"alg": "RS256", "kid": "issuer-key-1", "crit": ["exp"]})),
        b64(&claims(&rig.issuer.url()))
    );
    denied(exchange(&rig, &token).await, "crit_header");
    assert_eq!(rig.issuer.hits(), 0);
}

#[tokio::test]
async fn more_than_256_distinct_jwks_urls_never_block_a_new_issuer() {
    let rig = rig_with(PublisherSettings {
        exchange_burst: 10_000,
        exchange_rate_per_sec: 10_000,
        ..PublisherSettings::default()
    })
    .await;
    for n in 0..300 {
        let mut extra = record(&rig.issuer);
        extra.repository = format!("repo-{n}");
        extra.jwks_url = format!("{}/jwks?n={n}", rig.issuer.url());
        rig.registry.put(&extra).await.unwrap();
        rig.service
            .exchange(
                &sign(&claims(&rig.issuer.url())),
                "acme",
                &format!("repo-{n}"),
            )
            .await
            .expect("every url is served, none is refused for fullness");
    }
    // The original issuer (its slot may have been evicted) still works.
    exchange(&rig, &sign(&claims(&rig.issuer.url())))
        .await
        .expect("a new or evicted issuer is not blocked");
}

#[tokio::test]
async fn a_known_kid_still_verifies_while_an_unknown_kid_refetch_failed() {
    let rig = rig_with(PublisherSettings {
        jwks_refetch_cooldown: Duration::from_millis(300),
        ..PublisherSettings::default()
    })
    .await;
    exchange(&rig, &sign(&claims(&rig.issuer.url())))
        .await
        .expect("warm cache");
    tokio::time::sleep(Duration::from_millis(400)).await;
    rig.issuer.serve(500, "boom".into());
    // An invented kid forces a refetch, which fails and arms the negative cache.
    let ghost = sign_with("issuer-key-1.json", "ghost", &claims(&rig.issuer.url()));
    assert!(exchange(&rig, &ghost).await.is_err());
    // A known kid is served from the fresh set regardless.
    exchange(&rig, &sign(&claims(&rig.issuer.url())))
        .await
        .expect("a known kid is not locked out by an unknown one");
}

#[tokio::test]
async fn a_far_future_exp_does_not_pin_the_replay_cache() {
    let rig = rig_with(PublisherSettings {
        max_token_age: Duration::from_secs(1),
        leeway: Duration::ZERO,
        replay_capacity: 2,
        ..PublisherSettings::default()
    })
    .await;
    for _ in 0..2 {
        let mut body = claims(&rig.issuer.url());
        body["exp"] = json!(u64::MAX);
        exchange(&rig, &sign(&body)).await.expect("fits");
    }
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let mut body = claims(&rig.issuer.url());
    body["exp"] = json!(u64::MAX);
    exchange(&rig, &sign(&body))
        .await
        .expect("entries expire with the freshness window, not with exp");
}
