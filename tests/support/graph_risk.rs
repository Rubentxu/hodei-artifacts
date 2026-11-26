//! The risk facts of a deployment on the graph profile, seeded through the
//! real stores (no double of Dgraph, no hand-written rows).
//!
//! The ledger version of a tenant is the COUNT of relation deltas the
//! projection frontier has said, and the watermark of a materialized route is
//! the version the view had when the route was rebuilt. A test that wants
//! "the ledger says 9 and the view stopped at 7" therefore writes real
//! changesets through the frontier until the count is 7, rebuilds the routes,
//! and writes two more.

#![allow(dead_code)]

use std::sync::Arc;

use chrono::Utc;
use hodei_evidence::EvidenceId;
use hodei_evidence::application::projection::{ProjectionDelivery, ProjectionFrontier as _};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType,
};
use hodei_evidence_storage::{DgraphGraphStore, DgraphProjectionFrontier};
use hodei_risk_storage::dgraph_view::DgraphImpactView;
use kernel::Hrn;

use super::graph_profile::GraphTenant;

/// What a test says about the risk of its tenant.
pub struct RiskSeed {
    /// The version the ledger of deltas ends at.
    pub ledger_seq: u64,
    /// The materialized routes: advisory and the version the view had when it
    /// was rebuilt (its watermark).
    pub routes: Vec<(String, u64)>,
    /// The rows of the view: advisory and the subject it reaches.
    pub rows: Vec<(String, String)>,
}

/// The stores a seeding (and a later reading) needs over one tenant.
pub struct RiskWorld {
    pub frontier: Arc<DgraphProjectionFrontier>,
    pub view: DgraphImpactView,
    position: u64,
    /// Makes every name of this world unique: a second seeding of the same
    /// tenant must say NEW deltas, not replay the first ones.
    nonce: String,
}

fn hrn_in(tenant: &Hrn, kind: &str, name: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{}:{kind}/{name}",
        tenant.account_id()
    ))
    .expect("an HRN")
}

impl RiskWorld {
    /// The stores of the tenant, opened the way the service opens them.
    pub async fn open(graph: &GraphTenant) -> Self {
        let client = graph.client().await;
        let frontier = Arc::new(
            DgraphProjectionFrontier::open(client.clone(), &graph.namespace.to_string())
                .await
                .expect("the frontier opens"),
        );
        let store = Arc::new(DgraphGraphStore::new(client.clone()));
        let view = DgraphImpactView::new(client, store, frontier.clone());
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        Self {
            frontier,
            view,
            position: 0,
            nonce: format!("{nonce:x}"),
        }
    }

    /// The version the ledger of the tenant has reached.
    pub async fn said(&self, tenant: &Hrn) -> u64 {
        self.view
            .said_version(tenant)
            .await
            .expect("the ledger version reads")
    }

    async fn say(&mut self, changes: GraphChangeSet) {
        let position = self.position;
        self.position += 1;
        self.frontier
            .apply_and_checkpoint(
                changes,
                &ProjectionDelivery {
                    consumer: "risk-seed".to_owned(),
                    stream_id: format!("risk-seed-{}", self.nonce),
                    position,
                    event_id: format!("evt-risk-seed-{}-{position}", self.nonce),
                },
            )
            .await
            .expect("the seed goes through the frontier");
    }

    fn node(tenant: &Hrn, hrn: Hrn, kind: NodeKind) -> GraphNode {
        GraphNode::new(hrn, tenant.clone(), kind, Utc::now())
    }

    /// The subject holds a component the advisory is about: two relations,
    /// two deltas, and the walk of the advisory reaches the subject.
    pub async fn reach(&mut self, tenant: &Hrn, advisory: &Hrn, subject: &Hrn, tag: &str) {
        let tag = format!("{tag}-{}", self.nonce);
        let component = hrn_in(tenant, "Component", &format!("lib-{tag}"));
        let first = EvidenceId::new(format!("ev-{tag}-1"));
        let second = EvidenceId::new(format!("ev-{tag}-2"));
        let changes = GraphChangeSet::empty()
            .with_cause(format!("seed-{tag}"))
            .with_node(Self::node(tenant, component.clone(), NodeKind::Component))
            .with_node(Self::node(
                tenant,
                advisory.clone(),
                NodeKind::Vulnerability,
            ))
            .with_node(Self::node(tenant, subject.clone(), NodeKind::Artifact))
            .with_node(Self::node(
                tenant,
                hrn_in(tenant, "Evidence", first.as_str()),
                NodeKind::Evidence,
            ))
            .with_node(Self::node(
                tenant,
                hrn_in(tenant, "Evidence", second.as_str()),
                NodeKind::Evidence,
            ))
            .with_relation(GraphRelation::new(
                RelationType::Contains,
                subject.clone(),
                component.clone(),
                tenant.clone(),
                first,
                Utc::now(),
            ))
            .with_relation(GraphRelation::new(
                RelationType::AffectedBy,
                component,
                advisory.clone(),
                tenant.clone(),
                second,
                Utc::now(),
            ));
        self.say(changes).await;
    }

    /// One unrelated relation: exactly one more delta in the ledger.
    async fn one_more_delta(&mut self, tenant: &Hrn, index: u64) {
        let from = hrn_in(
            tenant,
            "Artifact",
            &format!("filler-{}-{index}-a", self.nonce),
        );
        let to = hrn_in(
            tenant,
            "Component",
            &format!("filler-{}-{index}-b", self.nonce),
        );
        let evidence = EvidenceId::new(format!("ev-filler-{}-{index}", self.nonce));
        let changes = GraphChangeSet::empty()
            .with_cause(format!("filler-{}-{index}", self.nonce))
            .with_node(Self::node(tenant, from.clone(), NodeKind::Artifact))
            .with_node(Self::node(tenant, to.clone(), NodeKind::Component))
            .with_node(Self::node(
                tenant,
                hrn_in(tenant, "Evidence", evidence.as_str()),
                NodeKind::Evidence,
            ))
            .with_relation(GraphRelation::new(
                RelationType::Contains,
                from,
                to,
                tenant.clone(),
                evidence,
                Utc::now(),
            ));
        self.say(changes).await;
    }

    /// Say unrelated deltas until the ledger reaches `version`.
    pub async fn pad_to(&mut self, tenant: &Hrn, version: u64) {
        let mut index = 0;
        loop {
            let said = self.said(tenant).await;
            assert!(
                said <= version,
                "the seed already said {said} deltas, past the asked version {version}"
            );
            if said == version {
                return;
            }
            self.one_more_delta(tenant, index).await;
            index += 1;
        }
    }

    /// Materialize the route of an advisory at the current ledger version.
    pub async fn materialize(&self, tenant: &Hrn, advisory: &Hrn) {
        self.view
            .rebuild_advisory(tenant, advisory)
            .await
            .expect("the route materializes");
    }
}

/// Write the plan: the rows reach first (their relations are real deltas),
/// then every route is rebuilt at its own watermark in ascending order, and
/// the ledger ends at `ledger_seq`.
pub async fn seed_risk(graph: &GraphTenant, tenant: &Hrn, plan: &RiskSeed) {
    let mut world = RiskWorld::open(graph).await;
    for (index, (advisory, subject)) in plan.rows.iter().enumerate() {
        let advisory = Hrn::from_string(advisory).expect("the advisory is an HRN");
        let subject = Hrn::from_string(subject).expect("the subject is an HRN");
        world
            .reach(tenant, &advisory, &subject, &format!("row{index}"))
            .await;
    }
    let mut routes: Vec<&(String, u64)> = plan.routes.iter().collect();
    routes.sort_by_key(|(_, watermark)| *watermark);
    for (advisory, watermark) in routes {
        world.pad_to(tenant, *watermark).await;
        let advisory = Hrn::from_string(advisory).expect("the advisory is an HRN");
        world.materialize(tenant, &advisory).await;
    }
    if plan.ledger_seq > 0 {
        world.pad_to(tenant, plan.ledger_seq).await;
    }
}
