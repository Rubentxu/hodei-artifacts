//! HSP-314 F3c: the graph profile serves the investigation of a subject.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT (`host:port`, no scheme, no TLS; the
//! profile needs ACL credentials, so HODEI_GRAPH_IT_USER/PASSWORD are read).
//! The service under test is the REAL binary booted by the real `bootstrap`
//! on `RUN_MODE=dgraph` against the real Dgraph: the test never builds the
//! investigation itself, so removing the wiring line of
//! `bootstrap_graph_profile` makes it fail. What it measures:
//!
//! - gates in order: no credential is `401`; a principal without the Cedar
//!   permission is `403`; a subject of another account and a subject that is
//!   not an artifact answer the SAME `404`;
//! - a subject with evidence and a permitted principal is `200` with a report
//!   that cites the evidence (with the digest that verifies it), names the
//!   relation of the graph and declares its coverage;
//! - the freshness route says `current` for the report just delivered and
//!   `stale` (with the reference that moved) once the relation it rests on is
//!   withdrawn through the projection frontier;
//! - fail-closed at serving time: with the alpha cut, the same request is
//!   `503`, never a `200` report that reads as "no evidence".

mod support;

use std::sync::Arc;

use chrono::Utc;
use hodei_evidence::application::projection::ProjectionDelivery;
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal, NodeKind, RelationType,
};
use hodei_evidence::{ContentRef, EvidenceStore};
use hodei_evidence_storage::{DgraphEvidenceStore, DgraphProjectionFrontier};
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_hub::blob::{BlobStore, FileBlobStore};
use kernel::Hrn;
use serde_json::{Value, json};
use support::{Curl, Deployment};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

/// The namespace of the login: the tenant the profile keys its frontier under.
const NAMESPACE: &str = "0";

const EVIDENCE_TEXT: &str = "SBOM of the subject: lib-a 1.2.3 is a dependency.";

fn nonce() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn graph_config(endpoint: &str) -> GraphClientConfig {
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
    GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates")
}

/// An independent connection to the alpha, to seed and move the world from
/// outside the service.
async fn fresh_client(endpoint: &str) -> Arc<GraphClient> {
    let mut client = GraphClient::connect(graph_config(endpoint))
        .await
        .expect("Dgraph connects");
    if std::env::var("HODEI_GRAPH_IT_USER").is_ok() {
        client.login().await.expect("Dgraph ACL login succeeds");
    }
    ensure_ready(&client).await.expect("the schema boots");
    Arc::new(client)
}

/// A relay to the alpha that the test can cut: cutting it drops every open
/// connection and stops listening, so the service's next call fails for real.
struct Relay {
    endpoint: String,
    tasks: JoinSet<()>,
}

impl Relay {
    async fn to(target: String) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("relay binds");
        let endpoint = listener.local_addr().expect("relay address").to_string();
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                let Ok((mut inbound, _)) = listener.accept().await else {
                    return;
                };
                let target = target.clone();
                connections.spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
            }
        });
        Relay { endpoint, tasks }
    }

    fn cut(&mut self) {
        self.tasks.abort_all();
    }
}

fn artifact(region: &str, account: &str, name: &str) -> Hrn {
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        region.to_string(),
        account.to_string(),
        "artifact".to_string(),
        name.to_string(),
    )
}

/// The schema of the deployment, with the action of the investigation.
fn the_schema() -> String {
    let mut schema: Value = serde_json::from_str(support::SCHEMA).expect("the schema parses");
    schema["Artifacts"]["actions"]["Investigate"] = json!({
        "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Artifact"] }
    });
    schema.to_string()
}

/// Only alice may investigate: bob is a principal of the same account that
/// holds no permission.
fn the_policy(account: &str) -> String {
    format!(
        "permit(principal == Iam::User::\"hrn:hodei:iam::{account}:User/alice\", action == Artifacts::Action::\"Investigate\", resource);\n"
    )
}

fn call(
    deployment: &Deployment,
    port: u16,
    path: &str,
    token: &str,
    body: &Value,
) -> (u16, Value, String) {
    let directory = deployment.directory.path();
    let into = directory.join("response.bin");
    let _ = std::fs::remove_file(&into);
    let sent = directory.join("request.bin");
    std::fs::write(&sent, body.to_string()).expect("the body is written");
    let answer = Curl::call(
        port,
        "POST",
        path,
        token,
        Some(sent.as_path()),
        Some("application/json"),
        Some(&into),
    )
    .run();
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    let json = serde_json::from_str(&text).unwrap_or(Value::Null);
    (answer.status, json, text)
}

fn investigate(
    deployment: &Deployment,
    port: u16,
    token: &str,
    subject: &Hrn,
) -> (u16, Value, String) {
    call(
        deployment,
        port,
        "/api/v1/investigations",
        token,
        &json!({"subject": subject.to_string(), "question": "why is it affected?"}),
    )
}

fn freshness(
    deployment: &Deployment,
    port: u16,
    token: &str,
    subject: &Hrn,
    read_set: &Value,
) -> (u16, Value, String) {
    call(
        deployment,
        port,
        "/api/v1/investigations/freshness",
        token,
        &json!({"subject": subject.to_string(), "read_set": read_set}),
    )
}

fn environment(endpoint: &str, deployment: &Deployment) -> Vec<(String, String)> {
    std::fs::write(
        deployment.directory.path().join("schema.json"),
        the_schema(),
    )
    .expect("schema");
    let mut environment: Vec<(String, String)> = vec![
        ("RUN_MODE".into(), "dgraph".into()),
        ("HODEI_LOGGING__FORMAT".into(), "pretty".into()),
        ("HODEI_GRAPH__ENDPOINT".into(), endpoint.to_string()),
        ("HODEI_GRAPH__TLS__ENABLED".into(), "false".into()),
        ("HODEI_GRAPH__TLS__CA_CERT_PATH".into(), String::new()),
        ("HODEI_GRAPH__TLS__SERVER_NAME".into(), String::new()),
        ("HODEI_GRAPH__NAMESPACE".into(), NAMESPACE.into()),
        ("HODEI_CONTENT__BACKEND".into(), "filesystem".into()),
        (
            "HODEI_SECURITY__INVESTIGATION_API_ENABLED".into(),
            "true".into(),
        ),
    ];
    if let (Ok(user), Ok(password)) = (
        std::env::var("HODEI_GRAPH_IT_USER"),
        std::env::var("HODEI_GRAPH_IT_PASSWORD"),
    ) {
        environment.push(("HODEI_GRAPH__CREDENTIALS__USERID".into(), user));
        environment.push(("HODEI_GRAPH__CREDENTIALS__PASSWORD".into(), password));
    }
    environment
}

fn delivery(account: &str, position: u64) -> ProjectionDelivery {
    ProjectionDelivery {
        consumer: "hsp314-f3c-it".to_string(),
        stream_id: format!("stream-{account}"),
        position,
        event_id: format!("evt-{position}-{}", nonce()),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_graph_profile_serves_the_investigation_of_a_subject() {
    let Ok(endpoint) = std::env::var("HODEI_GRAPH_IT_ENDPOINT") else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let tag = nonce();
    // A fresh account per run: the tenant of the graph is the account.
    let account = format!("f3ca{}", &tag[8..]);
    let account = account.as_str();
    let foreign_account = format!("f3cb{}", &tag[8..]);
    let region = "eu-west-1";
    let subject = artifact(region, account, "sha256-0123456789abcdef");
    let dependency = artifact(region, account, "sha256-fedcba9876543210");
    let foreign = artifact(region, &foreign_account, "sha256-0123456789abcdef");
    let tenant = hodei_artifacts_api::AppConfig::default()
        .investigation
        .the_graph_tenant(&subject)
        .expect("the tenant template names an HRN");

    let deployment = Deployment::new();
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        the_policy(account),
    )
    .expect("policy");

    // The service reaches the alpha through a relay the test can cut.
    let mut relay = Relay::to(endpoint.clone()).await;
    let environment = environment(&relay.endpoint, &deployment);
    let refs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let service = deployment.start(&refs);
    let port = service.port;
    let alice = support::token(account, "alice");
    let bob = support::token(account, "bob");
    let stranger = support::token(&foreign_account, "alice");
    let log = || support::without_the_colors(&deployment.the_log());

    // The world: the evidence (record in Dgraph, content in the CAS of the
    // deployment) and the relation it supports, projected through the frontier.
    let client = fresh_client(&endpoint).await;
    let blobs = FileBlobStore::new(&deployment.cas).expect("the content store");
    let digest = blobs
        .put(EVIDENCE_TEXT.as_bytes())
        .await
        .expect("the content is stored");
    let evidence_id = format!("ev-f3c-{tag}");
    let record = EvidenceRecord::builder(EvidenceId::new(evidence_id.clone()), tenant.clone())
        .evidence_type(EvidenceType::Sbom)
        .subjects(vec![subject.clone()])
        .source(SourceRef::by_external_id(format!("ext-{evidence_id}")))
        .content(ContentRef::by_digest(digest.clone()))
        .observed_at(Utc::now())
        .recorded_at(Utc::now())
        .verification(EvidenceVerification::verified("dsse"))
        .normalization(Normalization::new("cyclonedx", "1.0.0"))
        .producer(Producer::new("connector", "hsp314-f3c-it").with_version("1.0.0"))
        .build();
    DgraphEvidenceStore::open(client.clone())
        .await
        .expect("the evidence store opens")
        .put(record)
        .await
        .expect("the evidence is stored");
    let frontier = DgraphProjectionFrontier::open(client.clone(), NAMESPACE)
        .await
        .expect("the frontier opens");
    let now = Utc::now();
    let changes = GraphChangeSet::empty()
        .with_node(GraphNode::new(
            subject.clone(),
            tenant.clone(),
            NodeKind::Artifact,
            now,
        ))
        .with_node(GraphNode::new(
            dependency.clone(),
            tenant.clone(),
            NodeKind::Artifact,
            now,
        ))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            subject.clone(),
            dependency.clone(),
            tenant.clone(),
            EvidenceId::new(evidence_id.clone()),
            now,
        ))
        .with_cause(evidence_id.clone());
    frontier
        .apply_and_checkpoint_with(&changes, &delivery(account, 0), true)
        .await
        .expect("the relation is projected");

    // Gate 1: no credential, no look.
    let (status, _, _) = investigate(&deployment, port, "", &subject);
    assert_eq!(status, 401, "an unauthenticated caller is refused");

    // Gate 3: a principal without the permission is forbidden.
    let (status, _, body) = investigate(&deployment, port, &bob, &subject);
    assert_eq!(status, 403, "no Cedar permission, no investigation: {body}");

    // Gate 2: a subject of another account and a subject that is not an
    // artifact are the same 404, with the same body.
    let (foreign_status, _, foreign_body) = investigate(&deployment, port, &alice, &foreign);
    let not_an_artifact = Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        region.to_string(),
        account.to_string(),
        "repository".to_string(),
        "libs".to_string(),
    );
    let (missing_status, _, missing_body) =
        investigate(&deployment, port, &alice, &not_an_artifact);
    assert_eq!(
        foreign_status, 404,
        "a foreign subject is hidden: {foreign_body}"
    );
    assert_eq!(
        missing_status, 404,
        "a non-subject is not found: {missing_body}"
    );
    assert_eq!(
        foreign_body, missing_body,
        "the two absences are indistinguishable"
    );
    // And the stranger asking for the subject of the first account is the
    // same 404: the account is the one of the credential.
    let (status, _, body) = investigate(&deployment, port, &stranger, &subject);
    assert_eq!(status, 404, "the account comes from the principal: {body}");
    assert_eq!(body, foreign_body);

    // The report: citations and coverage.
    let (status, report, body) = investigate(&deployment, port, &alice, &subject);
    assert_eq!(status, 200, "the investigation answers: {body}\n{}", log());
    let stamps = report["read_set"]["stamps"]
        .as_array()
        .expect("the read set has stamps");
    let evidence_stamp = stamps
        .iter()
        .find(|stamp| stamp["resource"] == evidence_id.as_str())
        .unwrap_or_else(|| panic!("the evidence read is in the manifest: {body}"));
    assert_eq!(evidence_stamp["kind"], "evidence");
    assert_eq!(
        evidence_stamp["verified_by"],
        digest.to_string(),
        "the manifest carries the digest, not the content"
    );
    assert!(
        stamps.iter().any(|stamp| stamp["kind"] == "relation"),
        "the relation of the graph is in the manifest: {body}"
    );
    assert!(
        report["coverage"] == "complete" || report["coverage"]["partial"].is_object(),
        "the coverage is declared: {body}"
    );
    assert!(
        body.contains(&evidence_id),
        "the report cites the evidence by id: {body}"
    );

    // Freshness: the report just delivered still rests on the world.
    let read_set = report["read_set"].clone();
    let (status, view, body) = freshness(&deployment, port, &alice, &subject, &read_set);
    assert_eq!(status, 200, "freshness answers: {body}");
    assert_eq!(view["status"], "current", "nothing moved: {body}");

    // The relation is withdrawn through the frontier: the report is stale
    // and says which reference moved.
    let withdrawal = GraphChangeSet::empty()
        .with_withdrawal(GraphRelationWithdrawal::new(
            RelationType::Contains,
            subject.clone(),
            dependency.clone(),
            tenant.clone(),
            EvidenceId::new(evidence_id.clone()),
            Utc::now(),
        ))
        .with_cause(evidence_id.clone());
    frontier
        .apply_and_checkpoint_with(&withdrawal, &delivery(account, 1), true)
        .await
        .expect("the withdrawal is projected");
    let (status, view, body) = freshness(&deployment, port, &alice, &subject, &read_set);
    assert_eq!(status, 200, "freshness answers: {body}");
    assert_eq!(view["status"], "stale", "the support moved: {body}");
    assert!(
        view["changed"].as_array().is_some_and(|c| !c.is_empty()),
        "the stale verdict names what moved: {body}"
    );

    // Fail-closed at serving time: with the alpha cut the service does not
    // answer an empty report, it says it is not available.
    relay.cut();
    let (status, _, body) = investigate(&deployment, port, &alice, &subject);
    assert_eq!(
        status,
        503,
        "a silent alpha closes the investigation: {body}\n{}",
        log()
    );
    let (status, _, body) = freshness(&deployment, port, &alice, &subject, &read_set);
    assert!(
        status == 503 || (status == 200 && body.contains("unknown")),
        "a silent alpha never reads as current: {status} {body}"
    );
}
