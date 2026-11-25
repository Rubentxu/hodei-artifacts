//! HSP-314 F3a: the risk facts of a subject read from the Dgraph impact
//! view, against the real engine.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack. Proves:
//!
//! - a SEEDED fact (a critical advisory reaching the artifact through the
//!   walk, materialized by `rebuild_advisory`) is read back as an
//!   established critical, with the ledger version it was complete up to;
//! - an ABSENT fact on a current view is a verified zero, and a view that
//!   was never materialized is UNKNOWN, never zero;
//! - a subject of ANOTHER account sees no facts and leaks none;
//! - a view left BEHIND the ledger is UNKNOWN (fail-closed), and an
//!   advisory the catalog does not know keeps the answer incomplete.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use hodei_evidence::application::projection::{ProjectionDelivery, ProjectionFrontier};
use hodei_evidence::domain::graph::{NodeKind, RelationType};
use hodei_evidence::{EvidenceId, GraphChangeSet, GraphNode, GraphRelation};
use hodei_evidence_storage::{DgraphGraphStore, DgraphProjectionFrontier};
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_risk_storage::dgraph_view::DgraphImpactView;
use hodei_risk_storage::subject_risk::{
    AdvisoryCatalog, AdvisorySeverity, DgraphRiskFacts, EmptyCatalog, MemoryCatalog,
    RiskFactsReader,
};
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

async fn connect() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let started = Instant::now();
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let report = ensure_ready(&client)
        .await
        .expect("the versioned schema boots");
    eprintln!(
        "schema boot: decision={:?} binary_version={} stored_version={:?} applied={} elapsed_ms={}",
        report.decision,
        report.binary_version,
        report.stored_version,
        report.applied,
        started.elapsed().as_millis(),
    );
    Some(Arc::new(client))
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn tenant(tag: &str) -> Hrn {
    let account = format!("it{tag}{}", &nonce()[8..]);
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/hsp202"
    ))
    .expect("a valid tenant HRN")
}

fn hrn_of(tenant: &Hrn, kind: &str, name: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{}:{kind}/{name}",
        tenant.account_id()
    ))
    .expect("a valid HRN")
}

fn node(tenant: &Hrn, kind: NodeKind, name: &str) -> GraphNode {
    GraphNode::new(
        hrn_of(tenant, kind_name(&kind), name),
        tenant.clone(),
        kind,
        Utc::now(),
    )
}

fn kind_name(kind: &NodeKind) -> &str {
    match kind {
        NodeKind::Repository => "Repository",
        NodeKind::SourceRevision => "SourceRevision",
        NodeKind::Build => "Build",
        NodeKind::Artifact => "Artifact",
        NodeKind::Deployment => "Deployment",
        NodeKind::Evidence => "Evidence",
        NodeKind::Component => "Component",
        NodeKind::Vulnerability => "Vulnerability",
    }
}

/// One artifact holding the vulnerable component, with its cause.
fn the_contains(tenant: &Hrn, cause: &str, index: usize) -> GraphChangeSet {
    let evidence = EvidenceId::new(format!("ev-{index}"));
    let shared = EvidenceId::new("ev-shared");
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_node(node(tenant, NodeKind::Component, "lib-shared"))
        .with_node(node(tenant, NodeKind::Vulnerability, "GHSA-risk-0001"))
        .with_node(node(tenant, NodeKind::Artifact, &format!("app-{index}")))
        .with_node(node(tenant, NodeKind::Evidence, &format!("ev-{index}")))
        .with_node(node(tenant, NodeKind::Evidence, "ev-shared"))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            hrn_of(tenant, "Artifact", &format!("app-{index}")),
            hrn_of(tenant, "Component", "lib-shared"),
            tenant.clone(),
            evidence,
            Utc::now(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            hrn_of(tenant, "Component", "lib-shared"),
            the_advisory(tenant),
            tenant.clone(),
            shared,
            Utc::now(),
        ))
}

fn the_advisory(tenant: &Hrn) -> Hrn {
    hrn_of(tenant, "Vulnerability", "GHSA-risk-0001")
}

fn the_subject(tenant: &Hrn, index: usize) -> Hrn {
    hrn_of(tenant, "Artifact", &format!("app-{index}"))
}

fn delivery(stream: &str, position: u64) -> ProjectionDelivery {
    ProjectionDelivery {
        consumer: "graph-projector".to_owned(),
        stream_id: stream.to_owned(),
        position,
        event_id: format!("evt-{position}"),
    }
}

fn catalog_of(tenant: &Hrn, severity: AdvisorySeverity) -> Arc<dyn AdvisoryCatalog> {
    Arc::new(MemoryCatalog::new([(
        the_advisory(tenant).to_string(),
        severity,
    )]))
}

struct Fixture {
    client: Arc<GraphClient>,
    frontier: Arc<DgraphProjectionFrontier>,
    view: Arc<DgraphImpactView>,
    stream: String,
}

async fn fixture(tenant: &Hrn) -> Option<Fixture> {
    let client = connect().await?;
    let namespace = format!("ns-{}", tenant.account_id());
    let graph = Arc::new(DgraphGraphStore::new(client.clone()));
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens"),
    );
    let view = Arc::new(DgraphImpactView::new(
        client.clone(),
        graph,
        frontier.clone(),
    ));
    Some(Fixture {
        client,
        frontier,
        view,
        stream: format!("tenant-{}", tenant.account_id()),
    })
}

/// Seeds artifacts `0..count` holding the vulnerable component and
/// materializes the view of the advisory.
async fn seed(fixture: &Fixture, tenant: &Hrn, count: usize) {
    for index in 0..count {
        fixture
            .frontier
            .apply_and_checkpoint(
                the_contains(tenant, "evt-seed", index),
                &delivery(&fixture.stream, index as u64),
            )
            .await
            .expect("the seed goes through the frontier");
    }
    fixture
        .view
        .rebuild_advisory(tenant, &the_advisory(tenant))
        .await
        .expect("the view materializes");
}

#[tokio::test]
async fn a_seeded_critical_is_read_as_established_with_the_ledger_version() {
    let tenant = tenant("seed");
    let Some(fx) = fixture(&tenant).await else {
        return;
    };
    seed(&fx, &tenant, 2).await;
    let said = fx.view.said_version(&tenant).await.expect("the version");
    assert!(said > 0, "the ledger said something");

    let reader = DgraphRiskFacts::new(
        fx.view.clone(),
        catalog_of(&tenant, AdvisorySeverity::Critical),
    );
    let facts = reader
        .risk_facts(&tenant, &the_subject(&tenant, 0))
        .await
        .expect("the facts");
    assert!(
        facts.established,
        "a current view with known severity: {facts:?}"
    );
    assert_eq!(facts.critical_reachable, 1);
    assert_eq!(facts.critical_known, 1);
    assert_eq!(facts.advisories_reaching, 1);
    assert_eq!(facts.version, said, "the facts carry the ledger version");

    // The rows are the exact subject's, with its depth.
    let rows = fx
        .view
        .rows_reaching(&tenant, &the_subject(&tenant, 1))
        .await
        .expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].advisory, the_advisory(&tenant).to_string());
    assert_eq!(rows[0].subject, the_subject(&tenant, 1).to_string());
}

#[tokio::test]
async fn an_absent_fact_on_a_current_view_is_a_verified_zero() {
    let tenant = tenant("zero");
    let Some(fx) = fixture(&tenant).await else {
        return;
    };
    seed(&fx, &tenant, 1).await;
    let reader = DgraphRiskFacts::new(
        fx.view.clone(),
        catalog_of(&tenant, AdvisorySeverity::Critical),
    );
    // An artifact the advisory does not reach.
    let clean = hrn_of(&tenant, "Artifact", "clean-app");
    let facts = reader.risk_facts(&tenant, &clean).await.expect("facts");
    assert!(facts.established, "a verified zero: {facts:?}");
    assert_eq!(facts.critical_reachable, 0);
    assert_eq!(facts.advisories_reaching, 0);
}

#[tokio::test]
async fn a_view_never_materialized_is_unknown_not_zero() {
    let tenant = tenant("never");
    let Some(fx) = fixture(&tenant).await else {
        return;
    };
    let reader = DgraphRiskFacts::new(
        fx.view.clone(),
        catalog_of(&tenant, AdvisorySeverity::Critical),
    );
    let facts = reader
        .risk_facts(&tenant, &the_subject(&tenant, 0))
        .await
        .expect("facts");
    assert!(!facts.established, "nothing was measured: {facts:?}");
}

#[tokio::test]
async fn a_subject_of_another_account_sees_no_facts() {
    let owner = tenant("owner");
    let Some(fx) = fixture(&owner).await else {
        return;
    };
    seed(&fx, &owner, 1).await;
    let stranger = tenant("stranger");
    let reader = DgraphRiskFacts::new(
        fx.view.clone(),
        catalog_of(&owner, AdvisorySeverity::Critical),
    );

    // The stranger asks for the owner's subject: same unknown as any stranger.
    let facts = reader
        .risk_facts(&stranger, &the_subject(&owner, 0))
        .await
        .expect("facts");
    assert_eq!(
        facts,
        hodei_risk_storage::subject_risk::SubjectFacts::unknown()
    );
    let rows = fx
        .view
        .rows_reaching(&stranger, &the_subject(&owner, 0))
        .await
        .expect("rows");
    assert!(rows.is_empty(), "no row leaks across accounts");

    // The stranger asking for the same name inside ITS account sees nothing.
    let rows = fx
        .view
        .rows_reaching(&stranger, &the_subject(&stranger, 0))
        .await
        .expect("rows");
    assert!(rows.is_empty());
}

#[tokio::test]
async fn a_view_behind_the_ledger_is_unknown() {
    let tenant = tenant("stale");
    let Some(fx) = fixture(&tenant).await else {
        return;
    };
    seed(&fx, &tenant, 1).await;
    let reader = DgraphRiskFacts::new(
        fx.view.clone(),
        catalog_of(&tenant, AdvisorySeverity::Critical),
    );
    assert!(
        reader
            .risk_facts(&tenant, &the_subject(&tenant, 0))
            .await
            .expect("facts")
            .established
    );

    // The ledger moves on without the view: a new delivery, no rebuild.
    fx.frontier
        .apply_and_checkpoint(
            the_contains(&tenant, "evt-late", 9),
            &delivery(&fx.stream, 9),
        )
        .await
        .expect("the late delivery");
    let facts = reader
        .risk_facts(&tenant, &the_subject(&tenant, 0))
        .await
        .expect("facts");
    assert!(
        !facts.established,
        "a view behind the ledger is not a measurement: {facts:?}"
    );
}

#[tokio::test]
async fn an_advisory_the_catalog_does_not_know_keeps_the_answer_incomplete() {
    let tenant = tenant("nocat");
    let Some(fx) = fixture(&tenant).await else {
        return;
    };
    seed(&fx, &tenant, 1).await;
    let reader = DgraphRiskFacts::new(fx.view.clone(), Arc::new(EmptyCatalog));
    let facts = reader
        .risk_facts(&tenant, &the_subject(&tenant, 0))
        .await
        .expect("facts");
    assert!(!facts.established, "{facts:?}");
    assert_eq!(facts.advisories_reaching, 1);
    let _ = &fx.client;
}
