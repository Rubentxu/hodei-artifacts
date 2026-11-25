//! HSP-314 F3a: the graph profile decides the promotion with the risk facts
//! of the Dgraph impact view, not with "not measured".
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every Dgraph IT. The service state
//! is built by the real `bootstrap` over a `[graph]` configuration; the
//! facts are seeded into the SAME alpha (changesets through the projection
//! frontier, view materialized with `rebuild_advisory`), and the decision is
//! the pure trust evaluation the promotion handler runs, over the facts the
//! wired reader returns.
//!
//! - Without a materialized fact the decision is the fail-closed one it was
//!   with the old stub: the risk is UNKNOWN and a policy that requires the
//!   absence of criticals does not grant.
//! - A seeded view that proves a clean subject GRANTS, and carries the
//!   ledger version it measured (the stub could never grant).
//! - A seeded critical that reaches the subject REFUSES for a different
//!   reason (reachable critical, not unknown).
//! - An advisory the catalog does not know stays UNKNOWN.
//!
//! Production note (F3b): nothing maintains the view yet, so a deployment
//! stays "not measured" until the reactor is wired.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap};
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::application::projection::{ProjectionDelivery, ProjectionFrontier};
use hodei_evidence::domain::graph::{NodeKind, RelationType};
use hodei_evidence::{EvidenceId, GraphChangeSet, GraphNode, GraphRelation};
use hodei_evidence_storage::{DgraphGraphStore, DgraphProjectionFrontier};
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_risk_storage::dgraph_view::DgraphImpactView;
use hodei_risk_storage::subject_risk::RiskFactsReader;
use hodei_trust_policy::context::TrustContext;
use hodei_trust_policy::decision::evaluate;
use hodei_trust_policy::{ReasonCode, Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

async fn connect() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    ensure_ready(&client).await.expect("the schema boots");
    Some(Arc::new(client))
}

/// A subject of a fresh account, and the tenant the deployment derives for it.
fn subject_and_tenant(tag: &str, index: usize) -> (Hrn, Hrn, String) {
    let account = format!("it{tag}{}", &nonce()[8..]);
    let subject = Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/app-{index}"
    ))
    .expect("an HRN");
    let tenant = AppConfig::default()
        .investigation
        .the_graph_tenant(&subject)
        .expect("the tenant template names an HRN");
    (subject, tenant, account)
}

fn hrn_in(tenant: &Hrn, kind: &str, name: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{}:{kind}/{name}",
        tenant.account_id()
    ))
    .expect("an HRN")
}

fn node(tenant: &Hrn, kind: NodeKind, label: &str, name: &str) -> GraphNode {
    GraphNode::new(
        hrn_in(tenant, label, name),
        tenant.clone(),
        kind,
        Utc::now(),
    )
}

fn advisory_of(tenant: &Hrn) -> Hrn {
    hrn_in(tenant, "Vulnerability", "GHSA-f3a-0001")
}

/// The subject (named like the deployment names it) holds the vulnerable
/// component the advisory is about.
fn contains(tenant: &Hrn, subject: &Hrn) -> GraphChangeSet {
    GraphChangeSet::empty()
        .with_cause("evt-f3a")
        .with_node(node(tenant, NodeKind::Component, "Component", "lib-f3a"))
        .with_node(node(
            tenant,
            NodeKind::Vulnerability,
            "Vulnerability",
            "GHSA-f3a-0001",
        ))
        .with_node(GraphNode::new(
            subject.clone(),
            tenant.clone(),
            NodeKind::Artifact,
            Utc::now(),
        ))
        .with_node(node(tenant, NodeKind::Evidence, "Evidence", "ev-f3a-1"))
        .with_node(node(tenant, NodeKind::Evidence, "Evidence", "ev-f3a-2"))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            subject.clone(),
            hrn_in(tenant, "Component", "lib-f3a"),
            tenant.clone(),
            EvidenceId::new("ev-f3a-1"),
            Utc::now(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            hrn_in(tenant, "Component", "lib-f3a"),
            advisory_of(tenant),
            tenant.clone(),
            EvidenceId::new("ev-f3a-2"),
            Utc::now(),
        ))
}

/// Seeds the graph and materializes the view of the advisory.
async fn seed(client: &Arc<GraphClient>, tenant: &Hrn, holder: &Hrn) {
    let namespace = format!("ns-{}", tenant.account_id());
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens"),
    );
    frontier
        .apply_and_checkpoint(
            contains(tenant, holder),
            &ProjectionDelivery {
                consumer: "graph-projector".to_owned(),
                stream_id: format!("tenant-{}", tenant.account_id()),
                position: 0,
                event_id: "evt-f3a".to_owned(),
            },
        )
        .await
        .expect("the seed goes through the frontier");
    let graph = Arc::new(DgraphGraphStore::new(client.clone()));
    DgraphImpactView::new(client.clone(), graph, frontier)
        .rebuild_advisory(tenant, &advisory_of(tenant))
        .await
        .expect("the view materializes");
}

/// The state the real bootstrap builds over the graph profile.
async fn the_state(catalog: Option<&std::path::Path>) -> hodei_artifacts_api::AppState {
    let graph = config_from_env().expect("the endpoint is set");
    let mut config = AppConfig {
        graph: Some(graph),
        ..AppConfig::default()
    };
    config.trust.risk_catalog_file = catalog.map(|path| path.to_string_lossy().into_owned());
    let content = tempfile::tempdir().expect("temp dir");
    config.content.cas_path = content.path().join("cas").to_string_lossy().into_owned();
    std::mem::forget(content);
    bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("the graph profile boots")
}

fn catalog_file(entries: &[(&Hrn, &str)]) -> tempfile::NamedTempFile {
    let mut map = serde_json::Map::new();
    for (advisory, severity) in entries {
        map.insert(advisory.to_string(), serde_json::json!(severity));
    }
    let file = tempfile::NamedTempFile::new().expect("catalog file");
    std::fs::write(file.path(), serde_json::to_vec(&map).expect("json")).expect("write");
    file
}

fn policy() -> TrustPolicy {
    TrustPolicy::new(
        "risk-aware",
        "1.0.0",
        vec![Requirement::denying(
            RequirementKind::NoReachableCriticalVulnerability,
        )],
    )
}

/// What the promotion handler does with the facts the reader answers.
async fn decide(
    reader: &dyn RiskFactsReader,
    tenant: &Hrn,
    subject: &Hrn,
) -> hodei_trust_policy::decision::TrustDecision {
    let facts = reader.risk_facts(tenant, subject).await.expect("facts");
    let now = Utc::now();
    let capsule = CapsuleAssembler::new().assemble(subject, &[], now);
    let context = TrustContext::builder(&capsule)
        .with_risk(hodei_artifacts_api::risk_facts::to_trust_facts(&facts))
        .assembled_at(now)
        .build();
    evaluate(&context, &policy())
}

#[tokio::test]
async fn without_a_materialized_fact_the_decision_stays_fail_closed() {
    let Some(_client) = connect().await else {
        return;
    };
    let (subject, tenant, _) = subject_and_tenant("none", 0);
    let state = the_state(None).await;
    let decision = decide(state.risk_facts_reader().as_ref(), &tenant, &subject).await;
    assert!(!decision.advances(), "nothing was measured: {decision:?}");
    assert!(decision.has_reason(ReasonCode::RiskUnknown));
}

#[tokio::test]
async fn a_seeded_clean_view_grants_and_a_seeded_critical_refuses() {
    let Some(client) = connect().await else {
        return;
    };
    let (holder, tenant, account) = subject_and_tenant("seed", 0);
    seed(&client, &tenant, &holder).await;
    let catalog = catalog_file(&[(&advisory_of(&tenant), "critical")]);
    let state = the_state(Some(catalog.path())).await;
    let reader = state.risk_facts_reader();

    // The baseline: the stub the profile used before says "not measured".
    let baseline = decide(
        &hodei_artifacts_api::risk_facts::UnmeasuredRiskFacts,
        &tenant,
        &holder,
    )
    .await;
    assert!(!baseline.advances());
    assert!(baseline.has_reason(ReasonCode::RiskUnknown));

    // The subject the critical reaches is REFUSED, for the real reason.
    let refused = decide(reader.as_ref(), &tenant, &holder).await;
    assert!(!refused.advances(), "{refused:?}");
    assert!(refused.has_reason(ReasonCode::RiskReachableCriticalVulnerability));
    assert!(refused.risk_version > 0, "the ledger version travels");

    // A subject of the same account the advisory does not reach is GRANTED:
    // the stub could never say this.
    let clean = Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/clean"
    ))
    .expect("an HRN");
    let granted = decide(reader.as_ref(), &tenant, &clean).await;
    assert!(granted.advances(), "a verified zero grants: {granted:?}");
    assert_eq!(granted.risk_version, refused.risk_version);
}

#[tokio::test]
async fn an_advisory_the_catalog_does_not_know_stays_unknown() {
    let Some(client) = connect().await else {
        return;
    };
    let (holder, tenant, _) = subject_and_tenant("nocat", 0);
    seed(&client, &tenant, &holder).await;
    let state = the_state(None).await;
    let decision = decide(state.risk_facts_reader().as_ref(), &tenant, &holder).await;
    assert!(!decision.advances());
    assert!(decision.has_reason(ReasonCode::RiskUnknown));
}
