//! HSP-202 (fail-closed): the store failing NEVER turns UNKNOWN into
//! ALLOW.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT, and the real failure it proves is
//! gated on HODEI_GRAPH_IT_ALLOW_STORE_FAILURE=1 — the test stops the
//! alpha container the stack shares, answers through the outage, and
//! brings it back before it leaves. Everything the outage touches is
//! restored on Drop, whatever the outcome.
//!
//! What it proves, against the real engine:
//!
//! - while the store is down, every read through the view is an ERROR:
//!   `serve_impact`, `read` and `is_current` say Unavailable — never an
//!   empty answer that a caller could read as zero impact;
//! - the stale route the view still holds is not served either: a
//!   freshness that cannot be checked against the ledger's version is
//!   not a freshness at all;
//! - when the store comes back, the SAME view answers again — the
//!   materialized route was in the engine all along, not in memory.

use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use hodei_evidence::application::projection::ProjectionDelivery;
use hodei_evidence::application::projection::ProjectionFrontier;
use hodei_evidence::domain::graph::{NodeKind, RelationType};
use hodei_evidence::{EvidenceId, GraphChangeSet, GraphNode, GraphRelation};
use hodei_evidence_storage::{DgraphGraphStore, DgraphProjectionFrontier};
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_risk_storage::dgraph_view::DgraphImpactView;
use hodei_risk_storage::impact_view::Freshness;
use kernel::Hrn;

/// The alpha container the stack shares: the gRPC endpoint of every IT.
const ALPHA_CONTAINER: &str = "dgraph-alpha-h186";

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
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let report = ensure_ready(&client)
        .await
        .expect("the versioned schema boots");
    eprintln!(
        "schema boot: decision={:?} binary_version={} stored_version={:?} applied={}",
        report.decision, report.binary_version, report.stored_version, report.applied
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

fn tenant() -> Hrn {
    let account = format!("itfail{}", &nonce()[8..]);
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

/// Brings the alpha down for the body of the test and ALWAYS brings it
/// back, whatever the body does. The engine the stack shares is never
/// left behind.
struct StoreOutage {
    stopped: bool,
}

impl StoreOutage {
    /// Stops the alpha container. The caller opted in with
    /// HODEI_GRAPH_IT_ALLOW_STORE_FAILURE=1.
    fn begin() -> Option<StoreOutage> {
        if std::env::var("HODEI_GRAPH_IT_ALLOW_STORE_FAILURE")
            .ok()?
            .as_str()
            != "1"
        {
            eprintln!(
                "SKIP (failure drill): HODEI_GRAPH_IT_ALLOW_STORE_FAILURE=1 is not set, the shared engine is not stopped"
            );
            return None;
        }
        let outcome = Command::new("podman")
            .args(["stop", "-t", "5", ALPHA_CONTAINER])
            .output()
            .expect("podman runs");
        assert!(
            outcome.status.success(),
            "the alpha stopped: {}",
            String::from_utf8_lossy(&outcome.stderr)
        );
        eprintln!("HSP202 fail-closed: the alpha is DOWN, the store fails for real now");
        Some(StoreOutage { stopped: true })
    }
}

impl Drop for StoreOutage {
    fn drop(&mut self) {
        if !self.stopped {
            return;
        }
        let outcome = Command::new("podman")
            .args(["start", ALPHA_CONTAINER])
            .output()
            .expect("podman runs");
        if !outcome.status.success() {
            eprintln!(
                "CRITICAL: the alpha did NOT start again: {}",
                String::from_utf8_lossy(&outcome.stderr)
            );
            return;
        }
        eprintln!(
            "HSP202 fail-closed: the alpha container is started again, waiting for readiness"
        );
        self.stopped = false;
    }
}

/// One artifact holding the vulnerable component, one delivery, one
/// view: the smallest world the failure can be asked about.
fn the_contains(tenant: &Hrn, index: usize) -> GraphChangeSet {
    GraphChangeSet::empty()
        .with_cause("evt-load")
        .with_node(GraphNode::new(
            hrn_of(tenant, "Component", "lib-shared"),
            tenant.clone(),
            NodeKind::Component,
            Utc::now(),
        ))
        .with_node(GraphNode::new(
            hrn_of(tenant, "Vulnerability", "GHSA-fail-0001"),
            tenant.clone(),
            NodeKind::Vulnerability,
            Utc::now(),
        ))
        .with_node(GraphNode::new(
            hrn_of(tenant, "Artifact", &format!("app-{index}")),
            tenant.clone(),
            NodeKind::Artifact,
            Utc::now(),
        ))
        .with_node(GraphNode::new(
            hrn_of(tenant, "Evidence", "ev-shared"),
            tenant.clone(),
            NodeKind::Evidence,
            Utc::now(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            hrn_of(tenant, "Artifact", &format!("app-{index}")),
            hrn_of(tenant, "Component", "lib-shared"),
            tenant.clone(),
            EvidenceId::new(format!("ev-{index}")),
            Utc::now(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            hrn_of(tenant, "Component", "lib-shared"),
            hrn_of(tenant, "Vulnerability", "GHSA-fail-0001"),
            tenant.clone(),
            EvidenceId::new("ev-shared"),
            Utc::now(),
        ))
}

/// AC2 — the outage: through it, every door of the view is an error,
/// never an answer that looks like zero impact; and past it, the same
/// view answers again, straight from the engine.
#[tokio::test]
async fn the_store_failing_is_an_error_never_an_allow() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = tenant();
    let namespace = format!("ns-{}", tenant.account_id());
    let stream = format!("tenant-{}", tenant.account_id());
    let graph = Arc::new(DgraphGraphStore::new(client.clone()));
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens"),
    );
    let view = DgraphImpactView::new(client.clone(), graph.clone(), frontier.clone());
    let advisory = hrn_of(&tenant, "Vulnerability", "GHSA-fail-0001");

    frontier
        .apply_and_checkpoint(
            the_contains(&tenant, 0),
            &ProjectionDelivery {
                consumer: "graph-projector".to_owned(),
                stream_id: stream.clone(),
                position: 0,
                event_id: "evt-load".to_owned(),
            },
        )
        .await
        .expect("the load goes through the frontier");
    let snapshot = view
        .rebuild_advisory(&tenant, &advisory)
        .await
        .expect("the view materializes");
    assert_eq!(snapshot.subjects.len(), 2, "one artifact and the component");
    let answer = view
        .serve_impact(&tenant, &advisory)
        .await
        .expect("the answer runs");
    assert_eq!(
        answer.freshness,
        Freshness::Current,
        "the view is at the head"
    );

    // THE OUTAGE: the store fails for real, and nothing that answers
    // through it may look like a success. The outage is its own scope:
    // the guard brings the alpha back the moment the scope ends,
    // BEFORE the recovery is polled.
    {
        let Some(_outage) = StoreOutage::begin() else {
            return;
        };
        let started = Instant::now();
        let error = view
            .serve_impact(&tenant, &advisory)
            .await
            .expect_err("through the outage, serving is an error");
        eprintln!(
            "HSP202 fail-closed: serve_impact failed in {}ms with: {error}",
            started.elapsed().as_millis()
        );
        assert!(
            matches!(
                error,
                hodei_risk_storage::impact_view::ImpactReadError::Unavailable(_)
            ),
            "the outage is said as unavailability: {error:?}"
        );
        let error = view
            .read(&tenant, &advisory)
            .await
            .expect_err("through the outage, reading is an error");
        assert!(!error.is_empty(), "the failure says why: {error}");
        let error = view
            .is_current(&tenant)
            .await
            .expect_err("through the outage, freshness is an error");
        assert!(!error.is_empty(), "the failure says why: {error}");
    }

    // The engine comes back, and the SAME view answers again: the
    // route was in the engine all along, not in the process.
    let started = Instant::now();
    let mut recovered = None;
    for attempt in 0..60 {
        match GraphClient::connect(config_from_env().expect("the configuration")).await {
            Ok(mut fresh) => {
                if fresh.login().await.is_ok() && ensure_ready(&fresh).await.is_ok() {
                    recovered = Some(fresh);
                    break;
                }
                eprintln!(
                    "HSP202 fail-closed: attempt {attempt}: the login or the schema is not ready yet"
                );
            }
            Err(error) => eprintln!("HSP202 fail-closed: attempt {attempt}: {error}"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let fresh = recovered.expect("the engine recovers");
    eprintln!(
        "HSP202 fail-closed: the engine recovered in {}ms",
        started.elapsed().as_millis()
    );

    // A view built on a fresh client answers straight away: the view
    // state was durable, not a cache of the process.
    let fresh_client = Arc::new(fresh);
    let revived_view = DgraphImpactView::new(
        fresh_client.clone(),
        Arc::new(DgraphGraphStore::new(fresh_client.clone())),
        Arc::new(
            DgraphProjectionFrontier::open(fresh_client, &namespace)
                .await
                .expect("the frontier opens"),
        ),
    );
    let read = revived_view
        .read(&tenant, &advisory)
        .await
        .expect("the read runs after the recovery")
        .expect("the view survived the outage");
    assert_eq!(
        read.subjects, snapshot.subjects,
        "the durable view is the same view"
    );
    let answer = revived_view
        .serve_impact(&tenant, &advisory)
        .await
        .expect("the answer runs after the recovery");
    assert_eq!(answer.freshness, Freshness::Current);
}
