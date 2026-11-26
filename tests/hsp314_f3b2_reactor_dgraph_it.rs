//! HSP-314 F3b-2: the CVE reactor built over the Dgraph adapters.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every Dgraph IT. The reactor is the
//! SAME `VulnerabilityReactor` the embedded profile runs, assembled by
//! `VulnerabilityReactor::over_dgraph`: no embedded engine is linked or
//! opened by this test. What it proves, against the real engine:
//!
//! - an accepted advisory opens the security case in the store of ITS
//!   account, materializes the impact view, turns the risk facts of the
//!   subject from "not measured" into measured, and takes back the
//!   concession the new truth refuses;
//! - the validity sweep reads the unrevoked concessions through the port:
//!   a lapsed one and one decided under a superseded policy are
//!   reevaluated and renewed, a healthy one is left alone, and a repeated
//!   pass says nothing twice;
//! - the reconciliation reads the deltas of every tenant through the port
//!   and brings the view up to the graph;
//! - the sink of the cases never lets an account write into another one;
//! - with the alpha unreachable every entry fails closed: an error, and
//!   nothing written.
//!
//! The outage is a local TCP relay between the client and the alpha that
//! the test cuts: no shared container is stopped.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Duration, Utc};
use hodei_artifacts_api::config::InvestigationConfig;
use hodei_artifacts_api::evidence_dgraph::DgraphEvidenceReader;
use hodei_artifacts_api::trust::hrn::artifact_hrn;
use hodei_artifacts_api::trust::promotion::TrustPlane;
use hodei_artifacts_api::trust::reaction::{
    DgraphReactorParts, PerTenantCaseSink, ReactorError, VulnerabilityReactor,
};
use hodei_catalog::{TrustGrant, TrustLedger};
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::application::projection::{ProjectionDelivery, ProjectionFrontier};
use hodei_evidence::domain::graph::{NodeKind, RelationType};
use hodei_evidence::{ContentDigest, EvidenceId, GraphChangeSet, GraphNode, GraphRelation};
use hodei_evidence_storage::{DgraphEvidenceStore, DgraphGraphStore, DgraphProjectionFrontier};
use hodei_graph_client::schema::ensure_ready;
use hodei_graph_client::{GraphClient, GraphClientConfig, Tenant};
use hodei_reactive::reactions::{ImpactedSubject, SecurityCaseSink};
use hodei_risk::case::SecurityCase;
use hodei_risk::projection::vulnerability_hrn;
use hodei_risk_storage::dgraph_cases::DgraphSecurityCaseStore;
use hodei_risk_storage::dgraph_view::DgraphImpactView;
use hodei_risk_storage::subject_risk::{
    AdvisoryCatalog, AdvisorySeverity, DgraphRiskFacts, MemoryCatalog, RiskFactsReader,
};
use hodei_trust_policy::context::{Exposure, RiskFacts, TrustContext};
use hodei_trust_policy::{
    AuthorizationResult, DecisionId, DecisionStore, Outcome, PolicyRegistry, PromotionOutcome,
    PromotionRequest, Requirement, RequirementKind, TrustPolicy,
};
use hodei_trust_policy_storage::{DgraphDecisionStore, DgraphTrustStore};
use kernel::Hrn;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

const REGION: &str = "eu";
const POLICY_ID: &str = "default";
const POLICY_VERSION: &str = "1.0.0";

/// The CycloneDX inventory of the artifact: `web` holds `lib-a`, which
/// depends on `lib-b` and `lib-c`. The advisory matches `lib-a` by purl and
/// the radius climbs from there to the artifact.
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
    { "bom-ref": "pkg:maven/org.example/lib-a@1.2.3", "type": "library", "name": "lib-a", "version": "1.2.3", "purl": "pkg:maven/org.example/lib-a@1.2.3" },
    { "bom-ref": "pkg:maven/org.example/lib-b@2.0.0", "type": "library", "name": "lib-b", "version": "2.0.0", "purl": "pkg:maven/org.example/lib-b@2.0.0" },
    { "bom-ref": "pkg:golang/github.com/example/lib-c@v1.4.2", "type": "library", "name": "lib-c", "version": "v1.4.2", "purl": "pkg:golang/github.com/example/lib-c@v1.4.2" }
  ],
  "dependencies": [
    { "ref": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06", "dependsOn": ["pkg:maven/org.example/lib-a@1.2.3"] },
    { "ref": "pkg:maven/org.example/lib-a@1.2.3", "dependsOn": ["pkg:maven/org.example/lib-b@2.0.0"] },
    { "ref": "pkg:maven/org.example/lib-b@2.0.0", "dependsOn": ["pkg:golang/github.com/example/lib-c@v1.4.2"] },
    { "ref": "pkg:golang/github.com/example/lib-c@v1.4.2", "dependsOn": [] }
  ]
}"#;

fn osv(id: &str) -> String {
    serde_json::json!({
        "schema_version": "1.6.0",
        "id": id,
        "modified": "2026-11-18T00:00:00Z",
        "affected": [
            { "package": { "ecosystem": "Maven", "name": "lib-a", "purl": "pkg:maven/org.example/lib-a@1.2.3" } }
        ]
    })
    .to_string()
}

fn config_for(endpoint: &str) -> GraphClientConfig {
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
    GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates")
}

async fn connect_to(endpoint: &str) -> Arc<GraphClient> {
    let mut client = GraphClient::connect(config_for(endpoint))
        .await
        .expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    ensure_ready(&client)
        .await
        .expect("the versioned schema boots");
    Arc::new(client)
}

fn endpoint() -> Option<String> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok();
    if endpoint.is_none() {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
    }
    endpoint
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// A relay to the alpha that the test can cut: cutting it drops every open
/// connection and stops listening, so the client's next call fails for real.
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

/// The reactor over the Dgraph adapters, with the readers a test asserts
/// through. Every account is unique to the stack, so the shared alpha never
/// mixes two tests.
struct Stack {
    client: Arc<GraphClient>,
    account: String,
    tenant: Hrn,
    advisory_id: String,
    advisory: Hrn,
    trust: Arc<DgraphTrustStore>,
    decisions: Arc<DgraphDecisionStore>,
    plane: Arc<TrustPlane>,
    reactor: VulnerabilityReactor,
    view: Arc<DgraphImpactView>,
    facts: DgraphRiskFacts,
    frontier: Arc<DgraphProjectionFrontier>,
}

async fn stack(client: Arc<GraphClient>, tag: &str) -> Stack {
    stack_with(client, tag, None).await
}

/// `lifecycle` makes the plane the production per-account one (HSP-321);
/// without it the plane decides with the process-wide registry.
async fn stack_with(
    client: Arc<GraphClient>,
    tag: &str,
    lifecycle: Option<Arc<dyn hodei_trust_policy::LifecycleStore>>,
) -> Stack {
    let suffix = nonce();
    let account = format!("{tag}{}", &suffix[suffix.len() - 12..]);
    let namespace = format!("f3b2-{account}");
    let limits = InvestigationConfig::default();
    let tenant = Hrn::from_string(&format!(
        "hrn:hodei:artifacts:{REGION}:{account}:artifact/{account}"
    ))
    .expect("a tenant HRN");
    let advisory_id = format!("GHSA-f3b2-{account}");
    let advisory = vulnerability_hrn(&tenant, &advisory_id);

    let trust = Arc::new(
        DgraphTrustStore::open(
            client.clone(),
            Tenant::new(&namespace).expect("the namespace validates"),
        )
        .await
        .expect("the trust store opens"),
    );
    let decisions = Arc::new(
        DgraphDecisionStore::open(client.clone())
            .await
            .expect("the decision store opens"),
    );
    let registry = PolicyRegistry::empty();
    registry.activate(
        TrustPolicy::new(
            POLICY_ID,
            POLICY_VERSION,
            vec![Requirement::denying(
                RequirementKind::NoReachableCriticalVulnerability,
            )],
        ),
        "it",
        "the policy of the reactor IT",
        Utc::now(),
    );
    let plane = Arc::new(match lifecycle {
        Some(lifecycle) => TrustPlane::per_account(
            decisions.clone() as Arc<dyn DecisionStore>,
            lifecycle,
            PolicyRegistry::empty(),
            POLICY_ID,
            Duration::hours(1),
        ),
        None => TrustPlane::new(
            decisions.clone() as Arc<dyn DecisionStore>,
            registry,
            POLICY_ID,
            Duration::hours(1),
        ),
    });

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
    let catalog: Arc<dyn AdvisoryCatalog> = Arc::new(MemoryCatalog::new([(
        advisory.to_string(),
        AdvisorySeverity::Critical,
    )]));
    let facts_reader: Arc<dyn RiskFactsReader> =
        Arc::new(DgraphRiskFacts::new(view.clone(), catalog.clone()));
    let evidence = Arc::new(DgraphEvidenceReader::new(Arc::new(
        DgraphEvidenceStore::open(client.clone())
            .await
            .expect("the evidence store opens"),
    )));
    let reactor = VulnerabilityReactor::over_dgraph(DgraphReactorParts {
        client: client.clone(),
        namespace,
        trust: trust.clone(),
        decisions: decisions.clone(),
        plane: plane.clone(),
        evidence,
        risk_facts: facts_reader,
        limits,
    })
    .await
    .expect("the reactor opens over the graph authority");

    Stack {
        client,
        account,
        tenant,
        advisory_id,
        advisory,
        trust,
        decisions,
        plane,
        reactor,
        facts: DgraphRiskFacts::new(view.clone(), catalog),
        view,
        frontier,
    }
}

/// The subject, digest and grant of a promotion the way the promotion
/// endpoint leaves them: a decision that advanced, and the concession it
/// wrote in the book.
struct Promoted {
    subject: Hrn,
    digest: ContentDigest,
    repository: String,
    grant: TrustGrant,
}

/// Promotes a fresh artifact and writes its concession. `lapsed` makes the
/// horizon end an hour ago; `policy_version` is the version stamped on the
/// concession (the one the plane decides with when `None`).
async fn promote(
    stack: &Stack,
    repository: &str,
    lapsed: bool,
    policy_version: Option<&str>,
) -> Promoted {
    promote_in(stack, &stack.account, repository, lapsed, policy_version).await
}

async fn promote_in(
    stack: &Stack,
    account: &str,
    repository: &str,
    lapsed: bool,
    policy_version: Option<&str>,
) -> Promoted {
    let digest = ContentDigest::of_bytes(format!("f3b2-{}-{repository}", account).as_bytes());
    let subject = artifact_hrn(REGION, account, repository, &digest);
    let now = Utc::now();
    let capsule = CapsuleAssembler::new().assemble(&subject, &[], now);
    let context = TrustContext::builder(&capsule)
        .with_digest(format!("sha256:{}", digest.value))
        .with_risk(RiskFacts {
            critical_known: 0,
            critical_reachable: 0,
            exposure: Exposure::Internal,
            vex_accepted: 0,
            established: true,
            risk_version: 0,
        })
        .assembled_at(now)
        .build();
    let id = DecisionId::new(format!("dec-seed-{}-{repository}", account));
    let outcome = stack
        .plane
        .service()
        .request_promotion(
            id.clone(),
            PromotionRequest::new(
                format!("corr-{}-{repository}", account),
                "seed",
                account.to_string(),
                subject.to_string(),
                "trusted",
                now,
            ),
            AuthorizationResult::Permit,
            context,
            POLICY_ID,
        )
        .await
        .expect("the promotion is decided");
    assert!(
        matches!(outcome, PromotionOutcome::Promoted),
        "the seed promotion must advance: {outcome:?}"
    );
    let decision = stack
        .decisions
        .get(&id)
        .await
        .expect("the decision store answers")
        .expect("the decision was written");
    let grant = TrustGrant {
        account: account.to_string(),
        repository: repository.to_string(),
        digest: digest.clone(),
        decision_id: id.as_str().to_string(),
        actor: "seed".to_string(),
        granted_at: now,
        revoked_at: None,
        revoked_by: None,
        revoke_reason: None,
        subject: Some(subject.to_string()),
        policy_version: Some(
            policy_version
                .map(str::to_string)
                .unwrap_or_else(|| decision.trust.policy_version.clone()),
        ),
        capsule_hash: decision.trust.capsule_hash.clone(),
        risk_version: Some(decision.trust.risk_version),
        valid_until: Some(if lapsed {
            now - Duration::hours(1)
        } else {
            stack.plane.validity_of(&decision, now)
        }),
    };
    stack
        .trust
        .grant(std::slice::from_ref(&grant))
        .await
        .expect("the concession is written");
    Promoted {
        subject,
        digest,
        repository: repository.to_string(),
        grant,
    }
}

async fn active(stack: &Stack, promoted: &Promoted) -> Option<TrustGrant> {
    stack
        .trust
        .active_grant(
            &stack.account,
            &promoted.repository,
            &promoted.digest,
            Utc::now(),
        )
        .await
        .expect("the book answers")
}

fn a_node(tenant: &Hrn, kind: NodeKind, label: &str, name: &str) -> GraphNode {
    GraphNode::new(
        Hrn::from_string(&format!(
            "hrn:hodei:artifacts:{REGION}:{}:{label}/{name}",
            tenant.account_id()
        ))
        .expect("a node HRN"),
        tenant.clone(),
        kind,
        Utc::now(),
    )
}

fn hrn_of(tenant: &Hrn, label: &str, name: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:{REGION}:{}:{label}/{name}",
        tenant.account_id()
    ))
    .expect("an HRN")
}

/// An artifact that holds the vulnerable component, with its cause.
fn the_contains(stack: &Stack, cause: &str, index: usize) -> GraphChangeSet {
    let tenant = &stack.tenant;
    let evidence = EvidenceId::new(format!("ev-{index}"));
    let shared = EvidenceId::new("ev-shared");
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_node(a_node(
            tenant,
            NodeKind::Component,
            "Component",
            "lib-shared",
        ))
        .with_node(GraphNode::new(
            stack.advisory.clone(),
            tenant.clone(),
            NodeKind::Vulnerability,
            Utc::now(),
        ))
        .with_node(a_node(
            tenant,
            NodeKind::Artifact,
            "Artifact",
            &format!("app-{index}"),
        ))
        .with_node(a_node(
            tenant,
            NodeKind::Evidence,
            "Evidence",
            &format!("ev-{index}"),
        ))
        .with_node(a_node(tenant, NodeKind::Evidence, "Evidence", "ev-shared"))
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
            stack.advisory.clone(),
            tenant.clone(),
            shared,
            Utc::now(),
        ))
}

fn delivery_of(stack: &Stack, position: u64, event: &str) -> ProjectionDelivery {
    ProjectionDelivery {
        consumer: "graph-projector".to_owned(),
        stream_id: format!("tenant-{}", stack.account),
        position,
        event_id: event.to_owned(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_advisory_opens_the_case_measures_the_facts_and_takes_back_the_grant() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let stack = stack(connect_to(&endpoint).await, "acc").await;
    let promoted = promote(&stack, "releases", false, None).await;
    assert!(
        active(&stack, &promoted).await.is_some(),
        "the seed concession serves before the advisory"
    );
    // Nothing was measured yet: the view has no route for the account.
    let before = stack
        .facts
        .risk_facts(&stack.tenant, &promoted.subject)
        .await
        .expect("the facts answer");
    assert!(
        !before.established && before.critical_reachable == 0,
        "before the advisory the facts are not measured: {before:?}"
    );

    let admission = stack
        .reactor
        .admit_advisory(
            &stack.account,
            REGION,
            "releases",
            &promoted.digest,
            &osv(&stack.advisory_id),
            Some(INVENTORY),
        )
        .await
        .expect("the advisory is admitted over the graph authority");
    assert_eq!(admission.advisory, stack.advisory.to_string());
    assert!(!admission.duplicate);
    assert!(admission.receipts > 0, "the cycle closed receipts");
    assert!(admission.cut.is_none(), "the chain was not cut");

    // The case is in the store of ITS account.
    let case = DgraphSecurityCaseStore::new(stack.client.clone(), stack.tenant.clone())
        .get(&SecurityCase::id_for(&stack.advisory.to_string()))
        .await
        .expect("the case store answers")
        .expect("the case was opened");
    assert!(
        case.blast_radius
            .affected_subjects
            .contains(&promoted.subject.to_string()),
        "the case names the impacted subject: {case:?}"
    );

    // The impact view was materialized and the facts are now MEASURED.
    let rows = stack
        .view
        .rows_reaching(&stack.tenant, &promoted.subject)
        .await
        .expect("the view answers");
    assert_eq!(rows.len(), 1, "one advisory reaches the subject: {rows:?}");
    assert_eq!(rows[0].advisory, stack.advisory.to_string());
    let after = stack
        .facts
        .risk_facts(&stack.tenant, &promoted.subject)
        .await
        .expect("the facts answer");
    assert!(after.established, "the facts are measured: {after:?}");
    assert_eq!(after.critical_reachable, 1);

    // The new truth refuses the subject: the concession is taken back.
    assert!(
        active(&stack, &promoted).await.is_none(),
        "the reevaluation revoked the concession the CVE touches"
    );

    // The same advisory is the same event: said once.
    let again = stack
        .reactor
        .admit_advisory(
            &stack.account,
            REGION,
            "releases",
            &promoted.digest,
            &osv(&stack.advisory_id),
            Some(INVENTORY),
        )
        .await
        .expect("the redelivery is answered");
    assert!(again.duplicate, "a redelivery is not a new event");
}

/// HSP-318 (M6-UAT-01): a new CVE does not rewrite the past. The prior ALLOW
/// stays in the history and a NEW DENY decision is written next to it that
/// names the decision it supersedes (its correlation is the prior id).
#[tokio::test(flavor = "multi_thread")]
async fn a_new_cve_writes_a_deny_that_supersedes_the_prior_allow_and_keeps_it() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let stack = stack(connect_to(&endpoint).await, "sup").await;
    let promoted = promote(&stack, "releases", false, None).await;
    let prior_id = promoted.grant.decision_id.clone();

    stack
        .reactor
        .admit_advisory(
            &stack.account,
            REGION,
            "releases",
            &promoted.digest,
            &osv(&stack.advisory_id),
            Some(INVENTORY),
        )
        .await
        .expect("the advisory is admitted over the graph authority");

    let history = stack
        .decisions
        .list_for_subject(&stack.account, &promoted.subject.to_string())
        .await
        .expect("the decisions of the subject read");

    // The prior ALLOW is still there, untouched.
    let prior = history
        .iter()
        .find(|decision| decision.id.as_str() == prior_id)
        .expect("the prior decision stays in the history");
    assert_eq!(prior.outcome, Outcome::Allow);
    assert!(prior.applied);

    // And a new DENY names it as the one it replaces.
    let superseding: Vec<_> = history
        .iter()
        .filter(|decision| decision.id.as_str() != prior_id)
        .collect();
    assert_eq!(superseding.len(), 1, "one new decision: {superseding:?}");
    let deny = superseding[0];
    assert_eq!(deny.outcome, Outcome::Deny);
    assert!(!deny.applied, "a DENY advances nothing");
    assert_eq!(
        deny.request.correlation_id, prior_id,
        "the new decision says which one it supersedes"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_renews_the_lapsed_and_the_superseded_and_leaves_the_healthy_alone() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let stack = stack(connect_to(&endpoint).await, "swp").await;
    // An advisory that reaches nobody still materializes a route, which is
    // what makes the view of the account current: the subjects below are
    // then a verified zero, not an unknown.
    stack
        .reactor
        .admit_advisory(
            &stack.account,
            REGION,
            "bootstrap",
            &ContentDigest::of_bytes(b"f3b2-bootstrap"),
            &osv(&stack.advisory_id),
            None,
        )
        .await
        .expect("the bootstrap advisory is admitted");

    let lapsed = promote(&stack, "lapsed", true, None).await;
    let superseded = promote(&stack, "superseded", false, Some("0.0.1")).await;
    let healthy = promote(&stack, "healthy", false, None).await;
    assert!(
        active(&stack, &lapsed).await.is_none(),
        "the lapsed concession no longer serves"
    );

    let now = Utc::now();
    let swept = stack
        .reactor
        .sweep_stale_grants(now)
        .await
        .expect("the sweep reads the book through the port");
    assert!(swept.expired_reevaluated >= 1, "lapsed: {swept:?}");
    assert!(swept.superseded_reevaluated >= 1, "superseded: {swept:?}");
    assert!(swept.regenerated >= 2, "both were renewed: {swept:?}");

    // Renewed: a NEW decision backs the concession the plane computed.
    for promoted in [&lapsed, &superseded] {
        let renewed = stack
            .trust
            .grants_of(&stack.account, &promoted.repository)
            .await
            .expect("the book answers")
            .into_iter()
            .find(|grant| grant.decision_id != promoted.grant.decision_id)
            .unwrap_or_else(|| panic!("{} was not renewed", promoted.repository));
        assert_eq!(renewed.policy_version.as_deref(), Some(POLICY_VERSION));
        assert!(renewed.valid_until.is_some_and(|until| until > now));
        assert!(renewed.revoked_at.is_none());
    }
    // The healthy one was never reevaluated.
    let untouched = stack
        .trust
        .grants_of(&stack.account, &healthy.repository)
        .await
        .expect("the book answers");
    assert_eq!(untouched.len(), 1, "no second grant: {untouched:?}");
    assert_eq!(untouched[0].decision_id, healthy.grant.decision_id);

    // The same pass says nothing twice.
    let repeated = stack
        .reactor
        .sweep_stale_grants(now)
        .await
        .expect("the repeated sweep is answered");
    assert_eq!(repeated.expired_reevaluated, 0, "{repeated:?}");
    assert_eq!(repeated.superseded_reevaluated, 0, "{repeated:?}");
    assert!(repeated.already_said >= 2, "{repeated:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reconciliation_brings_the_view_up_to_the_deltas_of_the_graph() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let stack = stack(connect_to(&endpoint).await, "rec").await;
    stack
        .frontier
        .apply_and_checkpoint(
            the_contains(&stack, "evt-a", 0),
            &delivery_of(&stack, 0, "evt-a"),
        )
        .await
        .expect("the first artifact goes through the frontier");
    stack
        .view
        .rebuild_advisory(&stack.tenant, &stack.advisory)
        .await
        .expect("the route is materialized");
    let late = hrn_of(&stack.tenant, "Artifact", "app-1");
    stack
        .frontier
        .apply_and_checkpoint(
            the_contains(&stack, "evt-b", 1),
            &delivery_of(&stack, 1, "evt-b"),
        )
        .await
        .expect("the second artifact goes through the frontier");
    assert!(
        stack
            .view
            .rows_reaching(&stack.tenant, &late)
            .await
            .expect("the view answers")
            .is_empty(),
        "the view is behind the graph until the pass runs"
    );

    let applied = stack
        .reactor
        .reconcile_impact_view()
        .await
        .expect("the reconciliation reads the deltas through the port");
    assert!(applied >= 1, "at least one route was recomputed: {applied}");
    let rows = stack
        .view
        .rows_reaching(&stack.tenant, &late)
        .await
        .expect("the view answers");
    assert_eq!(rows.len(), 1, "the view caught up: {rows:?}");
    let facts = stack
        .facts
        .risk_facts(&stack.tenant, &late)
        .await
        .expect("the facts answer");
    assert!(
        facts.established && facts.critical_reachable == 1,
        "{facts:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_case_sink_never_lets_an_account_write_into_another_one() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let client = connect_to(&endpoint).await;
    let suffix = nonce();
    let mine = format!("mine{}", &suffix[suffix.len() - 10..]);
    let theirs = format!("other{}", &suffix[suffix.len() - 10..]);
    let tenant_of = |account: &str| {
        Hrn::from_string(&format!(
            "hrn:hodei:artifacts:{REGION}:{account}:artifact/{account}"
        ))
        .expect("a tenant HRN")
    };
    let opener = client.clone();
    let sink = PerTenantCaseSink::new(InvestigationConfig::default(), move |tenant| {
        Arc::new(DgraphSecurityCaseStore::new(opener.clone(), tenant.clone()))
    });
    let advisory = |account: &str| {
        format!("hrn:hodei:artifacts:{REGION}:{account}:Vulnerability/GHSA-sink-{account}")
    };
    let subject_of = |account: &str| ImpactedSubject {
        subject: format!("hrn:hodei:artifacts:{REGION}:{account}:artifact/app"),
        deployment: None,
        depth: 1,
    };

    let id = sink
        .open_or_update(&advisory(&mine), &[subject_of(&mine)], Utc::now())
        .await
        .expect("the case is opened in the store of its account");
    assert!(
        DgraphSecurityCaseStore::new(client.clone(), tenant_of(&mine))
            .get(&id)
            .await
            .expect("the store answers")
            .is_some()
    );
    assert!(
        DgraphSecurityCaseStore::new(client.clone(), tenant_of(&theirs))
            .get(&id)
            .await
            .expect("the store answers")
            .is_none(),
        "the other account never sees the case"
    );

    // The other account's advisory lands in the store of THAT account.
    let their_id = sink
        .open_or_update(&advisory(&theirs), &[subject_of(&theirs)], Utc::now())
        .await
        .expect("the case of the other account is opened in its own store");
    assert!(
        DgraphSecurityCaseStore::new(client.clone(), tenant_of(&theirs))
            .get(&their_id)
            .await
            .expect("the store answers")
            .is_some()
    );
    assert!(
        DgraphSecurityCaseStore::new(client.clone(), tenant_of(&mine))
            .get(&their_id)
            .await
            .expect("the store answers")
            .is_none(),
        "the case of one account is not in the store of the other"
    );

    // An advisory of one account that names a subject of another is refused.
    assert!(
        sink.open_or_update(&advisory(&mine), &[subject_of(&theirs)], Utc::now())
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_alpha_unreachable_every_entry_fails_closed_and_writes_nothing() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let mut relay = Relay::to(endpoint.clone()).await;
    let through_relay = connect_to(&relay.endpoint).await;
    let stack = stack(through_relay, "dwn").await;
    let promoted = promote(&stack, "releases", false, None).await;
    let direct = connect_to(&endpoint).await;

    relay.cut();

    let admission = stack
        .reactor
        .admit_advisory(
            &stack.account,
            REGION,
            "releases",
            &promoted.digest,
            &osv(&stack.advisory_id),
            Some(INVENTORY),
        )
        .await;
    assert!(
        matches!(admission, Err(ReactorError::Unavailable(_))),
        "an admission with the alpha down is an error, got {admission:?}"
    );
    assert!(
        matches!(
            stack.reactor.sweep_stale_grants(Utc::now()).await,
            Err(ReactorError::Unavailable(_))
        ),
        "the sweep fails closed"
    );
    assert!(
        matches!(
            stack.reactor.reconcile_impact_view().await,
            Err(ReactorError::Unavailable(_))
        ),
        "the reconciliation fails closed"
    );

    // Nothing was written: no case, and the concession still stands.
    assert!(
        DgraphSecurityCaseStore::new(direct.clone(), stack.tenant.clone())
            .get(&SecurityCase::id_for(&stack.advisory.to_string()))
            .await
            .expect("the store answers")
            .is_none(),
        "no case was opened"
    );
    let book = DgraphTrustStore::open(
        direct,
        Tenant::new(&format!("f3b2-check-{}", stack.account)).expect("the namespace validates"),
    )
    .await
    .expect("the book opens");
    assert!(
        book.active_grant(
            &stack.account,
            &promoted.repository,
            &promoted.digest,
            Utc::now()
        )
        .await
        .expect("the book answers")
        .is_some(),
        "an outage never revokes a concession"
    );
}

/// Saves (as the editor) and activates (as the approver) a revision of the
/// IT policy for `account`; the revision only differs by its version.
async fn activate_version(
    lifecycle: &Arc<dyn hodei_trust_policy::LifecycleStore>,
    account: &str,
    version: &str,
    expected_head: Option<u64>,
) -> u64 {
    let policy = TrustPolicy::new(
        POLICY_ID,
        version,
        vec![Requirement::denying(
            RequirementKind::NoReachableCriticalVulnerability,
        )],
    );
    let revision = lifecycle
        .save(account, policy, "editor", "it", Utc::now())
        .await
        .expect("the revision is saved");
    lifecycle
        .activate(
            account,
            POLICY_ID,
            revision.sequence,
            expected_head,
            "approver",
            "it",
            Utc::now(),
        )
        .await
        .expect("the revision is activated")
        .sequence
}

/// HSP-321: with the per-account plane, the drift of the sweep is judged
/// against the policy of the account of each grant. Two accounts decided
/// under their version 1; only one activates version 2: only ITS grant
/// drifts and is renewed, the other's is left alone.
#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_drifts_only_the_grants_of_the_account_whose_policy_changed() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let client = connect_to(&endpoint).await;
    let lifecycle: Arc<dyn hodei_trust_policy::LifecycleStore> = Arc::new(
        hodei_trust_policy_storage::DgraphLifecycleStore::open(
            client.clone(),
            hodei_trust_policy::lifecycle::SeparationPolicy::required(),
        )
        .await
        .expect("the lifecycle opens"),
    );
    let stack = stack_with(client, "drf", Some(lifecycle.clone())).await;
    let changed = stack.account.clone();
    let other = format!("{changed}b");

    let changed_head = activate_version(&lifecycle, &changed, "1", None).await;
    activate_version(&lifecycle, &other, "1", None).await;
    stack
        .reactor
        .admit_advisory(
            &changed,
            REGION,
            "bootstrap",
            &ContentDigest::of_bytes(b"f3b2-bootstrap"),
            &osv(&stack.advisory_id),
            None,
        )
        .await
        .expect("the bootstrap advisory is admitted");

    let on_changed = promote_in(&stack, &changed, "drifting", false, None).await;
    let on_other = promote_in(&stack, &other, "steady", false, None).await;
    assert_eq!(on_changed.grant.policy_version.as_deref(), Some("1"));
    assert_eq!(on_other.grant.policy_version.as_deref(), Some("1"));

    // Only `changed` activates version 2.
    activate_version(&lifecycle, &changed, "2", Some(changed_head)).await;

    let swept = stack
        .reactor
        .sweep_stale_grants(Utc::now())
        .await
        .expect("the sweep reads the book through the port");
    assert!(swept.superseded_reevaluated >= 1, "drift: {swept:?}");

    let renewed = stack
        .trust
        .grants_of(&changed, "drifting")
        .await
        .expect("the book answers")
        .into_iter()
        .find(|grant| grant.decision_id != on_changed.grant.decision_id)
        .expect("the grant of the account whose policy changed was renewed");
    assert_eq!(renewed.policy_version.as_deref(), Some("2"));

    let untouched = stack
        .trust
        .grants_of(&other, "steady")
        .await
        .expect("the book answers");
    assert_eq!(untouched.len(), 1, "no second grant: {untouched:?}");
    assert_eq!(untouched[0].decision_id, on_other.grant.decision_id);
    assert_eq!(untouched[0].policy_version.as_deref(), Some("1"));
}
