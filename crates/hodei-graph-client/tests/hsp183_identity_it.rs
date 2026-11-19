//! HSP-183 integration tests against a REAL local Dgraph server.
//!
//! Environment-gated like `dgraph_it.rs`: without
//! `HODEI_GRAPH_IT_ENDPOINT` the suite skips itself (exit 0), so ordinary
//! hermetic CI stays green. With the variables set (1 Zero + 1 Alpha with
//! TLS and ACL, same topology as HSP-181) the tests below prove the three
//! acceptance criteria:
//!
//! - **AC#1**: two tenants with identical names do not collide; ten
//!   concurrent upserts over the same `(tenant, HRN)` produce exactly one
//!   identity; references reached through native edges preserve the
//!   HRN and the deterministic digest.
//! - **AC#2**: the schema boot is idempotent (second boot applies
//!   nothing), a restart opens on the applied schema, and an
//!   incompatible server (newer stored version, foreign hash, missing
//!   predicate, wrong index) is REFUSED with a typed error — never
//!   opened with a fallback.
//! - **AC#3**: the typed fixture (identities, owner node, groups, native
//!   edges) demonstrates the cardinalities the schema declares: single
//!   edge replaced (1), list edge (N), reverse indexes readable, and the
//!   CAS revision guard of the aggregate.
//!
//! The server state persists between runs (AC#2 needs a restart on an
//! already-applied schema), so every fixture uses run-unique keys and no
//! test drops data.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hodei_graph_client::schema::SchemaBootError;
use hodei_graph_client::{
    BootDecision, CanonicalHrn, GraphClient, GraphClientConfig, GraphMutationBlock,
    GraphMutationPort, GraphUpsertTransaction, IdentityError, IdentityStore, PredicateSpec,
    SCHEMA_VERSION, Tenant, hodei_schema, identity_key, verify_predicates, write_schema_meta,
};

/// The TOML configuration from the environment, or `None` when the
/// real-server run is not requested (the suite skips itself).
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
    Some(
        GraphClientConfig::from_toml_str(&text)
            .expect("the environment configuration must validate"),
    )
}

/// Connects and logs in, or skips the calling test when no server is
/// configured. The connected client is shared by the store and the raw
/// fixture calls.
async fn client_or_skip() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set; the real-server run is gated");
        return None;
    };
    let mut client = GraphClient::connect(config)
        .await
        .expect("connect to the real server");
    client.login().await.expect("the ACL login must succeed");
    Some(Arc::new(client))
}

/// Every gated test shares the one server, and the AC#2 rejection cases
/// tamper with the schema metadata (and restore it). The tests therefore
/// run one at a time: the guard serializes the suite without giving up
/// the per-test isolation of assertions.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A run-unique suffix so fixtures never collide across runs (the server
/// persists; no test drops data).
fn nonce() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock must be set")
        .as_millis();
    format!("h183-{millis}")
}

/// Builds the HRN of the fixture for one tenant and run.
fn fixture_hrn(tenant: &str, name: &str, nonce: &str) -> CanonicalHrn {
    CanonicalHrn::new(&format!(
        "hrn:hodei:identity:local:{tenant}:account/{name}-{nonce}"
    ))
    .expect("the fixture HRN must be valid")
}

/// Counts the nodes that carry a unique key, straight from the graph.
async fn count_nodes_by_key(client: &GraphClient, key: &str) -> usize {
    let query = format!(
        "{{ q(func: eq(hodei.identity.key, \"{}\")) {{ uid }} }}",
        key.replace('"', "\\\"")
    );
    let response = client.execute_dql(&query).await.expect("the count query");
    response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

/// AC#1, first half: two tenants naming their accounts identically do
/// not collide. Each tenant gets its own key, its own digest, and both
/// keep the same display name.
#[tokio::test]
async fn ac1_two_tenants_with_the_same_name_do_not_collide() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let store = IdentityStore::open(client.clone())
        .await
        .expect("the store must open on a validated schema");
    let nonce = nonce();
    let tenant_a = Tenant::new("tenant-a").expect("valid tenant");
    let tenant_b = Tenant::new("tenant-b").expect("valid tenant");
    let hrn_a = fixture_hrn("tenant-a", "alice", &nonce);
    let hrn_b = fixture_hrn("tenant-b", "alice", &nonce);

    let record_a = store
        .ensure_identity(&tenant_a, &hrn_a, "alice")
        .await
        .expect("identity A must be ensured");
    let record_b = store
        .ensure_identity(&tenant_b, &hrn_b, "alice")
        .await
        .expect("identity B must be ensured");

    assert_ne!(
        record_a.key, record_b.key,
        "different tenants, different keys"
    );
    assert_ne!(
        record_a.digest, record_b.digest,
        "different tenants, different digests"
    );
    assert_eq!(record_a.name, "alice");
    assert_eq!(record_b.name, "alice");
    assert_eq!(record_a.tenant, "tenant-a");
    assert_eq!(record_b.tenant, "tenant-b");

    // The keys are exactly the documented derivation.
    assert_eq!(record_a.key, identity_key(&tenant_a, &hrn_a));
    assert_eq!(record_b.key, identity_key(&tenant_b, &hrn_b));

    // Both persist as two separate nodes.
    assert_eq!(
        count_nodes_by_key(&client, &record_a.key).await,
        1,
        "exactly one node for identity A"
    );
    assert_eq!(
        count_nodes_by_key(&client, &record_b.key).await,
        1,
        "exactly one node for identity B"
    );
    eprintln!(
        "ac1: keys {} / {} do not collide",
        record_a.key, record_b.key
    );
}

/// AC#1, second half: ten concurrent `ensure_identity` calls over the
/// SAME `(tenant, HRN)` converge on exactly one identity. The server's
/// `@upsert` index rejects the losers, the losers revalidate and read
/// the winner; no caller observes an error and no duplicate is created.
#[tokio::test]
async fn ac1_ten_concurrent_upserts_create_exactly_one_identity() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let store = Arc::new(
        IdentityStore::open(client.clone())
            .await
            .expect("the store must open on a validated schema"),
    );
    let nonce = nonce();
    let tenant = Tenant::new("tenant-race").expect("valid tenant");
    let hrn = fixture_hrn("tenant-race", "bob", &nonce);

    let expected_key = identity_key(&tenant, &hrn);
    let mut handles = Vec::new();
    for index in 0..10 {
        let store = store.clone();
        let tenant = tenant.clone();
        let hrn = hrn.clone();
        handles.push(tokio::spawn(async move {
            let name = format!("bob-writer-{index}");
            store
                .ensure_identity(&tenant, &hrn, &name)
                .await
                .expect("every concurrent creator must converge, not fail")
        }));
    }
    let mut records = Vec::new();
    for handle in handles {
        records.push(handle.await.expect("the task must not panic"));
    }

    // All ten callers see the same identity.
    let first = &records[0];
    for record in &records {
        assert_eq!(record.key, expected_key, "all callers get the same key");
        assert_eq!(
            record.digest, first.digest,
            "all callers get the same digest"
        );
        assert_eq!(
            record.revision, 0,
            "no concurrent creation advances the revision"
        );
    }
    // Exactly one node exists for the key, straight from the graph.
    assert_eq!(
        count_nodes_by_key(&client, &expected_key).await,
        1,
        "the concurrent upserts must create exactly one identity"
    );
    // A fresh read agrees with the winners.
    let found = store
        .find_identity(&tenant, &hrn)
        .await
        .expect("the read-back must work")
        .expect("the identity must exist");
    assert_eq!(found.digest, first.digest);
    eprintln!(
        "ac1: 10 concurrent upserts over {} produced 1 identity, digest {}",
        expected_key, found.digest
    );
}

/// AC#1, third half: a reference reached through a native edge preserves
/// the HRN and the digest of the referenced identity. The owner node
/// links to the identity; reading the identity from the owner side (the
/// reverse edge) yields the same canonical HRN and the same digest as a
/// direct lookup.
#[tokio::test]
async fn ac1_references_preserve_hrn_and_digest() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let store = IdentityStore::open(client.clone())
        .await
        .expect("the store must open on a validated schema");
    let nonce = nonce();
    let tenant = Tenant::new("tenant-ref").expect("valid tenant");
    let hrn = fixture_hrn("tenant-ref", "carol", &nonce);
    let record = store
        .ensure_identity(&tenant, &hrn, "carol")
        .await
        .expect("the referenced identity");

    // The owner node, typed, unique per run.
    let owner_name = format!("owner-{nonce}");
    client
        .upsert(GraphUpsertTransaction {
            query: format!(
                "{{ o as var(func: eq(hodei.identity.name, \"{}\")) }}",
                owner_name
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(o), 0)",
                format!(
                    "_:o <dgraph.type> \"HodeiOwner\" .\n\
                     _:o <hodei.identity.tenant> \"tenant-ref\" .\n\
                     _:o <hodei.identity.name> \"{owner_name}\" .\n"
                ),
            )],
            commit_now: true,
        })
        .await
        .expect("the owner fixture");

    // The identity points at its owner (native edge, cardinality 1).
    client
        .upsert(GraphUpsertTransaction {
            query: format!(
                "{{ i as var(func: eq(hodei.identity.key, \"{}\")) \
                  o as var(func: eq(hodei.identity.name, \"{}\")) }}",
                record.key.replace('"', "\\\""),
                owner_name
            ),
            blocks: vec![GraphMutationBlock::set(
                "uid(i) <hodei.identity.owner> uid(o) .\n",
            )],
            commit_now: true,
        })
        .await
        .expect("the owner edge");

    // Reading the identity FROM the owner side (reverse edge) preserves
    // the HRN and the digest of the reference target.
    let query = format!(
        "{{ q(func: eq(hodei.identity.name, \"{}\")) {{ \
              ~hodei.identity.owner {{ \
                  hodei.identity.hrn hodei.identity.digest hodei.identity.key \
              }} }} }}",
        owner_name
    );
    let response = client.execute_dql(&query).await.expect("the reverse read");
    let referenced = &response.json["q"][0]["~hodei.identity.owner"][0];
    assert_eq!(
        referenced["hodei.identity.hrn"].as_str(),
        Some(record.hrn.as_str()),
        "the reference preserves the canonical HRN"
    );
    assert_eq!(
        referenced["hodei.identity.digest"].as_str(),
        Some(record.digest.as_str()),
        "the reference preserves the deterministic digest"
    );
    assert_eq!(
        referenced["hodei.identity.key"].as_str(),
        Some(record.key.as_str()),
        "the reference preserves the composite key"
    );
    eprintln!("ac1: the reference through the owner edge preserved HRN and digest");
}

/// AC#2, first half: booting on a fresh graph applies the schema and
/// writes the metadata node; booting again applies NOTHING (idempotent);
/// a restart (a brand-new connection and store) opens on the applied
/// schema with the same version and hash.
#[tokio::test]
async fn ac2_boot_is_idempotent_and_survives_restart() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    // The store may have been opened by the previous tests; booting is
    // idempotent either way, and the FIRST boot of this test (a fresh
    // connection) must see either ApplyFresh (whole server fresh) or
    // AcceptCurrent (schema already applied by another test or run).
    let first = IdentityStore::open(client.clone())
        .await
        .expect("the first boot must succeed");
    let first_decision = first.boot_report().decision.clone();
    eprintln!("ac2: first boot decision: {first_decision:?}");

    // The second boot applies nothing: AcceptCurrent, applied = false.
    let second = IdentityStore::open(client.clone())
        .await
        .expect("the second boot must succeed");
    let report = second.boot_report();
    assert_eq!(report.decision, BootDecision::AcceptCurrent);
    assert!(!report.applied, "an idempotent boot applies nothing");
    assert_eq!(report.binary_version, SCHEMA_VERSION);
    assert_eq!(
        report.verified_predicates,
        hodei_schema().predicates().len(),
        "every required predicate is verified against the live server"
    );

    // The restart: a brand-new connection (no shared state with the
    // previous client) opens on the same applied schema.
    let fresh_client = {
        let config = config_from_env().expect("the environment is set");
        let mut client = GraphClient::connect(config).await.expect("reconnect");
        client.login().await.expect("relogin");
        Arc::new(client)
    };
    let restarted = IdentityStore::open(fresh_client)
        .await
        .expect("the restart must open on the applied schema");
    let report = restarted.boot_report();
    assert_eq!(report.decision, BootDecision::AcceptCurrent);
    assert!(!report.applied);
    // Whatever the first decision was, the stored version now equals the
    // binary version (this is what "reinicio con schema aplicado" means).
    match first_decision {
        BootDecision::ApplyFresh => {}
        BootDecision::AcceptCurrent => {}
        BootDecision::UpgradeFrom { stored } => {
            assert!(stored < SCHEMA_VERSION);
        }
    }
    eprintln!(
        "ac2: restart on the applied schema, {} predicates verified",
        report.verified_predicates
    );
}

/// AC#2, second half: the boot REFUSES an incompatible server.
/// Case by case, with the metadata restored after each:
/// - a stored version NEWER than the binary: refused (downgrade);
/// - a stored hash that does not match the declared schema at the same
///   version: refused (foreign schema);
/// and the readiness verification refuses a live server where a required
/// predicate is missing or its index is wrong (proven with a variant
/// schema against the real server, without corrupting the real one).
#[tokio::test]
async fn ac2_boot_rejects_incompatible_schemas() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let schema = hodei_schema();

    // Make sure the server is at the current version before tampering.
    IdentityStore::open(client.clone())
        .await
        .expect("the baseline boot");

    // Case 1: a stored version newer than this binary.
    write_schema_meta(
        &client,
        schema.version() + 1,
        &schema.hash(),
        "2026-11-19T00:00:00Z",
    )
    .await
    .expect("write the newer metadata");
    let error = IdentityStore::open(client.clone())
        .await
        .expect_err("a newer stored version must be refused");
    match &error {
        IdentityError::Schema(SchemaBootError::NewerThanBinary { stored, binary }) => {
            assert_eq!(*stored, schema.version() + 1);
            assert_eq!(*binary, schema.version());
        }
        other => panic!("unexpected error: {other}"),
    }
    eprintln!("ac2: newer stored version refused: {error}");

    // Case 2: same version, foreign hash.
    write_schema_meta(
        &client,
        schema.version(),
        "deadbeef-foreign-schema",
        "2026-11-19T00:00:00Z",
    )
    .await
    .expect("write the foreign metadata");
    let error = IdentityStore::open(client.clone())
        .await
        .expect_err("a foreign hash must be refused");
    assert!(
        matches!(
            error,
            IdentityError::Schema(SchemaBootError::HashMismatch { .. })
        ),
        "unexpected: {error}"
    );
    eprintln!("ac2: foreign hash refused: {error}");

    // Restore the truthful metadata and prove the store opens again.
    write_schema_meta(
        &client,
        schema.version(),
        &schema.hash(),
        "2026-11-19T00:00:00Z",
    )
    .await
    .expect("restore the metadata");
    IdentityStore::open(client.clone())
        .await
        .expect("after restoring the metadata the store opens again");
    eprintln!("ac2: metadata restored, the store opens again");

    // Case 3: a required predicate missing in the live server. The
    // variant schema expects one predicate the real server does not
    // have; readiness must refuse it without applying anything.
    let mut missing_predicate_schema = schema.clone();
    missing_predicate_schema
        .predicates_mut()
        .push(PredicateSpec {
            name: "hodei.identity.nickname",
            declaration: "hodei.identity.nickname: string @index(exact) .",
            expected_type: "string",
            expected_index: Some("exact"),
            expected_list: false,
            expected_reverse: false,
            expected_upsert: false,
        });
    let error = verify_predicates(&client, &missing_predicate_schema)
        .await
        .expect_err("a missing predicate must be refused");
    assert!(
        matches!(error, SchemaBootError::PredicateMissing { .. }),
        "unexpected: {error}"
    );
    eprintln!("ac2: missing predicate refused: {error}");

    // Case 4: a required index absent in the live server. The real
    // predicate `hodei.identity.key` is indexed `exact`; the variant
    // demands `term`, which the server cannot satisfy.
    let mut wrong_index_schema = hodei_schema();
    let key_spec = wrong_index_schema
        .predicates_mut()
        .iter_mut()
        .find(|spec| spec.name == "hodei.identity.key")
        .expect("the key predicate is declared");
    key_spec.expected_index = Some("term");
    let error = verify_predicates(&client, &wrong_index_schema)
        .await
        .expect_err("a wrong index must be refused");
    assert!(
        matches!(error, SchemaBootError::PredicateIndexMissing { .. }),
        "unexpected: {error}"
    );
    eprintln!("ac2: wrong index refused: {error}");
}

/// AC#3: the typed fixture with the cardinalities the schema declares.
///
/// - single-valued predicates and the single `owner` edge: setting a
///   second value REPLACES the first — the read-back shows exactly one;
/// - the list edge `member_of`: three groups produce exactly three
///   targets;
/// - `@reverse`: the groups see their members from the other side;
/// - the node types are the declared ones (`dgraph.type` read back);
/// - the CAS revision guard: a rename with a stale revision is refused
///   with the current revision; a rename with the right revision
///   commits and advances the revision by one.
#[tokio::test]
async fn ac3_typed_fixture_demonstrates_declared_cardinalities() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let store = IdentityStore::open(client.clone())
        .await
        .expect("the store must open on a validated schema");
    let nonce = nonce();
    let tenant = Tenant::new("tenant-card").expect("valid tenant");
    let hrn = fixture_hrn("tenant-card", "dave", &nonce);
    let record = store
        .ensure_identity(&tenant, &hrn, "dave")
        .await
        .expect("the fixture identity");

    // Three typed group nodes, unique per run.
    let mut group_uids_queries = Vec::new();
    for index in 0..3 {
        let group_name = format!("group-{index}-{nonce}");
        client
            .upsert(GraphUpsertTransaction {
                query: format!("{{ g as var(func: eq(hodei.identity.name, \"{group_name}\")) }}"),
                blocks: vec![GraphMutationBlock::set_if(
                    "eq(len(g), 0)",
                    format!(
                        "_:g <dgraph.type> \"HodeiGroup\" .\n\
                         _:g <hodei.identity.tenant> \"tenant-card\" .\n\
                         _:g <hodei.identity.name> \"{group_name}\" .\n"
                    ),
                )],
                commit_now: true,
            })
            .await
            .expect("the group fixture");
        group_uids_queries.push(group_name);
    }

    // The identity joins the three groups (list edge) and points at its
    // owner (single edge).
    let key_literal = record.key.replace('"', "\\\"");
    let mut group_vars = String::new();
    let mut group_sets = String::new();
    for (index, group_name) in group_uids_queries.iter().enumerate() {
        group_vars.push_str(&format!(
            " g{index} as var(func: eq(hodei.identity.name, \"{group_name}\"))",
        ));
        group_sets.push_str(&format!(
            " uid(i) <hodei.identity.member_of> uid(g{index}) .\n"
        ));
    }
    let owner_name = format!("card-owner-{nonce}");
    client
        .upsert(GraphUpsertTransaction {
            query: format!(
                "{{ i as var(func: eq(hodei.identity.key, \"{key_literal}\")){group_vars} }}"
            ),
            blocks: vec![GraphMutationBlock::set(format!(
                "_:o <dgraph.type> \"HodeiOwner\" .\n\
                     _:o <hodei.identity.tenant> \"tenant-card\" .\n\
                     _:o <hodei.identity.name> \"{owner_name}\" .\n\
                     uid(i) <hodei.identity.owner> _:o .\n{group_sets}"
            ))],
            commit_now: true,
        })
        .await
        .expect("the edge fixture");

    // Read the whole fixture back.
    let query = format!(
        "{{ q(func: eq(hodei.identity.key, \"{key_literal}\")) {{ \
              dgraph.type \
              hodei.identity.hrn \
              hodei.identity.owner {{ dgraph.type hodei.identity.name }} \
              hodei.identity.member_of {{ dgraph.type hodei.identity.name }} \
         }} }}"
    );
    let response = client.execute_dql(&query).await.expect("the fixture read");
    let node = &response.json["q"][0];
    assert_eq!(
        node["dgraph.type"][0].as_str(),
        Some("HodeiIdentity"),
        "the identity node carries its declared type"
    );
    // A single-valued uid edge reads back as ONE object (not a list):
    // that shape IS the cardinality-1 demonstration.
    assert!(
        node["hodei.identity.owner"].is_object(),
        "the owner edge must be single-valued, got: {}",
        node["hodei.identity.owner"]
    );
    assert_eq!(
        node["hodei.identity.owner"]["dgraph.type"][0].as_str(),
        Some("HodeiOwner"),
        "the owner node carries its declared type"
    );
    let members = node["hodei.identity.member_of"]
        .as_array()
        .expect("the list edge is present");
    assert_eq!(
        members.len(),
        3,
        "the list edge carries exactly the three groups"
    );
    for member in members {
        assert_eq!(
            member["dgraph.type"][0].as_str(),
            Some("HodeiGroup"),
            "every group node carries its declared type"
        );
    }

    // The reverse edges: each group sees the identity from its side.
    let group_name = &group_uids_queries[0];
    let reverse_query = format!(
        "{{ q(func: eq(hodei.identity.name, \"{group_name}\")) {{ \
              ~hodei.identity.member_of {{ hodei.identity.key }} }} }}"
    );
    let response = client
        .execute_dql(&reverse_query)
        .await
        .expect("the reverse read");
    let reverse = &response.json["q"][0]["~hodei.identity.member_of"];
    assert_eq!(
        reverse[0]["hodei.identity.key"].as_str(),
        Some(record.key.as_str()),
        "the reverse edge reaches the identity"
    );

    // Cardinality 1 means REPLACE: pointing the identity at a second
    // owner moves the edge instead of adding a second target.
    let second_owner_name = format!("card-owner-2-{nonce}");
    client
        .upsert(GraphUpsertTransaction {
            query: format!("{{ i as var(func: eq(hodei.identity.key, \"{key_literal}\")) }}"),
            blocks: vec![GraphMutationBlock::set(format!(
                "_:o2 <dgraph.type> \"HodeiOwner\" .\n\
                 _:o2 <hodei.identity.tenant> \"tenant-card\" .\n\
                 _:o2 <hodei.identity.name> \"{second_owner_name}\" .\n\
                 uid(i) <hodei.identity.owner> _:o2 .\n"
            ))],
            commit_now: true,
        })
        .await
        .expect("the second owner fixture");
    let response = client
        .execute_dql(&query)
        .await
        .expect("the fixture re-read");
    let node = &response.json["q"][0];
    assert_eq!(
        node["hodei.identity.owner"]["hodei.identity.name"].as_str(),
        Some(second_owner_name.as_str()),
        "the single owner edge was replaced, not appended"
    );

    // The CAS revision guard of the aggregate.
    let stale = store
        .rename_identity(&tenant, &hrn, record.revision + 7, "dave-renamed")
        .await
        .expect_err("a stale revision must be refused");
    assert!(
        matches!(
            &stale,
            IdentityError::CasRejected {
                expected_revision: 7,
                current_revision: 0,
                ..
            }
        ),
        "unexpected: {stale}"
    );
    let renamed = store
        .rename_identity(&tenant, &hrn, record.revision, "dave-renamed")
        .await
        .expect("the rename with the current revision commits");
    assert_eq!(renamed.name, "dave-renamed");
    assert_eq!(
        renamed.revision,
        record.revision + 1,
        "the revision advanced by one"
    );
    let stale_after = store
        .rename_identity(&tenant, &hrn, record.revision, "dave-again")
        .await
        .expect_err("the old revision is stale now");
    assert!(
        matches!(
            stale_after,
            IdentityError::CasRejected {
                current_revision: 1,
                ..
            }
        ),
        "unexpected: {stale_after}"
    );
    eprintln!("ac3: cardinalities 1 and 3, reverse edges and the CAS guard demonstrated");
}
