//! In-process HTTP contract for the trust capsule of a release.
//!
//! The capsule endpoint answers the question the capsule exists for: what
//! evidence do we have about this artefact, what is missing, and where did
//! each assertion come from. It is a read, it is authorized, and it never
//! crosses an account.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::evidence::{EvidenceReader, EvidenceStoreError};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, AuthorizationOutcome, Credentials, IndeterminateReason,
    PrincipalAuthenticator, PrincipalClaims, PrincipalId,
};
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::{ContentDigest, ContentRef};
use kernel::Hrn;
use tempfile::TempDir;
use tower::ServiceExt;

struct TestAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for TestAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        let (account, name) = match token {
            "alice" => ("acme", "alice"),
            "bob" => ("acme", "bob"),
            "carol" => ("other", "carol"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

/// A reader that holds the evidence of one subject.
struct StubEvidenceReader {
    account: String,
    subject: Hrn,
    evidence: Vec<EvidenceRecord>,
}

#[async_trait]
impl EvidenceReader for StubEvidenceReader {
    async fn evidence_of_subject(
        &self,
        account: &str,
        subject: &Hrn,
    ) -> Result<Vec<EvidenceRecord>, EvidenceStoreError> {
        if account != self.account || subject != &self.subject {
            return Ok(Vec::new());
        }
        Ok(self.evidence.clone())
    }
}

/// A reader that never has any evidence.
struct NoEvidenceReader;

#[async_trait]
impl EvidenceReader for NoEvidenceReader {
    async fn evidence_of_subject(
        &self,
        _account: &str,
        _subject: &Hrn,
    ) -> Result<Vec<EvidenceRecord>, EvidenceStoreError> {
        Ok(Vec::new())
    }
}

fn artefact_hrn(account: &str) -> Hrn {
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu-west-1".to_string(),
        account.to_string(),
        "artifact".to_string(),
        "web-1.0.0".to_string(),
    )
}

fn evidence(subject: &Hrn, id: &str, kind: EvidenceType, verified: bool) -> EvidenceRecord {
    let tenant = Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu-west-1".to_string(),
        subject.account_id().to_string(),
        "Artifact".to_string(),
        "web-1.0.0".to_string(),
    );
    EvidenceRecord::builder(EvidenceId::new(id), tenant)
        .evidence_type(kind)
        .subjects(vec![subject.clone()])
        .source(SourceRef::by_external_id(format!("external-{id}")))
        .content(ContentRef::by_digest(ContentDigest::of_bytes(
            id.as_bytes(),
        )))
        .observed_at(chrono::Utc::now())
        .recorded_at(chrono::Utc::now())
        .verification(if verified {
            EvidenceVerification::verified("dsse")
        } else {
            EvidenceVerification::unverified()
        })
        .normalization(Normalization::new("cyclonedx", "1.0.0"))
        .producer(Producer::new("connector", "pipelinek").with_version("1.0.0"))
        .build()
}

fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadCapsule",
        resource
    );"#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

fn cedar_deny() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
        resource
    );"#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

struct IndeterminateAuthorizer;

impl AuthorizerPort for IndeterminateAuthorizer {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::indeterminate(IndeterminateReason::EvaluationError)
    }
}

use hodei_authz::AuthorizationRequest;

async fn app_with_evidence(
    authorizer: Arc<dyn AuthorizerPort>,
    reader: Arc<dyn EvidenceReader>,
) -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.rocksdb.path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap");
    let state = state.with_authorization(authorizer).with_repository_auth(
        Arc::new(TestAuthenticator),
        Arc::new(hodei_artifacts_api::repository::InMemoryRepositoryReader::empty()),
    );
    let state = state.with_evidence_store(reader);
    (build_router(state, &config), tmp)
}

/// The request for the capsule of an artefact.
fn request(account_token: &str, hrn: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/capsules?hrn={}",
            hrn.replace(':', "%3A").replace('/', "%2F")
        ))
        .header("authorization", format!("Bearer {account_token}"))
        .body(Body::empty())
        .expect("a request")
}

/// The capsule endpoint answers with the capsule and its profile.
#[tokio::test]
async fn the_capsule_of_a_release_comes_with_its_profile_and_its_evidence() {
    let subject = artefact_hrn("acme");
    let reader = Arc::new(StubEvidenceReader {
        account: "acme".to_string(),
        subject: subject.clone(),
        evidence: vec![
            evidence(
                &subject,
                "ev-integrity",
                EvidenceType::ArtifactIntegrity,
                true,
            ),
            evidence(&subject, "ev-sbom", EvidenceType::Sbom, true),
            evidence(
                &subject,
                "ev-signature",
                EvidenceType::SignatureVerification,
                true,
            ),
        ],
    });
    let (app, _tmp) = app_with_evidence(cedar(), reader).await;

    let response = app
        .oneshot(request("alice", &subject.to_string()))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .expect("the body is JSON");

    assert_eq!(body["subject"], subject.to_string());
    assert!(!body["content_hash"].as_str().unwrap_or("").is_empty());
    assert!(!body["version"].as_str().unwrap_or("").is_empty());

    let dimensions = body["profile"]["dimensions"]
        .as_array()
        .expect("the profile has dimensions");
    assert!(!dimensions.is_empty());

    // The dimension of the integrity holds up the evidence that was
    // verified, and it links to it.
    let integrity = dimensions
        .iter()
        .find(|dimension| dimension["name"] == "integrity")
        .expect("the integrity dimension is there");
    assert_eq!(integrity["state"], "VERIFIED");
    assert_eq!(integrity["evidence"].as_array().unwrap().len(), 1);
    assert_eq!(
        integrity["evidence"][0]["evidence_id"], "ev-integrity",
        "the dimension links to the evidence that holds it up"
    );
    assert_eq!(integrity["evidence"][0]["normalizer"], "cyclonedx");
    assert_eq!(integrity["evidence"][0]["normalizer_version"], "1.0.0");

    // The dimension that has no evidence says so, and it is not a good one.
    let source = dimensions
        .iter()
        .find(|dimension| dimension["name"] == "source_assurance")
        .expect("the dimension is there");
    assert_eq!(source["state"], "MISSING");
    assert!(source["evidence"].as_array().unwrap().is_empty());
}

/// The endpoint says what is missing when there is no evidence at all.
#[tokio::test]
async fn a_release_without_evidence_gets_a_capsule_that_says_so() {
    let subject = artefact_hrn("acme");
    let (app, _tmp) = app_with_evidence(cedar(), Arc::new(NoEvidenceReader)).await;

    let response = app
        .oneshot(request("alice", &subject.to_string()))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .expect("the body is JSON");

    let dimensions = body["profile"]["dimensions"].as_array().unwrap();
    for dimension in dimensions {
        assert_ne!(
            dimension["state"], "VERIFIED",
            "a dimension without evidence is never affirmed"
        );
    }
}

/// The capsule of another account is not there.
#[tokio::test]
async fn the_capsule_of_another_account_is_not_there() {
    let subject = artefact_hrn("other");
    let (app, _tmp) = app_with_evidence(cedar(), Arc::new(NoEvidenceReader)).await;

    let response = app
        .oneshot(request("alice", &subject.to_string()))
        .await
        .expect("the request");
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "another account's capsule is hidden behind the same 404"
    );
}

/// The request has to be authenticated.
#[tokio::test]
async fn the_capsule_needs_a_credential() {
    let subject = artefact_hrn("acme");
    let (app, _tmp) = app_with_evidence(cedar(), Arc::new(NoEvidenceReader)).await;

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/capsules?hrn={subject}"))
        .body(Body::empty())
        .expect("a request");
    let response = app.oneshot(request).await.expect("the request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// The request has to be allowed.
#[tokio::test]
async fn the_capsule_needs_the_permission() {
    let subject = artefact_hrn("acme");
    let (app, _tmp) = app_with_evidence(cedar_deny(), Arc::new(NoEvidenceReader)).await;

    let response = app
        .oneshot(request("alice", &subject.to_string()))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// An authorizer that cannot decide does not open the door.
#[tokio::test]
async fn an_indeterminate_decision_does_not_open_the_door() {
    let subject = artefact_hrn("acme");
    let (app, _tmp) = app_with_evidence(
        Arc::new(IndeterminateAuthorizer),
        Arc::new(NoEvidenceReader),
    )
    .await;

    let response = app
        .oneshot(request("alice", &subject.to_string()))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// The request has to name the artefact.
#[tokio::test]
async fn the_capsule_needs_to_name_the_artefact() {
    let (app, _tmp) = app_with_evidence(cedar(), Arc::new(NoEvidenceReader)).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/capsules")
        .header("authorization", "Bearer alice")
        .body(Body::empty())
        .expect("a request");
    let response = app.oneshot(request).await.expect("the request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// The request has to name an artefact and not a repository.
#[tokio::test]
async fn the_capsule_of_something_that_is_not_an_artefact_is_not_there() {
    let repository = Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu-west-1".to_string(),
        "acme".to_string(),
        "repository".to_string(),
        "releases".to_string(),
    );
    let (app, _tmp) = app_with_evidence(cedar(), Arc::new(NoEvidenceReader)).await;

    let response = app
        .oneshot(request("alice", &repository.to_string()))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
