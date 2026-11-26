//! HSP-206: the investigation and its freshness over the graph authority.
//!
//! IT conditioned on `HODEI_GRAPH_IT_ENDPOINT` like all those of the stack.
//! It runs against the REAL shared alpha (asserts by MEMBERSHIP over the
//! test's tenant, never over global counts, HSP-198/219/220) and
//! What it proves:
//!
//! - REPRODUCIBILITY: two reads at the same watermark seal the SAME
//!   read-set byte for byte (identical manifest and fingerprint), with the
//!   versions of the supports that the report cites (HSP-89);
//! - SUPPORT WITHDRAWAL: a report that rests on a withdrawn support is
//!   Stale with the citation of what moved; the report that did not rest
//!   on the movement stays Current; and a support ADDED to a relation
//!   that the report read does NOT expire what the report said;
//! - FAIL-CLOSED (AC#2): the budget cut is DECLARED (Coverage
//!   Partial + OmittedByBudget) and the read-set of a truncated read
//!   still seals what was read; a mute authority closes the read with
//!   Unavailable, never an invented snapshot; and a manifest that cites
//!   an incomprehensible relation type produces UNKNOWN, never a
//!   fabricated Current;
//! - SCOPE: a subject of another account is not queried: declared UNKNOWN.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::Utc;
use hodei_evidence::application::projection::ProjectionDelivery;
use hodei_evidence::domain::delta::{
    RelationDelta, RelationDeltaLedger, RelationDeltaSubscription,
};
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal, NodeKind, RelationType,
};
use hodei_evidence::{ContentDigest, ContentRef, ContentStore, EvidenceStore};
use hodei_evidence_storage::{
    DgraphDeltaLedger, DgraphEvidenceStore, DgraphGraphStore, DgraphProjectionFrontier,
    InMemoryContentStore,
};
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_investigator::freshness::{ChangeKind, Freshness, GRAPH_SOURCE};
use hodei_investigator::investigator::EvidenceRetriever;
use hodei_investigator::read_set::{ReadKind, ReadSet, ReadStamp};
use hodei_investigator::scope::{
    Coverage, EvidenceReadAuthorizer, FactStatus, RetrievalBudget, RetrievalError, RetrievalScope,
    RetrievedContext,
};
use hodei_investigator_storage::freshness::LedgerFreshness;
use hodei_investigator_storage::{DgraphEvidenceRetriever, GraphSource, InvestigationRetriever};
use kernel::Hrn;

/// The authorization gate is the core's, already tested by the
/// reference (m8, HSP-89): here the read, the seal and the
/// three-way verdict over the native graph are tested.
struct EveryoneReads;

#[async_trait]
impl EvidenceReadAuthorizer for EveryoneReads {
    async fn can_read(&self, _actor: &Hrn, _resource: &Hrn) -> Result<(), String> {
        Ok(())
    }
}

/// A graph source whose authority does not answer: the watermark is not
/// invented as zero, and the relations that fail are not delivered as
/// absence.
struct SilentSource;

#[async_trait]
impl GraphSource for SilentSource {
    async fn generation(&self, _tenant: &Hrn) -> Result<u64, String> {
        Err("the authority did not answer".to_string())
    }

    async fn relations_from(
        &self,
        _tenant: &Hrn,
        _node: &Hrn,
    ) -> Result<Vec<GraphRelation>, String> {
        Err("the authority did not answer".to_string())
    }

    async fn relations_to(&self, _tenant: &Hrn, _node: &Hrn) -> Result<Vec<GraphRelation>, String> {
        Err("the authority did not answer".to_string())
    }
}

/// The reader of a delta history that does not answer: the verdict
/// on a report has to be UNKNOWN, never a Current computed
/// over silence.
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
        "schema boot: decision={:?} binary_version={} stored_version={:?}",
        report.decision, report.binary_version, report.stored_version
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

/// One tenant per test and per run: on the shared alpha each
/// slice leaves its mark, and the assertions are of membership over the
/// own one, not of global totals.
fn fresh_tenant(tag: &str) -> Hrn {
    let account = format!("it{tag}{}", &nonce()[7..]);
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/hsp206"
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

fn kind_label(kind: &NodeKind) -> &'static str {
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

fn node(tenant: &Hrn, kind: NodeKind, name: &str) -> GraphNode {
    GraphNode::new(
        hrn_of(tenant, kind_label(&kind), name),
        tenant.clone(),
        kind,
        Utc::now(),
    )
}

fn relation(
    tenant: &Hrn,
    kind: RelationType,
    from: &Hrn,
    to: &Hrn,
    evidence: &str,
) -> GraphRelation {
    GraphRelation::new(
        kind,
        from.clone(),
        to.clone(),
        tenant.clone(),
        EvidenceId::new(evidence),
        Utc::now(),
    )
}

fn withdrawal(
    tenant: &Hrn,
    kind: RelationType,
    from: &Hrn,
    to: &Hrn,
    evidence: &str,
) -> GraphRelationWithdrawal {
    GraphRelationWithdrawal::new(
        kind,
        from.clone(),
        to.clone(),
        tenant.clone(),
        EvidenceId::new(evidence),
        Utc::now(),
    )
}

/// The evidence registry of the world: content by digest, subject by
/// HRN. The digest is a function of the text, and so the evidence seal
/// is the SAME string in both engines.
fn evidence_record(tenant: &Hrn, id: &str, subject: &Hrn, text: &str) -> EvidenceRecord {
    let digest = ContentDigest::of_bytes(text.as_bytes());
    EvidenceRecord::builder(EvidenceId::new(id), tenant.clone())
        .evidence_type(EvidenceType::Sbom)
        .subjects(vec![subject.clone()])
        .source(SourceRef::by_external_id(format!("ext-{id}")))
        .content(ContentRef::by_digest(digest))
        .observed_at(Utc::now())
        .recorded_at(Utc::now())
        .verification(EvidenceVerification::verified("dsse"))
        .normalization(Normalization::new("cyclonedx", "1.0.0"))
        .producer(Producer::new("connector", "hsp206-it").with_version("1.0.0"))
        .build()
}

fn the_actor(tenant: &Hrn) -> Hrn {
    hrn_of(tenant, "User", "alice")
}

fn the_subject(tenant: &Hrn) -> Hrn {
    hrn_of(tenant, "Artifact", "app")
}

/// The world over the graph authority: graph, frontier (the only
/// door of the deltas, HSP-199), evidence, content and the two
/// readers over the SAME history.
struct World {
    graph: Arc<DgraphGraphStore>,
    frontier: Arc<DgraphProjectionFrontier>,
    deltas: Arc<DgraphDeltaLedger>,
    evidence: Arc<DgraphEvidenceStore>,
    content: Arc<InMemoryContentStore>,
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
        let evidence = Arc::new(
            DgraphEvidenceStore::open(client)
                .await
                .expect("the evidence store opens"),
        );
        Self {
            graph,
            frontier,
            deltas,
            evidence,
            content: Arc::new(InMemoryContentStore::new()),
            tenant,
        }
    }

    /// Admits an evidence and its content into the CAS.
    async fn admit(&self, id: &str, subject: &Hrn, text: &str) {
        self.content
            .put(text.as_bytes().to_vec())
            .await
            .expect("the content is stored");
        let record = evidence_record(&self.tenant, id, subject, text);
        self.evidence
            .put(record)
            .await
            .expect("the evidence is admitted");
    }

    /// Projects a changeset through the frontier: mutation, deltas and
    /// checkpoint as ONE single fact.
    async fn project(&self, changes: GraphChangeSet, position: u64) {
        let delivery = ProjectionDelivery {
            consumer: "hsp206-it".to_string(),
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

    /// The investigation retriever over the graph authority.
    fn retriever(&self) -> DgraphEvidenceRetriever {
        DgraphEvidenceRetriever::new(
            self.evidence.clone(),
            self.content.clone(),
            self.graph.clone(),
            self.deltas.clone(),
            Arc::new(EveryoneReads),
            self.tenant.clone(),
        )
    }

    async fn read(&self, scope: RetrievalScope) -> RetrievedContext {
        self.retriever()
            .retrieve(&scope)
            .await
            .expect("the read answers")
    }

    fn scope(&self) -> RetrievalScope {
        RetrievalScope::new(the_actor(&self.tenant), the_subject(&self.tenant))
    }

    /// The freshness checker over the SAME history against which
    /// the retriever sealed: the HSP-89 oracle on the authority.
    fn checker(&self) -> LedgerFreshness {
        LedgerFreshness::new(
            self.evidence.clone(),
            self.deltas.clone(),
            self.tenant.clone(),
        )
    }
}

/// The relation seals of a manifest, sorted.
fn relation_stamps(report: &ReadSet) -> Vec<ReadStamp> {
    let mut stamps: Vec<ReadStamp> = report
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Relation)
        .cloned()
        .collect();
    stamps.sort();
    stamps
}

/// AC #1: the read-set is reproduced, the support withdrawal is said Stale and
/// the verdict is the same over both engines.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_read_set_is_reproduced_and_the_retirement_is_stale() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = fresh_tenant("206a");
    let world = World::open(client, tenant.clone()).await;
    let subject = the_subject(&tenant);
    let component = hrn_of(&tenant, "Component", "lib");
    let identity = format!("contains|{subject}|{component}");
    // The key of the evidence in the authority is GLOBAL (the id, without
    // account): on an alpha shared across runs, each run
    // admits its own evidences with a run suffix. The reference
    // uses the same ids because it is fresh in memory.
    let run = &nonce()[7..];
    let sbom = format!("ev-{run}-sbom");
    let advisory = format!("ev-{run}-advisory");

    // The material: two evidences with their content, and a relation
    // supported by the first. The evidence nodes go in the same
    // changeset: the relation claims its evidence and the graph does not
    // accept it without it.
    world.admit(&sbom, &subject, "the sbom declares lib").await;
    world
        .admit(&advisory, &subject, "the advisory warns about lib")
        .await;
    world
        .project(
            GraphChangeSet::empty()
                .with_cause(format!("assert-{}", nonce()))
                .with_node(node(&tenant, NodeKind::Artifact, "app"))
                .with_node(node(&tenant, NodeKind::Component, "lib"))
                .with_node(node(&tenant, NodeKind::Evidence, &sbom))
                .with_node(node(&tenant, NodeKind::Evidence, &advisory))
                .with_relation(relation(
                    &tenant,
                    RelationType::Contains,
                    &subject,
                    &component,
                    &sbom,
                )),
            0,
        )
        .await;

    // SEALED READ: the watermark is the version of the history, and the
    // manifest says the relation with the support that sustains it.
    let report = world.read(world.scope()).await;
    assert_eq!(
        relation_stamps(&report.read_set),
        vec![ReadStamp::relation(&identity, &sbom)],
        "the report seals the relation with the support it read"
    );
    assert_eq!(report.coverage, Coverage::Complete, "nothing was left out");
    let sealed = report
        .read_set
        .generations
        .iter()
        .find(|(source, _)| source == GRAPH_SOURCE)
        .expect("the report says which version of the world it read")
        .1;
    assert_eq!(
        sealed, 1,
        "the Added delta of the delivery is the watermark"
    );

    // REPRODUCIBILITY: another read at the same watermark reproduces the
    // read-set byte for byte — the manifest is a function of the state and of
    // the stable keys, never of the order in which the store answered.
    let replay = world.read(world.scope()).await;
    assert_eq!(
        report.read_set.manifest(),
        replay.read_set.manifest(),
        "two reads at the same watermark give the same manifest"
    );
    assert_eq!(
        report.read_set.fingerprint(),
        replay.read_set.fingerprint(),
        "the seal is the byte-level identity of the read-set"
    );

    // CURRENT: the world did not move under the report.
    let verdict = world
        .checker()
        .freshness_of(&report.read_set, &subject)
        .await;
    assert!(
        matches!(verdict, Freshness::Current),
        "an untouched world keeps the report current: {verdict:?}"
    );

    // A NEW SUPPORT DOES NOT EXPIRE WHAT WAS SAID: the relation gained the advisory
    // (SupportChanged delta); the report that rested on the SBOM stays
    // standing, because what the report did NOT read cannot expire what it
    // read. The read of now seals the last support said — like the
    // reference row.
    world
        .project(
            GraphChangeSet::empty()
                .with_cause(format!("second-{}", nonce()))
                .with_relation(relation(
                    &tenant,
                    RelationType::Contains,
                    &subject,
                    &component,
                    &advisory,
                )),
            1,
        )
        .await;
    let verdict = world
        .checker()
        .freshness_of(&report.read_set, &subject)
        .await;
    assert!(
        matches!(verdict, Freshness::Current),
        "a support ADDED to a read relation does not retire what the report said: {verdict:?}"
    );
    let second = world.read(world.scope()).await;
    assert_eq!(
        relation_stamps(&second.read_set),
        vec![ReadStamp::relation(&identity, &advisory)],
        "the record says which support asserted it last, like the reference row"
    );

    // SUPPORT WITHDRAWAL: the advisory is withdrawn and the relation REMAINS
    // asserted (the SBOM is left). The report sealed against the advisory
    // is Stale with the citation of what moved; the one sealed against the SBOM
    // stays Current — the same three-way verdict with the same material.
    world
        .project(
            GraphChangeSet::empty()
                .with_cause(format!("retire-{}", nonce()))
                .with_withdrawal(withdrawal(
                    &tenant,
                    RelationType::Contains,
                    &subject,
                    &component,
                    &advisory,
                )),
            2,
        )
        .await;
    let verdict = world
        .checker()
        .freshness_of(&second.read_set, &subject)
        .await;
    match verdict {
        Freshness::Stale { changed } => {
            assert!(
                changed.iter().any(|change| change.evidence == advisory
                    && change.resource == identity
                    && change.what == ChangeKind::Removed),
                "the verdict cites the support that moved: {changed:?}"
            );
        }
        other => panic!("a report that rests on a retired support is Stale, not {other:?}"),
    }
    let verdict = world
        .checker()
        .freshness_of(&report.read_set, &subject)
        .await;
    assert!(
        matches!(verdict, Freshness::Current),
        "the support the report read was not the one retired: {verdict:?}"
    );

    // THE RELATION GOES WITH ITS LAST SUPPORT: without supports it is not an
    // assertion (status unknown), and it leaves the read-set; the report that
    // rested on it is Stale, never a Current fabricated from
    // absence.
    world
        .project(
            GraphChangeSet::empty()
                .with_cause(format!("last-{}", nonce()))
                .with_withdrawal(withdrawal(
                    &tenant,
                    RelationType::Contains,
                    &subject,
                    &component,
                    &sbom,
                )),
            3,
        )
        .await;
    let emptied = world.read(world.scope()).await;
    assert!(
        relation_stamps(&emptied.read_set).is_empty(),
        "a relation without supports is UNKNOWN and answers no graph fact"
    );
    let verdict = world
        .checker()
        .freshness_of(&second.read_set, &subject)
        .await;
    assert!(
        matches!(verdict, Freshness::Stale { .. }),
        "the report's own support was retired under it: {verdict:?}"
    );

    // SCOPE: the foreign account is not queried; the verdict is UNKNOWN
    // with the reason, not an answer about what is not theirs.
    let foreign_subject = hrn_of(&fresh_tenant("206f"), "Artifact", "other");
    let verdict = world
        .checker()
        .freshness_of(&report.read_set, &foreign_subject)
        .await;
    match verdict {
        Freshness::Unknown { reason } => {
            assert!(
                reason.contains("tenant"),
                "the reason says the subject is not of this tenant: {reason}"
            );
        }
        other => panic!("a subject of another account is UNKNOWN, not {other:?}"),
    }
}

/// AC #2: what is incomplete is DECLARED and what is unreadable is UNKNOWN — never a
/// verified fact that is not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_incomplete_read_is_declared_and_an_unreadable_world_is_unknown() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = fresh_tenant("206b");
    let world = World::open(client, tenant.clone()).await;
    let subject = the_subject(&tenant);

    // Three components under three supports: the relation budget
    // leaves two out. The evidence registry is NOT admitted: the read-set
    // that gets cut is the relations', and the test needs no more
    // material than the graph.
    let mut changes = GraphChangeSet::empty()
        .with_cause(format!("fanout-{}", nonce()))
        .with_node(node(&tenant, NodeKind::Artifact, "app"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-1"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-2"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-3"));
    for index in 1..=3 {
        changes = changes
            .with_node(node(&tenant, NodeKind::Component, &format!("c{index}")))
            .with_relation(relation(
                &tenant,
                RelationType::Contains,
                &subject,
                &hrn_of(&tenant, "Component", &format!("c{index}")),
                &format!("ev-{index}"),
            ));
    }
    world.project(changes, 0).await;

    let mut scope = world.scope();
    scope.budget = RetrievalBudget {
        max_relations: 1,
        ..RetrievalBudget::default()
    };
    let cut = world.read(scope).await;
    assert_eq!(
        cut.coverage,
        Coverage::Partial { omitted: 2 },
        "the cut is declared, never hidden"
    );
    assert_eq!(
        relation_stamps(&cut.read_set).len(),
        3,
        "the manifest seals what was read even when the delivery was cut"
    );
    assert_eq!(
        cut.facts
            .iter()
            .filter(|fact| fact.status == FactStatus::OmittedByBudget)
            .count(),
        2,
        "the cut facts carry the omitted status"
    );
    assert_eq!(
        cut.facts
            .iter()
            .filter(|fact| fact.status == FactStatus::Loaded)
            .count(),
        1,
        "exactly the admitted one is a fact"
    );

    // A MUTE AUTHORITY CLOSES THE READ: the missing watermark is not
    // invented as zero. The retriever delivers NOTHING — neither facts nor
    // manifest — that could be presented as a verified fact.
    let retriever = InvestigationRetriever::new(
        world.evidence.clone(),
        world.content.clone(),
        SilentSource,
        Arc::new(EveryoneReads),
        tenant.clone(),
    );
    let failure = retriever
        .retrieve(&world.scope())
        .await
        .expect_err("a silent authority closes the read");
    assert!(
        matches!(failure, RetrievalError::Unavailable(_)),
        "the silence is an unavailable world, not an empty one: {failure:?}"
    );

    // AN UNREADABLE WORLD IS NOT A FACT: a manifest that cites a relation
    // type that cannot be interpreted makes the read of the
    // changes fail, and the verdict is UNKNOWN with the source named —
    // never an invented Current. The watermark is put BEHIND the
    // history: it is the only door through which the checker has to
    // interpret the manifest types, and the interpretation fails.
    let mut forged = ReadSet::empty();
    forged.generation(GRAPH_SOURCE, 0);
    forged.read(ReadStamp::relation(
        "made-up|hrn:hodei:artifacts:eu-west-1:acme:Artifact/x|hrn:hodei:artifacts:eu-west-1:acme:Artifact/y",
        "ev-1",
    ));
    let verdict = world.checker().freshness_of(&forged, &subject).await;
    match verdict {
        Freshness::Unknown { reason } => {
            assert!(
                reason.contains(GRAPH_SOURCE),
                "the reason names the source that could not be consulted: {reason}"
            );
        }
        other => panic!("an unreadable world is UNKNOWN, not {other:?}"),
    }

    // THE HISTORY THAT DOES NOT ANSWER IS NOT A FACT EITHER: a mute delta
    // reader cannot say whether the report is still current, and it says so.
    let deaf = LedgerFreshness::new(
        world.evidence.clone(),
        Arc::new(SilentLedger),
        tenant.clone(),
    );
    let verdict = deaf.freshness_of(&cut.read_set, &subject).await;
    assert!(
        matches!(verdict, Freshness::Unknown { .. }),
        "a silent story is UNKNOWN, never Current: {verdict:?}"
    );
}
