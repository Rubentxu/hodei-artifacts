//! The list of the repositories of the account.
//!
//! The contract of `GET /api/v1/repositories` promises a list with `limit`
//! and `offset`, and the code used to answer `400` to everything that did
//! not name an `hrn`. This suite measures what the route serves now, and
//! what the contract says about it.
//!
//! What is measured here:
//!
//! 1. The list is the account's, and the account comes from the verified
//!    principal: a repository of another account does not come out and is
//!    not counted.
//! 2. Cedar decides one repository at a time: what it denies does not come
//!    out, and it is not counted either.
//! 3. The page is the one that was asked for, and a page outside of what
//!    the contract promises is a bad request.
//! 4. A decision that could not be taken is not an empty list.
//! 5. The read by `hrn` keeps answering with the repository, and a
//!    repository of another account keeps being the same `404` that a
//!    repository that does not exist is.
//! 6. The contract and the code say the same thing.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::repository::{
    InMemoryRepositoryReader, InMemoryRepositoryStore, RepositoryReadError, RepositoryReader,
    RepositoryView,
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, AuthorizationOutcome, AuthorizationRequest, Credentials,
    IndeterminateReason, PrincipalAuthenticator, PrincipalClaims, PrincipalId,
};
use kernel::Hrn;
use serde_json::Value;
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;
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
            "carol" => ("other", "carol"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

struct IndeterminateAuthorizer;

impl AuthorizerPort for IndeterminateAuthorizer {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::indeterminate(IndeterminateReason::EvaluationError)
    }
}

/// The canonical name of a repository of an account.
fn hrn_of(account: &str, name: &str) -> Hrn {
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu".to_string(),
        account.to_string(),
        "repository".to_string(),
        name.to_string(),
    )
}

/// The row of a repository of an account.
fn view_of(account: &str, name: &str) -> RepositoryView {
    RepositoryView::new(
        hrn_of(account, name).to_string(),
        account.to_string(),
        name.to_string(),
        Some(format!("the {name} repository")),
    )
}

/// The reader of a registry with three repositories of `acme` and one of
/// `other`.
fn the_reader() -> Arc<InMemoryRepositoryReader> {
    Arc::new(
        InMemoryRepositoryReader::empty()
            .with(hrn_of("acme", "releases"), view_of("acme", "releases"))
            .with(hrn_of("acme", "snapshots"), view_of("acme", "snapshots"))
            .with(hrn_of("acme", "nightly"), view_of("acme", "nightly"))
            .with(hrn_of("other", "private"), view_of("other", "private")),
    )
}

/// A policy that lets alice read every repository of her account.
fn cedar_everything() -> Arc<dyn AuthorizerPort> {
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

/// A policy that lets alice read one repository and not the others.
fn cedar_one() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/releases"
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

/// A policy that lets alice create a repository and read it.
fn cedar_create_and_read() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"CreateRepository",
        resource
    );
    permit(
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

async fn app_with(
    authorizer: Arc<dyn AuthorizerPort>,
    reader: Arc<dyn RepositoryReader>,
) -> (axum::Router, Workdir) {
    let tmp = Workdir::new();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    tmp.apply(&mut config);

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
    let state = state
        .with_authorization(authorizer)
        .with_repository_auth(Arc::new(TestAuthenticator), reader);
    (build_router(state, &config), tmp)
}

/// The application over a store that reads and writes.
async fn app_with_store(
    authorizer: Arc<dyn AuthorizerPort>,
    store: Arc<InMemoryRepositoryStore>,
) -> (axum::Router, Workdir) {
    let tmp = Workdir::new();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    tmp.apply(&mut config);

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
    let state = state
        .with_authorization(authorizer)
        .with_repository_auth(Arc::new(TestAuthenticator), store.clone())
        .with_repository_store(store);
    (build_router(state, &config), tmp)
}

async fn get(app: &axum::Router, token: Option<&str>, query: &str) -> (StatusCode, Value) {
    let uri = if query.is_empty() {
        "/api/v1/repositories".to_string()
    } else {
        format!("/api/v1/repositories?{query}")
    };
    let mut builder = Request::builder().uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// A reader whose row cannot be read as a canonical name.
struct CorruptReader;

#[async_trait]
impl RepositoryReader for CorruptReader {
    async fn find_by_hrn(
        &self,
        _account: &str,
        _hrn: &Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError> {
        Ok(None)
    }

    async fn list(&self, _account: &str) -> Result<Vec<RepositoryView>, RepositoryReadError> {
        Ok(vec![RepositoryView::new(
            "no es un nombre canonico".to_string(),
            "acme".to_string(),
            "releases".to_string(),
            None,
        )])
    }
}

/// A reader whose storage returns a row of another account.
struct ForeignAccountReader;

#[async_trait]
impl RepositoryReader for ForeignAccountReader {
    async fn find_by_hrn(
        &self,
        _account: &str,
        _hrn: &Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError> {
        Ok(None)
    }

    async fn list(&self, _account: &str) -> Result<Vec<RepositoryView>, RepositoryReadError> {
        Ok(vec![
            view_of("acme", "releases"),
            view_of("other", "private"),
        ])
    }
}

/// A reader whose storage returns a row that disagrees with itself: the
/// account of the row and the account of its canonical name are not the
/// same.
struct ContradictingReader;

#[async_trait]
impl RepositoryReader for ContradictingReader {
    async fn find_by_hrn(
        &self,
        _account: &str,
        _hrn: &Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError> {
        Ok(None)
    }

    async fn list(&self, _account: &str) -> Result<Vec<RepositoryView>, RepositoryReadError> {
        Ok(vec![RepositoryView::new(
            hrn_of("other", "private").to_string(),
            "acme".to_string(),
            "releases".to_string(),
            None,
        )])
    }
}

/// What is stored and cannot be read is reported: a list that
/// shrinks silently asserts that there are no repositories where there is one that
/// cannot be read.
#[tokio::test]
async fn a_row_that_cannot_be_read_is_corruption_and_not_an_empty_list() {
    let (app, _tmp) = app_with(cedar_everything(), Arc::new(CorruptReader)).await;

    let (status, body) = get(&app, Some("alice"), "").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"], "corruption");
}

/// A row of another account does not come out of the list: it does not exist for this
/// principal, and it is not counted.
#[tokio::test]
async fn a_row_of_another_account_is_not_shown_and_is_not_counted() {
    let (app, _tmp) = app_with(cedar_everything(), Arc::new(ForeignAccountReader)).await;

    let (status, body) = get(&app, Some("alice"), "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(the_names(&body), ["releases"]);
    assert_eq!(
        body["total"], 1,
        "the total counted a repository of another account: {body}"
    );
}

/// A row that contradicts itself is corruption, and not an empty list: what
/// is stored and cannot be read is reported.
#[tokio::test]
async fn a_row_that_contradicts_itself_is_corruption() {
    let (app, _tmp) = app_with(cedar_everything(), Arc::new(ContradictingReader)).await;

    let (status, body) = get(&app, Some("alice"), "").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"], "corruption");
}

/// The read by `hrn` does not turn into a `400` because the client sends a
/// page that is of no use to it.
#[tokio::test]
async fn the_read_by_hrn_ignores_the_page_parameters() {
    let (app, _tmp) = app_with(cedar_everything(), the_reader()).await;

    let (status, body) = get(
        &app,
        Some("alice"),
        "hrn=hrn%3Ahodei%3Aartifacts%3Aeu%3Aacme%3Arepository%2Freleases&limit=0",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "releases");
}

/// The names of the repositories of a page.
fn the_names(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .expect("the items of the page")
        .iter()
        .map(|item| item["name"].as_str().expect("the name").to_string())
        .collect()
}

/// Creates a repository through the route.
async fn post(app: &axum::Router, token: Option<&str>, body: &str) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/repositories")
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// The repository that is created is in the list: the list and the
/// creation are the same registry.
#[tokio::test]
async fn a_created_repository_is_in_the_list() {
    let store = Arc::new(InMemoryRepositoryStore::empty());
    let (app, _tmp) = app_with_store(cedar_create_and_read(), store).await;

    let (status, _created) = post(&app, Some("alice"), r#"{"name":"releases"}"#).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get(&app, Some("alice"), "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    assert_eq!(the_names(&body), ["releases"]);
}

#[tokio::test]
async fn the_list_serves_the_repositories_of_the_account() {
    let (app, _tmp) = app_with(cedar_everything(), the_reader()).await;

    let (status, body) = get(&app, Some("alice"), "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 3);
    assert_eq!(body["limit"], 20);
    assert_eq!(body["offset"], 0);
    assert_eq!(the_names(&body), ["nightly", "releases", "snapshots"]);
}

#[tokio::test]
async fn the_list_does_not_name_the_repositories_of_another_account() {
    let (app, _tmp) = app_with(cedar_everything(), the_reader()).await;

    let (status, body) = get(&app, Some("alice"), "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 3);
    assert!(
        !the_names(&body).contains(&"private".to_string()),
        "the list named a repository of another account: {body}"
    );
}

#[tokio::test]
async fn the_list_only_shows_what_cedar_permits_and_does_not_count_the_rest() {
    let (app, _tmp) = app_with(cedar_one(), the_reader()).await;

    let (status, body) = get(&app, Some("alice"), "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(the_names(&body), ["releases"]);
    assert_eq!(
        body["total"], 1,
        "the total counted a repository that Cedar denied: {body}"
    );
}

#[tokio::test]
async fn the_page_is_the_one_that_was_asked_for() {
    let (app, _tmp) = app_with(cedar_everything(), the_reader()).await;

    let (status, body) = get(&app, Some("alice"), "limit=2&offset=1").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 3);
    assert_eq!(body["limit"], 2);
    assert_eq!(body["offset"], 1);
    assert_eq!(the_names(&body), ["releases", "snapshots"]);
}

#[tokio::test]
async fn a_page_outside_of_the_contract_is_a_bad_request() {
    let (app, _tmp) = app_with(cedar_everything(), the_reader()).await;

    for query in [
        "limit=0",
        "limit=101",
        "limit=many",
        "offset=-1",
        "limit=2&limit=3",
    ] {
        let (status, body) = get(&app, Some("alice"), query).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "the query {query} was answered with {status}: {body}"
        );
        assert_eq!(body["error"], "invalid");
    }
}

#[tokio::test]
async fn a_decision_that_could_not_be_taken_is_not_an_empty_list() {
    let (app, _tmp) = app_with(Arc::new(IndeterminateAuthorizer), the_reader()).await;

    let (status, body) = get(&app, Some("alice"), "").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"], "indeterminate");
}

#[tokio::test]
async fn the_list_needs_a_credential() {
    let (app, _tmp) = app_with(cedar_everything(), the_reader()).await;

    let (status, body) = get(&app, None, "").await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthenticated");
}

#[tokio::test]
async fn the_read_by_hrn_still_answers_with_the_repository() {
    let (app, _tmp) = app_with(cedar_everything(), the_reader()).await;

    let (status, body) = get(
        &app,
        Some("alice"),
        "hrn=hrn%3Ahodei%3Aartifacts%3Aeu%3Aacme%3Arepository%2Freleases",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "releases");
    assert_eq!(
        body["hrn"],
        "hrn:hodei:artifacts:eu:acme:repository/releases"
    );
    assert!(
        body.get("total").is_none(),
        "the read by hrn answered with the list: {body}"
    );
}

#[tokio::test]
async fn a_repository_of_another_account_is_the_same_404_that_a_missing_one_is() {
    let (app, _tmp) = app_with(cedar_everything(), the_reader()).await;

    let (foreign_status, foreign_body) = get(
        &app,
        Some("alice"),
        "hrn=hrn%3Ahodei%3Aartifacts%3Aeu%3Aother%3Arepository%2Fprivate",
    )
    .await;
    let (missing_status, missing_body) = get(
        &app,
        Some("alice"),
        "hrn=hrn%3Ahodei%3Aartifacts%3Aeu%3Aacme%3Arepository%2Fmissing",
    )
    .await;

    assert_eq!(foreign_status, StatusCode::NOT_FOUND);
    assert_eq!(missing_status, StatusCode::NOT_FOUND);
    assert_eq!(foreign_body, missing_body);
}

/// The contract, as it is bundled.
fn the_contract() -> serde_yaml::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/openapi/bundled.yaml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the contract cannot be read: {error}"));
    serde_yaml::from_str(&text).expect("the contract is a valid YAML")
}

/// The names of the properties of a schema.
fn the_properties(schema: &serde_yaml::Value) -> Vec<String> {
    schema["properties"]
        .as_mapping()
        .expect("the properties of the schema")
        .keys()
        .map(|key| key.as_str().expect("the name of the property").to_string())
        .collect()
}

#[tokio::test]
async fn the_contract_and_the_code_say_the_same_thing() {
    let contract = the_contract();
    let (app, _tmp) = app_with(cedar_everything(), the_reader()).await;
    let (status, body) = get(&app, Some("alice"), "").await;
    assert_eq!(status, StatusCode::OK);

    // The path the service serves is the one the contract declares.
    let operation = &contract["paths"]["/api/v1/repositories"]["get"];
    assert!(
        !operation.is_null(),
        "the contract does not declare the path the service serves"
    );

    // The parameters of the contract are the ones the route reads.
    let parameters: Vec<String> = operation["parameters"]
        .as_sequence()
        .expect("the parameters of the operation")
        .iter()
        .map(|parameter| parameter["name"].as_str().expect("the name").to_string())
        .collect();
    for name in ["hrn", "limit", "offset"] {
        assert!(
            parameters.contains(&name.to_string()),
            "the contract does not declare the parameter {name}: {parameters:?}"
        );
    }

    // The page the route serves is the page the contract declares.
    let list = &contract["components"]["schemas"]["RepositoryListResponse"];
    let declared = the_properties(list);
    let served: Vec<String> = body
        .as_object()
        .expect("the body of the list")
        .keys()
        .map(|key| key.to_string())
        .collect();
    for name in ["total", "items", "limit", "offset"] {
        assert!(
            declared.contains(&name.to_string()),
            "the contract does not declare {name}: {declared:?}"
        );
        assert!(
            served.contains(&name.to_string()),
            "the route does not serve {name}: {served:?}"
        );
    }

    // The repository of the page is the repository the contract declares.
    let repository = &contract["components"]["schemas"]["Repository"];
    let declared = the_properties(repository);
    let served: Vec<String> = body["items"][0]
        .as_object()
        .expect("the first repository of the page")
        .keys()
        .map(|key| key.to_string())
        .collect();
    for name in ["hrn", "account", "name", "description"] {
        assert!(
            declared.contains(&name.to_string()),
            "the contract does not declare {name}: {declared:?}"
        );
        assert!(
            served.contains(&name.to_string()),
            "the route does not serve {name}: {served:?}"
        );
    }

    // The failures the route serves are the failures the contract declares.
    let responses = operation["responses"]
        .as_mapping()
        .expect("the responses of the operation")
        .keys()
        .map(|key| key.as_str().expect("the status").to_string())
        .collect::<Vec<String>>();
    for status in ["200", "400", "401", "403", "404", "500", "503"] {
        assert!(
            responses.contains(&status.to_string()),
            "the contract does not declare the {status}: {responses:?}"
        );
    }

    // The body of a failure is the one the contract declares: the class of
    // the failure and a message, and nothing else that the service does not
    // send.
    let error = &contract["components"]["schemas"]["Error"];
    let required: Vec<String> = error["required"]
        .as_sequence()
        .expect("the required fields of the error")
        .iter()
        .map(|value| value.as_str().expect("the name").to_string())
        .collect();
    assert_eq!(required, ["error", "message"], "the contract: {error:?}");

    let (status, failure) = get(
        &app,
        Some("alice"),
        "hrn=hrn%3Ahodei%3Aartifacts%3Aeu%3Aacme%3Arepository%2Fmissing",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let served: Vec<String> = failure
        .as_object()
        .expect("the body of the failure")
        .keys()
        .map(|key| key.to_string())
        .collect();
    for name in &required {
        assert!(
            served.contains(name),
            "the route does not serve {name} of the error: {served:?}"
        );
    }
}
