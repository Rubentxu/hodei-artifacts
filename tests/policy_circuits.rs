//! The static gate bundle and the persisted IAM evaluator are separate
//! authorities. A persisted administrative policy must not silently become an
//! effective permission for repository requests, and both circuits have to
//! reach the same decision for the same request.
//!
//! The two circuits are compared over the whole decision matrix — permit,
//! deny, forbid and indeterminate — because a divergence that only shows up on
//! one branch of the matrix is a divergence all the same.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::{Entities, Schema};
use hodei_artifacts_api::authorization::{
    AuthorizeResourceUseCase, AuthorizerPort, SharedAuthorizer,
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, AuthorizationOutcome, Credentials, DenyReason,
    PrincipalAuthenticator, PrincipalClaims, PrincipalId, ResourceRef, VerifiedContext,
    verify_principal,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const PRINCIPAL: &str = "hrn:hodei:iam::acme:User/alice";
const RESOURCE: &str = "hrn:hodei:artifacts:eu:acme:repository/releases";

struct Alice;

#[async_trait]
impl PrincipalAuthenticator for Alice {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        match credentials {
            Credentials::Bearer(token) if token == "alice" => Ok(PrincipalClaims::new(
                PrincipalId::new(PRINCIPAL).unwrap(),
                AccountId::new("acme").unwrap(),
            )),
            _ => Err(AuthenticationError::InvalidCredentials),
        }
    }
}

/// The schema both circuits share.
///
/// The repository carries an `owner` attribute so a policy can be valid
/// against the schema and still be impossible to evaluate against an entity
/// that does not carry it. That is the indeterminate case. Two actions are
/// declared so a policy can name an action that does not apply to the request
/// under test, which is the deny case.
///
/// The same text feeds the Cedar authorizer and the bundle the deployment
/// declares, so both circuits really do work on one schema.
const SHARED_SCHEMA_JSON: &str = r#"{
  "": { "entityTypes": {}, "actions": {} },
  "Iam": {
    "entityTypes": { "User": { "shape": { "type": "Record", "attributes": {} } } },
    "actions": {}
  },
  "Admin": {
    "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
    "actions": {
      "AccessAdminApi": {
        "appliesTo": {
          "principalTypes": ["Iam::User"],
          "resourceTypes": ["Admin::AdminApi"]
        }
      }
    }
  },
  "Artifacts": {
    "entityTypes": {
      "Repository": {
        "shape": {
          "type": "Record",
          "attributes": { "owner": { "type": "String", "required": false } }
        }
      },
      "Artifact": {
        "shape": {
          "type": "Record",
          "attributes": {}
        }
      }
    },
    "actions": {
      "ReadRepository": {
        "appliesTo": {
          "principalTypes": ["Iam::User"],
          "resourceTypes": ["Artifacts::Repository"]
        }
      },
      "CreateRepository": {
        "appliesTo": {
          "principalTypes": ["Iam::User"],
          "resourceTypes": ["Artifacts::Repository"]
        }
      },
      "ReadCapsule": {
        "appliesTo": {
          "principalTypes": ["Iam::User"],
          "resourceTypes": ["Artifacts::Artifact"]
        }
      }
    }
  }
}"#;

/// The permit that opens the administrative surface.
///
/// The IAM routes are behind the admin gate, so the bundle has to carry it
/// for the persisted evaluator to be reachable at all. It is conditioned on
/// its own action, so it never matches the repository request under test.
const ADMIN_PERMIT: &str =
    r#"permit(principal, action == Admin::Action::"AccessAdminApi", resource);"#;

/// The schema both circuits share.
fn shared_schema() -> Schema {
    Schema::from_json_str(SHARED_SCHEMA_JSON).expect("the shared schema")
}

/// The decision each circuit has to reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// The policy permits the request.
    Permit,
    /// A `forbid` matched the request.
    Forbid,
    /// No policy matched the request.
    NoMatchingPolicy,
    /// The policy cannot be evaluated against the entity.
    Indeterminate,
}

/// One row of the decision matrix.
struct Case {
    name: &'static str,
    policy: &'static str,
    /// The service the action under test belongs to.
    service: &'static str,
    /// The action under test.
    action: &'static str,
    expected: Expected,
    /// Whether the request is allowed.
    allowed: bool,
}

const CASES: &[Case] = &[
    Case {
        name: "permit",
        policy: r#"permit(principal, action == Artifacts::Action::"ReadRepository", resource);"#,
        service: "artifacts",
        action: "ReadRepository",
        expected: Expected::Permit,
        allowed: true,
    },
    Case {
        // The IAM stores one policy per row, and Cedar parses one policy at a
        // time, so a forbid is stored on its own.
        name: "forbid",
        policy: r#"forbid(principal, action == Artifacts::Action::"ReadRepository", resource);"#,
        service: "artifacts",
        action: "ReadRepository",
        expected: Expected::Forbid,
        allowed: false,
    },
    Case {
        name: "deny",
        policy: r#"permit(principal, action == Artifacts::Action::"CreateRepository", resource);"#,
        service: "artifacts",
        action: "ReadRepository",
        expected: Expected::NoMatchingPolicy,
        allowed: false,
    },
    Case {
        // The action under test is one the schema does not declare, so the
        // gate cannot build the request and stays undecided.
        name: "indeterminate",
        policy: "permit(principal, action, resource);",
        service: "iam",
        action: "CreateUser",
        expected: Expected::Indeterminate,
        allowed: false,
    },
];

/// Boot the application with the shared schema declared as the bundle.
///
/// The IAM schema is registered too, so the test also proves that registering
/// it does not displace the bundle: the schema the gate enforces and the
/// schema the validator applies have to be the same one.
async fn app(policy: &str) -> (axum::Router, TempDir) {
    app_with_version(policy, None).await
}

/// The same application, with the IAM schema registered under a version.
async fn app_with_version(policy: &str, version: Option<&str>) -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = true;
    config.security.auth_jwt_secret = Some("hsp-28-matrix-secret".to_string());
    config.security.auth_issuer = Some("https://issuer.example".to_string());
    config.security.auth_audience = Some("hodei-artifacts".to_string());
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();

    let schema_path = tmp.path().join("schema.json");
    let policy_path = tmp.path().join("policies.cedar");
    std::fs::write(&schema_path, SHARED_SCHEMA_JSON).unwrap();
    std::fs::write(&policy_path, format!("{ADMIN_PERMIT}\n{policy}")).unwrap();
    config.security.authorization_schema_path = Some(schema_path.to_string_lossy().into_owned());
    config.security.authorization_policy_path = Some(policy_path.to_string_lossy().into_owned());

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: true,
            schema_version: version.map(str::to_string),
            validate_schemas: false,
        },
    )
    .await
    .unwrap()
    .with_authenticator(Arc::new(Alice));
    (build_router(state, &config), tmp)
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", "Bearer alice");
    let response = app
        .clone()
        .oneshot(match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        })
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The gate's decision for one policy set.
async fn gate_decision(policy: &str, service: &str, action_name: &str) -> AuthorizationOutcome {
    let authorizer: Arc<dyn AuthorizerPort> = Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
            shared_schema(),
        )
        .unwrap(),
    )));
    let use_case = AuthorizeResourceUseCase::new(authorizer);
    let context = VerifiedContext::from_principal(
        verify_principal(&Alice, &Credentials::bearer("alice"))
            .await
            .unwrap(),
    );
    let action = hodei_authz::ActionId::new(service, action_name).unwrap();
    let hrn: hodei_authz::hodei_hrn::Hrn = RESOURCE.parse().unwrap();
    use_case.execute(context, action, ResourceRef::new(hrn).unwrap())
}

#[tokio::test]
async fn the_decision_matrix_agrees_across_both_circuits() {
    for case in CASES {
        let (app, _tmp) = app(case.policy).await;

        // The gate decides with the bundle schema and the policy set.
        let outcome = gate_decision(case.policy, case.service, case.action).await;
        match case.expected {
            Expected::Permit => assert_eq!(
                outcome,
                AuthorizationOutcome::Permit,
                "the gate must permit: {}",
                case.name
            ),
            Expected::Forbid => assert_eq!(
                outcome,
                AuthorizationOutcome::deny(DenyReason::ExplicitForbid),
                "the gate must forbid: {}",
                case.name
            ),
            Expected::NoMatchingPolicy => assert_eq!(
                outcome,
                AuthorizationOutcome::deny(DenyReason::NoMatchingPolicy),
                "the gate must find no matching policy: {}",
                case.name
            ),
            Expected::Indeterminate => assert!(
                matches!(outcome, AuthorizationOutcome::Indeterminate(_)),
                "the gate must be unable to decide: {}, got {outcome:?}",
                case.name
            ),
        }
        assert_eq!(outcome.allows_access(), case.allowed, "gate: {}", case.name);

        // The IAM circuit stores the same policy set and evaluates the same
        // request against it.
        let (status, created) = call(
            &app,
            "POST",
            "/api/v1/iam/policies",
            Some(json!({
                "policy_id": format!("case-{}", case.name),
                "policy_content": case.policy,
                "attached_principals": [PRINCIPAL]
            })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the IAM circuit must accept the policy of the case {}: {created:?}",
            case.name
        );

        let (status, decision) = call(
            &app,
            "POST",
            "/api/v1/iam/evaluate",
            Some(json!({
                "principal_hrn": PRINCIPAL,
                "action": case.action,
                "resource_hrn": RESOURCE
            })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "evaluate {}: {decision:?}",
            case.name
        );
        let iam_allowed = decision["decision"].as_bool().unwrap_or(false);

        // Both circuits reach the same effective decision.
        assert_eq!(
            iam_allowed,
            case.allowed,
            "the evaluator disagrees on {}: gate allows {}, the evaluator allows {iam_allowed}",
            case.name,
            outcome.allows_access(),
        );
    }
}

#[tokio::test]
async fn registering_the_iam_schema_does_not_displace_the_bundle_schema() {
    let (app, _tmp) = app(CASES[0].policy).await;

    // The bundle declares Artifacts::ReadRepository, so a policy that uses it
    // is valid. The IAM schema does not declare it, so a validator that
    // selected the IAM schema would reject the very policies the gate
    // enforces.
    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": "bundle-action",
            "policy_content":
                "permit(principal, action == Artifacts::Action::\"ReadRepository\", resource);",
            "attached_principals": [PRINCIPAL]
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the bundle schema must be the one that validates: {created:?}"
    );

    // The IAM schema declares Iam::CreateUser, but the bundle does not, so
    // the gate could not enforce it either. The validator has to agree.
    let (status, rejected) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": "iam-only-action",
            "policy_content":
                "permit(principal, action == Iam::Action::\"CreateUser\", resource);",
            "attached_principals": [PRINCIPAL]
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a policy the gate would reject must not be accepted: {rejected:?}"
    );
}

#[tokio::test]
async fn persisted_iam_policy_does_not_change_the_static_gate() {
    // The gate's bundle opens the admin surface and nothing else, so the
    // repository surface stays closed.
    let (app, _tmp) =
        app(r#"permit(principal, action == Artifacts::Action::"CreateRepository", resource);"#)
            .await;

    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": "diagnostic",
            "policy_content":
                "permit(principal, action == Artifacts::Action::\"ReadRepository\", resource);",
            "attached_principals": [PRINCIPAL]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create: {created:?}");

    // The persisted evaluator can grant its diagnostic decision...
    let (status, decision) = call(
        &app,
        "POST",
        "/api/v1/iam/evaluate",
        Some(json!({
            "principal_hrn": PRINCIPAL,
            "action": "ReadRepository",
            "resource_hrn": RESOURCE
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(decision["decision"], json!(true));

    // ...but it is not the authority for the repository gate.
    let (status, _) = call(
        &app,
        "GET",
        "/api/v1/repositories?hrn=hrn%3Ahodei%3Aartifacts%3Aeu%3Aacme%3Arepository%2Freleases",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// A request whose entity types the schema does not describe is denied with
/// a reason, not answered with an error.
///
/// The gate answers the same way: a request that does not fit the schema is
/// a denial, because the caller asked for something the schema does not
/// describe. Answering with a server error would tell the caller nothing it
/// could act on, and would differ from the gate.
#[tokio::test]
async fn a_request_that_does_not_fit_the_schema_is_denied_with_a_reason() {
    let (app, _tmp) = app(CASES[0].policy).await;

    // The evaluator only evaluates the policies attached to the principal, so
    // the request needs one to reach the engine at all.
    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": "attached",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": [PRINCIPAL]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create: {created:?}");

    // The resource is of a type the schema does not declare.
    let (status, decision) = call(
        &app,
        "POST",
        "/api/v1/iam/evaluate",
        Some(json!({
            "principal_hrn": PRINCIPAL,
            "action": "ReadRepository",
            "resource_hrn": "hrn:hodei:storage::acme:Document/doc1"
        })),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "a request the schema does not describe must be denied, not fail: {decision:?}"
    );
    assert_eq!(decision["decision"], json!(false));
    let reason = decision["reason"].as_str().expect("a reason");
    assert!(
        reason.contains("schema"),
        "the reason must say the schema does not describe the request: {reason:?}"
    );
}

/// A bundle that does not declare the actions the service authorizes with is
/// a deployment that does not start.
///
/// The gate builds the Cedar uid of an action from its service and its name;
/// the persisted evaluator resolves the verb against the schema. A bundle
/// that declared the verb under another namespace would leave the gate unable
/// to decide while the evaluator decided happily, so the disagreement is
/// caught at boot instead of in production.
#[tokio::test]
async fn a_bundle_that_disagrees_with_the_service_does_not_start() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = true;
    config.security.auth_jwt_secret = Some("hsp-28-mismatch-secret".to_string());
    config.security.auth_issuer = Some("https://issuer.example".to_string());
    config.security.auth_audience = Some("hodei-artifacts".to_string());
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();

    // The verb is declared, but under another namespace than the one the
    // service builds: `Warehouse::Action::"ReadRepository"` instead of
    // `Artifacts::Action::"ReadRepository"`.
    let schema_path = tmp.path().join("schema.json");
    let policy_path = tmp.path().join("policies.cedar");
    std::fs::write(
        &schema_path,
        r#"{
  "": { "entityTypes": {}, "actions": {} },
  "Iam": { "entityTypes": { "User": { "shape": { "type": "Record", "attributes": {} } } }, "actions": {} },
  "Admin": {
    "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
    "actions": {
      "AccessAdminApi": {
        "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] }
      }
    }
  },
  "Warehouse": {
    "entityTypes": {
      "Repository": { "shape": { "type": "Record", "attributes": {} } }
    },
    "actions": {
      "ReadRepository": {
        "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Warehouse::Repository"] }
      },
      "CreateRepository": {
        "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Warehouse::Repository"] }
      }
    }
  }
}"#,
    )
    .unwrap();
    std::fs::write(
        &policy_path,
        r#"permit(principal, action == Warehouse::Action::"ReadRepository", resource);"#,
    )
    .unwrap();
    config.security.authorization_schema_path = Some(schema_path.to_string_lossy().into_owned());
    config.security.authorization_policy_path = Some(policy_path.to_string_lossy().into_owned());

    let outcome = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await;

    let error = match outcome {
        Ok(_) => panic!("the service must not start with a bundle it disagrees with"),
        Err(error) => error,
    };

    let message = error.to_string();
    assert!(
        message.contains("ReadRepository"),
        "the error must name the action that disagrees: {message}"
    );
    assert!(
        message.contains("Artifacts") && message.contains("Warehouse"),
        "the error must name both namespaces: {message}"
    );
}

/// A decision says which schema governed it.
///
/// A decision without the version that produced it cannot be attributed: the
/// same request, evaluated against another version of the schema, can reach
/// another decision, and then nobody can say which semantics applied.
///
/// The deployment declares the bundle authoritative, and a schema is stored
/// under its own version, so the version that governed the evaluation is the
/// identity of the bundle.
#[tokio::test]
async fn the_evaluation_names_the_schema_that_governed_it() {
    let (app, _tmp) = app_with_version(
        r#"permit(principal, action == Artifacts::Action::"ReadRepository", resource);"#,
        Some("v1"),
    )
    .await;

    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": "versioned",
            "policy_content":
                "permit(principal, action == Artifacts::Action::\"ReadRepository\", resource);",
            "attached_principals": [PRINCIPAL]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create: {created:?}");

    let (status, decision) = call(
        &app,
        "POST",
        "/api/v1/iam/evaluate",
        Some(json!({
            "principal_hrn": PRINCIPAL,
            "action": "ReadRepository",
            "resource_hrn": RESOURCE
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "evaluate: {decision:?}");

    assert_eq!(
        decision["schema_version"].as_str(),
        Some("authorization-bundle"),
        "the decision must name the schema that governed it: {decision:?}"
    );
}
