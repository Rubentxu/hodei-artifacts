//! HSP-318: the M7 simulation UAT (HSP-67) rewritten on Dgraph.
//!
//! The retired `m7_uat` ran the simulation over the embedded engine. HSP-208
//! already proves the structural side over Dgraph (isolation of the fork,
//! stable diff, stale base, the HSP-92 conformance suite) but with an
//! in-memory decider. The gap this IT closes is the REAL decision: the
//! `CapsuleDecisionEvaluator` deciding with the capsule assembled from the
//! evidence persisted in Dgraph, over the anchored Dgraph graph.
//!
//! - M7-UAT-01 a proposed stricter policy plus a hypothetical critical
//!   vulnerability flips the decision ALLOW -> DENY, with both policy
//!   versions, while the production evidence and graph stay untouched
//!   (the isolation of M7-UAT-04 for the decision path).
//! - M7-UAT-03 a compromised-builder window leaves unverified the evidence
//!   observed inside it, and the decision of the subject changes.
//! - M7-UAT-07 a failed evidence read is a typed `ReadFailed`, never an
//!   empty capsule and never an ALLOW.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack. Run
//! serially: `cargo test -p hodei-simulation-storage --test
//! hsp318_m7_simulation_dgraph_it -- --test-threads=1`.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use hodei_evidence::EvidenceId;
use hodei_evidence::domain::capsule::{DimensionName, DimensionState};
use hodei_evidence::domain::delta::RelationDeltaLedger;
use hodei_evidence::domain::evidence::{
    EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer, SourceRef,
};
use hodei_evidence::ports::{EvidenceStore, StoreError};
use hodei_evidence::{ContentDigest, ContentRef};
use hodei_evidence_storage::{
    DgraphDeltaLedger, DgraphEvidenceStore, DgraphGraphStore, DgraphProjectionFrontier,
};
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, SimulationTag};
use hodei_simulation::memory::InMemoryDecisions;
use hodei_simulation::ports::{DecisionEvaluator, SimulationGraph};
use hodei_simulation::service::SimulationError;
use hodei_simulation::{Checkpoint, ConformanceWorld, Fixture, SimulationFork, SimulationService};
use hodei_simulation_storage::{CapsuleDecisionEvaluator, DgraphConformance};
use hodei_trust_policy::{Outcome, Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

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

/// One tenant per test and per run, so the shared alpha never mixes two.
fn fresh_tenant(tag: &str) -> Hrn {
    let account = format!("hsp318{tag}{}", &nonce()[7..]);
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/hsp318"
    ))
    .expect("a valid tenant HRN")
}

/// The clock of the simulation: a fixed instant, never the wall clock.
fn the_clock() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-01T12:00:00Z")
        .expect("an instant")
        .with_timezone(&Utc)
}

/// The policy of today: it allows while no critical vulnerability is
/// reachable.
fn the_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "1.0.0",
        vec![Requirement::denying(
            RequirementKind::NoReachableCriticalVulnerability,
        )],
    )
}

/// The hypothetical policy: also demands that the risk is established.
fn the_strict_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "2.0.0",
        vec![
            Requirement::denying(RequirementKind::NoReachableCriticalVulnerability),
            Requirement::denying(RequirementKind::RiskEstablished),
        ],
    )
}

/// The policy that demands a verified build provenance.
fn the_provenance_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::BuildProvenance,
            accepts: vec![DimensionState::Verified],
        })],
    )
}

/// The world: the anchored graph, the persisted evidence and the service
/// that decides with the capsule assembled from that evidence.
struct World {
    tenant: Hrn,
    fixture: Fixture,
    evidence: Arc<DgraphEvidenceStore>,
    graph: Arc<dyn SimulationGraph>,
    generation: u64,
}

impl World {
    /// Opens the world with `reader` as the evidence the decider reads, and
    /// leaves the fixture in the graph.
    async fn open(
        client: Arc<GraphClient>,
        tag: &str,
        failing_reader: bool,
    ) -> (Self, SimulationService) {
        let tenant = fresh_tenant(tag);
        let namespace = format!("ns-{}", tenant.account_id());
        let graph_store = Arc::new(DgraphGraphStore::new(client.clone()));
        let frontier = Arc::new(
            DgraphProjectionFrontier::open(client.clone(), &namespace)
                .await
                .expect("the frontier opens"),
        );
        let deltas = Arc::new(DgraphDeltaLedger::new(frontier));
        let evidence = Arc::new(
            DgraphEvidenceStore::open(client.clone())
                .await
                .expect("the evidence store opens"),
        );
        let reader: Arc<dyn EvidenceStore> = if failing_reader {
            Arc::new(DownEvidence {
                inner: evidence.clone(),
            })
        } else {
            evidence.clone()
        };
        let decider: Arc<dyn DecisionEvaluator> =
            Arc::new(CapsuleDecisionEvaluator::new(reader, tenant.account_id()));

        let anchor = DgraphConformance::open(
            client,
            graph_store,
            deltas.clone(),
            tenant.clone(),
            "hsp318-m7",
            the_clock(),
            Arc::new(InMemoryDecisions),
        )
        .await
        .expect("the anchored world opens");
        let fixture = Fixture::of(tenant.account_id(), tenant.clone());
        anchor
            .prepare(&fixture)
            .await
            .expect("the fixture is prepared");
        let generation = deltas
            .version_of(&tenant)
            .await
            .expect("the watermark reads");

        let graph: Arc<dyn SimulationGraph> = anchor.the_graph();
        let service = SimulationService::new(graph.clone(), decider);
        (
            Self {
                tenant,
                fixture,
                evidence,
                graph,
                generation,
            },
            service,
        )
    }

    fn subject(&self) -> Hrn {
        self.fixture.artefact.clone()
    }

    /// A verified piece of evidence of the subject, persisted in Dgraph.
    async fn admit(&self, external_id: &str, kind: EvidenceType, observed_at: DateTime<Utc>) {
        let content = format!("the content of {external_id}");
        let record = EvidenceRecord::builder(
            EvidenceId::new(format!("{}-{external_id}", self.tenant.account_id())),
            self.tenant.clone(),
        )
        .subjects(vec![self.subject()])
        .evidence_type(kind)
        .source(SourceRef::by_external_id(external_id))
        .content(ContentRef::by_digest(ContentDigest::of_bytes(
            content.as_bytes(),
        )))
        .observed_at(observed_at)
        .recorded_at(observed_at)
        .verification(EvidenceVerification::verified("signature"))
        .normalization(Normalization::new("pipelinek", "1.0.0"))
        .producer(Producer::new("connector", "pipelinek"))
        .build();
        self.evidence
            .put(record)
            .await
            .expect("the evidence is persisted");
    }

    fn fork(&self, policy: TrustPolicy) -> SimulationFork {
        SimulationFork::new(
            "sim-m7",
            Checkpoint::new(
                "checkpoint-m7",
                self.tenant.account_id(),
                the_clock(),
                Some(0),
                self.generation,
            ),
            policy,
            the_clock(),
        )
        .with_subjects(vec![self.subject()])
    }

    fn tag() -> SimulationTag {
        SimulationTag::new("sim-m7", the_clock())
    }
}

/// An evidence reader whose subject read fails: the store is down.
struct DownEvidence {
    inner: Arc<DgraphEvidenceStore>,
}

#[async_trait]
impl EvidenceStore for DownEvidence {
    async fn put(&self, evidence: EvidenceRecord) -> Result<EvidenceId, StoreError> {
        self.inner.put(evidence).await
    }
    async fn get(&self, id: &EvidenceId) -> Result<Option<EvidenceRecord>, StoreError> {
        self.inner.get(id).await
    }
    async fn find_by_external_id(
        &self,
        producer: &Producer,
        external_id: &str,
    ) -> Result<Option<EvidenceRecord>, StoreError> {
        self.inner.find_by_external_id(producer, external_id).await
    }
    async fn evidence_of_subject(
        &self,
        _account: &str,
        _subject: &Hrn,
    ) -> Result<Vec<EvidenceRecord>, StoreError> {
        Err(StoreError::Unavailable(
            "the evidence store is down".to_string(),
        ))
    }
}

/// M7-UAT-01 (+ the isolation of M7-UAT-04): the proposed policy and a
/// hypothetical critical vulnerability change the decision; production
/// evidence and graph are untouched.
#[tokio::test]
async fn m7_uat_01_the_proposed_policy_changes_the_decision_and_writes_nothing() {
    let Some(client) = connect().await else {
        return;
    };
    let (world, service) = World::open(client, "u1", false).await;
    world.admit("sbom-1", EvidenceType::Sbom, the_clock()).await;

    let relations_before = world.graph.all_relations().await.expect("readable");
    let evidence_before = world
        .evidence
        .evidence_of_subject(world.tenant.account_id(), &world.subject())
        .await
        .expect("the evidence reads");

    let fork = world
        .fork(the_policy())
        .with_policy(the_strict_policy())
        .with(Hypothetical {
            tag: World::tag(),
            claim: HypotheticalClaim::Risk {
                subject: world.subject(),
                advisory: "GHSA-m7-0001".to_string(),
                critical: true,
                reachable: true,
            },
        });
    let diff = service
        .simulate(&fork, the_clock())
        .await
        .expect("the simulation runs");

    assert_eq!(
        diff.decisions.len(),
        1,
        "the decision that changes: {:?}",
        diff.decisions
    );
    let change = &diff.decisions[0];
    assert_eq!(change.subject, world.subject());
    assert_eq!(change.before, Outcome::Allow);
    assert_eq!(change.after, Outcome::Deny);
    assert_eq!(change.policy_version_before, "1.0.0");
    assert_eq!(change.policy_version_after, "2.0.0");

    // Production is untouched: the same relations and the same evidence.
    let relations_after = world.graph.all_relations().await.expect("readable");
    assert_eq!(relations_after.len(), relations_before.len());
    let evidence_after = world
        .evidence
        .evidence_of_subject(world.tenant.account_id(), &world.subject())
        .await
        .expect("the evidence reads");
    assert_eq!(evidence_after.len(), evidence_before.len());
}

/// M7-UAT-03: the evidence observed inside the window of a compromised
/// builder stops being verified, and the decision of the subject changes.
#[tokio::test]
async fn m7_uat_03_the_compromised_builder_window_changes_the_decision() {
    let Some(client) = connect().await else {
        return;
    };
    let (world, service) = World::open(client, "u3", false).await;
    let observed_at = the_clock() - ChronoDuration::hours(2);
    world
        .admit("provenance-window", EvidenceType::Provenance, observed_at)
        .await;

    let fork = world.fork(the_provenance_policy()).with(Hypothetical {
        tag: World::tag(),
        claim: HypotheticalClaim::UntrustedWindow {
            from: observed_at - ChronoDuration::minutes(30),
            to: observed_at + ChronoDuration::minutes(30),
            subjects: vec![world.subject()],
        },
    });
    let diff = service
        .simulate(&fork, the_clock())
        .await
        .expect("the simulation runs");

    assert_eq!(
        diff.decisions.len(),
        1,
        "the decision that changes: {:?}",
        diff.decisions
    );
    assert_eq!(diff.decisions[0].subject, world.subject());
    assert_eq!(diff.decisions[0].before, Outcome::Allow);
    assert_eq!(diff.decisions[0].after, Outcome::Deny);
}

/// M7-UAT-07: a failed evidence read is a typed error that says why; it is
/// not disguised as an empty capsule or an ALLOW.
#[tokio::test]
async fn m7_uat_07_a_failed_evidence_read_is_not_an_allow() {
    let Some(client) = connect().await else {
        return;
    };
    let (world, service) = World::open(client, "u7", true).await;
    let fork = world.fork(the_policy()).with_policy(the_strict_policy());

    let error = service
        .simulate(&fork, the_clock())
        .await
        .expect_err("the read failure arrives, it is not disguised");
    assert!(
        matches!(&error, SimulationError::ReadFailed(reason)
            if reason.contains("the evidence store is down")),
        "the error is typed and says why: {error}"
    );
}
