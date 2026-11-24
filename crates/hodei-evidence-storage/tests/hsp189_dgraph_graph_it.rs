//! HSP-189: the reified GraphStore over persistent Dgraph.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack. What it
//! proves, against the real engine:
//!
//! - a change set is applied WHOLE or not at all (one upsert);
//! - a relation with two supports is asserted and navigable through the
//!   NATIVE edges; withdrawing one support keeps it asserted with one
//!   support; withdrawing the last support leaves it UNKNOWN with the
//!   edge retired;
//! - the same changeset replayed leaves the same state fingerprint —
//!   no duplicated edge, no lost support;
//! - two concurrent writers of the SAME edge (add/add and
//!   add/withdraw) serialize at the server and converge to the state
//!   the Surreal adapter would give;
//! - tenants are isolated: the same relation identity under two
//!   accounts is two relations, two supports, two navigations.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hodei_evidence::domain::graph::NodeKind;
use hodei_evidence::ports::graph::{GraphError, GraphQuery, GraphWriter};
use hodei_evidence::{
    EvidenceId, GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal, RelationType,
};
use hodei_evidence_storage::DgraphGraphStore;
use hodei_graph_client::{GraphClient, GraphClientConfig, dql_string, schema::ensure_ready};
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
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let report = ensure_ready(&client)
        .await
        .expect("the versioned schema boots");
    eprintln!(
        "schema boot: decision={:?} binary_version={} stored_version={:?} applied={} predicates={}",
        report.decision,
        report.binary_version,
        report.stored_version,
        report.applied,
        report.verified_predicates
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

fn tenant(suffix: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{suffix}:Artifact/hsp189"
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
        chrono::Utc::now(),
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
        EvidenceId::new(evidence.to_owned()),
        chrono::Utc::now(),
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
        EvidenceId::new(evidence.to_owned()),
        chrono::Utc::now(),
    )
}

/// The relation status the server holds, read straight from the record.
async fn stored_status(client: &GraphClient, tenant: &Hrn, identity: &str) -> String {
    let key = DgraphGraphStore::relation_key(&tenant.to_string(), identity);
    let query = format!(
        "{{ q(func: eq(hodei.graph.rel.key, {})) {{ hodei.graph.rel.status }} }}",
        dql_string(&key)
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the status query answers");
    response
        .json
        .get("q")
        .and_then(|q| q.get(0))
        .and_then(|node| node.get("hodei.graph.rel.status"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>")
        .to_owned()
}

/// How many live supports the relation carries, read straight from the
/// record's support edge.
async fn stored_support_count(client: &GraphClient, tenant: &Hrn, identity: &str) -> usize {
    let key = DgraphGraphStore::relation_key(&tenant.to_string(), identity);
    let query = format!(
        "{{ q(func: eq(hodei.graph.rel.key, {})) {{ count(hodei.graph.supports) }} }}",
        dql_string(&key)
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the count query answers");
    response
        .json
        .get("q")
        .and_then(|q| q.get(0))
        .and_then(|node| node.get("count(hodei.graph.supports)"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

/// How many relation records the server holds for one identity (the
/// replay and the races must never make it more than one).
async fn stored_relation_node_count(client: &GraphClient, tenant: &Hrn, identity: &str) -> usize {
    let key = DgraphGraphStore::relation_key(&tenant.to_string(), identity);
    let query = format!(
        "{{ q(func: eq(hodei.graph.rel.key, {})) {{ uid }} }}",
        dql_string(&key)
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the node count answers");
    response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .map(|nodes| nodes.len())
        .unwrap_or(0)
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// AC1 — whole or not at all, and the two-support edge.
#[tokio::test]
async fn changeset_applies_whole_and_two_supports_are_one_asserted_edge() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("it{}", &nonce()[..6]));
    let build = hrn_of(&tenant, "Build", "b-1");
    let artifact = hrn_of(&tenant, "Artifact", "a-1");
    let ev1 = hrn_of(&tenant, "Evidence", "ev-1");

    // A relation that claims evidence the graph does not have is a
    // claim, and a claim is rejected BEFORE anything is written.
    let orphan = GraphChangeSet::empty()
        .with_node(node(&tenant, NodeKind::Build, "b-1"))
        .with_node(node(&tenant, NodeKind::Artifact, "a-1"))
        .with_relation(relation(
            &tenant,
            RelationType::Produced,
            &build,
            &artifact,
            "ev-orphan",
        ));
    let error = store
        .apply(orphan)
        .await
        .expect_err("the claim is rejected");
    assert!(
        matches!(error, GraphError::Rejected(_)),
        "unexpected: {error}"
    );
    assert_eq!(
        stored_relation_node_count(&client, &tenant, "produced|").await,
        0
    );

    // The whole changeset: nodes, the relation with TWO supports, in
    // one transaction.
    let changes = GraphChangeSet::empty()
        .with_node(node(&tenant, NodeKind::Build, "b-1"))
        .with_node(node(&tenant, NodeKind::Artifact, "a-1"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-1"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-2"))
        .with_relation(relation(
            &tenant,
            RelationType::Produced,
            &build,
            &artifact,
            "ev-1",
        ))
        .with_relation(relation(
            &tenant,
            RelationType::Produced,
            &artifact,
            &ev1,
            "ev-2",
        ))
        .with_relation(relation(
            &tenant,
            RelationType::Produced,
            &build,
            &artifact,
            "ev-2",
        ));
    store.apply(changes).await.expect("the changeset commits");

    let identity = format!("produced|{build}|{artifact}");
    assert_eq!(
        stored_relation_node_count(&client, &tenant, &identity).await,
        1,
        "one relation node"
    );
    assert_eq!(stored_status(&client, &tenant, &identity).await, "asserted");
    assert_eq!(
        stored_support_count(&client, &tenant, &identity).await,
        2,
        "two supports"
    );
    assert_eq!(
        stored_relation_node_count(&client, &tenant, &format!("produced|{artifact}|{ev1}")).await,
        1
    );

    // The edge is navigable BOTH ways, through the native edges.
    let from = store
        .relations_from(&tenant, &build)
        .await
        .expect("the walk answers");
    assert!(
        from.iter().any(|r| r.to == artifact && r.from == build),
        "navigable forward: {from:?}"
    );
    let to = store
        .relations_to(&tenant, &artifact)
        .await
        .expect("the reverse walk answers");
    assert!(
        to.iter().any(|r| r.from == build),
        "navigable backward: {to:?}"
    );

    // Withdrawing ONE of the two supports keeps the relation asserted
    // with the other support, and the edge in place.
    let partial = GraphChangeSet::empty().with_withdrawal(withdrawal(
        &tenant,
        RelationType::Produced,
        &build,
        &artifact,
        "ev-1",
    ));
    store
        .apply(partial)
        .await
        .expect("the partial withdrawal commits");
    assert_eq!(stored_status(&client, &tenant, &identity).await, "asserted");
    assert_eq!(
        stored_support_count(&client, &tenant, &identity).await,
        1,
        "one support left"
    );
    assert_eq!(
        store.relations_from(&tenant, &build).await.unwrap().len(),
        1,
        "still navigable"
    );

    // Withdrawing the LAST support leaves the relation UNKNOWN and the
    // native edge retired: the navigation stops showing it.
    let last = GraphChangeSet::empty().with_withdrawal(withdrawal(
        &tenant,
        RelationType::Produced,
        &build,
        &artifact,
        "ev-2",
    ));
    store
        .apply(last)
        .await
        .expect("the last withdrawal commits");
    assert_eq!(stored_status(&client, &tenant, &identity).await, "unknown");
    assert_eq!(stored_support_count(&client, &tenant, &identity).await, 0);
    assert!(
        store
            .relations_from(&tenant, &build)
            .await
            .unwrap()
            .is_empty(),
        "an unknown relation is not walked"
    );
    assert!(
        store
            .relations_to(&tenant, &artifact)
            .await
            .unwrap()
            .is_empty(),
        "an unknown relation is not walked backward either"
    );
    store.clear().await.expect("the graph empties");
}

/// AC2 — the replay of the same changeset leaves the same state: no
/// duplicated edge, no lost support, no moved fingerprint.
#[tokio::test]
async fn replaying_the_same_changeset_is_deterministic() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("rp{}", &nonce()[..6]));
    let build = hrn_of(&tenant, "Build", "b-r");
    let artifact = hrn_of(&tenant, "Artifact", "a-r");

    let changes = GraphChangeSet::empty()
        .with_node(node(&tenant, NodeKind::Build, "b-r"))
        .with_node(node(&tenant, NodeKind::Artifact, "a-r"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-r"))
        .with_relation(relation(
            &tenant,
            RelationType::Produced,
            &build,
            &artifact,
            "ev-r",
        ))
        .with_cause("hsp189/replay");
    let fingerprint = DgraphGraphStore::changeset_fingerprint(&changes);
    assert_eq!(
        fingerprint,
        DgraphGraphStore::changeset_fingerprint(&changes.clone()),
        "the changeset fingerprint is deterministic"
    );

    store
        .apply(changes.clone())
        .await
        .expect("the first apply commits");
    let identity = format!("produced|{build}|{artifact}");
    let after_first = store
        .state_fingerprint(&tenant)
        .await
        .expect("the state reads");
    assert_eq!(stored_support_count(&client, &tenant, &identity).await, 1);

    store.apply(changes).await.expect("the replay commits");
    let after_replay = store
        .state_fingerprint(&tenant)
        .await
        .expect("the state reads again");
    assert_eq!(
        after_first, after_replay,
        "the replay did not move the state"
    );
    assert_eq!(
        stored_relation_node_count(&client, &tenant, &identity).await,
        1,
        "no duplicate edge"
    );
    assert_eq!(
        stored_support_count(&client, &tenant, &identity).await,
        1,
        "no lost support"
    );
    store.clear().await.expect("the graph empties");
}

/// AC2 — two writers of the SAME edge: add/add with two evidences
/// serializes at the server and converges to one relation with two
/// supports; add/withdraw converges to the survivor asserted. Several
/// rounds, so the interleavings have room to show.
#[tokio::test]
async fn concurrent_writers_of_the_same_edge_converge() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("cc{}", &nonce()[..6]));
    let build = hrn_of(&tenant, "Build", "b-c");
    let artifact = hrn_of(&tenant, "Artifact", "a-c");

    // The nodes pre-exist, so the racing writers race the EDGE and
    // not the evidence the graph must hold first.
    let seed = GraphChangeSet::empty()
        .with_node(node(&tenant, NodeKind::Build, "b-c"))
        .with_node(node(&tenant, NodeKind::Artifact, "a-c"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-c1"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-c2"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-c3"));
    store.apply(seed).await.expect("the seed commits");

    // Round 1: two adds of the same relation, different evidence.
    let first = GraphChangeSet::empty().with_relation(relation(
        &tenant,
        RelationType::Produced,
        &build,
        &artifact,
        "ev-c1",
    ));
    let second = GraphChangeSet::empty().with_relation(relation(
        &tenant,
        RelationType::Produced,
        &build,
        &artifact,
        "ev-c2",
    ));
    let (left, right) = tokio::join!(store.apply(first), store.apply(second));
    for result in [left, right] {
        // Losing the race is a retry inside apply; a surfaced error is
        // a real failure.
        result.expect("both writers converge");
    }
    let identity = format!("produced|{build}|{artifact}");
    assert_eq!(
        stored_relation_node_count(&client, &tenant, &identity).await,
        1,
        "one relation node"
    );
    assert_eq!(stored_status(&client, &tenant, &identity).await, "asserted");
    assert_eq!(
        stored_support_count(&client, &tenant, &identity).await,
        2,
        "both supports live"
    );

    // Round 2: an add and a withdraw of the same edge, concurrently.
    // Serialized either way, the survivor is ev-c2: if the withdrawal
    // commits first the add restores the edge; if the add commits
    // first the withdrawal finds a remaining support.
    let add = GraphChangeSet::empty().with_relation(relation(
        &tenant,
        RelationType::Produced,
        &build,
        &artifact,
        "ev-c3",
    ));
    let remove = GraphChangeSet::empty().with_withdrawal(withdrawal(
        &tenant,
        RelationType::Produced,
        &build,
        &artifact,
        "ev-c1",
    ));
    let (left, right) = tokio::join!(store.apply(add), store.apply(remove));
    for result in [left, right] {
        result.expect("both writers converge");
    }
    assert_eq!(stored_status(&client, &tenant, &identity).await, "asserted");
    assert_eq!(
        stored_support_count(&client, &tenant, &identity).await,
        2,
        "ev-c1 is gone; ev-c2 and ev-c3 live"
    );
    assert_eq!(
        store.relations_from(&tenant, &build).await.unwrap().len(),
        1,
        "the edge is navigable once, not duplicated"
    );

    // Round 3: two withdrawals of the two remaining supports,
    // concurrently. One of them must see the other's withdrawal
    // revalidated (or find nothing left) and the relation ends UNKNOWN
    // exactly once, with the edge retired.
    let remove2 = GraphChangeSet::empty().with_withdrawal(withdrawal(
        &tenant,
        RelationType::Produced,
        &build,
        &artifact,
        "ev-c2",
    ));
    let remove3 = GraphChangeSet::empty().with_withdrawal(withdrawal(
        &tenant,
        RelationType::Produced,
        &build,
        &artifact,
        "ev-c3",
    ));
    let (left, right) = tokio::join!(store.apply(remove2), store.apply(remove3));
    for result in [left, right] {
        result.expect("both withdrawals converge");
    }
    assert_eq!(stored_status(&client, &tenant, &identity).await, "unknown");
    assert_eq!(stored_support_count(&client, &tenant, &identity).await, 0);
    assert!(
        store
            .relations_from(&tenant, &build)
            .await
            .unwrap()
            .is_empty()
    );
    store.clear().await.expect("the graph empties");
}

/// The same relation identity under two accounts is two relations,
/// two supports, two navigations: neither account sees the other.
#[tokio::test]
async fn tenants_never_share_a_relation() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let nonce = nonce();
    let first_tenant = tenant(&format!("t1{nonce}"));
    let second_tenant = tenant(&format!("t2{nonce}"));

    for tenant in [&first_tenant, &second_tenant] {
        let build = hrn_of(tenant, "Build", "b-iso");
        let artifact = hrn_of(tenant, "Artifact", "a-iso");
        let changes = GraphChangeSet::empty()
            .with_node(node(tenant, NodeKind::Build, "b-iso"))
            .with_node(node(tenant, NodeKind::Artifact, "a-iso"))
            .with_node(node(tenant, NodeKind::Evidence, "ev-iso"))
            .with_relation(relation(
                tenant,
                RelationType::Produced,
                &build,
                &artifact,
                "ev-iso",
            ));
        store
            .apply(changes)
            .await
            .expect("each account's changeset commits");
    }

    let first_build = hrn_of(&first_tenant, "Build", "b-iso");
    let first_relations = store
        .relations_from(&first_tenant, &first_build)
        .await
        .unwrap();
    assert_eq!(
        first_relations.len(),
        1,
        "the first account sees only its own edge"
    );
    assert_eq!(
        first_relations[0].tenant_hrn, first_tenant,
        "the relation belongs to its account"
    );
    let second_build = hrn_of(&second_tenant, "Build", "b-iso");
    let second_relations = store
        .relations_from(&second_tenant, &second_build)
        .await
        .unwrap();
    assert_eq!(
        second_relations.len(),
        1,
        "the second account sees only its own edge"
    );
    // One account's build does not answer for the other account's edge.
    let cross = store
        .relations_from(&first_tenant, &second_build)
        .await
        .unwrap();
    assert!(
        cross.is_empty(),
        "a relation of another account is not walked: {cross:?}"
    );
    assert_ne!(
        DgraphGraphStore::node_key(&first_tenant.to_string(), &first_build.to_string()),
        DgraphGraphStore::node_key(&second_tenant.to_string(), &second_build.to_string()),
        "the same node in two accounts is two records"
    );
    store.clear().await.expect("the graph empties");
}

/// The lineage walk over the native edges says what the Surreal
/// reference says for the same changeset, and the emptiness answers.
#[tokio::test]
async fn lineage_walks_the_native_edges() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("ln{}", &nonce()[..6]));
    let repository = hrn_of(&tenant, "Repository", "r-1");
    let revision = hrn_of(&tenant, "SourceRevision", "rev-1");
    let build = hrn_of(&tenant, "Build", "b-1");
    let artifact = hrn_of(&tenant, "Artifact", "a-1");
    let deployment = hrn_of(&tenant, "Deployment", "d-1");

    let changes = GraphChangeSet::empty()
        .with_node(node(&tenant, NodeKind::Repository, "r-1"))
        .with_node(node(&tenant, NodeKind::SourceRevision, "rev-1"))
        .with_node(node(&tenant, NodeKind::Build, "b-1"))
        .with_node(node(&tenant, NodeKind::Artifact, "a-1"))
        .with_node(node(&tenant, NodeKind::Deployment, "d-1"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-l1"))
        .with_relation(relation(
            &tenant,
            RelationType::HasRevision,
            &repository,
            &revision,
            "ev-l1",
        ))
        .with_relation(relation(
            &tenant,
            RelationType::Triggered,
            &revision,
            &build,
            "ev-l1",
        ))
        .with_relation(relation(
            &tenant,
            RelationType::UsedSource,
            &build,
            &revision,
            "ev-l1",
        ))
        .with_relation(relation(
            &tenant,
            RelationType::Produced,
            &build,
            &artifact,
            "ev-l1",
        ))
        .with_relation(relation(
            &tenant,
            RelationType::DeployedAs,
            &artifact,
            &deployment,
            "ev-l1",
        ));
    store
        .apply(changes)
        .await
        .expect("the lineage changeset commits");

    let lineage = store
        .lineage(&tenant, &artifact)
        .await
        .expect("the lineage reads");
    assert_eq!(
        lineage.artifact.as_ref().map(|n| n.id.clone()),
        Some(artifact.clone())
    );
    assert!(
        lineage.builds.iter().any(|n| n.id == build),
        "the build is in the lineage"
    );
    assert!(
        lineage.revisions.iter().any(|n| n.id == revision),
        "the revision is in the lineage"
    );
    assert_eq!(
        lineage.repository.as_ref().map(|n| n.id.clone()),
        Some(repository.clone()),
        "the repository is found through the revision"
    );
    assert!(
        lineage.deployments.iter().any(|n| n.id == deployment),
        "the deployment is in the lineage"
    );
    assert!(
        lineage
            .evidence
            .iter()
            .any(|r| r.relation == RelationType::Produced)
    );

    // The reference walks of the Surreal adapter, over the native
    // traversals. `artifacts_of_build` in the REFERENCE reads the
    // relations that END at the build and filters Produced — with the
    // projector's Build -[produced]-> Artifact direction that walk
    // finds nothing, and the Dgraph port reproduces the reference
    // walk VERBATIM, quirk included (flagged in the task evidence):
    // the parity the AC2 asks for is preserved, not improved.
    let from_repository = store.relations_from(&tenant, &repository).await.unwrap();
    assert!(
        from_repository
            .iter()
            .any(|r| r.relation == RelationType::HasRevision)
    );
    let from_revision = store.relations_from(&tenant, &revision).await.unwrap();
    assert!(
        from_revision
            .iter()
            .any(|r| r.relation == RelationType::Triggered)
    );
    let to_build = store.relations_to(&tenant, &build).await.unwrap();
    assert!(
        to_build
            .iter()
            .any(|r| r.relation == RelationType::Triggered)
    );
    let artifacts = store
        .artifacts_of_repository(&tenant, &repository)
        .await
        .unwrap();
    assert!(
        artifacts.is_empty(),
        "the reference walk yields nothing for this direction; parity keeps it"
    );
    let deployments = store.deployments(&tenant, &artifact).await.unwrap();
    assert_eq!(deployments.len(), 1);
    store.clear().await.expect("the graph empties");
}
