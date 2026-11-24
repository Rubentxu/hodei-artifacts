//! HSP-189 design probe: the upsert mechanics the graph store needs.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack. This
//! probe settles, against the real server, the two behaviors the
//! changeset upsert leans on:
//!
//! 1. blank node labels used in DIFFERENT mutation blocks of ONE upsert
//!    refer to the same node (co-created entities can be linked);
//! 2. a write of an edge over the same (uid, predicate) key by two
//!    concurrent transactions aborts the loser (per-node serialization).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hodei_graph_client::{
    GraphClient, GraphClientConfig, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
    dql_string,
};

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
    client
        .alter_schema(
            "probe.key: string @index(exact) .\nprobe.name: string .\nprobe.link: [uid] @reverse .\n",
        )
        .await
        .expect("the probe predicates are declared");
    Some(Arc::new(client))
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// Probe 1: blank labels across blocks of one upsert unify. Node `a` is
/// created in block 1, node `b` in block 2, and the edge from `a` to `b`
/// is written in block 2 using `_:a`. If blanks unify, the committed
/// graph has one `a` and one `b`, and the edge exists.
#[tokio::test]
async fn blank_labels_unify_across_upsert_blocks() {
    let Some(client) = connect().await else {
        return;
    };
    let stamp = nonce();
    let key_a = format!("probe:{stamp}:a");
    let key_b = format!("probe:{stamp}:b");
    let ka = dql_string(&key_a);
    let kb = dql_string(&key_b);
    let transaction = GraphUpsertTransaction {
        query: format!(
            "{{ a as var(func: eq(probe.key, {ka}))\n b as var(func: eq(probe.key, {kb})) }}"
        ),
        blocks: vec![
            GraphMutationBlock::set_if(
                "eq(len(a), 0)",
                format!(
                    "_:a <dgraph.type> \"ProbeNode\" .\n _:a <probe.key> {ka} .\n _:a <probe.name> \"a\" .\n"
                ),
            ),
            GraphMutationBlock::set_if(
                "eq(len(b), 0)",
                format!(
                    "_:b <dgraph.type> \"ProbeNode\" .\n _:b <probe.key> {kb} .\n _:b <probe.name> \"b\" .\n\
                     _:b <probe.link> _:a .\n"
                ),
            ),
        ],
        commit_now: true,
    };
    client
        .upsert(transaction)
        .await
        .expect("the upsert commits");

    // Replay: the same upsert again must be a no-op, not a duplicate.
    let transaction = GraphUpsertTransaction {
        query: format!(
            "{{ a as var(func: eq(probe.key, {ka}))\n b as var(func: eq(probe.key, {kb})) }}"
        ),
        blocks: vec![
            GraphMutationBlock::set_if("eq(len(a), 0)", format!("_:a <probe.key> {ka} .\n")),
            GraphMutationBlock::set_if(
                "eq(len(b), 0)",
                format!("_:b <probe.key> {kb} .\n _:b <probe.link> _:a .\n"),
            ),
        ],
        commit_now: true,
    };
    client
        .upsert(transaction)
        .await
        .expect("the replay commits");

    let response = client
        .execute_dql(&format!(
            "{{ a(func: eq(probe.key, {ka})) {{ probe.key count(~probe.link) uid }} \
               b(func: eq(probe.key, {kb})) {{ probe.key probe.link {{ probe.key }} }} }}"
        ))
        .await
        .expect("the verification query answers");
    let a_nodes = response
        .json
        .get("a")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let b_nodes = response
        .json
        .get("b")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(a_nodes.len(), 1, "exactly one node 'a': {a_nodes:?}");
    assert_eq!(b_nodes.len(), 1, "exactly one node 'b': {b_nodes:?}");
    let b = &b_nodes[0];
    let linked = b
        .get("probe.link")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(linked.len(), 1, "the edge from 'b' to 'a' exists: {b:?}");
    assert_eq!(
        linked[0]
            .get("probe.key")
            .and_then(serde_json::Value::as_str),
        Some(key_a.as_str()),
        "the edge points at the co-created 'a'"
    );
    // The reverse counts how many nodes link at 'a': the replay must not
    // have duplicated the edge.
    let incoming = a_nodes[0]
        .get("count(~probe.link)")
        .and_then(serde_json::Value::as_u64);
    assert_eq!(incoming, Some(1), "the replay did not duplicate the edge");

    // Cleanup.
    let _ = client
        .upsert(GraphUpsertTransaction {
            query: format!(
                "{{ q(func: eq(probe.key, {ka})) {{ uid }}\n r(func: eq(probe.key, {kb})) {{ uid }} }}"
            ),
            blocks: vec![GraphMutationBlock {
                cond: None,
                set_nquads: String::new(),
                delete_nquads: "uid(q) * * .\nuid(r) * * .\n".to_owned(),
            }],
            commit_now: true,
        })
        .await;
}

/// Probe 2: two concurrent transactions writing the same scalar predicate
/// of the same node: the loser is aborted by the server (write-write
/// conflict on the data key), which is the per-node serialization the
/// relation status relies on.
#[tokio::test]
async fn concurrent_writes_of_one_predicate_conflict() {
    let Some(client) = connect().await else {
        return;
    };
    let stamp = nonce();
    let key = format!("probe:{stamp}:n");
    let k = dql_string(&key);
    client
        .upsert(GraphUpsertTransaction {
            query: format!("{{ n as var(func: eq(probe.key, {k})) }}"),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(n), 0)",
                format!("_:n <probe.key> {k} .\n_:n <probe.name> \"seed\" .\n"),
            )],
            commit_now: true,
        })
        .await
        .expect("the seed commits");

    // Two transactions, each binds the node and writes a different value.
    let build = |value: &str| GraphUpsertTransaction {
        query: format!("{{ n as var(func: eq(probe.key, {k})) }}"),
        blocks: vec![GraphMutationBlock::set(format!(
            "uid(n) <probe.name> {} .\n",
            dql_string(value)
        ))],
        commit_now: true,
    };
    let (first, second) = tokio::join!(
        client.upsert(build("writer-one")),
        client.upsert(build("writer-two"))
    );
    let first_conflict = matches!(
        first,
        Err(hodei_graph_client::GraphClientError::UpsertConflict { .. })
    );
    let second_conflict = matches!(
        second,
        Err(hodei_graph_client::GraphClientError::UpsertConflict { .. })
    );
    assert!(
        first_conflict || second_conflict,
        "at least one concurrent writer must be aborted: {first:?} {second:?}"
    );
    // The server serializes: one of the two values is the final one.
    let response = client
        .execute_dql(&format!(
            "{{ q(func: eq(probe.key, {k})) {{ probe.name }} }}"
        ))
        .await
        .expect("the read answers");
    let name = response
        .json
        .get("q")
        .and_then(|q| q.get(0))
        .and_then(|n| n.get("probe.name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();
    assert!(
        name == "writer-one" || name == "writer-two",
        "the final value is one of the two writers: {name}"
    );

    let _ = client
        .upsert(GraphUpsertTransaction {
            query: format!("{{ q(func: eq(probe.key, {k})) {{ uid }} }}"),
            blocks: vec![GraphMutationBlock {
                cond: None,
                set_nquads: String::new(),
                delete_nquads: "uid(q) * * .\n".to_owned(),
            }],
            commit_now: true,
        })
        .await;
}

/// HSP-189 diagnosis: what does an edge written through blank labels in
/// a third conditional block really land on?
#[tokio::test]
async fn diagnose_edge_through_three_way_blanks() {
    let Some(client) = connect().await else {
        return;
    };
    let stamp = nonce();
    let ka = dql_string(&format!("diag:{stamp}:a"));
    let kb = dql_string(&format!("diag:{stamp}:b"));
    let kr = dql_string(&format!("diag:{stamp}:r"));
    // Block 0: create a. Block 1: create b. Block 2: create r and the
    // two edges a->r and r->b, all through blank labels.
    let transaction = GraphUpsertTransaction {
        query: format!(
            "{{ a as var(func: eq(probe.key, {ka}))\n b as var(func: eq(probe.key, {kb}))\n r as var(func: eq(probe.key, {kr})) }}"
        ),
        blocks: vec![
            GraphMutationBlock::set_if("eq(len(a), 0)", format!("_:a <probe.key> {ka} .\n")),
            GraphMutationBlock::set_if("eq(len(b), 0)", format!("_:b <probe.key> {kb} .\n")),
            GraphMutationBlock::set_if(
                "eq(len(r), 0) AND eq(len(a), 0) AND eq(len(b), 0)",
                format!("_:r <probe.key> {kr} .\n_:r <probe.link> _:b .\n_:a <probe.link> _:r .\n"),
            ),
        ],
        commit_now: true,
    };
    client
        .upsert(transaction)
        .await
        .expect("the upsert commits");
    let response = client
        .execute_dql(&format!(
            "{{ a(func: eq(probe.key, {ka})) {{ count(probe.link) }} \
               r(func: eq(probe.key, {kr})) {{ count(~probe.link) count(probe.link) }} \
               nodes(func: type(ProbeNode)) {{ count(uid) }} }}"
        ))
        .await
        .expect("the verification answers");
    eprintln!("DIAG: {}", response.json);
    let _ = client
        .upsert(GraphUpsertTransaction {
            query: format!("{{ q(func: eq(probe.key, {ka})) {{ uid }}\n r2(func: eq(probe.key, {kb})) {{ uid }}\n r3(func: eq(probe.key, {kr})) {{ uid }} }}"),
            blocks: vec![GraphMutationBlock { cond: None, set_nquads: String::new(), delete_nquads: "uid(q) * * .\nuid(r2) * * .\nuid(r3) * * .\n".to_owned() }],
            commit_now: true,
        })
        .await;
}
