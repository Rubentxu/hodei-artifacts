//! HSP-78 AC3 (slice of HSP-323): the status view of the reactive effects.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT (`host:port`, no scheme, no TLS). The
//! service is the REAL binary booted on `RUN_MODE=dgraph` against the real
//! Dgraph; the effects are written through the real `DgraphIntentStore` (the
//! domain outbox port), as the reactor does, and read back through
//! `GET /api/v1/reactions/intents`. What it measures:
//!
//! - no credential is 401 and a principal without the Cedar permit is 403;
//! - a permitted caller sees the effects of ITS tenant with identity,
//!   tenant, cause, attempts, state and failure reason;
//! - a caller of another tenant sees none of them (and nothing about them);
//! - the secrets seeded in the payload of the effect and in the failure
//!   reason never appear in the response.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use hodei_artifacts_api::trust::hrn::artifact_hrn;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_reactive::intent::{EventOrigin, Intent, IntentOrigin, IntentOutbox, Receipt};
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use hodei_trust_policy_storage::DgraphIntentStore;
use serde_json::{Value, json};
use support::{Curl, Deployment};

const PAYLOAD_SECRET: &str = "PAYLOAD-SECRET-9f3a7c";
const REASON_SECRET: &str = "REASON-SECRET-51be2d";

fn nonce() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn graph_config() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let mut text =
        format!("endpoint = \"{endpoint}\"\n\n[tls]\nenabled = false\nca_cert_path = \"\"\n");
    if let (Ok(user), Ok(password)) = (
        std::env::var("HODEI_GRAPH_IT_USER"),
        std::env::var("HODEI_GRAPH_IT_PASSWORD"),
    ) {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

/// An independent connection to the alpha, to measure from outside the service.
async fn fresh_client() -> Arc<GraphClient> {
    let mut client = GraphClient::connect(graph_config().expect("gated"))
        .await
        .expect("Dgraph connects");
    if std::env::var("HODEI_GRAPH_IT_USER").is_ok() {
        client.login().await.expect("Dgraph ACL login succeeds");
    }
    ensure_ready(&client).await.expect("the schema boots");
    Arc::new(client)
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

fn the_policy(permitted: &[(&str, &str)]) -> String {
    let mut policy = String::new();
    for (account, user) in permitted {
        policy.push_str(&format!(
            "permit(principal == Iam::User::\"hrn:hodei:iam::{account}:User/{user}\", action == Admin::Action::\"AccessAdminApi\", resource);\n"
        ));
    }
    policy
}

fn call(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> (u16, String) {
    let directory = deployment.directory.path();
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
    let answer = Curl::call(port, method, path, token, file, content_type, Some(&into)).run();
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

/// An effect of `account`, as the reactive cycle says it.
fn effect(account: &str, tag: &str, key: &str) -> Intent {
    let digest = ContentDigest::of_bytes(format!("{tag}-{key}").as_bytes());
    let subject = artifact_hrn("eu", account, "releases", &digest);
    Intent::new(
        IntentOrigin {
            behavior: "planned_vulnerability_reaction".to_string(),
            event: EventOrigin::new(
                format!("evt-{tag}-{key}"),
                vec![format!("evt-root-{tag}"), format!("evt-{tag}-{key}")],
            ),
        },
        "hodei",
        "decision_reevaluation",
        format!("decision:{tag}:{key}"),
        "v1",
        json!({
            "subject": subject.to_string(),
            "reason": "vulnerability_impacted",
            "api_token": PAYLOAD_SECRET,
        }),
    )
}

#[tokio::test]
async fn the_status_view_is_scoped_authorized_and_redacted() {
    if graph_config().is_none() {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    }
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").expect("gated");
    let tag = nonce();
    let mine = format!("r323a{}", &tag[8..]);
    let theirs = format!("r323b{}", &tag[8..]);

    // The effects, written through the real outbox over the same alpha.
    let client = fresh_client().await;
    let store = DgraphIntentStore::open(client.clone())
        .await
        .expect("the intent store opens");
    let failed = effect(&mine, &tag, "failed");
    let pending = effect(&mine, &tag, "pending");
    let foreign = effect(&theirs, &tag, "foreign");
    store
        .put_pending(&failed)
        .await
        .expect("the effect is said");
    let now = chrono::Utc::now();
    store
        .claim(
            &failed.identity,
            "worker-1",
            now + chrono::Duration::minutes(5),
            now,
        )
        .await
        .expect("the claim answers")
        .expect("the claim is won");
    store
        .record_receipt(
            &failed.identity,
            &Receipt::Failed {
                reason: format!("the decision store refused: password={REASON_SECRET} host=db"),
                retryable: false,
            },
        )
        .await
        .expect("the receipt is written");

    let deployment = Deployment::new();
    std::fs::write(
        deployment.directory.path().join("schema.json"),
        the_schema(),
    )
    .expect("schema");
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        the_policy(&[(&mine, "alice"), (&theirs, "bob")]),
    )
    .expect("policy");
    let policy = TrustPolicy::new(
        "default",
        "1.0.0",
        vec![Requirement::denying(
            RequirementKind::NoReachableCriticalVulnerability,
        )],
    );
    let policy_path: PathBuf = deployment.directory.path().join("trust-policy.json");
    std::fs::write(&policy_path, serde_json::to_vec(&policy).expect("json")).expect("policy file");
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
    let port = service.port;
    // The service resumes the pending intents it finds at boot, so the
    // effects that must stay pending are said once it is up.
    for intent in [&pending, &foreign] {
        store.put_pending(intent).await.expect("the effect is said");
    }
    let path = "/api/v1/reactions/intents";
    let get = |token: &str, query: &str| {
        call(
            &deployment,
            port,
            "GET",
            &format!("{path}{query}"),
            token,
            None,
        )
    };
    let log = || support::without_the_colors(&deployment.the_log());

    // Authentication and authorization come first.
    let (status, _) = get("", "");
    assert_eq!(status, 401, "no credential is refused");
    let nobody = support::token(&mine, "carol");
    let (status, _) = get(&nobody, "");
    assert_eq!(status, 403, "a principal without the permit is refused");

    // The caller's tenant: its two effects, with the fields of AC3.
    let alice = support::token(&mine, "alice");
    let (status, body) = get(&alice, "");
    assert_eq!(status, 200, "{body}\n{}", log());
    let view: Value = serde_json::from_str(&body).expect("a JSON body");
    let items = view["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "the two effects of the tenant: {body}");
    assert_eq!(view["truncated"], false);
    let failed_view = items
        .iter()
        .find(|item| item["effect_id"] == failed.identity)
        .expect("the failed effect is listed");
    assert_eq!(failed_view["tenant"], mine);
    assert_eq!(failed_view["kind"], "decision_reevaluation");
    assert_eq!(failed_view["state"], "failed");
    assert_eq!(failed_view["attempts"], 1);
    assert_eq!(failed_view["cause_event"], format!("evt-{tag}-failed"));
    assert_eq!(
        failed_view["causation"],
        json!([format!("evt-root-{tag}"), format!("evt-{tag}-failed")])
    );
    assert_eq!(failed_view["failure"]["retryable"], false);
    let reason = failed_view["failure"]["reason"].as_str().expect("a reason");
    assert!(reason.contains("the decision store refused"), "{reason}");
    let pending_view = items
        .iter()
        .find(|item| item["effect_id"] == pending.identity)
        .expect("the pending effect is listed");
    assert_eq!(pending_view["state"], "pending");
    assert_eq!(pending_view["attempts"], 0);
    assert!(pending_view["failure"].is_null());

    // No secret leaves: neither the payload's nor the reason's.
    for secret in [PAYLOAD_SECRET, REASON_SECRET] {
        assert!(!body.contains(secret), "a secret leaked: {body}");
    }
    assert!(
        !body.contains("api_token"),
        "the payload is not copied: {body}"
    );

    // The state filter narrows.
    let (_, body) = get(&alice, "?state=failed");
    let narrowed: Value = serde_json::from_str(&body).expect("a JSON body");
    assert_eq!(narrowed["items"].as_array().expect("items").len(), 1);

    // Another tenant sees its own effect and nothing of the first tenant.
    let bob = support::token(&theirs, "bob");
    let (status, body) = get(&bob, "");
    assert_eq!(status, 200, "{body}");
    let other: Value = serde_json::from_str(&body).expect("a JSON body");
    let other_items = other["items"].as_array().expect("items");
    assert_eq!(other_items.len(), 1, "{body}");
    assert_eq!(other_items[0]["effect_id"], foreign.identity);
    for mine_id in [&failed.identity, &pending.identity] {
        assert!(
            !body.contains(mine_id.as_str()),
            "a foreign effect is visible: {body}"
        );
    }
    assert!(!body.contains(&mine), "the other tenant is named: {body}");
    // ...and the first tenant does not see the second's.
    let (_, body) = get(&alice, "");
    assert!(
        !body.contains(&foreign.identity),
        "a foreign effect is visible: {body}"
    );
    let _ = Arc::strong_count(&client);
}
