//! HSP-208: the simulation over the snapshot and its overlays in the
//! graph authority.
//!
//! IT conditioned on `HODEI_GRAPH_IT_ENDPOINT`, as every test of the
//! stack. It runs against the REAL shared alpha (membership asserts over
//! the test's own tenant, never a global census — HSP-198/219/220) with
//! per-run nonce tenants. What it proves:
//!
//! - AC#1 (the diff of two forks over one anchor is stable, and a
//!   simulation writes NOTHING real): two forks with the same facts in a
//!   different order seal the same canonical diff, and the hypothetical
//!   relation is ABSENT from the graph afterwards — asserted by its own
//!   relation key and by the anchored read of the account, which still
//!   sees exactly the fixture;
//! - AC#2 (a concurrent change and a store error are indeterminate or
//!   incomplete per the contract): a real delta committed through the
//!   frontier moves the watermark and the next simulation says stale —
//!   `StaleBase{source: "graph"}`, never a diff over a world nobody has;
//!   the anchored reads themselves refuse a world above the sealed
//!   snapshot; a delta story that does not answer closes the simulation
//!   with `Unavailable`; and a decider that does not answer says
//!   `ReadFailed`;
//! - the ORACLE of HSP-92: the same five conformance scenarios that the
//!   Surreal reference passes, passed over the Dgraph authority.
//!
//! Run serially against the shared alpha: `cargo test -p
//! hodei-simulation-storage --test hsp208_simulation_dgraph_it --
//! --test-threads=1`.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::application::projection::ProjectionDelivery;
use hodei_evidence::domain::delta::{
    RelationDelta, RelationDeltaLedger, RelationDeltaSubscription,
};
use hodei_evidence::domain::evidence::EvidenceId;
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType,
};
use hodei_evidence::domain::read_set::ReadSet;
use hodei_evidence_storage::{DgraphDeltaLedger, DgraphGraphStore, DgraphProjectionFrontier};
use hodei_graph_client::{GraphClient, GraphClientConfig, dql_string, schema::ensure_ready};
use hodei_simulation::diff::TheDecision;
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, SimulationTag};
use hodei_simulation::memory::InMemoryDecisions;
use hodei_simulation::ports::DecisionEvaluator;
use hodei_simulation::ports::SimulationGraph;
use hodei_simulation::service::SimulationError;
use hodei_simulation::{
    Checkpoint, ConformanceWorld, Fixture, SimulationFork, SimulationService, run_suite,
};
use hodei_simulation_storage::{DgraphConformance, DgraphSimulationGraph, SimulationAnchor};
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

/// The bounded retry of the connect preamble: on a shared alpha a
/// transient refusal is the norm at the start of the window.
const CONNECT_ATTEMPTS: usize = 5;

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
    for attempt in 1..=CONNECT_ATTEMPTS {
        let outcome: Result<Arc<GraphClient>, String> = async {
            let mut client = GraphClient::connect(config.clone())
                .await
                .map_err(|error| error.to_string())?;
            client.login().await.map_err(|error| error.to_string())?;
            ensure_ready(&client)
                .await
                .map_err(|error| error.to_string())?;
            Ok(Arc::new(client))
        }
        .await;
        match outcome {
            Ok(client) => return Some(client),
            Err(error) if attempt < CONNECT_ATTEMPTS => {
                eprintln!("connect attempt {attempt} failed: {error}; retrying");
                tokio::time::sleep(Duration::from_millis(300 * attempt as u64)).await;
            }
            Err(error) => panic!("the authority never answered: {error}"),
        }
    }
    None
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// One tenant per test and per run: on a shared alpha every slice
/// leaves its mark, and the asserts are of membership over the test's
/// own account — never global totals.
fn fresh_tenant(tag: &str) -> Hrn {
    let account = format!("hsp208{tag}{}", &nonce()[7..]);
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/hsp208"
    ))
    .expect("a valid tenant HRN")
}

/// The clock of a simulation: a fixed instant, never the wall clock.
fn the_clock() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-01T12:00:00Z")
        .expect("an instant")
        .with_timezone(&Utc)
}

/// The policy of the forks: the same shape the conformance suite uses —
/// a real policy that denies an established risk.
fn the_policy() -> TrustPolicy {
    TrustPolicy::new(
        "policy-hsp208",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// The delta story of a tenant that does not answer: the silent
/// authority of the contract — silence closes, it never invents.
struct SilentLedger;

#[async_trait]
impl RelationDeltaLedger for SilentLedger {
    async fn append(&self, _deltas: &[RelationDelta]) -> Result<Option<u64>, String> {
        Err("this ledger only fails".to_string())
    }

    async fn pending(
        &self,
        _subscription: &RelationDeltaSubscription,
        _after_version: u64,
        _quota: usize,
    ) -> Result<(Vec<(u64, RelationDelta)>, u64), String> {
        Err("the story did not answer".to_string())
    }

    async fn version_of(&self, _tenant: &Hrn) -> Result<u64, String> {
        Err("the story did not answer".to_string())
    }
}

/// The decider that does not answer: a decision that cannot be read is
/// an incomplete simulation, never an ALLOW by omission.
struct FailingEvaluator;

#[async_trait]
impl DecisionEvaluator for FailingEvaluator {
    async fn decide(
        &self,
        _subject: &Hrn,
        _policy: &TrustPolicy,
        _facts: &[Hypothetical],
        _now: DateTime<Utc>,
    ) -> Result<TheDecision, String> {
        Err("the decider did not answer".to_string())
    }

    async fn the_reads_of(&self, _subject: &Hrn) -> Result<ReadSet, String> {
        Err("the decider did not answer".to_string())
    }
}

/// The world over the authority: the graph store, the frontier (the
/// only door of the deltas, HSP-199), the ledger over the same story,
/// and the anchored pieces on top.
struct World {
    client: Arc<GraphClient>,
    graph: Arc<DgraphGraphStore>,
    frontier: Arc<DgraphProjectionFrontier>,
    deltas: Arc<DgraphDeltaLedger>,
    tenant: Hrn,
}

impl World {
    async fn open(client: Arc<GraphClient>, tenant: Hrn) -> Self {
        let namespace = format!("ns-{}", tenant.account_id());
        let graph = Arc::new(DgraphGraphStore::new(client.clone()));
        let frontier = Arc::new(
            DgraphProjectionFrontier::open(client.clone(), &namespace)
                .await
                .expect("the frontier opens"),
        );
        let deltas = Arc::new(DgraphDeltaLedger::new(frontier.clone()));
        Self {
            client,
            graph,
            frontier,
            deltas,
            tenant,
        }
    }

    /// The anchored conformance world of this tenant, opened under the
    /// simulation's name and the fixed clock.
    async fn conformance(&self, simulation: &str) -> DgraphConformance {
        DgraphConformance::open(
            self.client.clone(),
            self.graph.clone(),
            self.deltas.clone(),
            self.tenant.clone(),
            simulation,
            the_clock(),
            Arc::new(InMemoryDecisions),
        )
        .await
        .expect("the conformance world anchors")
    }

    /// Commits a REAL delta through the frontier: the concurrent change
    /// the simulation must notice. A fresh cause at a fresh position is
    /// a new delivery — never a replay.
    async fn commit_a_real_move(&self, position: u64, from: &Hrn, to: &Hrn) {
        let evidence = EvidenceId::new(format!("hsp208-ev-{position}-{}", nonce()));
        let changes = GraphChangeSet::empty()
            .with_cause(format!("hsp208-move-{position}-{}", nonce()))
            .with_node(GraphNode::new(
                evidence_hrn(&self.tenant, &evidence),
                self.tenant.clone(),
                NodeKind::Evidence,
                the_clock(),
            ))
            .with_relation(GraphRelation::new(
                RelationType::DependsOn,
                from.clone(),
                to.clone(),
                self.tenant.clone(),
                evidence,
                the_clock(),
            ));
        // The ends of the relation exist already (the fixture's nodes),
        // and the evidence node rides in the same changeset, so the
        // writer port accepts it.
        let delivery = ProjectionDelivery {
            consumer: "hsp208-it".to_string(),
            stream_id: format!("stream-{}", self.tenant.account_id()),
            position,
            event_id: format!("evt-{position}-{}", nonce()),
        };
        let commit = self
            .frontier
            .apply_and_checkpoint_with(&changes, &delivery, true)
            .await
            .expect("the frontier commits");
        assert!(!commit.replayed, "a fresh delivery is never a replay");
    }

    /// The key of the relation a hypothetical says would exist, read
    /// straight from the authority: the assert of AC#1.
    async fn relation_row_exists(&self, identity: &str) -> bool {
        let key = DgraphGraphStore::relation_key(&self.tenant.to_string(), identity);
        let response = self
            .client
            .execute_dql(&format!(
                "{{ q(func: eq(hodei.graph.rel.key, {})) {{ uid }} }}",
                dql_string(&key)
            ))
            .await
            .expect("the authority answers");
        response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|rows| !rows.is_empty())
    }
}

/// The HRN the fixture's evidence claims carry, as the projection says
/// it (the same mapping the conformance world's prepare uses).
fn evidence_hrn(tenant: &Hrn, id: &EvidenceId) -> Hrn {
    hodei_evidence::application::projector::evidence_hrn(tenant, id)
}

/// The fork over the live generation of the world, with the given
/// facts in the given order.
fn the_fork(
    tenant: &Hrn,
    id: &str,
    generation: u64,
    subjects: Vec<Hrn>,
    facts: Vec<Hypothetical>,
) -> SimulationFork {
    let fork = SimulationFork::new(
        id,
        Checkpoint::new(
            format!("cp-{id}"),
            tenant.account_id(),
            the_clock(),
            Some(0),
            generation,
        ),
        the_policy(),
        the_clock(),
    )
    .with_subjects(subjects);
    facts.into_iter().fold(fork, |fork, fact| fork.with(fact))
}

/// AC#1 + the oracle: two forks over one anchor give the same canonical
/// diff, and the hypothetical is nowhere in the graph afterwards. The
/// five conformance scenarios pass over the authority.
#[tokio::test]
async fn two_forks_over_one_anchor_simulate_and_write_nothing_real() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = fresh_tenant("a1");
    let world = World::open(client, tenant.clone()).await;
    let anchor = world.conformance("hsp208-ac1").await;

    // The REAL base: the fixture, left by the conformance world through
    // the writer port — no cause, so preparing a world moves no
    // watermark.
    let fixture = Fixture::of(tenant.account_id(), tenant.clone());
    anchor
        .prepare(&fixture)
        .await
        .expect("the fixture is prepared");
    let watermark = world.deltas.version_of(&tenant).await.expect("live");
    assert_eq!(watermark, 0, "a cause-less prepare emits no deltas");

    let the_graph = anchor.the_graph();
    let service = SimulationService::new(the_graph.clone(), Arc::new(InMemoryDecisions));

    // The same hypothetical, in the same two forks with the facts in a
    // different order and a different simulation id: stable means the
    // canonical diff does not care.
    let added = HypotheticalClaim::Relation {
        relation: RelationType::DependsOn,
        from: fixture.lib_v2.clone(),
        to: fixture.artefact.clone(),
        replaces: None,
    };
    let removed = HypotheticalClaim::Relation {
        relation: RelationType::DependsOn,
        from: fixture.artefact.clone(),
        to: fixture.lib_v1.clone(),
        replaces: None,
    };
    let one = the_fork(
        &tenant,
        "fork-one",
        watermark,
        vec![fixture.artefact.clone(), fixture.lib_v1.clone()],
        vec![
            Hypothetical {
                tag: SimulationTag::new("sim-one", the_clock()),
                claim: added.clone(),
            },
            Hypothetical {
                tag: SimulationTag::new("sim-one", the_clock()),
                claim: removed.clone(),
            },
        ],
    );
    let other = the_fork(
        &tenant,
        "fork-other",
        watermark,
        vec![fixture.lib_v1.clone(), fixture.artefact.clone()],
        vec![
            Hypothetical {
                tag: SimulationTag::new("sim-two", the_clock()),
                claim: removed,
            },
            Hypothetical {
                tag: SimulationTag::new("sim-two", the_clock()),
                claim: added,
            },
        ],
    );

    let first = service
        .simulate(&one, the_clock())
        .await
        .expect("a fork over the live generation simulates");
    let second = service
        .simulate(&other, the_clock())
        .await
        .expect("the second fork simulates too");
    assert_eq!(
        first.canonical(),
        second.canonical(),
        "two forks over one anchor give one canonical diff"
    );

    // The overlays are not facts: the hypothetical's identity is not a
    // relation of the account, by its own key in the authority...
    let identity = format!(
        "{}|{}|{}",
        RelationType::DependsOn.as_str(),
        fixture.lib_v2,
        fixture.artefact
    );
    assert!(
        !world.relation_row_exists(&identity).await,
        "the hypothetical relation was persisted as real: {identity}"
    );
    // ...and by the anchored read of the account, which still sees
    // exactly the fixture. Membership of the own tenant, never a
    // global census.
    let after = the_graph.all_relations().await.expect("still readable");
    assert_eq!(after.len(), 2, "the graph still holds only the fixture");

    // The oracle of HSP-92 over the same authority: the five scenarios.
    let report = run_suite(&anchor).await;
    assert_eq!(report.scenarios.len(), 5);
    assert!(
        report.passed(),
        "the conformance suite over Dgraph: {:?}",
        report.failures()
    );
}

/// AC#2: a concurrent change and a store error are indeterminate or
/// incomplete per the contract — never a diff over a moved world, never
/// an answer built on silence.
#[tokio::test]
async fn a_moved_world_and_a_silent_store_close_the_simulation_per_contract() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = fresh_tenant("a2");
    let world = World::open(client.clone(), tenant.clone()).await;
    let anchor = world.conformance("hsp208-ac2").await;
    let fixture = Fixture::of(tenant.account_id(), tenant.clone());
    anchor.prepare(&fixture).await.expect("prepared");
    let the_graph = anchor.the_graph();
    let service = SimulationService::new(the_graph.clone(), Arc::new(InMemoryDecisions));
    let fork = the_fork(
        &tenant,
        "fork-stale",
        0,
        vec![fixture.artefact.clone()],
        vec![Hypothetical {
            tag: SimulationTag::new("sim-stale", the_clock()),
            claim: HypotheticalClaim::Risk {
                subject: fixture.artefact.clone(),
                advisory: "CVE-2026-0001".to_string(),
                critical: true,
                reachable: true,
            },
        }],
    );

    // While the fork sleeps, the world really moves: a delta committed
    // through the frontier, fresh cause and fresh position. The delta
    // takes sequence position + 1, so delivery 0 lands the watermark
    // on 1.

    world
        .commit_a_real_move(0, &fixture.lib_v1, &fixture.lib_v3)
        .await;
    assert_eq!(
        the_graph.generation().await.expect("live"),
        1,
        "the anchored port answers the live watermark"
    );

    // The service notices before anything is computed: stale, per the
    // contract, with the numbers the world moved between.
    let error = service
        .simulate(&fork, the_clock())
        .await
        .expect_err("a fork over a moved world does not give a diff");
    match error {
        SimulationError::StaleBase {
            source,
            expected,
            found,
        } => {
            assert_eq!(source, "graph");
            assert_eq!(expected, "0");
            assert_eq!(found, "1");
        }
        other => panic!("the stale world said: {other}"),
    }

    // And the anchored reads themselves are fail-closed: a world above
    // the sealed snapshot is refused, never read as if it were the
    // snapshot.
    let refusal = the_graph
        .all_relations()
        .await
        .expect_err("the snapshot refuses a moved world");
    assert!(
        refusal.contains("sealed snapshot"),
        "the refusal names the snapshot: {refusal}"
    );

    // A delta story that does not answer: the simulation is
    // indeterminate — Unavailable, and never an answer over silence.
    let deaf_graph = SimulationService::new(
        Arc::new(DgraphSimulationGraph::new(
            world.graph.clone(),
            Arc::new(SilentLedger),
            tenant.clone(),
            SimulationAnchor::new(0, the_clock()),
        )),
        Arc::new(InMemoryDecisions),
    );
    let error = deaf_graph
        .simulate(&fork, the_clock())
        .await
        .expect_err("a silent story closes the simulation");
    assert!(matches!(error, SimulationError::Unavailable(_)), "{error}");

    // A decider that does not answer: incomplete per the contract —
    // ReadFailed, the decision half of the same rule. The base of this
    // one is the world AFTER the move: over a stale base the service
    // refuses before it ever asks a decider.
    let mute_service = SimulationService::new(
        Arc::new(DgraphSimulationGraph::new(
            world.graph.clone(),
            world.deltas.clone(),
            tenant.clone(),
            SimulationAnchor::new(1, the_clock()),
        )),
        Arc::new(FailingEvaluator),
    );
    let error = mute_service
        .simulate(
            &the_fork(
                &tenant,
                "fork-muted",
                1,
                vec![fixture.artefact.clone()],
                vec![],
            ),
            the_clock(),
        )
        .await
        .expect_err("a silent decider does not decide");
    assert!(matches!(error, SimulationError::ReadFailed(_)), "{error}");
}

/// The anchor row is the ONLY state the simulation writes: the
/// create-if-absent replays, and a second snapshot under the same key
/// is refused — never silently overwritten.
#[tokio::test]
async fn the_anchor_freezes_once_and_replays_itself() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = fresh_tenant("an");
    let world = World::open(client, tenant.clone()).await;

    // Opening twice at the same watermark and clock replays the anchor.
    let first = world.conformance("hsp208-anchor").await;
    let second = world.conformance("hsp208-anchor").await;
    assert_eq!(
        first.the_anchor(),
        second.the_anchor(),
        "the anchor is what the graph froze, not what the caller asked"
    );

    // A third world at the same key with ANOTHER clock must refuse: the
    // snapshot and the instant are said once.
    let moved = hodei_simulation_storage::DgraphSimulationAnchor::new(world.client.clone())
        .record(
            "hsp208-anchor",
            tenant.account_id(),
            &SimulationAnchor::new(0, the_clock() + chrono::Duration::days(1)),
        )
        .await;
    match moved {
        Err(hodei_simulation_storage::AnchorError::AlreadyAnchored { .. }) => {}
        other => panic!("a moved clock was accepted at the same key: {other:?}"),
    }
}
