//! HSP-314 F3b-0: the two Dgraph ports the CVE reactor needs,
//! against the real engine.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack.
//!
//! - `DgraphSecurityCaseStore` (the `SecurityCaseSink` of the reaction):
//!   the case is opened ONCE however many times it is asked for, two
//!   accounts reacting to the same advisory have two cases that never see
//!   each other, an absent case is `None`, a subject of another account is
//!   refused, and a store that fails answers with an error and leaves no
//!   partial state (the outage drill needs
//!   HODEI_GRAPH_IT_ALLOW_STORE_FAILURE=1 and
//!   HODEI_GRAPH_IT_ALPHA_CONTAINER, and always brings the alpha back).
//! - `blast_radius_dgraph`: a graph seeded with a KNOWN reach answers with
//!   exactly that reach (direct and transitive depth, the components the
//!   vulnerability is in, nothing of an unrelated artifact), it refuses a
//!   walk that hit its bounds, and it never reads another account.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use hodei_evidence::domain::graph::{NodeKind, RelationType};
use hodei_evidence::{EvidenceId, GraphChangeSet, GraphNode, GraphRelation};
use hodei_evidence_storage::DgraphGraphStore;
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
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

use hodei_evidence::GraphWriter;
use hodei_evidence::ports::graph::GraphReadLimits;
use hodei_reactive::reactions::{ImpactedSubject, SecurityCaseSink};
use hodei_risk_storage::dgraph_cases::DgraphSecurityCaseStore;
use hodei_risk_storage::dgraph_view::blast_radius_dgraph;

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
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/hsp314f3b0"
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

fn node(tenant: &Hrn, kind: NodeKind, kind_text: &str, name: &str) -> GraphNode {
    GraphNode::new(
        hrn_of(tenant, kind_text, name),
        tenant.clone(),
        kind,
        Utc::now(),
    )
}

fn subject(tenant: &Hrn, name: &str, depth: usize) -> ImpactedSubject {
    ImpactedSubject {
        subject: hrn_of(tenant, "Artifact", name).to_string(),
        deployment: Some(hrn_of(tenant, "Deployment", "prod").to_string()),
        depth,
    }
}

const ADVISORY: &str = "GHSA-f3b0-0001";

#[tokio::test]
async fn the_case_is_opened_once_however_many_times_it_is_asked() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = tenant("once");
    let store = DgraphSecurityCaseStore::new(client, tenant.clone());
    let now = Utc::now();

    let first = store
        .open_or_update(ADVISORY, &[subject(&tenant, "app-a", 1)], now)
        .await
        .expect("the case opens");
    assert_eq!(first, format!("case-{ADVISORY}"));

    // Asking again, even with a different radius, gives the case that is
    // there: it is not a second case and it is not overwritten.
    let second = store
        .open_or_update(
            ADVISORY,
            &[subject(&tenant, "app-a", 1), subject(&tenant, "app-b", 2)],
            now,
        )
        .await
        .expect("the case is found");
    assert_eq!(second, first);
    assert_eq!(
        store.of_trigger(ADVISORY).await.expect("cases").len(),
        1,
        "one case for one vulnerability"
    );

    let case = store
        .get(&first)
        .await
        .expect("the read runs")
        .expect("the case is there");
    assert_eq!(case.trigger, ADVISORY);
    assert_eq!(
        case.blast_radius.affected_subjects,
        vec![hrn_of(&tenant, "Artifact", "app-a").to_string()],
        "the first radius stands"
    );
    assert_eq!(case.blast_radius.depths[0].depth, 1);
}

#[tokio::test]
async fn concurrent_reactions_open_one_case() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = tenant("race");
    let store = Arc::new(DgraphSecurityCaseStore::new(client, tenant.clone()));
    let mut tasks = Vec::new();
    for index in 0..6 {
        let store = store.clone();
        let tenant = tenant.clone();
        tasks.push(tokio::spawn(async move {
            store
                .open_or_update(
                    ADVISORY,
                    &[subject(&tenant, &format!("app-{index}"), 1)],
                    Utc::now(),
                )
                .await
        }));
    }
    for task in tasks {
        let id = task.await.expect("joins").expect("every reaction answers");
        assert_eq!(id, format!("case-{ADVISORY}"));
    }
    assert_eq!(
        store.of_trigger(ADVISORY).await.expect("cases").len(),
        1,
        "the race opened ONE case"
    );
}

#[tokio::test]
async fn an_absent_case_is_none_not_an_error() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = tenant("absent");
    let store = DgraphSecurityCaseStore::new(client, tenant);
    assert!(
        store
            .get("case-GHSA-never-opened")
            .await
            .expect("the read runs")
            .is_none()
    );
    assert!(
        store
            .of_trigger("GHSA-never-opened")
            .await
            .expect("the read runs")
            .is_empty()
    );
}

#[tokio::test]
async fn two_accounts_have_two_cases_and_never_see_each_other() {
    let Some(client) = connect().await else {
        return;
    };
    let one = tenant("one");
    let other = tenant("two");
    let store_one = DgraphSecurityCaseStore::new(client.clone(), one.clone());
    let store_other = DgraphSecurityCaseStore::new(client.clone(), other.clone());

    // The SAME advisory reaches both accounts.
    let id_one = store_one
        .open_or_update(ADVISORY, &[subject(&one, "app-one", 1)], Utc::now())
        .await
        .expect("the first account opens");
    let id_other = store_other
        .open_or_update(ADVISORY, &[subject(&other, "app-two", 3)], Utc::now())
        .await
        .expect("the second account opens its own");
    assert_eq!(id_one, id_other, "the id is derived from the vulnerability");

    // Each account reads ITS case, with ITS radius.
    let case_one = store_one.get(&id_one).await.unwrap().expect("case one");
    let case_other = store_other.get(&id_other).await.unwrap().expect("case two");
    assert_eq!(
        case_one.blast_radius.affected_subjects,
        vec![hrn_of(&one, "Artifact", "app-one").to_string()]
    );
    assert_eq!(
        case_other.blast_radius.affected_subjects,
        vec![hrn_of(&other, "Artifact", "app-two").to_string()]
    );
    assert_eq!(case_other.blast_radius.depths[0].depth, 3);

    // The reads by trigger do not cross accounts either.
    let by_trigger_one = store_one.of_trigger(ADVISORY).await.unwrap();
    assert_eq!(by_trigger_one.len(), 1, "only the cases of the account");
    assert_eq!(by_trigger_one[0].blast_radius, case_one.blast_radius);

    // An account that never reacted sees nothing of the others.
    let third = tenant("three");
    let store_third = DgraphSecurityCaseStore::new(client, third);
    assert!(store_third.get(&id_one).await.unwrap().is_none());
    assert!(store_third.of_trigger(ADVISORY).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_subject_of_another_account_is_refused_and_nothing_is_written() {
    let Some(client) = connect().await else {
        return;
    };
    let one = tenant("mine");
    let other = tenant("theirs");
    let store = DgraphSecurityCaseStore::new(client, one.clone());

    let outcome = store
        .open_or_update(
            ADVISORY,
            &[
                subject(&one, "app-mine", 1),
                subject(&other, "app-theirs", 1),
            ],
            Utc::now(),
        )
        .await;
    assert!(outcome.is_err(), "a foreign subject is a leak: {outcome:?}");
    assert!(
        store
            .get(&format!("case-{ADVISORY}"))
            .await
            .unwrap()
            .is_none(),
        "the refusal left no case behind"
    );

    let not_an_hrn = store
        .open_or_update(
            ADVISORY,
            &[ImpactedSubject {
                subject: "not-an-hrn".to_owned(),
                deployment: None,
                depth: 1,
            }],
            Utc::now(),
        )
        .await;
    assert!(not_an_hrn.is_err(), "an unparseable subject is refused");
}

/// Brings the alpha down for the body of the test and ALWAYS brings it
/// back, whatever the body does.
struct StoreOutage {
    container: String,
}

impl StoreOutage {
    fn begin() -> Option<StoreOutage> {
        if std::env::var("HODEI_GRAPH_IT_ALLOW_STORE_FAILURE")
            .ok()?
            .as_str()
            != "1"
        {
            eprintln!("SKIP (failure drill): HODEI_GRAPH_IT_ALLOW_STORE_FAILURE=1 is not set");
            return None;
        }
        let container = std::env::var("HODEI_GRAPH_IT_ALPHA_CONTAINER").ok()?;
        let outcome = std::process::Command::new("podman")
            .args(["stop", "-t", "5", &container])
            .output()
            .expect("podman runs");
        assert!(
            outcome.status.success(),
            "the alpha stopped: {}",
            String::from_utf8_lossy(&outcome.stderr)
        );
        Some(StoreOutage { container })
    }
}

impl Drop for StoreOutage {
    fn drop(&mut self) {
        let outcome = std::process::Command::new("podman")
            .args(["start", &self.container])
            .output()
            .expect("podman runs");
        if !outcome.status.success() {
            eprintln!(
                "CRITICAL: the alpha did NOT start again: {}",
                String::from_utf8_lossy(&outcome.stderr)
            );
        }
    }
}

#[tokio::test]
async fn a_store_that_fails_answers_with_an_error_and_leaves_no_partial_case() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = tenant("outage");
    let store = DgraphSecurityCaseStore::new(client.clone(), tenant.clone());
    let graph = DgraphGraphStore::new(client);
    let id = format!("case-{ADVISORY}");

    let outage = StoreOutage::begin();
    if outage.is_none() {
        return;
    }
    let opened = store
        .open_or_update(ADVISORY, &[subject(&tenant, "app-a", 1)], Utc::now())
        .await;
    assert!(opened.is_err(), "a dead alpha is an error: {opened:?}");
    assert!(
        store.get(&id).await.is_err(),
        "a read of a dead alpha is an error"
    );
    assert!(store.of_trigger(ADVISORY).await.is_err());
    // The blast radius over a dead alpha is an error too, never an empty
    // radius that reads as "nothing is reached".
    let radius = blast_radius_dgraph(
        &graph,
        &tenant,
        &hrn_of(&tenant, "Vulnerability", ADVISORY),
        &GraphReadLimits::default(),
    )
    .await;
    assert!(
        radius.is_err(),
        "a dead alpha is not an empty radius: {radius:?}"
    );
    drop(outage);

    // The alpha comes back: the failed open left NOTHING behind, and the
    // store works again.
    let mut ready = false;
    for _ in 0..60 {
        if store.get(&id).await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    assert!(ready, "the alpha did not come back");
    assert!(store.get(&id).await.unwrap().is_none(), "no partial case");
    let reopened = store
        .open_or_update(ADVISORY, &[subject(&tenant, "app-a", 1)], Utc::now())
        .await
        .expect("the case opens once the store is back");
    assert_eq!(reopened, id);
}

// --- blast radius -------------------------------------------------------

/// The seeded world. Reach, by construction:
///
/// - `lib-vuln` is the component the advisory is about;
/// - `app-direct` holds it (depth 1) and is deployed;
/// - `lib-mid` depends on `lib-vuln`, and `app-transitive` holds `lib-mid`
///   (depth 2);
/// - `app-unrelated` holds `lib-clean` and is NOT reached.
fn seed_world(tenant: &Hrn) -> GraphChangeSet {
    let ev = EvidenceId::new("ev-f3b0");
    let relation = |kind: RelationType, from: Hrn, to: Hrn| {
        GraphRelation::new(kind, from, to, tenant.clone(), ev.clone(), Utc::now())
    };
    let component = |name: &str| hrn_of(tenant, "Component", name);
    let artifact = |name: &str| hrn_of(tenant, "Artifact", name);
    GraphChangeSet::empty()
        .with_cause("evt-f3b0")
        .with_node(node(
            tenant,
            NodeKind::Vulnerability,
            "Vulnerability",
            ADVISORY,
        ))
        .with_node(node(tenant, NodeKind::Component, "Component", "lib-vuln"))
        .with_node(node(tenant, NodeKind::Component, "Component", "lib-mid"))
        .with_node(node(tenant, NodeKind::Component, "Component", "lib-clean"))
        .with_node(node(tenant, NodeKind::Artifact, "Artifact", "app-direct"))
        .with_node(node(
            tenant,
            NodeKind::Artifact,
            "Artifact",
            "app-transitive",
        ))
        .with_node(node(
            tenant,
            NodeKind::Artifact,
            "Artifact",
            "app-unrelated",
        ))
        .with_node(node(tenant, NodeKind::Deployment, "Deployment", "prod"))
        .with_node(node(tenant, NodeKind::Evidence, "Evidence", "ev-f3b0"))
        .with_relation(relation(
            RelationType::AffectedBy,
            component("lib-vuln"),
            hrn_of(tenant, "Vulnerability", ADVISORY),
        ))
        .with_relation(relation(
            RelationType::Contains,
            artifact("app-direct"),
            component("lib-vuln"),
        ))
        .with_relation(relation(
            RelationType::DependsOn,
            component("lib-mid"),
            component("lib-vuln"),
        ))
        .with_relation(relation(
            RelationType::Contains,
            artifact("app-transitive"),
            component("lib-mid"),
        ))
        .with_relation(relation(
            RelationType::Contains,
            artifact("app-unrelated"),
            component("lib-clean"),
        ))
        .with_relation(relation(
            RelationType::DeployedAs,
            artifact("app-direct"),
            hrn_of(tenant, "Deployment", "prod"),
        ))
}

async fn seeded_graph(tenant: &Hrn) -> Option<Arc<DgraphGraphStore>> {
    let client = connect().await?;
    let graph = Arc::new(DgraphGraphStore::new(client));
    graph
        .apply(seed_world(tenant))
        .await
        .expect("the world is written");
    Some(graph)
}

#[tokio::test]
async fn the_blast_radius_is_the_known_reach_with_its_depths() {
    let tenant = tenant("radius");
    let Some(graph) = seeded_graph(&tenant).await else {
        return;
    };
    let radius = blast_radius_dgraph(
        &graph,
        &tenant,
        &hrn_of(&tenant, "Vulnerability", ADVISORY),
        &GraphReadLimits::default(),
    )
    .await
    .expect("the walk answers whole");

    let direct = hrn_of(&tenant, "Artifact", "app-direct").to_string();
    let transitive = hrn_of(&tenant, "Artifact", "app-transitive").to_string();
    let mut expected = vec![direct.clone(), transitive.clone()];
    expected.sort();
    assert_eq!(
        radius.affected_subjects, expected,
        "the unrelated app is out"
    );
    assert_eq!(
        radius.affected_components,
        vec![hrn_of(&tenant, "Component", "lib-vuln").to_string()]
    );
    let depth_of = |name: &str| {
        radius
            .depths
            .iter()
            .find(|entry| entry.subject == name)
            .map(|entry| entry.depth)
    };
    assert_eq!(depth_of(&direct), Some(1), "direct dependency");
    assert_eq!(depth_of(&transitive), Some(2), "transitive dependency");
    assert_eq!(radius.depths.len(), 2);
    assert!(
        !radius.affected_deployments.is_empty(),
        "the deployment is reported"
    );
    for entry in &radius.depths {
        assert_eq!(entry.deployments, radius.affected_deployments);
    }
    let mut sorted = radius
        .depths
        .iter()
        .map(|d| d.subject.clone())
        .collect::<Vec<_>>();
    sorted.sort();
    assert_eq!(
        sorted,
        radius
            .depths
            .iter()
            .map(|d| d.subject.clone())
            .collect::<Vec<_>>(),
        "the depths come out sorted by subject"
    );
}

/// HSP-318 (M4-UAT-03): the same vulnerable artifact deployed to two
/// environments is reported in BOTH, and the risk is not collapsed into a
/// property of the artifact alone: one affected subject, two deployments.
///
/// Regression guard: `blast_radius_dgraph` once filled `affected_deployments`
/// with `row.from` (the ARTIFACT) of each `DeployedAs` relation, so two
/// environments of one artifact collapsed into a single entry that was not a
/// deployment. It now uses `row.to`, like the reference walk in
/// `hodei-risk::traversal`.
#[tokio::test]
async fn the_blast_radius_names_every_environment_the_artifact_runs_in() {
    let tenant = tenant("envs");
    let Some(graph) = seeded_graph(&tenant).await else {
        return;
    };
    let vulnerability = hrn_of(&tenant, "Vulnerability", ADVISORY);
    let direct = hrn_of(&tenant, "Artifact", "app-direct");
    let ev = EvidenceId::new("ev-f3b0");

    let before = blast_radius_dgraph(&graph, &tenant, &vulnerability, &GraphReadLimits::default())
        .await
        .expect("the walk answers whole");
    let prod = hrn_of(&tenant, "Deployment", "prod").to_string();
    assert_eq!(before.affected_deployments, vec![prod.clone()]);

    // The same artifact is also deployed to staging.
    graph
        .apply(
            GraphChangeSet::empty()
                .with_cause("evt-f3b0-staging")
                .with_node(node(&tenant, NodeKind::Deployment, "Deployment", "staging"))
                .with_relation(GraphRelation::new(
                    RelationType::DeployedAs,
                    direct,
                    hrn_of(&tenant, "Deployment", "staging"),
                    tenant.clone(),
                    ev,
                    Utc::now(),
                )),
        )
        .await
        .expect("the second deployment is written");

    let after = blast_radius_dgraph(&graph, &tenant, &vulnerability, &GraphReadLimits::default())
        .await
        .expect("the walk answers whole");
    let staging = hrn_of(&tenant, "Deployment", "staging").to_string();
    let mut expected = vec![prod, staging];
    expected.sort();
    let mut got = after.affected_deployments.clone();
    got.sort();
    assert_eq!(got, expected, "both environments are named");
    assert_eq!(
        after.affected_subjects, before.affected_subjects,
        "the environments are not collapsed into more or fewer subjects"
    );
}

#[tokio::test]
async fn a_walk_that_hits_its_bounds_is_refused_not_returned_partial() {
    let tenant = tenant("bounds");
    let Some(graph) = seeded_graph(&tenant).await else {
        return;
    };
    let vulnerability = hrn_of(&tenant, "Vulnerability", ADVISORY);

    // Depth: the transitive artifact is two levels up.
    let shallow = GraphReadLimits {
        max_depth: 1,
        ..GraphReadLimits::default()
    };
    let refused = blast_radius_dgraph(&graph, &tenant, &vulnerability, &shallow).await;
    assert!(
        refused.is_err(),
        "a depth cut is not an answer: {refused:?}"
    );

    // Edges: the walk meets more relations than the budget admits.
    let narrow = GraphReadLimits {
        max_edges: 1,
        ..GraphReadLimits::default()
    };
    let refused = blast_radius_dgraph(&graph, &tenant, &vulnerability, &narrow).await;
    assert!(
        refused.is_err(),
        "an edge cut is not an answer: {refused:?}"
    );

    // Round trips.
    let few = GraphReadLimits {
        max_round_trips: 1,
        ..GraphReadLimits::default()
    };
    let refused = blast_radius_dgraph(&graph, &tenant, &vulnerability, &few).await;
    assert!(
        refused.is_err(),
        "a round trip cut is not an answer: {refused:?}"
    );

    // The same graph under roomy limits answers.
    let whole = blast_radius_dgraph(&graph, &tenant, &vulnerability, &GraphReadLimits::default())
        .await
        .expect("the roomy walk answers");
    assert_eq!(whole.affected_subjects.len(), 2);
}

#[tokio::test]
async fn another_account_reaches_nothing_of_this_graph() {
    let tenant = tenant("owner");
    let stranger = self::tenant("stranger");
    let Some(graph) = seeded_graph(&tenant).await else {
        return;
    };

    // The stranger asks about the owner's advisory under its own account.
    let radius = blast_radius_dgraph(
        &graph,
        &stranger,
        &hrn_of(&tenant, "Vulnerability", ADVISORY),
        &GraphReadLimits::default(),
    )
    .await
    .expect("the walk runs");
    assert!(radius.affected_subjects.is_empty(), "{radius:?}");
    assert!(radius.affected_components.is_empty());
    assert!(radius.depths.is_empty());

    // And with ITS OWN advisory name over the owner's data: nothing.
    let radius = blast_radius_dgraph(
        &graph,
        &stranger,
        &hrn_of(&stranger, "Vulnerability", ADVISORY),
        &GraphReadLimits::default(),
    )
    .await
    .expect("the walk runs");
    assert!(radius.affected_subjects.is_empty(), "{radius:?}");

    // The owner still sees its own reach: the stranger's walk took nothing.
    let owned = blast_radius_dgraph(
        &graph,
        &tenant,
        &hrn_of(&tenant, "Vulnerability", ADVISORY),
        &GraphReadLimits::default(),
    )
    .await
    .expect("the owner's walk runs");
    assert_eq!(owned.affected_subjects.len(), 2);
}
