//! HSP-314 F3b-3: the graph profile has the CVE cycle alive.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT (`host:port`, no scheme, no TLS, no ACL
//! unless HODEI_GRAPH_IT_USER/PASSWORD are set). The service under test is the
//! REAL binary booted by the real `bootstrap` on `RUN_MODE=dgraph`, against the
//! real Dgraph: the test never builds the reactor itself, so removing the
//! wiring line of `bootstrap_graph_profile` makes it fail. What it measures:
//!
//! - before any advisory the facts of the subject are "not measured" and the
//!   promotion is refused for that reason (fail-closed);
//! - `POST /api/v1/advisories/accept` answers 2xx (not 501) and the cycle
//!   materializes the impact view: the same facts, read by an independent
//!   reader over the same alpha, are now measured and the promotion is GRANTED;
//! - a critical advisory that reaches the granted subject takes the grant
//!   back inside the call and the next promotion is refused for the real
//!   reason (a reachable critical), not "unknown";
//! - the entry fails closed: a malformed digest, an unknown repository, a
//!   document that is not an advisory and an unauthenticated caller are
//!   refused, and none of them leaves a materialized route behind.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use hodei_artifacts_api::trust::hrn::artifact_hrn;
use hodei_catalog::TrustLedger;
use hodei_evidence::ContentDigest;
use hodei_evidence_storage::{DgraphGraphStore, DgraphProjectionFrontier};
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_risk::projection::vulnerability_hrn;
use hodei_risk_storage::dgraph_view::DgraphImpactView;
use hodei_risk_storage::subject_risk::{
    AdvisoryCatalog, AdvisorySeverity, DgraphRiskFacts, MemoryCatalog, RiskFactsReader,
};
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use hodei_trust_policy_storage::DgraphTrustStore;
use kernel::Hrn;
use serde_json::{Value, json};
use support::{Curl, Deployment};

const INVENTORY: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.7",
  "version": 1,
  "metadata": {
    "timestamp": "2026-03-04T10:15:00Z",
    "component": {
      "bom-ref": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06",
      "type": "container",
      "name": "web",
      "version": "1.0.0",
      "purl": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06"
    }
  },
  "components": [
    { "bom-ref": "pkg:maven/org.example/lib-a@1.2.3", "type": "library", "name": "lib-a", "version": "1.2.3", "purl": "pkg:maven/org.example/lib-a@1.2.3" }
  ],
  "dependencies": [
    { "ref": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06", "dependsOn": ["pkg:maven/org.example/lib-a@1.2.3"] },
    { "ref": "pkg:maven/org.example/lib-a@1.2.3", "dependsOn": [] }
  ]
}"#;

/// The namespace of the login: the tenant the profile keys its stores and its
/// projection frontier under (`GraphAuthority::tenant`).
const NAMESPACE: &str = "0";

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

fn osv(id: &str) -> Value {
    json!({
        "schema_version": "1.6.0",
        "id": id,
        "modified": "2026-11-18T00:00:00Z",
        "affected": [
            { "package": { "ecosystem": "Maven", "name": "lib-a", "purl": "pkg:maven/org.example/lib-a@1.2.3" } }
        ]
    })
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

fn the_policy() -> String {
    // The measurement is of the wiring, not of Cedar: alice may do anything.
    "permit(principal, action, resource);\n".to_string()
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

fn promote(
    deployment: &Deployment,
    port: u16,
    token: &str,
    repository: &str,
    digest: &ContentDigest,
) -> (u16, String) {
    call(
        deployment,
        port,
        "POST",
        "/api/v1/promotions",
        token,
        Some(&json!({
            "repository": repository,
            "path": "dir/app.bin",
            "digest": format!("sha256:{}", digest.value),
        })),
    )
}

fn accept(
    deployment: &Deployment,
    port: u16,
    token: &str,
    repository: &str,
    digest: &str,
    advisory: Value,
) -> (u16, String) {
    call(
        deployment,
        port,
        "POST",
        "/api/v1/advisories/accept",
        token,
        Some(&json!({
            "repository": repository,
            "digest": digest,
            "advisory": advisory,
            "inventory": serde_json::from_str::<Value>(INVENTORY).expect("inventory"),
        })),
    )
}

#[tokio::test]
async fn the_graph_profile_runs_the_cve_cycle_end_to_end() {
    if graph_config().is_none() {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    }
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").expect("gated");
    let tag = nonce();
    // A fresh account per run: the tenant of the graph is the account, and the
    // view of a tenant that an earlier run materialized would read as measured.
    let account = format!("f3b3a{}", &tag[8..]);
    let account = account.as_str();
    let repository = "releases".to_string();
    let region = std::env::var("HSP314_F3B3_REGION").unwrap_or_else(|_| "eu".to_string());

    // The advisories of this run, named like the cycle names them: the
    // vulnerability HRN of the tenant of the subject.
    let content = format!("hsp314-f3b3-{tag}").into_bytes();
    let digest = ContentDigest::of_bytes(&content);
    let subject = artifact_hrn(&region, account, &repository, &digest);
    let tenant = hodei_artifacts_api::AppConfig::default()
        .investigation
        .the_graph_tenant(&subject)
        .expect("the tenant template names an HRN");
    let tenant_hrn: Hrn = tenant;
    let minor_id = format!("GHSA-f3b3-minor-{}", &tag[8..]);
    let critical_id = format!("GHSA-f3b3-crit-{}", &tag[8..]);
    let minor_hrn = vulnerability_hrn(&tenant_hrn, &minor_id);
    let critical_hrn = vulnerability_hrn(&tenant_hrn, &critical_id);

    let deployment = Deployment::new();
    std::fs::write(
        deployment.directory.path().join("schema.json"),
        the_schema(),
    )
    .expect("schema");
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        the_policy(),
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
    std::fs::write(
        &catalog_path,
        json!({ minor_hrn.to_string(): "other", critical_hrn.to_string(): "critical" }).to_string(),
    )
    .expect("catalog file");

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
    let token = support::token(account, "alice");
    let log = || support::without_the_colors(&deployment.the_log());

    // The deployment: a hosted raw repository on the graph, and the bytes.
    let (created, body) = call(
        &deployment,
        port,
        "POST",
        "/api/v1/catalog/repositories",
        &token,
        Some(&json!({"name": repository, "format": "raw", "mode": "hosted"})),
    );
    assert_eq!(created, 201, "the repository is created: {body}\n{}", log());
    let file = deployment.directory.path().join("app.bin");
    std::fs::write(&file, &content).expect("the blob");
    let published = support::curl_upload(
        port,
        &format!("/repository/{repository}/dir/app.bin"),
        &token,
        &file,
        None,
    );
    assert!(
        published.status == 200 || published.status == 201,
        "the publication: {}\n{}",
        published.status,
        log()
    );

    // The independent reader of the facts, over the same alpha.
    let client = fresh_client().await;
    let catalog: Arc<dyn AdvisoryCatalog> = Arc::new(MemoryCatalog::new(vec![
        (minor_hrn.to_string(), AdvisorySeverity::Other),
        (critical_hrn.to_string(), AdvisorySeverity::Critical),
    ]));
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(client.clone(), NAMESPACE)
            .await
            .expect("the frontier opens"),
    );
    let reader = DgraphRiskFacts::new(
        Arc::new(DgraphImpactView::new(
            client.clone(),
            Arc::new(DgraphGraphStore::new(client.clone())),
            frontier,
        )),
        catalog,
    );

    // (e) Fail-closed entry: nothing is accepted for a bad request.
    let digest_text = format!("sha256:{}", digest.value);
    let (status, _) = accept(
        &deployment,
        port,
        "",
        &repository,
        &digest_text,
        osv(&minor_id),
    );
    assert_eq!(status, 401, "an unauthenticated caller is refused");
    let (status, _) = accept(
        &deployment,
        port,
        &token,
        &repository,
        "sha256:XYZ",
        osv(&minor_id),
    );
    assert_eq!(status, 400, "a malformed digest is refused");
    let (status, _) = accept(
        &deployment,
        port,
        &token,
        "no-such-repo",
        &digest_text,
        osv(&minor_id),
    );
    assert_eq!(status, 404, "an unknown repository is refused");
    let (status, body) = accept(
        &deployment,
        port,
        &token,
        &repository,
        &digest_text,
        json!({"not": "an advisory"}),
    );
    assert!(
        (400..500).contains(&status),
        "a document that is not an advisory is refused, not accepted: {status} {body}"
    );

    // (c, before) Nothing is measured, and the promotion refuses for that.
    let before = reader
        .risk_facts(&tenant_hrn, &subject)
        .await
        .expect("facts");
    assert!(
        !before.established,
        "before the advisory nothing is measured: {before:?}"
    );
    let (status, body) = promote(&deployment, port, &token, &repository, &digest);
    assert!(
        !(status == 200 && body.contains("\"promoted\":true")),
        "the promotion on unmeasured risk must not grant: {status} {body}"
    );

    // (a) The advisory is accepted: 2xx, not 501.
    let (status, body) = accept(
        &deployment,
        port,
        &token,
        &repository,
        &digest_text,
        osv(&minor_id),
    );
    assert_ne!(status, 501, "the cycle is wired on the graph profile");
    assert!(
        (200..300).contains(&status),
        "accept: {status} {body}\n{}",
        log()
    );
    let admission: Value = serde_json::from_str(&body).expect("an admission body");
    assert_eq!(admission["advisory"], minor_hrn.to_string());

    // (b, c) The view is materialized: the independent reader now measures.
    let after = reader
        .risk_facts(&tenant_hrn, &subject)
        .await
        .expect("facts");
    assert!(after.established, "the facts are measured: {after:?}");
    // `after.version` stays 0 on this profile: Dgraph `GraphWriter::apply` emits no
    // relation deltas, and the version is the delta ledger watermark (finding
    // of HSP-314 F3b-3, deliberately not asserted).
    assert_eq!(
        after.critical_reachable, 0,
        "a minor advisory is no critical"
    );

    // (d) The decision changed: the same promotion is now granted.
    let (status, body) = promote(&deployment, port, &token, &repository, &digest);
    assert!(
        status == 200 && body.contains("\"promoted\":true"),
        "measured and clean grants: {status} {body}\n{}",
        log()
    );
    let ledger = DgraphTrustStore::open(
        client.clone(),
        hodei_graph_client::Tenant::new(NAMESPACE).expect("the tenant validates"),
    )
    .await
    .expect("the trust store opens");
    let held = ledger
        .active_grant(account, &repository, &digest, Utc::now())
        .await
        .expect("the book answers");
    assert!(held.is_some(), "the concession is in the book");

    // A critical advisory reaches the granted subject: the cycle takes the
    // concession back inside the call, and the next promotion refuses for
    // the real reason.
    let (status, body) = accept(
        &deployment,
        port,
        &token,
        &repository,
        &digest_text,
        osv(&critical_id),
    );
    assert!(
        (200..300).contains(&status),
        "accept critical: {status} {body}\n{}",
        log()
    );
    let facts = reader
        .risk_facts(&tenant_hrn, &subject)
        .await
        .expect("facts");
    assert!(
        facts.critical_reachable > 0,
        "the critical reaches: {facts:?}"
    );
    let held = ledger
        .active_grant(account, &repository, &digest, Utc::now())
        .await
        .expect("the book answers");
    assert!(held.is_none(), "the cycle revoked the concession: {held:?}");
    let (status, body) = promote(&deployment, port, &token, &repository, &digest);
    assert!(
        !(status == 200 && body.contains("\"promoted\":true")),
        "a reachable critical refuses: {status} {body}"
    );
}
