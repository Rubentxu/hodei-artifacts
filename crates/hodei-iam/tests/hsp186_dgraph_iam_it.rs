//! HSP-186 integration tests: IAM over the REAL remote Dgraph authority.
//!
//! Environment-gated like `hsp184_guards_it.rs`: without
//! `HODEI_GRAPH_IT_ENDPOINT` the suite skips itself (exit 0), so ordinary
//! hermetic CI stays green. With the variables set (1 Zero + 1 Alpha of
//! Dgraph v25.5.1 with TLS and ACL, endpoint WITHOUT scheme) the tests
//! prove the two acceptance criteria of HSP-186 against the server the
//! ports actually talk to:
//!
//! - **AC#1**: the policy/user/group CRUD round-trips over the remote
//!   graph with the Surreal semantics (scoped `{account}~{short}` ids,
//!   unscoped HRNs in the account listing, pagination and totals decided
//!   inside the server); account isolation is structural — another
//!   account's policies are not in the candidate set at all; a Cedar
//!   authorization evaluated through `EvaluateIamPoliciesUseCase`
//!   permits exactly the attached principal and denies everyone else;
//!   and an unreachable authority fails closed — the adapter never
//!   exists, so nothing can be permitted.
//! - **AC#2**: activating a policy under `expected_revision` is a CAS:
//!   the loser of a genuine two-connection race gets the typed
//!   `VersionConflict` (never a phantom not-found), the revision
//!   advances exactly once, and everything — policies, schema,
//!   revisions, marks — survives into a brand-new connection.
//!
//! The server state persists between runs, so every fixture uses
//! run-unique accounts and ids and no test drops data.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_policies::build_schema::error::BuildSchemaError;
use hodei_policies::build_schema::ports::SchemaStoragePort;
use kernel::Hrn;

use hodei_iam::features::add_user_to_group::dto::UserPersistenceDto as AddUserPersistenceDto;
use hodei_iam::features::add_user_to_group::ports::{GroupFinder, UserFinder, UserGroupPersister};
use hodei_iam::features::create_group::dto::GroupPersistenceDto;
use hodei_iam::features::create_group::ports::CreateGroupPort;
use hodei_iam::features::create_policy::dto::CreatePolicyCommand;
use hodei_iam::features::create_policy::error::CreatePolicyError;
use hodei_iam::features::create_policy::ports::CreatePolicyPort;
use hodei_iam::features::create_user::dto::UserPersistenceDto as CreateUserPersistenceDto;
use hodei_iam::features::create_user::ports::CreateUserPort;
use hodei_iam::features::delete_policy::error::DeletePolicyError;
use hodei_iam::features::delete_policy::ports::DeletePolicyPort;
use hodei_iam::features::evaluate_iam_policies::adapter::HrnEntityResolver;
use hodei_iam::features::evaluate_iam_policies::dto::EvaluateIamPoliciesCommand;
use hodei_iam::features::evaluate_iam_policies::ports::{
    EvaluateIamPoliciesPort, PolicyFinderPort as IamEvaluatePolicyFinderPort,
};
use hodei_iam::features::evaluate_iam_policies::use_case::EvaluateIamPoliciesUseCase;
use hodei_iam::features::get_effective_policies::ports::{
    GroupFinderPort, PolicyFinderPort as EffectivePolicyFinderPort,
};
use hodei_iam::features::get_policy::ports::PolicyReader;
use hodei_iam::features::list_policies::dto::ListPoliciesQuery;
use hodei_iam::features::list_policies::ports::PolicyLister;
use hodei_iam::features::update_policy::dto::{
    PolicyView as UpdatePolicyView, UpdatePolicyCommand,
};
use hodei_iam::features::update_policy::error::UpdatePolicyError;
use hodei_iam::features::update_policy::ports::UpdatePolicyPort;
use hodei_iam::infrastructure::DgraphIamAdapter;

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

/// Connects, logs in (the JWT the platform authorizes with) and opens the
/// adapter through the schema boot gate — or skips the calling test when
/// no server is configured. Every call makes a NEW connection: the race
/// and restart tests need genuinely separate clients.
async fn adapter_or_skip() -> Option<Arc<DgraphIamAdapter>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set; the real-server run is gated");
        return None;
    };
    let mut client = GraphClient::connect(config)
        .await
        .expect("connect to the real server");
    client.login().await.expect("the ACL login must succeed");
    let adapter = DgraphIamAdapter::open(Arc::new(client))
        .await
        .expect("the schema boot must accept the server");
    Some(Arc::new(adapter))
}

/// Every gated test shares the one server, so the suite runs one test at
/// a time (the boot is idempotent, but serializing keeps the fixtures
/// independent).
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A run-unique suffix so fixtures never collide across runs (the server
/// persists; no test drops data).
fn nonce() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock must be set")
        .as_millis();
    format!("h186-{millis}")
}

/// The canonical HRN the HTTP edge hands to `get_by_hrn` for a scoped
/// policy id (mirrors `handlers/iam.rs`: the tenant slot carries the
/// account, the resource id carries the scoped `{account}~{short}`).
fn scoped_policy_hrn(account: &str, scoped: &str) -> Hrn {
    Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        account.to_string(),
        "Policy".to_string(),
        scoped.to_string(),
    )
}

fn create_command(
    scoped: &str,
    content: &str,
    description: &str,
    attached: Vec<String>,
) -> CreatePolicyCommand {
    CreatePolicyCommand {
        policy_id: scoped.to_string(),
        policy_content: content.to_string(),
        description: Some(description.to_string()),
        attached_principals: attached,
    }
}

const PERMIT: &str = "permit(principal, action, resource);";
const FORBID: &str = "forbid(principal, action, resource);";

// ============================================================================
// AC#1 — the policy CRUD, pagination and isolation oracles over the graph
// ============================================================================

#[tokio::test]
async fn policy_crud_pagination_and_account_isolation_over_dgraph() {
    let _serial = SERIAL.lock().await;
    let Some(adapter) = adapter_or_skip().await else {
        return;
    };

    let account = format!("acct-{}", nonce());
    let foreign = format!("acct-{}-other", nonce());
    let scoped_a = format!("{account}~alpha");
    let scoped_b = format!("{account}~beta");
    let scoped_c = format!("{account}~gamma");
    let foreign_scoped = format!("{foreign}~delta");

    // Create.
    let created = adapter
        .create(create_command(
            &scoped_a,
            PERMIT,
            "first",
            vec!["hrn:hodei:iam::x:User/ghost".to_string()],
        ))
        .await
        .expect("the create must succeed");
    assert_eq!(created.policy.id().as_str(), scoped_a);
    assert!(created.created_at.is_some(), "the create must leave a mark");

    // Duplicate create answers like the Surreal unique record id.
    let duplicate = adapter
        .create(create_command(&scoped_a, PERMIT, "again", vec![]))
        .await;
    assert!(
        matches!(duplicate, Err(CreatePolicyError::PolicyAlreadyExists(_))),
        "a second create of the same id must be the duplicate error, got {duplicate:?}"
    );

    // Read back: the view is the caller's HRN, content, marks and the
    // attachment list as stored.
    let view = adapter
        .get_by_hrn(&scoped_policy_hrn(&account, &scoped_a))
        .await
        .expect("the policy must be readable");
    assert_eq!(view.content, PERMIT);
    assert_eq!(view.description.as_deref(), Some("first"));
    assert_eq!(
        view.attached_principals,
        vec!["hrn:hodei:iam::x:User/ghost"]
    );
    assert_eq!(view.created_at, created.created_at);
    assert_eq!(
        view.hrn,
        scoped_policy_hrn(&account, &scoped_a),
        "the read echoes the caller's HRN"
    );

    // Update (unversioned): content and description replaced.
    let updated = adapter
        .update(UpdatePolicyCommand {
            policy_id: scoped_a.clone(),
            policy_content: Some(FORBID.to_string()),
            description: Some("edited".to_string()),
        })
        .await
        .expect("the unversioned update must succeed");
    assert_eq!(updated.content, FORBID);
    assert_eq!(updated.description.as_deref(), Some("edited"));
    assert!(
        updated.updated_at >= updated.created_at,
        "the update mark cannot precede creation"
    );
    assert_eq!(
        adapter
            .policy_revision(&scoped_a)
            .await
            .expect("the revision read must answer")
            .expect("the node exists"),
        1,
        "one guarded update is one revision"
    );

    // Pagination decided inside the server, totals included.
    adapter
        .create(create_command(&scoped_b, PERMIT, "second", vec![]))
        .await
        .expect("beta");
    adapter
        .create(create_command(&scoped_c, PERMIT, "third", vec![]))
        .await
        .expect("gamma");
    let page = adapter
        .list_for_account(&account, ListPoliciesQuery::with_pagination(2, 0))
        .await
        .expect("the first page must answer");
    assert_eq!(page.policies.len(), 2, "first: 2");
    assert_eq!(page.total_count, 3, "total: 3 for the account");
    assert!(page.has_next_page && !page.has_previous_page);
    let tail = adapter
        .list_for_account(&account, ListPoliciesQuery::with_pagination(2, 2))
        .await
        .expect("the tail page must answer");
    assert_eq!(tail.policies.len(), 1);
    assert!(!tail.has_next_page && tail.has_previous_page);
    // The listing answers with the short id under the account slot, the
    // Surreal contract.
    let names: Vec<&str> = page.policies.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"alpha") && names.contains(&"beta"),
        "the key-ordered page is alpha,beta: {names:?}"
    );
    assert!(
        page.policies
            .iter()
            .all(|p| p.hrn.account_id() == account && p.hrn.resource_id() == p.name),
        "the listed HRNs are unscoped and under the account"
    );

    // Account isolation is structural: the other tenant's listing does
    // not even see these rows as candidates.
    let foreign_list = adapter
        .list_for_account(&foreign, ListPoliciesQuery::default())
        .await
        .expect("the foreign list must answer");
    assert_eq!(
        foreign_list.total_count, 0,
        "the foreign tenant sees no rows"
    );
    // The foreign account can create under its own tenant and the homonym
    // short id is a different aggregate.
    adapter
        .create(create_command(&foreign_scoped, PERMIT, "foreign", vec![]))
        .await
        .expect("the foreign create is a different key");
    let own = adapter
        .list_for_account(&account, ListPoliciesQuery::default())
        .await
        .expect("own list");
    assert_eq!(own.total_count, 3, "the foreign create did not leak");
    let other = adapter
        .list_for_account(&foreign, ListPoliciesQuery::default())
        .await
        .expect("foreign list after its own create");
    assert_eq!(other.total_count, 1, "each tenant lists only its own");
    assert_eq!(other.policies[0].name, "delta");

    // Delete frees the key (a later recreation is a fresh aggregate).
    adapter
        .delete(&scoped_c)
        .await
        .expect("the delete must succeed");
    assert!(
        matches!(
            adapter.delete(&scoped_c).await,
            Err(DeletePolicyError::PolicyNotFound(_))
        ),
        "deleting twice is not-found"
    );
    let after = adapter
        .list_for_account(&account, ListPoliciesQuery::default())
        .await
        .expect("list after delete");
    assert_eq!(after.total_count, 2);
    adapter
        .create(create_command(&scoped_c, PERMIT, "reborn", vec![]))
        .await
        .expect("the key is free again");
    assert_eq!(
        adapter
            .policy_revision(&scoped_c)
            .await
            .expect("revision")
            .expect("node"),
        0,
        "a recreated aggregate starts at revision 0"
    );
}

// ============================================================================
// AC#1 — user and group CRUD over the graph
// ============================================================================

#[tokio::test]
async fn user_and_group_crud_over_dgraph() {
    let _serial = SERIAL.lock().await;
    let Some(adapter) = adapter_or_skip().await else {
        return;
    };

    let account = format!("acct-{}", nonce());
    let user_hrn = format!("hrn:hodei:iam::{account}:User/carol");
    let group_hrn = format!("hrn:hodei:iam::{account}:Group/engineers");
    let other_group = format!("hrn:hodei:iam::{account}:Group/auditors");

    CreateUserPort::save_user(
        &*adapter,
        &CreateUserPersistenceDto {
            hrn: user_hrn.clone(),
            name: "carol".to_string(),
            email: "carol@example.test".to_string(),
            group_hrns: vec![group_hrn.clone()],
            tags: vec!["staff".to_string(), "eu".to_string()],
        },
    )
    .await
    .expect("the user create must succeed");

    let found = adapter
        .find_user_by_hrn(&Hrn::from_string(&user_hrn).expect("valid hrn"))
        .await
        .expect("the lookup must answer")
        .expect("the user exists");
    assert_eq!(found.hrn, user_hrn);
    assert_eq!(found.email, "carol@example.test");
    assert_eq!(found.group_hrns, vec![group_hrn.clone()]);
    assert_eq!(found.tags.len(), 2);

    // The groups the user is attached to are replaced as a list, not
    // appended (the Surreal semantics).
    UserGroupPersister::save_user(
        &*adapter,
        &AddUserPersistenceDto {
            hrn: user_hrn.clone(),
            name: "carol".to_string(),
            email: "carol@example.test".to_string(),
            group_hrns: vec![other_group.clone()],
            tags: vec!["staff".to_string()],
        },
    )
    .await
    .expect("the membership write must succeed");
    let moved = adapter
        .find_user_by_hrn(&Hrn::from_string(&user_hrn).expect("valid hrn"))
        .await
        .expect("answer")
        .expect("exists");
    assert_eq!(
        moved.group_hrns,
        vec![other_group.clone()],
        "the list was replaced"
    );
    assert_eq!(moved.tags, vec!["staff"]);

    // Unknown users are not silently created by the membership write.
    let ghost = UserGroupPersister::save_user(
        &*adapter,
        &AddUserPersistenceDto {
            hrn: format!("hrn:hodei:iam::{account}:User/nobody"),
            name: "nobody".to_string(),
            email: String::new(),
            group_hrns: vec![],
            tags: vec![],
        },
    )
    .await;
    assert!(ghost.is_err(), "a membership write for a ghost user fails");

    // Groups: create, kind-guarded lookup, and the get-effective finder.
    adapter
        .save_group(&GroupPersistenceDto {
            hrn: group_hrn.clone(),
            name: "engineers".to_string(),
            tags: vec!["core".to_string()],
        })
        .await
        .expect("group create");
    adapter
        .save_group(&GroupPersistenceDto {
            hrn: other_group.clone(),
            name: "auditors".to_string(),
            tags: vec![],
        })
        .await
        .expect("second group create");
    let group = adapter
        .find_group_by_hrn(&Hrn::from_string(&group_hrn).expect("valid hrn"))
        .await
        .expect("answer")
        .expect("the group exists");
    assert_eq!(group.hrn, group_hrn);
    assert_eq!(group.tags, vec!["core"]);
    // The group ports must see a group, never the user node.
    let kind_guard = adapter
        .find_group_by_hrn(
            &Hrn::from_string(&format!("hrn:hodei:iam::{account}:Group/carol")).expect("valid hrn"),
        )
        .await
        .expect("answer");
    assert!(
        kind_guard.is_none(),
        "the kind guard rejects the wrong node"
    );

    // The faithful port of the Surreal stub: every group is returned
    // (membership by user is not modelled), so both new groups answer.
    let groups = adapter
        .find_groups_by_user_hrn(&Hrn::from_string(&user_hrn).expect("valid hrn"))
        .await
        .expect("the effective finder must answer");
    let texts: Vec<&str> = groups.iter().map(|g| g.hrn.as_str()).collect();
    assert!(
        texts.contains(&group_hrn.as_str()) && texts.contains(&other_group.as_str()),
        "both fixtures are in the answer: {texts:?}"
    );

    // The effective-policy port finds exactly the attached policies.
    let scoped = format!("{account}~for-carol");
    adapter
        .create(create_command(
            &scoped,
            PERMIT,
            "eff",
            vec![user_hrn.clone()],
        ))
        .await
        .expect("attach");
    let effective = adapter
        .find_policies_by_principal(&Hrn::from_string(&user_hrn).expect("valid hrn"))
        .await
        .expect("the effective lookup must answer");
    assert_eq!(effective.len(), 1);
    assert_eq!(effective[0].id().as_str(), scoped);
}

// ============================================================================
// AC#1 — Cedar authorization over the remote authority
// ============================================================================

#[tokio::test]
async fn cedar_authorization_uses_the_persisted_attachment() {
    let _serial = SERIAL.lock().await;
    let Some(adapter) = adapter_or_skip().await else {
        return;
    };

    let account = format!("acct-{}", nonce());
    let principal = format!("hrn:hodei:iam::{account}:User/ada");
    let outsider = format!("hrn:hodei:iam::{account}:User/eve");
    let scoped = format!("{account}~allow-read");
    let policy = format!("permit(principal == Iam::User::\"{principal}\", action, resource);");

    adapter
        .create(create_command(
            &scoped,
            &policy,
            "wired",
            vec![principal.clone()],
        ))
        .await
        .expect("the attached policy must persist");

    let evaluate = EvaluateIamPoliciesUseCase::new(
        adapter.clone() as Arc<dyn IamEvaluatePolicyFinderPort>,
        Arc::new(HrnEntityResolver::new()),
        Arc::new(HrnEntityResolver::new()),
        Arc::new(EmptySchemaStorage),
    );

    let request = |who: &str| EvaluateIamPoliciesCommand {
        principal_hrn: Hrn::from_string(who).expect("valid hrn"),
        action_name: "read".to_string(),
        resource_hrn: Hrn::from_string(&format!("hrn:hodei:iam::{account}:Document/doc"))
            .expect("valid hrn"),
    };

    // The attached principal is permitted, by the stored policy.
    let answer = evaluate
        .evaluate(request(&principal))
        .await
        .expect("the evaluation must answer");
    assert!(
        answer.decision,
        "the attached principal must be permitted: {}",
        answer.reason
    );
    assert!(
        answer.policy_ids_evaluated.contains(&scoped),
        "the persisted policy was the one consulted: {:?}",
        answer.policy_ids_evaluated
    );

    // Everyone else is implicitly denied — the attachment is the grant.
    let denied = evaluate
        .evaluate(request(&outsider))
        .await
        .expect("the evaluation must answer");
    assert!(
        !denied.decision,
        "an unattached principal is never permitted"
    );
}

// ============================================================================
// AC#1 — fail closed when the server is unreachable
// ============================================================================

#[tokio::test]
async fn fail_closed_when_the_authority_does_not_answer() {
    let _serial = SERIAL.lock().await;
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set; the real-server run is gated");
        return;
    };

    // Nothing listening: the client cannot even be built, so no adapter
    // — and therefore no authorization — can exist over a dead server.
    let mut dead =
        GraphClientConfig::from_toml_str("endpoint = \"127.0.0.1:1\"\n\n[tls]\nenabled = false\n")
            .expect("the dead configuration must validate");
    dead.connect_timeout_ms = 1_000;
    assert!(
        GraphClient::connect(dead).await.is_err(),
        "a client to an unreachable authority must not be built"
    );

    // The HTTPS/admin port answers HTTP, not the graph protocol: the
    // boot gate rejects it and the adapter never opens. Fail-closed by
    // construction — there is no handle from which to invent a permit.
    let mut wrong_plane = config;
    wrong_plane.endpoint = "127.0.0.1:8286".to_string();
    let opened = async {
        let mut client = GraphClient::connect(wrong_plane).await.ok()?;
        client.login().await.ok()?;
        DgraphIamAdapter::open(Arc::new(client)).await.ok()
    }
    .await;
    assert!(
        opened.is_none(),
        "the adapter may not open on an endpoint that does not answer the graph protocol"
    );

    // And with no policies attached, the evaluation denies rather than
    // permits: absence is never a grant.
    let Some(adapter) = adapter_or_skip().await else {
        return;
    };
    let evaluate = EvaluateIamPoliciesUseCase::new(
        adapter as Arc<dyn IamEvaluatePolicyFinderPort>,
        Arc::new(HrnEntityResolver::new()),
        Arc::new(HrnEntityResolver::new()),
        Arc::new(EmptySchemaStorage),
    );
    let lonely = format!("hrn:hodei:iam::acct-{}:User/lonely", nonce());
    let answer = evaluate
        .evaluate(EvaluateIamPoliciesCommand {
            principal_hrn: Hrn::from_string(&lonely).expect("valid hrn"),
            action_name: "read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:iam::default:Document/x").expect("valid hrn"),
        })
        .await
        .expect("the evaluation of a principal with no policies must answer");
    assert!(!answer.decision, "no attachment, no permit");
}

// ============================================================================
// AC#2 — activation under the expected revision is a CAS
// ============================================================================

async fn activate(
    store: &DgraphIamAdapter,
    scoped: &str,
    revision: u64,
) -> Result<UpdatePolicyView, UpdatePolicyError> {
    store
        .activate_policy(
            UpdatePolicyCommand {
                policy_id: scoped.to_string(),
                policy_content: Some(PERMIT.to_string()),
                description: Some("live".to_string()),
            },
            revision,
        )
        .await
}

#[tokio::test]
async fn activation_under_expected_revision_loses_once_in_a_real_race() {
    let _serial = SERIAL.lock().await;
    let Some(adapter) = adapter_or_skip().await else {
        return;
    };

    let account = format!("acct-{}", nonce());
    let scoped = format!("{account}~staged");
    adapter
        .create(create_command(&scoped, FORBID, "draft", vec![]))
        .await
        .expect("the draft policy persists at revision 0");

    // A stale expectation on an existing node is a version conflict,
    // never a not-found and never a silent apply.
    let stale = activate(&adapter, &scoped, 7).await;
    assert!(
        matches!(stale, Err(UpdatePolicyError::VersionConflict)),
        "a stale expectation must be VersionConflict, got {stale:?}"
    );

    // The exact race: two connections, same expectation, both fire.
    let second = adapter_or_skip().await.expect("a second connection");
    let (first, other) = tokio::join!(
        activate(&adapter, &scoped, 0),
        activate(&second, &scoped, 0)
    );
    let results = [first, other];
    let winners = results.iter().filter(|outcome| outcome.is_ok()).count();
    assert_eq!(winners, 1, "exactly one activation may win the revision");
    let loser = results
        .into_iter()
        .find(|outcome| outcome.is_err())
        .expect("one activation must lose");
    assert!(
        matches!(loser, Err(UpdatePolicyError::VersionConflict)),
        "the loser of the race sees the typed conflict, got {loser:?}"
    );

    let view = adapter
        .get_by_hrn(&scoped_policy_hrn(&account, &scoped))
        .await
        .expect("the activated policy reads back");
    assert_eq!(view.content, PERMIT);
    assert_eq!(view.description.as_deref(), Some("live"));
    assert_eq!(
        adapter
            .policy_revision(&scoped)
            .await
            .expect("revision")
            .expect("node"),
        1,
        "the race advanced the revision exactly once"
    );

    // Activating a policy that does not exist is still not-found.
    let missing = adapter
        .activate_policy(
            UpdatePolicyCommand {
                policy_id: format!("{account}~phantom"),
                policy_content: Some(PERMIT.to_string()),
                description: None,
            },
            0,
        )
        .await;
    assert!(
        matches!(missing, Err(UpdatePolicyError::PolicyNotFound(_))),
        "a missing aggregate is not a conflict, got {missing:?}"
    );
}

// ============================================================================
// AC#2 — policies and schema survive into a new connection
// ============================================================================

#[tokio::test]
async fn policies_and_schema_persist_across_a_new_connection() {
    let _serial = SERIAL.lock().await;
    let Some(adapter) = adapter_or_skip().await else {
        return;
    };

    let account = format!("acct-{}", nonce());
    let scoped = format!("{account}~durable");
    let user_hrn = format!("hrn:hodei:iam::{account}:User/survivor");
    let content = "permit(principal, action, resource) unless { action == Action::\"delete\" };";
    adapter
        .create(create_command(
            &scoped,
            content,
            "written before the restart",
            vec![user_hrn.clone()],
        ))
        .await
        .expect("the policy persists");
    CreateUserPort::save_user(
        &*adapter,
        &CreateUserPersistenceDto {
            hrn: user_hrn.clone(),
            name: "survivor".to_string(),
            email: "s@example.test".to_string(),
            group_hrns: vec![],
            tags: vec!["keep".to_string()],
        },
    )
    .await
    .expect("the user persists");

    // Brand-new connection and adapter, as the process would make after a
    // server restart (the data plane is durable either way; the boot
    // gate re-verifies the stored schema against the code and accepts
    // the current one).
    let reopened = adapter_or_skip().await.expect("the reopen must succeed");
    let view = reopened
        .get_by_hrn(&scoped_policy_hrn(&account, &scoped))
        .await
        .expect("the policy survives the new connection");
    assert_eq!(view.content, content);
    assert_eq!(
        view.description.as_deref(),
        Some("written before the restart")
    );
    assert_eq!(view.attached_principals, vec![user_hrn.clone()]);
    let user = reopened
        .find_user_by_hrn(&Hrn::from_string(&user_hrn).expect("valid hrn"))
        .await
        .expect("answer")
        .expect("the user survives");
    assert_eq!(user.email, "s@example.test");
    assert_eq!(user.tags, vec!["keep"]);

    // The aggregate is still writable at its revision — the CAS state,
    // not just the payload, came back.
    activate(&reopened, &scoped, 0)
        .await
        .expect("the revision survived too");
    let effective = reopened
        .get_effective_policies(&Hrn::from_string(&user_hrn).expect("valid hrn"))
        .await
        .expect("the effective lookup works after the restart");
    assert_eq!(effective.len(), 1, "the attachment survived the restart");
}

// ============================================================================
// Local helpers
// ============================================================================

/// A schema storage without any schema stored (the evaluations run in
/// NoSchema mode; the port is only a dependency of the use case).
struct EmptySchemaStorage;

#[async_trait::async_trait]
impl SchemaStoragePort for EmptySchemaStorage {
    async fn save_schema(
        &self,
        _schema_json: String,
        _version: Option<String>,
    ) -> Result<String, BuildSchemaError> {
        Err(BuildSchemaError::SchemaStorageError(
            "this storage is read-only".to_string(),
        ))
    }

    async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
        Ok(None)
    }

    async fn get_schema_by_version(
        &self,
        _version: &str,
    ) -> Result<Option<String>, BuildSchemaError> {
        Ok(None)
    }

    async fn delete_schema(&self, _schema_id: &str) -> Result<bool, BuildSchemaError> {
        Ok(false)
    }

    async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
        Ok(Vec::new())
    }
}
