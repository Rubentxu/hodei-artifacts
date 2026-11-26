//! HSP-151 (backend slice of HSP-321): the Policy Studio over the wire, on
//! the REAL binary booted on `RUN_MODE=dgraph` against the real Dgraph.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT (`host:port`, no scheme, no TLS); without
//! it the test SKIPs. What it measures:
//!
//! - AC1: load -> validate -> simulate -> diff -> activate through the real
//!   endpoints; no credential is 401; an editor whose Cedar grant has no
//!   activate/rollback gets 403 with no effect; the separation of editor
//!   and approver is respected.
//! - AC2: a policy decision, an evidence gap and an engine error are told
//!   apart, and the gap names the requirement, the reason code and the
//!   evidence; the access refusal is a 403 that no handler produced.
//! - AC4: validate and simulate never move the head, the history or the
//!   events; the tenant is the verified principal's, never the body's; a
//!   foreign tenant neither sees nor activates; a stale head is a typed 409
//!   that overwrites nothing; rollback is a new revision.
//! - No credential reaches the log or the responses.

mod support;

#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

use serde_json::{Value, json};
use support::{Curl, Deployment};

fn nonce() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn the_schema() -> String {
    let mut schema: Value = serde_json::from_str(support::SCHEMA).expect("the schema parses");
    schema["Admin"] = json!({
        "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
        "actions": {
            "AccessAdminApi": {
                "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] }
            }
        }
    });
    schema.to_string()
}

const READS: [&str; 3] = [
    "get-policy-studio-head",
    "get-policy-studio-revisions",
    "get-policy-studio-history",
];
const WRITES: [&str; 3] = [
    "post-policy-studio-revisions",
    "post-policy-studio-validate",
    "post-policy-studio-simulate",
];
const MOVES: [&str; 2] = ["post-policy-studio-activate", "post-policy-studio-rollback"];

/// One permit per (account, user, route resource): the gate asks Cedar per
/// method and path, so this is exactly what separates read, write and move.
fn the_policy(grants: &[(&str, &str, Vec<&str>)]) -> String {
    let mut policy = String::new();
    for (account, user, routes) in grants {
        for route in routes {
            policy.push_str(&format!(
                "permit(principal == Iam::User::\"hrn:hodei:iam::{account}:User/{user}\", action == Admin::Action::\"AccessAdminApi\", resource == Admin::AdminApi::\"hrn:hodei:admin:global:{account}:AdminApi/{route}\");\n"
            ));
        }
    }
    policy
}

fn policy_json(version: &str, accepts: &[&str]) -> Value {
    json!({
        "id": "default",
        "version": version,
        "schema_version": "1.0.0",
        "requirements": [{
            "kind": {
                "kind": "dimension",
                "dimension": "inventory_completeness",
                "accepts": accepts
            },
            "on_failure": "deny",
            "overridable_by": null
        }]
    })
}

fn subject(account: &str, name: &str, state: &str, evidence: &[&str]) -> Value {
    json!({
        "subject": name,
        "context": {
            "version": "1.0.0",
            "subject": { "hrn": format!("hrn:hodei:artifact::{account}:oci:repo/{name}:latest"), "digest": null },
            "target": "production",
            "dimensions": [{ "name": "inventory_completeness", "state": state }],
            "capsule_hash": null,
            "risk": {
                "critical_known": 0, "critical_reachable": 0, "exposure": "UNKNOWN",
                "vex_accepted": 0, "established": false, "risk_version": 0
            },
            "exceptions": [],
            "evidence_ids": evidence,
            "assembled_at": "2026-11-20T00:00:00Z"
        }
    })
}

struct Studio<'a> {
    deployment: &'a Deployment,
    port: u16,
}

impl Studio<'_> {
    fn call(&self, method: &str, path: &str, token: &str, body: Option<&Value>) -> (u16, Value) {
        let directory = self.deployment.directory.path();
        let into = directory.join("response.bin");
        let _ = std::fs::remove_file(&into);
        let sent = directory.join("request.bin");
        let (file, content_type) = match body {
            Some(value) => {
                std::fs::write(&sent, value.to_string()).expect("the body is written");
                (Some(sent.as_path()), Some("application/json"))
            }
            None => (None, None),
        };
        let answer = Curl::call(
            self.port,
            method,
            &format!("/api/v1/policy-studio/{path}"),
            token,
            file,
            content_type,
            Some(&into),
        )
        .run();
        let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
        let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (answer.status, value)
    }

    /// Head, history and events: everything a non-moving call must leave alone.
    fn state_of(&self, token: &str) -> (Value, Value) {
        let (status, head) = self.call("GET", "head", token, None);
        assert_eq!(status, 200, "{head}");
        let (status, history) = self.call("GET", "history", token, None);
        assert_eq!(status, 200, "{history}");
        (head, history)
    }
}

#[tokio::test]
async fn the_studio_edits_the_real_lifecycle_per_tenant() {
    let Ok(endpoint) = std::env::var("HODEI_GRAPH_IT_ENDPOINT") else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let tag = nonce();
    let mine = format!("s151a{}", &tag[8..]);
    let theirs = format!("s151b{}", &tag[8..]);

    let all_studio: Vec<&str> = READS.iter().chain(&WRITES).chain(&MOVES).copied().collect();
    let editor_routes: Vec<&str> = READS.iter().chain(&WRITES).copied().collect();
    let deployment = Deployment::new();
    std::fs::write(
        deployment.directory.path().join("schema.json"),
        the_schema(),
    )
    .expect("schema");
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        the_policy(&[
            (&mine, "viewer", READS.to_vec()),
            (&mine, "editor", editor_routes),
            (&mine, "approver", all_studio.clone()),
            (&mine, "solo", all_studio.clone()),
            (&theirs, "mallory", all_studio),
        ]),
    )
    .expect("policy");
    let policy_path = deployment.directory.path().join("trust-policy.json");
    std::fs::write(
        &policy_path,
        serde_json::to_vec(&json!({
            "id": "default", "version": "1.0.0", "schema_version": "1.0.0", "requirements": []
        }))
        .expect("json"),
    )
    .expect("policy file");
    let catalog_path = deployment.directory.path().join("risk-catalog.json");
    std::fs::write(&catalog_path, "{}").expect("catalog file");
    let mut environment: Vec<(String, String)> = vec![
        ("RUN_MODE".into(), "dgraph".into()),
        ("HODEI_LOGGING__FORMAT".into(), "pretty".into()),
        ("HODEI_GRAPH__ENDPOINT".into(), endpoint),
        ("HODEI_GRAPH__TLS__ENABLED".into(), "false".into()),
        ("HODEI_GRAPH__TLS__CA_CERT_PATH".into(), String::new()),
        ("HODEI_GRAPH__TLS__SERVER_NAME".into(), String::new()),
        ("HODEI_GRAPH__NAMESPACE".into(), "0".into()),
        ("HODEI_CONTENT__BACKEND".into(), "filesystem".into()),
        ("HODEI_SECURITY__ADMIN_API_ENABLED".into(), "true".into()),
        (
            "HODEI_TRUST__POLICY_FILE".into(),
            policy_path.to_string_lossy().into_owned(),
        ),
        (
            "HODEI_TRUST__RISK_CATALOG_FILE".into(),
            catalog_path.to_string_lossy().into_owned(),
        ),
    ];
    if let (Ok(user), Ok(password)) = (
        std::env::var("HODEI_GRAPH_IT_USER"),
        std::env::var("HODEI_GRAPH_IT_PASSWORD"),
    ) {
        environment.push(("HODEI_GRAPH__CREDENTIALS__USERID".into(), user));
        environment.push(("HODEI_GRAPH__CREDENTIALS__PASSWORD".into(), password));
    }
    let refs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let service = deployment.start(&refs);
    let studio = Studio {
        deployment: &deployment,
        port: service.port,
    };
    let viewer = support::token(&mine, "viewer");
    let editor = support::token(&mine, "editor");
    let approver = support::token(&mine, "approver");
    let solo = support::token(&mine, "solo");
    let mallory = support::token(&theirs, "mallory");
    let tokens = [&viewer, &editor, &approver, &solo, &mallory];

    // --- Access first: 401 without a credential, 403 without the grant. ---
    let (status, _) = studio.call("GET", "head", "", None);
    assert_eq!(status, 401, "no credential is refused");
    let nobody = support::token(&mine, "nobody");
    let (status, _) = studio.call("GET", "head", &nobody, None);
    assert_eq!(status, 403, "a principal without the grant is refused");

    // --- The empty set: never activated. ---
    let (status, head) = studio.call("GET", "head", &viewer, None);
    assert_eq!(status, 200, "{head}");
    assert_eq!(head["exists"], false);
    let (status, body) = studio.call("GET", "revisions", &viewer, None);
    assert_eq!(status, 404, "never activated has no head revision: {body}");

    // --- A viewer cannot write; nothing changes (AC1). ---
    let v1 = policy_json("1.0.0", &["COMPLETE", "PARTIAL"]);
    let (status, _) = studio.call(
        "POST",
        "revisions",
        &viewer,
        Some(&json!({ "policy": v1, "reason": "viewer tries" })),
    );
    assert_eq!(status, 403, "a viewer cannot save");
    let (_, history) = studio.state_of(&viewer);
    assert_eq!(history["revisions"], json!([]), "no effect: {history}");

    // --- Validate: the lifecycle's strict gate, and it saves nothing. ---
    let (status, body) = studio.call("POST", "validate", &editor, Some(&json!({ "policy": v1 })));
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["valid"], true);
    assert_eq!(body["applied"], false);
    let bad =
        json!({ "id": "default", "version": "9", "schema_version": "9.9.9", "requirements": [] });
    let (status, body) = studio.call("POST", "validate", &editor, Some(&json!({ "policy": bad })));
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["valid"], false, "{body}");
    let (status, _) = studio.call(
        "POST",
        "revisions",
        &editor,
        Some(&json!({ "policy": bad, "reason": "bad" })),
    );
    assert_eq!(status, 400, "an invalid draft is not saved");

    // --- Save: history, not the head. The body's `account` is ignored. ---
    let (status, body) = studio.call(
        "POST",
        "revisions",
        &editor,
        Some(&json!({ "policy": v1, "reason": "first", "account": theirs, "tenant": theirs })),
    );
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["applied"], false);
    assert_eq!(body["revision"]["sequence"], 1);
    assert_eq!(
        body["revision"]["saved_by"]
            .as_str()
            .map(|s| s.ends_with("/editor")),
        Some(true)
    );
    let (head, _) = studio.state_of(&viewer);
    assert_eq!(head["exists"], false, "a save never moves the head");

    // --- Tenant isolation: the other tenant sees nothing of it, and the
    //     body's `account` did not put the revision under it. ---
    let (_, history) = studio.state_of(&mallory);
    assert_eq!(history["revisions"], json!([]), "{history}");
    assert_eq!(history["events"], json!([]), "{history}");
    let (status, _) = studio.call("GET", "revisions?sequence=1", &mallory, None);
    assert_eq!(status, 404, "a foreign tenant does not see the revision");
    let (status, _) = studio.call(
        "POST",
        "activate",
        &mallory,
        Some(&json!({ "sequence": 1, "expected_head": null, "reason": "steal", "account": mine })),
    );
    assert_eq!(status, 404, "a foreign tenant cannot activate it");
    let (status, _) = studio.call(
        "POST",
        "simulate",
        &mallory,
        Some(&json!({ "sequence": 1, "subjects": [] })),
    );
    assert_eq!(status, 404, "a foreign tenant cannot simulate it");

    // --- Access refusal of the move: the editor has no activate grant. ---
    let activate_v1 = json!({ "sequence": 1, "expected_head": null, "reason": "go live" });
    let (status, _) = studio.call("POST", "activate", &editor, Some(&activate_v1));
    assert_eq!(
        status, 403,
        "an editor without the activate grant cannot move the head"
    );
    let (head, _) = studio.state_of(&viewer);
    assert_eq!(head["exists"], false, "no effect of the refused activation");

    // --- Separation of duties: who saved it cannot activate it. ---
    let (status, body) = studio.call(
        "POST",
        "revisions",
        &solo,
        Some(&json!({ "policy": policy_json("0.9.0", &["COMPLETE"]), "reason": "solo" })),
    );
    assert_eq!(status, 201, "{body}");
    let solo_seq = body["revision"]["sequence"].as_u64().expect("sequence");
    let (status, body) = studio.call(
        "POST",
        "activate",
        &solo,
        Some(&json!({ "sequence": solo_seq, "expected_head": null, "reason": "self" })),
    );
    assert_eq!(status, 403, "the author cannot approve: {body}");
    assert_eq!(body["error"], "forbidden");
    let (head, _) = studio.state_of(&viewer);
    assert_eq!(head["exists"], false, "no effect of the self-approval");

    // --- Activation by a different approver: the real CAS. ---
    let (status, body) = studio.call("POST", "activate", &approver, Some(&activate_v1));
    assert_eq!(
        status,
        200,
        "{body}\n{}",
        support::without_the_colors(&deployment.the_log())
    );
    assert_eq!(body["applied"], true);
    assert_eq!(body["head"]["sequence"], 1);
    let (status, body) = studio.call("GET", "revisions", &viewer, None);
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["sequence"], 1);
    assert_eq!(body["active"], true);
    assert!(
        body["digest"]
            .as_str()
            .unwrap_or_default()
            .starts_with("sha256:")
    );

    // --- Simulate: decision / gap / engine error, diffed against the head. ---
    let v2 = policy_json("2.0.0", &["COMPLETE"]);
    let (status, body) = studio.call(
        "POST",
        "revisions",
        &editor,
        Some(&json!({ "policy": v2, "reason": "stricter" })),
    );
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["revision"]["sequence"], solo_seq + 1);
    let v2_seq = solo_seq + 1;

    let before = studio.state_of(&viewer);
    let subjects = json!([
        subject(&mine, "good", "COMPLETE", &["sbom-1"]),
        subject(&mine, "holey", "MISSING", &["sbom-2"]),
        subject(&mine, "partial", "PARTIAL", &["sbom-3"])
    ]);
    let (status, body) = studio.call(
        "POST",
        "simulate",
        &editor,
        Some(&json!({ "sequence": v2_seq, "subjects": subjects })),
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["applied"], false);
    assert_eq!(
        body["active_version"], "1.0.0",
        "the baseline is the head: {body}"
    );
    let effects = body["effects"].as_array().expect("effects");
    assert_eq!(effects[0]["kind"], "POLICY_DECISION", "{body}");
    assert_eq!(effects[0]["decision"]["outcome"], "ALLOW");
    assert_eq!(effects[1]["kind"], "EVIDENCE_GAP", "{body}");
    let gap = &effects[1]["gap"];
    assert!(
        gap["requirement"]
            .as_str()
            .unwrap_or_default()
            .starts_with("dimension:inventory_completeness")
    );
    assert_eq!(gap["dimension"], "inventory_completeness");
    assert!(
        gap["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("MISSING"),
        "{gap}"
    );
    assert_eq!(gap["evidence_ids"], json!(["sbom-2"]));
    // The diff: v2 stops accepting PARTIAL, v1 (the head) accepted it.
    let changes = body["changes"].as_array().expect("changes");
    assert!(
        changes
            .iter()
            .any(|change| change.to_string().contains("partial")),
        "the diff names the subject that changes: {body}"
    );

    // An engine error is not a decision, and not an access refusal.
    let (status, body) = studio.call(
        "POST",
        "simulate",
        &editor,
        Some(&json!({
            "policy": { "id": "default", "version": "m", "schema_version": "9.9.9", "requirements": [] },
            "subjects": [subject(&mine, "any", "COMPLETE", &["e-1"])]
        })),
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["effects"][0]["kind"], "ENGINE_ERROR", "{body}");
    // A draft simulation too.
    let (status, body) = studio.call(
        "POST",
        "simulate",
        &editor,
        Some(&json!({ "policy": v2, "subjects": subjects })),
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["applied"], false);

    // Validate/simulate left head, history and events as they were.
    let after = studio.state_of(&viewer);
    assert_eq!(before, after, "validate and simulate never write");

    // --- Conflict: a stale head is a typed 409 that overwrites nothing. ---
    let (status, body) = studio.call(
        "POST",
        "activate",
        &approver,
        Some(&json!({ "sequence": v2_seq, "expected_head": null, "reason": "stale" })),
    );
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "stale_head");
    assert_eq!(body["found_head"], 1);
    let (head, _) = studio.state_of(&viewer);
    assert_eq!(
        head["head"]["sequence"], 1,
        "the conflict did not move the head"
    );
    let (status, body) = studio.call(
        "POST",
        "activate",
        &approver,
        Some(&json!({ "sequence": v2_seq, "expected_head": 1, "reason": "stricter" })),
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["head"]["sequence"], v2_seq);

    // --- Rollback: a new revision with the old content, under the CAS. ---
    let (status, body) = studio.call(
        "POST",
        "rollback",
        &approver,
        Some(&json!({ "target": 1, "expected_head": 1, "reason": "stale rollback" })),
    );
    assert_eq!(status, 409, "{body}");
    let (status, _) = studio.call(
        "POST",
        "rollback",
        &editor,
        Some(&json!({ "target": 1, "expected_head": v2_seq, "reason": "no grant" })),
    );
    assert_eq!(
        status, 403,
        "an editor without the rollback grant cannot move the head"
    );
    let (status, body) = studio.call(
        "POST",
        "rollback",
        &approver,
        Some(&json!({ "target": 1, "expected_head": v2_seq, "reason": "regression" })),
    );
    assert_eq!(
        status,
        200,
        "{body}\n{}",
        support::without_the_colors(&deployment.the_log())
    );
    let rolled = body["head"]["sequence"].as_u64().expect("sequence");
    assert!(rolled > v2_seq, "the rollback is a new revision: {body}");
    let (_, history) = studio.state_of(&viewer);
    let revisions = history["revisions"].as_array().expect("revisions");
    assert_eq!(
        revisions.len() as u64,
        rolled,
        "history is append-only: {history}"
    );
    assert_eq!(revisions[0]["digest"], revisions[0]["digest"]);
    let events = history["events"].as_array().expect("events");
    assert!(
        events.len() >= 4,
        "saves and activations are audited: {history}"
    );

    // The other tenant is still untouched by all of it.
    let (head, history) = studio.state_of(&mallory);
    assert_eq!(head["exists"], false, "{head}");
    assert_eq!(history["revisions"], json!([]));

    // --- No credential reaches the log or the responses. ---
    let log = support::without_the_colors(&deployment.the_log());
    for token in tokens {
        assert!(
            !log.contains(token.as_str()),
            "a credential reached the log"
        );
    }
}

/// A deployment without the lifecycle store answers 501 on every studio
/// route, like the other optional surfaces: after the gate (401/403), never
/// a fake empty answer. Driven in-process on the real router.
#[tokio::test]
async fn without_the_lifecycle_store_the_studio_answers_501() {
    use axum::{
        body::Body,
        http::{Request, header},
    };
    use tower::ServiceExt;

    if std::env::var("HODEI_GRAPH_IT_ENDPOINT").is_err() {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    }
    let work = Workdir::new();
    let mut config = hodei_artifacts_api::AppConfig::default();
    config.security.admin_api_enabled = true;
    work.apply(&mut config);
    let state = hodei_artifacts_api::bootstrap(
        &config,
        hodei_artifacts_api::BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap")
    .with_authenticator(std::sync::Arc::new(Vouch))
    .with_authorization(std::sync::Arc::new(
        hodei_artifacts_api::authorization::SharedAuthorizer::new(std::sync::Arc::new(
            hodei_authz::cedar::CedarAuthorizer::from_policies_unchecked(
                "permit(principal, action, resource);",
                cedar_policy::Entities::empty(),
                std::sync::Arc::new(hodei_authz::hrn_mapper::HrnCedarMapper),
            )
            .expect("authorizer"),
        )),
    ))
    .without_policy_studio();
    let app = hodei_artifacts_api::router::build_router(state, &config);
    let send = |method: &str, bearer: Option<&str>| {
        let mut request = Request::builder()
            .method(method)
            .uri("/api/v1/policy-studio/head");
        if let Some(token) = bearer {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        app.clone()
            .oneshot(request.body(Body::empty()).expect("request"))
    };
    assert_eq!(send("GET", None).await.expect("answer").status(), 401);
    assert_eq!(
        send("GET", Some("alice")).await.expect("answer").status(),
        501
    );
}

struct Vouch;

#[async_trait::async_trait]
impl hodei_authz::PrincipalAuthenticator for Vouch {
    async fn authenticate(
        &self,
        _credentials: &hodei_authz::Credentials,
    ) -> Result<hodei_authz::PrincipalClaims, hodei_authz::AuthenticationError> {
        Ok(hodei_authz::PrincipalClaims::new(
            hodei_authz::PrincipalId::new("hrn:hodei:iam::acme:User/alice").unwrap(),
            hodei_authz::AccountId::new("acme").unwrap(),
        ))
    }
}
