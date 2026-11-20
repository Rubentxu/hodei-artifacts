//! Dgraph infrastructure for the IAM vertical slice (HSP-186).
//!
//! This module ports the SEMANTICS of the SurrealDB adapters
//! (`policy_adapter`, `user_adapter`, `group_adapter` next door) onto the
//! remote graph authority, reusing `hodei-graph-client` for everything the
//! Surreal layer got from the document store: the identity anchor
//! (`hodei.identity.key` = `tenant|canonical-HRN`, unique by `@upsert`),
//! the CAS revision (`hodei.identity.revision`), the versioned schema boot
//! (`ensure_ready`) and the typed transport errors that make every failure
//! fail closed. Nothing here re-implements a client, a TLS dial or an
//! index: the port is the IAM meaning over the existing graph stack.
//!
//! The storage model:
//!
//! - every IAM node (policy, user, group) is one `HodeiIam` node carrying
//!   the identity-family predicates plus the `hodei.iam.*` payload;
//! - the uniqueness anchor mirrors the Surreal record id exactly: the
//!   tenant is the account the create path derived (`policy_id` prefix
//!   before `~`, else `default`; the HRN account for users and groups)
//!   and the canonical HRN is `hrn:hodei:iam:default:{Kind}/{resource-id}`;
//! - the list-valued relationships (a policy's `attached_principals`, a
//!   user's `group_hrns`) are stored as `hodei.iam.attachment` string
//!   edges with an exact index, so "policies attached to this principal"
//!   is one indexed lookup (`eq(hodei.iam.attachment, "<hrn>")`);
//! - the marks are stored as the exact response text (`string`, not
//!   `datetime`) for the same reason the Surreal layer stores them as
//!   text: what a response says about a timestamp must be character for
//!   character what the database holds;
//! - mutation is guarded by the revision CAS: the write binds the node
//!   only while the transaction's own snapshot still shows the expected
//!   revision (`@filter(eq(hodei.identity.revision, R))`), the readable
//!   state block reveals a skipped block, and success is decided by the
//!   state after the commit — never by the transaction merely having
//!   committed (the `rename_identity` pattern of `hodei-graph-client`).

// The error enum carries the graph client's typed errors (~200 bytes).
// They are constructed only on failure paths and never stored hot, so
// boxing would complicate every `?` in the port for no measurable gain
// (the same call `hodei-graph-client` makes for its own error type).
#![allow(clippy::result_large_err)]

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use kernel::Hrn;
use kernel::domain::policy::{HodeiPolicy, HodeiPolicySet, PolicyId};
use serde_json::Value;
use tracing::{debug, info, warn};

use hodei_graph_client::{
    CanonicalHrn, GraphClient, GraphClientError, GraphMutationBlock, GraphMutationPort,
    GraphUpsertTransaction, Tenant, dql_string, ensure_ready, identity_digest, identity_key,
};

use crate::features::add_user_to_group::dto::GroupLookupDto as AddGroupLookupDto;
use crate::features::add_user_to_group::dto::{
    UserLookupDto as AddUserLookupDto, UserPersistenceDto as AddUserPersistenceDto,
};
use crate::features::add_user_to_group::error::AddUserToGroupError;
use crate::features::add_user_to_group::ports::GroupFinder;
use crate::features::add_user_to_group::ports::{UserFinder, UserGroupPersister};
use crate::features::create_group::dto::GroupPersistenceDto;
use crate::features::create_group::error::CreateGroupError;
use crate::features::create_group::ports::CreateGroupPort;
use crate::features::create_policy::dto::{CreatePolicyCommand, CreatedPolicy};
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::create_policy::ports::CreatePolicyPort;
use crate::features::create_user::dto::UserPersistenceDto as CreateUserPersistenceDto;
use crate::features::create_user::error::CreateUserError;
use crate::features::create_user::ports::CreateUserPort;
use crate::features::delete_policy::error::DeletePolicyError;
use crate::features::delete_policy::ports::DeletePolicyPort;
use crate::features::evaluate_iam_policies::ports::{
    PolicyFinderError, PolicyFinderPort as IamPolicyFinderPort,
};
use crate::features::get_effective_policies::dto::{
    GroupLookupDto, UserLookupDto as EffectiveUserLookupDto,
};
use crate::features::get_effective_policies::error::GetEffectivePoliciesError;
use crate::features::get_effective_policies::ports::{
    GroupFinderPort, PolicyFinderPort, UserFinderPort,
};
use crate::features::get_policy::dto::PolicyView as GetPolicyView;
use crate::features::get_policy::error::GetPolicyError;
use crate::features::get_policy::ports::PolicyReader;
use crate::features::list_policies::dto::{ListPoliciesQuery, ListPoliciesResponse, PolicySummary};
use crate::features::list_policies::error::ListPoliciesError;
use crate::features::list_policies::ports::PolicyLister;
use crate::features::update_policy::dto::{PolicyView as UpdatePolicyView, UpdatePolicyCommand};
use crate::features::update_policy::error::UpdatePolicyError;
use crate::features::update_policy::ports::UpdatePolicyPort;

/// The type tag of IAM nodes in the graph (declared in the v3 schema).
pub const IAM_TYPE: &str = "HodeiIam";

/// The kind discriminator values (`hodei.iam.kind`).
pub const KIND_POLICY: &str = "Policy";
/// See [`KIND_POLICY`].
pub const KIND_USER: &str = "User";
/// See [`KIND_POLICY`].
pub const KIND_GROUP: &str = "Group";

/// The tenant the Surreal create path falls back to when the policy id
/// carries no `account~` prefix. Ported verbatim: the Dgraph uniqueness
/// anchor must not widen what the Surreal record id already meant.
const DEFAULT_ACCOUNT: &str = "default";

/// Retry budget of the CAS loop: the same bound `hodei-graph-client`
/// gives its guarded rename — attempts, doubling backoff, a cap, and
/// jitter so concurrent losers of one round do not retry in lockstep.
const MAX_ATTEMPTS: usize = 12;
const RETRY_BASE: Duration = Duration::from_millis(10);
const RETRY_CAP: Duration = Duration::from_millis(100);

/// Every predicate an IAM read needs, in one literal so every read path
/// asks the graph for the same shape.
const NODE_FIELDS: &str = "hodei.identity.key hodei.identity.hrn hodei.identity.tenant \
     hodei.identity.revision hodei.identity.name hodei.iam.kind hodei.iam.content \
     hodei.iam.description hodei.iam.email hodei.iam.tags hodei.iam.attachment \
     hodei.iam.created_at hodei.iam.updated_at";

/// Why an IAM graph operation failed, before it is mapped to the
/// feature-level error of the port that surfaced it.
#[derive(Debug, thiserror::Error)]
pub enum IamGraphError {
    /// A transport, auth, conflict or boot failure of the graph client.
    /// An IAM port never resolves this into a success: it becomes the
    /// feature's typed storage/repository error (fail closed).
    #[error(transparent)]
    Graph(#[from] GraphClientError),
    /// A stored node is not readable as the shape this module writes.
    #[error("corrupt IAM node for key '{key}': {detail}")]
    Corrupt {
        /// The composite key the node was read under.
        key: String,
        /// What about the node could not be read.
        detail: String,
    },
    /// The composite key could not be derived from the identifier the
    /// caller supplied.
    #[error(transparent)]
    Key(#[from] hodei_graph_client::IdentityError),
    /// The versioned schema boot refused the server: the adapter never
    /// opens — and never writes — on an incompatible graph.
    #[error(transparent)]
    Boot(#[from] hodei_graph_client::SchemaBootError),
}

/// The spelling a timestamp is stored with. Ported verbatim from the
/// Surreal policy adapter, including the invariant its unit test pins:
/// the stored text is character for character the text the response
/// serialises.
fn mark(instant: chrono::DateTime<chrono::Utc>) -> String {
    instant.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

/// The mark of a stored timestamp, as the domain type. An unreadable mark
/// is reported as absent, never invented.
fn parse_mark(value: Option<String>) -> Option<chrono::DateTime<chrono::Utc>> {
    let value = value?;
    match chrono::DateTime::parse_from_rfc3339(&value) {
        Ok(mark) => Some(mark.with_timezone(&chrono::Utc)),
        Err(error) => {
            warn!("The stored timestamp could not be read: {}", error);
            None
        }
    }
}

/// The account the Surreal create path derives from a policy id: the
/// prefix before the first `~`, else `default`.
fn policy_account(policy_id: &str) -> &str {
    policy_id
        .split_once('~')
        .map(|(account, _)| account)
        .unwrap_or(DEFAULT_ACCOUNT)
}

/// The uniqueness anchor of one IAM node: everything a write or read is
/// keyed by, derived the same way for every port.
#[derive(Debug)]
pub(crate) struct IamAnchor {
    kind: &'static str,
    tenant: String,
    /// The canonical HRN stored on `hodei.identity.hrn`.
    canonical: String,
    /// The composite key (`tenant|canonical`), unique by `@upsert`.
    key: String,
    digest: String,
}

impl IamAnchor {
    fn new(kind: &'static str, tenant: &str, resource_id: &str) -> Result<Self, IamGraphError> {
        let canonical = CanonicalHrn::new(&format!("hrn:hodei:iam:default:{kind}/{resource_id}"))?;
        let tenant = Tenant::new(tenant)?;
        Ok(Self {
            kind,
            tenant: tenant.as_str().to_owned(),
            canonical: canonical.as_str().to_owned(),
            key: identity_key(&tenant, &canonical),
            digest: identity_digest(&tenant, &canonical),
        })
    }

    /// The anchor of a policy by its (possibly `account~`-scoped) id,
    /// exactly as the create path stores it.
    fn policy(policy_id: &str) -> Result<Self, IamGraphError> {
        Self::new(KIND_POLICY, policy_account(policy_id), policy_id)
    }

    /// The anchor of a user or group by its full HRN. The resource type
    /// of the HRN decides the kind.
    fn principal(hrn: &Hrn) -> Result<Self, IamGraphError> {
        let kind = match hrn.resource_type() {
            KIND_USER => KIND_USER,
            KIND_GROUP => KIND_GROUP,
            other => {
                return Err(IamGraphError::Corrupt {
                    key: hrn.to_string(),
                    detail: format!("'{other}' is not an IAM principal resource type"),
                });
            }
        };
        Self::new(kind, hrn.account_id(), hrn.resource_id())
    }
}

/// One IAM node as the graph holds it.
#[derive(Debug, Clone)]
pub struct IamNode {
    /// The composite key.
    pub key: String,
    /// The canonical HRN.
    pub hrn: String,
    /// The tenant (account) half of the key.
    pub tenant: String,
    /// The kind discriminator.
    pub kind: String,
    /// The CAS revision (0 on creation).
    pub revision: u64,
    /// The policy content (Cedar text).
    pub content: Option<String>,
    /// The description, when one was stored.
    pub description: Option<String>,
    /// The user email, when present.
    pub email: Option<String>,
    /// The tags list.
    pub tags: Vec<String>,
    /// The attachment list: a policy's principal HRNs, a user's group HRNs.
    pub attachments: Vec<String>,
    /// The creation mark, as the exact stored text.
    pub created_at: Option<String>,
    /// The update mark, as the exact stored text.
    pub updated_at: Option<String>,
}

fn opt_string(node: &Value, field: &str) -> Option<String> {
    node.get(field)
        .and_then(Value::as_str)
        .map(|s| s.to_owned())
        .filter(|s| !s.is_empty())
}

fn string_list(node: &Value, field: &str) -> Vec<String> {
    node.get(field)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl IamNode {
    fn parse(key: &str, node: &Value) -> Result<Self, IamGraphError> {
        Ok(Self {
            key: key.to_owned(),
            hrn: opt_string(node, "hodei.identity.hrn").unwrap_or_default(),
            tenant: opt_string(node, "hodei.identity.tenant").unwrap_or_default(),
            kind: opt_string(node, "hodei.iam.kind").unwrap_or_default(),
            revision: node
                .get("hodei.identity.revision")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            content: opt_string(node, "hodei.iam.content"),
            description: opt_string(node, "hodei.iam.description"),
            email: opt_string(node, "hodei.iam.email"),
            tags: string_list(node, "hodei.iam.tags"),
            attachments: string_list(node, "hodei.iam.attachment"),
            created_at: opt_string(node, "hodei.iam.created_at"),
            updated_at: opt_string(node, "hodei.iam.updated_at"),
        })
    }

    /// The short id this module derives for responses: the Surreal
    /// adapters answer with the record id, which is the resource-id part
    /// of the canonical HRN.
    fn short_id(&self) -> String {
        self.hrn
            .rsplit_once('/')
            .map(|(_, id)| id.to_owned())
            .unwrap_or_else(|| self.hrn.clone())
    }

    /// The caller-facing HRN of the node: the Surreal adapters answer
    /// with the HRN the entity was stored under, whose account slot is
    /// the tenant half of the key (the canonical form keeps the `default`
    /// slot because the uniqueness — not the naming — lives in the key).
    fn display_hrn(&self) -> String {
        Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            self.tenant.clone(),
            self.kind.clone(),
            self.short_id(),
        )
        .to_string()
    }
}

/// Whether the retry loop keeps going: only the server-forced conflict or
/// the skipped CAS block is retryable, and only up to the bound.
fn retry_delay(attempt: usize, retryable: bool) -> Option<Duration> {
    if !retryable || attempt >= MAX_ATTEMPTS {
        return None;
    }
    let exponential = RETRY_BASE * (1u32 << (attempt - 1));
    let base = exponential.min(RETRY_CAP);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    Some(base + Duration::from_millis((nanos % 21) as u64))
}

/// The three answers a versioned guarded write can give. Collapsing
/// `Rejected` into `Missing` would turn a lost optimistic-lock race into
/// a 404, which is the one mapping AC#2 forbids: the loser must see a
/// version conflict (HTTP 409 at the handler), never a phantom delete.
pub(crate) enum CasOutcome {
    Applied(Box<IamNode>),
    Rejected,
    Missing,
}

/// The IAM adapter over the remote graph authority. One handle
/// implements every IAM port of the vertical slice, exactly as the
/// Surreal layer expects its adapters to (the composition wires each
/// `Arc<dyn Port>` from the same handle).
pub struct DgraphIamAdapter {
    client: Arc<GraphClient>,
}

impl DgraphIamAdapter {
    /// Opens the adapter on a connected, logged-in client. The versioned
    /// schema boot is the readiness gate: the adapter never exists — and
    /// therefore never writes — on an incompatible server. A transport
    /// failure here (including an unreachable alpha) is returned as the
    /// typed error: fail closed.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, IamGraphError> {
        let report = ensure_ready(&client).await?;
        info!(
            decision = ?report.decision,
            binary_version = report.binary_version,
            verified_predicates = report.verified_predicates,
            "the IAM adapter opened on a validated graph schema"
        );
        Ok(Self { client })
    }

    /// The connected client, for callers that need the raw graph
    /// (the IT oracles query the same server through it).
    pub fn client(&self) -> &Arc<GraphClient> {
        &self.client
    }

    /// Reads one IAM node by anchor. `None` is a genuine absence; an
    /// `Err` is the server refusing to answer (never treated as absent).
    pub(crate) async fn node(&self, anchor: &IamAnchor) -> Result<Option<IamNode>, IamGraphError> {
        let query = format!(
            "{{ q(func: eq(hodei.identity.key, {})) {{ {NODE_FIELDS} }} }}",
            dql_string(&anchor.key)
        );
        let response = self.client.execute_dql(&query).await?;
        self.one_node(&anchor.key, &response.json, "q")
    }

    /// Reads one node root out of a query answer, refusing duplicates.
    fn one_node(
        &self,
        key: &str,
        json: &Value,
        root: &str,
    ) -> Result<Option<IamNode>, IamGraphError> {
        let nodes = json
            .get(root)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            0 => Ok(None),
            1 => Ok(Some(IamNode::parse(key, &nodes[0])?)),
            n => Err(IamGraphError::Corrupt {
                key: key.to_owned(),
                detail: format!("{n} nodes answered the unique key lookup"),
            }),
        }
    }

    /// The create write: bind the node by key, create it only when the
    /// key was absent (`eq(len(u), 0)`), atomically with the payload.
    /// Concurrent creators conflict on the `@upsert` key index; the
    /// loser's revalidation (the query is part of every attempt) finds
    /// the winner's node.
    async fn create_node(
        &self,
        anchor: &IamAnchor,
        name: &str,
        payload: &str,
    ) -> Result<Option<IamNode>, IamGraphError> {
        let key_literal = dql_string(&anchor.key);
        let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut set = format!(
            "_:i <dgraph.type> \"{IAM_TYPE}\" .\n\
             _:i <hodei.identity.key> {key_literal} .\n\
             _:i <hodei.identity.hrn> {} .\n\
             _:i <hodei.identity.tenant> {} .\n\
             _:i <hodei.identity.digest> {} .\n\
             _:i <hodei.identity.name> {} .\n\
             _:i <hodei.identity.revision> \"0\" .\n\
             _:i <hodei.identity.created_at> \"{created_at}\" .\n\
             _:i <hodei.iam.kind> {} .\n",
            dql_string(&anchor.canonical),
            dql_string(&anchor.tenant),
            dql_string(&anchor.digest),
            dql_string(name),
            dql_string(anchor.kind),
        );
        set.push_str(payload);
        let mut last_error = None;
        for attempt in 1..=MAX_ATTEMPTS {
            let transaction = GraphUpsertTransaction {
                query: format!(
                    "{{ q(func: eq(hodei.identity.key, {key_literal})) {{ u as uid \
                     {NODE_FIELDS} }} }}"
                ),
                blocks: vec![GraphMutationBlock::set_if("eq(len(u), 0)", set.clone())],
                commit_now: true,
            };
            match self.client.upsert(transaction).await {
                Ok(response) => {
                    // The answer carries the in-transaction lookup: a node
                    // means this attempt lost the existence race (or the
                    // key already existed) — never a second create.
                    return match self.one_node(&anchor.key, &response.json, "q")? {
                        Some(existing) => Ok(Some(existing)),
                        None => Ok(None),
                    };
                }
                Err(error @ GraphClientError::UpsertConflict { .. }) => {
                    debug!(attempt, key = %anchor.key, "creation lost the write race; revalidating");
                    last_error = Some(error);
                    if let Some(delay) = retry_delay(attempt, true) {
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    break;
                }
                Err(other) => return Err(other.into()),
            }
        }
        Err(last_error
            .map(IamGraphError::from)
            .unwrap_or_else(|| IamGraphError::Corrupt {
                key: anchor.key.clone(),
                detail: "the creation loop exhausted its attempts without an answer".to_owned(),
            }))
    }

    /// The guarded mutation: apply `effect` (already written against
    /// `uid(iv)`) only while the transaction's own snapshot still shows
    /// `expected_revision`. Returns whether the block applied, decided
    /// from the in-transaction state the answer carries.
    async fn cas_write(
        &self,
        anchor: &IamAnchor,
        expected_revision: u64,
        set_nquads: String,
        delete_nquads: String,
    ) -> Result<bool, IamGraphError> {
        let key_literal = dql_string(&anchor.key);
        let revision = expected_revision + 1;
        let mut block = GraphMutationBlock {
            cond: Some("eq(len(iv), 1)".to_owned()),
            set_nquads,
            delete_nquads,
        };
        block.set_nquads.push_str(&format!(
            "uid(iv) <hodei.identity.revision> \"{revision}\" .\n"
        ));
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ iv as var(func: eq(hodei.identity.key, {key_literal})) \
                 @filter(eq(hodei.identity.revision, {expected_revision}))\n \
                 idstate(func: eq(hodei.identity.key, {key_literal})) {{ \
                 hodei.identity.revision }} }}"
            ),
            blocks: vec![block],
            commit_now: true,
        };
        match self.client.upsert(transaction).await {
            Ok(response) => Ok(self.in_tx_revision_matches(&response.json, expected_revision)?),
            Err(GraphClientError::UpsertConflict { .. }) => Ok(false),
            Err(other) => Err(other.into()),
        }
    }

    /// Whether the CAS block applied: the readable state block carries
    /// the in-transaction revision BEFORE the mutation, so `R` after a
    /// committed transaction means the block was skipped.
    fn in_tx_revision_matches(
        &self,
        json: &Value,
        expected_revision: u64,
    ) -> Result<bool, IamGraphError> {
        let nodes = json
            .get("idstate")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            0 => Ok(false),
            1 => {
                let in_tx = nodes[0]
                    .get("hodei.identity.revision")
                    .and_then(Value::as_u64);
                Ok(in_tx == Some(expected_revision))
            }
            n => Err(IamGraphError::Corrupt {
                key: String::new(),
                detail: format!("{n} state nodes answered the in-transaction check"),
            }),
        }
    }

    /// The unconditional guarded write (port of the Surreal `update`
    /// semantics, which carries no version): read the current revision,
    /// apply under it, retry a bounded number of times if a concurrent
    /// commit moved the aggregate. `None` when the node does not exist.
    async fn advance(
        &self,
        anchor: &IamAnchor,
        mut set_nquads: String,
        delete_nquads: String,
    ) -> Result<Option<IamNode>, IamGraphError> {
        // The revision line is appended by `cas_write`; the payload here
        // must not contain it.
        set_nquads.push_str(&format!(
            "uid(iv) <hodei.iam.updated_at> {} .\n",
            dql_string(&mark(chrono::Utc::now()))
        ));
        for attempt in 1..=MAX_ATTEMPTS {
            let Some(current) = self.node(anchor).await? else {
                return Ok(None);
            };
            if self
                .cas_write(
                    anchor,
                    current.revision,
                    set_nquads.clone(),
                    delete_nquads.clone(),
                )
                .await?
            {
                // The state after the commit decides the answer.
                let after = self
                    .node(anchor)
                    .await?
                    .filter(|node| node.revision == current.revision + 1)
                    .ok_or_else(|| IamGraphError::Corrupt {
                        key: anchor.key.clone(),
                        detail: "the CAS block applied but the revision did not advance".to_owned(),
                    })?;
                return Ok(Some(after));
            }
            if let Some(delay) = retry_delay(attempt, true) {
                tokio::time::sleep(delay).await;
                continue;
            }
            break;
        }
        Err(IamGraphError::Corrupt {
            key: anchor.key.clone(),
            detail: "the guarded update lost every attempt of its bounded retry".to_owned(),
        })
    }

    /// The versioned guarded write: commit only if the aggregate is
    /// exactly at `expected_revision`. The outcome distinguishes the
    /// three answers the ports need: the winner sees `Applied`, a stale
    /// expectation or the loser of a race sees `Rejected` (mapped to the
    /// typed version conflict), and a key with no node at all sees
    /// `Missing` (mapped to not-found).
    pub(crate) async fn cas_advance(
        &self,
        anchor: &IamAnchor,
        expected_revision: u64,
        set_nquads: String,
        delete_nquads: String,
    ) -> Result<CasOutcome, IamGraphError> {
        let Some(current) = self.node(anchor).await? else {
            return Ok(CasOutcome::Missing);
        };
        if current.revision != expected_revision {
            return Ok(CasOutcome::Rejected);
        }
        let mut set = set_nquads;
        set.push_str(&format!(
            "uid(iv) <hodei.iam.updated_at> {} .\n",
            dql_string(&mark(chrono::Utc::now()))
        ));
        for attempt in 1..=MAX_ATTEMPTS {
            if self
                .cas_write(
                    anchor,
                    expected_revision,
                    set.clone(),
                    delete_nquads.clone(),
                )
                .await?
            {
                let after = self
                    .node(anchor)
                    .await?
                    .filter(|node| node.revision == expected_revision + 1)
                    .ok_or_else(|| IamGraphError::Corrupt {
                        key: anchor.key.clone(),
                        detail: "the CAS block applied but the revision did not advance".to_owned(),
                    })?;
                return Ok(CasOutcome::Applied(Box::new(after)));
            }
            // A skipped block under a still-correct pre-read means a
            // concurrent writer moved the aggregate: re-check and, if it
            // moved, this expectation is genuinely stale.
            let Some(current) = self.node(anchor).await? else {
                return Ok(CasOutcome::Missing);
            };
            if current.revision != expected_revision {
                return Ok(CasOutcome::Rejected);
            }
            if let Some(delay) = retry_delay(attempt, true) {
                tokio::time::sleep(delay).await;
                continue;
            }
            break;
        }
        Ok(CasOutcome::Rejected)
    }

    /// The delete write: drop the payload and the identity anchor of the
    /// node in one guarded transaction, so the key frees for a later
    /// recreation (the Surreal `delete` removes the whole record).
    async fn delete_node(&self, anchor: &IamAnchor) -> Result<bool, IamGraphError> {
        let key_literal = dql_string(&anchor.key);
        let mut delete = String::new();
        for predicate in [
            "hodei.identity.key",
            "hodei.identity.hrn",
            "hodei.identity.tenant",
            "hodei.identity.digest",
            "hodei.identity.name",
            "hodei.identity.revision",
            "hodei.identity.created_at",
            "hodei.iam.kind",
            "hodei.iam.content",
            "hodei.iam.description",
            "hodei.iam.email",
            "hodei.iam.tags",
            "hodei.iam.attachment",
            "hodei.iam.created_at",
            "hodei.iam.updated_at",
        ] {
            delete.push_str(&format!("uid(d) <{predicate}> * .\n"));
        }
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ d as var(func: eq(hodei.identity.key, {key_literal}))\n \
                 dstage(func: eq(hodei.identity.key, {key_literal})) {{ uid }} }}"
            ),
            blocks: vec![GraphMutationBlock {
                cond: Some("eq(len(d), 1)".to_owned()),
                set_nquads: String::new(),
                delete_nquads: delete,
            }],
            commit_now: true,
        };
        match self.client.upsert(transaction).await {
            Ok(response) => {
                // The readable root saw the node at transaction start;
                // an absent one means there was nothing to delete — the
                // post-commit read alone cannot tell that apart from a
                // successful delete.
                let present = response
                    .json
                    .get("dstage")
                    .and_then(Value::as_array)
                    .is_some_and(|rows| !rows.is_empty());
                if !present {
                    return Ok(false);
                }
                // Success is the state after the commit, never the
                // commit itself: a concurrent recreate must survive.
                Ok(self.node(anchor).await?.is_none())
            }
            Err(other) => Err(other.into()),
        }
    }

    /// The full node document of a policy payload (create path).
    fn policy_payload(
        content: &str,
        description: Option<&str>,
        attachments: &[String],
        created_at: &str,
    ) -> String {
        let mut quads = format!("_:i <hodei.iam.content> {} .\n", dql_string(content),);
        if let Some(description) = description {
            quads.push_str(&format!(
                "_:i <hodei.iam.description> {} .\n",
                dql_string(description)
            ));
        }
        for attachment in attachments {
            quads.push_str(&format!(
                "_:i <hodei.iam.attachment> {} .\n",
                dql_string(attachment)
            ));
        }
        quads.push_str(&format!(
            "_:i <hodei.iam.created_at> {} .\n_:i <hodei.iam.updated_at> {} .\n",
            dql_string(created_at),
            dql_string(created_at),
        ));
        quads
    }

    /// The principal HRNs attached to one principal (effective-policy
    /// lookup, the graph shape of `SELECT ... WHERE $principal IN
    /// attached_principals`).
    async fn policies_attached_to(
        &self,
        principal_hrn: &str,
    ) -> Result<Vec<IamNode>, IamGraphError> {
        let query = format!(
            "{{ q(func: eq(hodei.iam.attachment, {})) @filter(eq(hodei.iam.kind, \"Policy\")) \
             {{ {NODE_FIELDS} }} }}",
            dql_string(principal_hrn)
        );
        let response = self.client.execute_dql(&query).await?;
        let nodes = response
            .json
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        nodes
            .iter()
            .map(|node| {
                let key = node
                    .get("hodei.identity.key")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                IamNode::parse(key, node)
            })
            .collect()
    }

    /// The paginated node list. The page and the total come back in one
    /// query: the server does the filtering, counting and offsetting —
    /// no in-process filtering anywhere.
    ///
    /// The narrowing predicate goes in `@filter`, never inside `and()`
    /// in `func:`: v25.5.1 rejects a nested `eq` there ("Only
    /// val/count/len/uid allowed as function within another"), while
    /// func + filter on both roots answers page and total in one round
    /// trip.
    async fn list_nodes(
        &self,
        func: &str,
        filter: Option<&str>,
        query: &ListPoliciesQuery,
    ) -> Result<(Vec<IamNode>, usize), IamGraphError> {
        let clause = filter.unwrap_or("");
        let dql = format!(
            "{{ page(func: {func}, orderasc: hodei.identity.key, first: {}, offset: {}) {clause} \
             {{ {NODE_FIELDS} }} total(func: {func}) {clause} {{ count(uid) }} }}",
            query.limit, query.offset
        );
        let response = self.client.execute_dql(&dql).await?;
        let nodes = response
            .json
            .get("page")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let parsed: Vec<IamNode> = nodes
            .iter()
            .map(|node| {
                let key = node
                    .get("hodei.identity.key")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                IamNode::parse(key, node)
            })
            .collect::<Result<_, _>>()?;
        let total = response
            .json
            .get("total")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("count(uid)").or_else(|| row.get("count")))
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        Ok((parsed, total))
    }

    /// The revision an existing policy aggregate carries, when it exists.
    pub async fn policy_revision(&self, policy_id: &str) -> Result<Option<u64>, IamGraphError> {
        let anchor = IamAnchor::policy(policy_id)?;
        Ok(self.node(&anchor).await?.map(|node| node.revision))
    }
}

/// The N-Quads the payload part of a guarded IAM update writes (the
/// revision and update mark are appended by the guard).
fn iam_set_lines(
    content: Option<&str>,
    description: Option<Option<&str>>,
    email: Option<&str>,
    tags: Option<&[String]>,
    attachments: Option<&[String]>,
) -> (String, String) {
    let mut set = String::new();
    let mut delete = String::new();
    if let Some(content) = content {
        set.push_str(&format!(
            "uid(iv) <hodei.iam.content> {} .\n",
            dql_string(content)
        ));
    }
    if let Some(description) = description {
        // `hodei.iam.description` is single-valued: a set line replaces
        // the stored value (proven against v25.5.1). The delete-all is
        // only for clearing it — issuing both in one block loses the
        // write (the server's delete pass runs after the set pass for
        // same-predicate lines), which is why the list predicates are
        // the only ones carrying a delete-all alongside their sets.
        match description {
            Some(description) => set.push_str(&format!(
                "uid(iv) <hodei.iam.description> {} .\n",
                dql_string(description)
            )),
            None => delete.push_str("uid(iv) <hodei.iam.description> * .\n"),
        }
    }
    if let Some(email) = email {
        set.push_str(&format!(
            "uid(iv) <hodei.iam.email> {} .\n",
            dql_string(email)
        ));
    }
    if let Some(tags) = tags {
        delete.push_str("uid(iv) <hodei.iam.tags> * .\n");
        for tag in tags {
            set.push_str(&format!("uid(iv) <hodei.iam.tags> {} .\n", dql_string(tag)));
        }
    }
    if let Some(attachments) = attachments {
        delete.push_str("uid(iv) <hodei.iam.attachment> * .\n");
        for attachment in attachments {
            set.push_str(&format!(
                "uid(iv) <hodei.iam.attachment> {} .\n",
                dql_string(attachment)
            ));
        }
    }
    (set, delete)
}

// ============================================================================
// Policy ports
// ============================================================================

#[async_trait]
impl CreatePolicyPort for DgraphIamAdapter {
    async fn create(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<CreatedPolicy, CreatePolicyError> {
        info!("Creating policy with ID: {}", command.policy_id);
        let anchor = IamAnchor::policy(&command.policy_id).map_err(|e| match e {
            IamGraphError::Key(key) => CreatePolicyError::InvalidHrn(key.to_string()),
            other => CreatePolicyError::StorageError(other.to_string()),
        })?;
        let created_at = mark(chrono::Utc::now());
        let payload = DgraphIamAdapter::policy_payload(
            &command.policy_content,
            command.description.as_deref(),
            &command.attached_principals,
            &created_at,
        );
        match self
            .create_node(&anchor, &command.policy_id, &payload)
            .await
            .map_err(|e| CreatePolicyError::StorageError(e.to_string()))?
        {
            Some(_) => {
                // The key answered before this transaction could create:
                // the Surreal unique record id says the same thing.
                warn!("Policy already exists: {}", command.policy_id);
                Err(CreatePolicyError::PolicyAlreadyExists(command.policy_id))
            }
            None => {
                // The node is this attempt's creation; the marks it
                // carries are the ones written above.
                Ok(CreatedPolicy {
                    policy: HodeiPolicy::new(
                        PolicyId::new(command.policy_id),
                        command.policy_content,
                    ),
                    created_at: parse_mark(Some(created_at.clone())),
                    updated_at: parse_mark(Some(created_at)),
                })
            }
        }
    }
}

#[async_trait]
impl PolicyReader for DgraphIamAdapter {
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<GetPolicyView, GetPolicyError> {
        debug!("Getting policy by HRN: {}", hrn);
        let anchor = IamAnchor::policy(hrn.resource_id())
            .map_err(|e| GetPolicyError::RepositoryError(e.to_string()))?;
        match self.node(&anchor).await {
            Ok(Some(node)) => Ok(GetPolicyView {
                hrn: hrn.clone(),
                name: node.short_id(),
                content: node.content.unwrap_or_default(),
                description: node.description,
                attached_principals: node.attachments,
                created_at: parse_mark(node.created_at),
                updated_at: parse_mark(node.updated_at),
            }),
            Ok(None) => {
                warn!("Policy not found: {}", hrn);
                Err(GetPolicyError::PolicyNotFound(hrn.to_string()))
            }
            Err(e) => {
                error_store("getting policy", &e.to_string());
                Err(GetPolicyError::RepositoryError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl PolicyLister for DgraphIamAdapter {
    async fn list(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError> {
        info!(
            "Listing policies with limit={}, offset={}",
            query.limit, query.offset
        );
        let (nodes, total) = self
            .list_nodes(
                &format!("eq(hodei.iam.kind, \"{KIND_POLICY}\")"),
                None,
                &query,
            )
            .await
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?;
        let policies = nodes
            .into_iter()
            .map(|node| {
                let short = node.short_id();
                PolicySummary {
                    hrn: Hrn::new(
                        "hodei".to_string(),
                        "iam".to_string(),
                        DEFAULT_ACCOUNT.to_string(),
                        KIND_POLICY.to_string(),
                        short.clone(),
                    ),
                    name: short,
                    description: node.description,
                    created_at: parse_mark(node.created_at),
                    updated_at: parse_mark(node.updated_at),
                }
            })
            .collect();
        Ok(paged(policies, total, &query))
    }

    async fn list_for_account(
        &self,
        account: &str,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError> {
        // The listing filters, counts and paginates inside the server,
        // scoped by the tenant half of the key: another account's
        // policies are not in the candidate set at all. The kind
        // narrowing rides in @filter (see [`Self::list_nodes`]).
        let func = format!("eq(hodei.identity.tenant, {})", dql_string(account));
        let (nodes, total) = self
            .list_nodes(
                &func,
                Some(&format!("@filter(eq(hodei.iam.kind, \"{KIND_POLICY}\"))")),
                &query,
            )
            .await
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?;
        let policies = nodes
            .into_iter()
            .map(|node| {
                // The stored id is the scoped `{account}~{short}`; the
                // Surreal listing answers with the short half under the
                // account slot of the HRN.
                let stored = node.short_id();
                let short = stored
                    .split_once('~')
                    .map(|(_, id)| id.to_owned())
                    .unwrap_or(stored);
                PolicySummary {
                    hrn: Hrn::new(
                        "hodei".to_string(),
                        "iam".to_string(),
                        account.to_string(),
                        KIND_POLICY.to_string(),
                        short.clone(),
                    ),
                    name: short,
                    description: node.description,
                    created_at: parse_mark(node.created_at),
                    updated_at: parse_mark(node.updated_at),
                }
            })
            .collect();
        Ok(paged(policies, total, &query))
    }
}

fn paged(
    policies: Vec<PolicySummary>,
    total: usize,
    query: &ListPoliciesQuery,
) -> ListPoliciesResponse {
    ListPoliciesResponse {
        policies,
        total_count: total,
        has_next_page: query.offset + query.limit < total,
        has_previous_page: query.offset > 0,
    }
}

#[async_trait]
impl UpdatePolicyPort for DgraphIamAdapter {
    async fn update(
        &self,
        command: UpdatePolicyCommand,
    ) -> Result<UpdatePolicyView, UpdatePolicyError> {
        info!("Updating policy: {}", command.policy_id);
        self.apply_update(command, None).await
    }
}

#[async_trait]
impl DeletePolicyPort for DgraphIamAdapter {
    async fn delete(&self, policy_id: &str) -> Result<(), DeletePolicyError> {
        info!("Deleting policy: {policy_id}");
        let anchor = IamAnchor::policy(policy_id)
            .map_err(|e| DeletePolicyError::StorageError(e.to_string()))?;
        match self
            .delete_node(&anchor)
            .await
            .map_err(|e| DeletePolicyError::StorageError(e.to_string()))?
        {
            true => Ok(()),
            false => {
                warn!("Policy not found for deletion: {policy_id}");
                Err(DeletePolicyError::PolicyNotFound(policy_id.to_string()))
            }
        }
    }
}

#[async_trait]
impl PolicyFinderPort for DgraphIamAdapter {
    async fn find_policies_by_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<HodeiPolicy>, GetEffectivePoliciesError> {
        debug!("Finding policies for principal: {}", principal_hrn);
        let text = principal_hrn.to_string();
        let nodes = self
            .policies_attached_to(&text)
            .await
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;
        Ok(nodes
            .into_iter()
            .map(|node| {
                HodeiPolicy::new(
                    PolicyId::new(node.short_id()),
                    node.content.unwrap_or_default(),
                )
            })
            .collect())
    }
}

#[async_trait]
impl IamPolicyFinderPort for DgraphIamAdapter {
    async fn get_effective_policies(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<HodeiPolicySet, PolicyFinderError> {
        debug!("Finding effective IAM policies for: {}", principal_hrn);
        // Only the policies ATTACHED to the principal are effective; a
        // transport failure is an error, never an empty set (the caller
        // denies by error, and an empty set is only ever the truth).
        let text = principal_hrn.to_string();
        let nodes = self
            .policies_attached_to(&text)
            .await
            .map_err(|e| PolicyFinderError::RepositoryError(e.to_string()))?;
        let policies: Vec<HodeiPolicy> = nodes
            .into_iter()
            .map(|node| {
                HodeiPolicy::new(
                    PolicyId::new(node.short_id()),
                    node.content.unwrap_or_default(),
                )
            })
            .collect();
        let policy_set = HodeiPolicySet::new(policies);
        info!(
            "Found {} effective IAM policies for: {}",
            policy_set.len(),
            principal_hrn
        );
        Ok(policy_set)
    }
}

// ============================================================================
// CAS activation (AC#2): the versioned policy update
// ============================================================================

impl DgraphIamAdapter {
    /// Activates a policy version under the CAS guard: the write commits
    /// only if the aggregate is exactly at `expected_revision`. The loser
    /// of the race — or a stale expectation — gets
    /// [`UpdatePolicyError::VersionConflict`], which the HTTP edge maps
    /// to 409. No admin surface is added: this is the same payload the
    /// unversioned update writes, guarded by the revision the graph
    /// already maintains.
    pub async fn activate_policy(
        &self,
        command: UpdatePolicyCommand,
        expected_revision: u64,
    ) -> Result<UpdatePolicyView, UpdatePolicyError> {
        info!(
            "Activating policy {} at revision {expected_revision}",
            command.policy_id
        );
        self.apply_update(command, Some(expected_revision)).await
    }

    async fn apply_update(
        &self,
        command: UpdatePolicyCommand,
        expected_revision: Option<u64>,
    ) -> Result<UpdatePolicyView, UpdatePolicyError> {
        let anchor = IamAnchor::policy(&command.policy_id)
            .map_err(|e| UpdatePolicyError::StorageError(e.to_string()))?;
        let (set, delete) = iam_set_lines(
            command.policy_content.as_deref(),
            // `None` leaves the stored description alone, as the Surreal
            // update does; only an explicit value replaces it.
            command.description.as_deref().map(Some),
            None,
            None,
            // The unversioned Surreal update leaves the attachment alone;
            // `None` means "do not touch the list".
            None,
        );
        let outcome = match expected_revision {
            Some(revision) => match self
                .cas_advance(&anchor, revision, set, delete)
                .await
                .map_err(|e| UpdatePolicyError::StorageError(e.to_string()))?
            {
                CasOutcome::Applied(node) => Ok(Some(*node)),
                // AC#2: the loser of a versioned activation sees the
                // typed conflict, never a not-found.
                CasOutcome::Rejected => Err(UpdatePolicyError::VersionConflict),
                CasOutcome::Missing => {
                    Err(UpdatePolicyError::PolicyNotFound(command.policy_id.clone()))
                }
            },
            None => self
                .advance(&anchor, set, delete)
                .await
                .map_err(|e| UpdatePolicyError::StorageError(e.to_string())),
        }?;
        match outcome {
            Some(node) => Ok(UpdatePolicyView {
                hrn: Hrn::new(
                    "hodei".to_string(),
                    "iam".to_string(),
                    DEFAULT_ACCOUNT.to_string(),
                    KIND_POLICY.to_string(),
                    command.policy_id.clone(),
                ),
                name: command.policy_id.clone(),
                content: node.content.unwrap_or_default(),
                description: node.description,
                created_at: parse_mark(node.created_at),
                updated_at: parse_mark(node.updated_at),
            }),
            None => Err(UpdatePolicyError::PolicyNotFound(command.policy_id)),
        }
    }
}

// ============================================================================
// User ports
// ============================================================================

#[async_trait]
impl CreateUserPort for DgraphIamAdapter {
    async fn save_user(&self, user_dto: &CreateUserPersistenceDto) -> Result<(), CreateUserError> {
        info!("Saving user with HRN: {}", user_dto.hrn);
        let hrn = Hrn::from_string(&user_dto.hrn)
            .ok_or_else(|| CreateUserError::PersistenceError("Invalid HRN".to_string()))?;
        let anchor = IamAnchor::principal(&hrn)
            .map_err(|e| CreateUserError::PersistenceError(e.to_string()))?;
        let created_at = mark(chrono::Utc::now());
        let mut payload = String::new();
        if !user_dto.email.is_empty() {
            payload.push_str(&format!(
                "_:i <hodei.iam.email> {} .\n",
                dql_string(&user_dto.email)
            ));
        }
        for tag in &user_dto.tags {
            payload.push_str(&format!("_:i <hodei.iam.tags> {} .\n", dql_string(tag)));
        }
        for group in &user_dto.group_hrns {
            payload.push_str(&format!(
                "_:i <hodei.iam.attachment> {} .\n",
                dql_string(group)
            ));
        }
        payload.push_str(&format!(
            "_:i <hodei.iam.created_at> {} .\n_:i <hodei.iam.updated_at> {} .\n",
            dql_string(&created_at),
            dql_string(&created_at),
        ));
        match self
            .create_node(&anchor, &user_dto.name, &payload)
            .await
            .map_err(|e| CreateUserError::PersistenceError(e.to_string()))?
        {
            Some(_) => Err(CreateUserError::PersistenceError(format!(
                "user {} already exists",
                user_dto.hrn
            ))),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl UserGroupPersister for DgraphIamAdapter {
    async fn save_user(&self, user_dto: &AddUserPersistenceDto) -> Result<(), AddUserToGroupError> {
        info!("Updating user with HRN: {}", user_dto.hrn);
        let hrn = Hrn::from_string(&user_dto.hrn)
            .ok_or_else(|| AddUserToGroupError::PersistenceError("Invalid HRN".to_string()))?;
        let anchor = IamAnchor::principal(&hrn)
            .map_err(|e| AddUserToGroupError::PersistenceError(e.to_string()))?;
        let (set, delete) = iam_set_lines(
            None,
            None,
            Some(&user_dto.email),
            Some(&user_dto.tags),
            Some(&user_dto.group_hrns),
        );
        match self
            .advance(&anchor, set, delete)
            .await
            .map_err(|e| AddUserToGroupError::PersistenceError(e.to_string()))?
        {
            Some(_) => Ok(()),
            None => Err(AddUserToGroupError::PersistenceError(format!(
                "User {} not found",
                user_dto.hrn
            ))),
        }
    }
}

#[async_trait]
impl UserFinder for DgraphIamAdapter {
    async fn find_user_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<AddUserLookupDto>, AddUserToGroupError> {
        debug!("Finding user by HRN: {}", hrn);
        self.user_lookup(hrn)
            .await
            .map_err(|e| AddUserToGroupError::PersistenceError(e.to_string()))
    }
}

#[async_trait]
impl UserFinderPort for DgraphIamAdapter {
    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<EffectiveUserLookupDto>, GetEffectivePoliciesError> {
        debug!("Finding user by HRN for policy lookup: {}", hrn);
        let anchor = IamAnchor::principal(hrn)
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;
        match self.node(&anchor).await {
            Ok(Some(node)) => Ok(Some(EffectiveUserLookupDto {
                hrn: node.display_hrn(),
                name: node.short_id(),
                email: node.email.unwrap_or_default(),
                group_hrns: node.attachments,
                tags: node.tags,
            })),
            Ok(None) => Ok(None),
            Err(e) => Err(GetEffectivePoliciesError::RepositoryError(e.to_string())),
        }
    }
}

impl DgraphIamAdapter {
    async fn user_lookup(&self, hrn: &Hrn) -> Result<Option<AddUserLookupDto>, IamGraphError> {
        let anchor = IamAnchor::principal(hrn)?;
        match self.node(&anchor).await? {
            Some(node) => Ok(Some(AddUserLookupDto {
                hrn: node.display_hrn(),
                name: node.short_id(),
                email: node.email.unwrap_or_default(),
                group_hrns: node.attachments,
                tags: node.tags,
            })),
            None => Ok(None),
        }
    }
}

// ============================================================================
// Group ports
// ============================================================================

#[async_trait]
impl CreateGroupPort for DgraphIamAdapter {
    async fn save_group(&self, group_dto: &GroupPersistenceDto) -> Result<(), CreateGroupError> {
        info!("Saving group with HRN: {}", group_dto.hrn);
        let hrn = Hrn::from_string(&group_dto.hrn)
            .ok_or_else(|| CreateGroupError::PersistenceError("Invalid HRN".to_string()))?;
        let anchor = IamAnchor::principal(&hrn)
            .map_err(|e| CreateGroupError::PersistenceError(e.to_string()))?;
        let created_at = mark(chrono::Utc::now());
        let mut payload = String::new();
        for tag in &group_dto.tags {
            payload.push_str(&format!("_:i <hodei.iam.tags> {} .\n", dql_string(tag)));
        }
        payload.push_str(&format!(
            "_:i <hodei.iam.created_at> {} .\n_:i <hodei.iam.updated_at> {} .\n",
            dql_string(&created_at),
            dql_string(&created_at),
        ));
        match self
            .create_node(&anchor, &group_dto.name, &payload)
            .await
            .map_err(|e| CreateGroupError::PersistenceError(e.to_string()))?
        {
            Some(_) => Err(CreateGroupError::PersistenceError(format!(
                "group {} already exists",
                group_dto.hrn
            ))),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl GroupFinder for DgraphIamAdapter {
    async fn find_group_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<AddGroupLookupDto>, AddUserToGroupError> {
        debug!("Finding group by HRN: {}", hrn);
        let anchor = IamAnchor::principal(hrn)
            .map_err(|e| AddUserToGroupError::PersistenceError(e.to_string()))?;
        match self
            .node(&anchor)
            .await
            .map_err(|e| AddUserToGroupError::PersistenceError(e.to_string()))?
        {
            Some(node) if node.kind == KIND_GROUP => Ok(Some(AddGroupLookupDto {
                hrn: node.display_hrn(),
                name: node.short_id(),
                tags: node.tags,
            })),
            Some(_) => Ok(None),
            None => Ok(None),
        }
    }
}

#[async_trait]
impl GroupFinderPort for DgraphIamAdapter {
    async fn find_groups_by_user_hrn(
        &self,
        user_hrn: &Hrn,
    ) -> Result<Vec<GroupLookupDto>, GetEffectivePoliciesError> {
        info!("Finding groups for user: {}", user_hrn);
        // The Surreal adapter answers with every group (membership is
        // tracked on the user, not on the group); the graph port keeps
        // that observable behaviour and the use case intersects it with
        // the user's own group list before evaluating.
        let _ = user_hrn;
        let query =
            format!("{{ q(func: eq(hodei.iam.kind, \"{KIND_GROUP}\")) {{ {NODE_FIELDS} }} }}");
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        nodes
            .iter()
            .map(|node| {
                let key = node
                    .get("hodei.identity.key")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                IamNode::parse(key, node).map(|node| GroupLookupDto {
                    hrn: node.display_hrn(),
                    name: node.short_id(),
                    tags: node.tags,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))
    }
}

/// One error line per storage failure, naming the operation and the
/// typed cause — never the credential-bearing detail.
fn error_store(operation: &str, detail: &str) {
    warn!("Database error while {operation} policy: {detail}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stored mark and the response have to be the same text — the
    /// same invariant the Surreal policy adapter pins, ported verbatim:
    /// the Dgraph payload stores this exact text, so what a response
    /// says is what the database holds.
    #[test]
    fn the_stored_mark_is_the_text_the_response_says() {
        let awkward = [
            "2023-11-14T22:13:20.000000000Z",
            "2023-11-14T22:13:20.123456000Z",
            "2023-11-14T22:13:20.123456789Z",
            "2023-11-14T22:13:20.000001000Z",
            "2023-11-14T22:13:20.100000000Z",
        ];
        for text in awkward {
            let instant = chrono::DateTime::parse_from_rfc3339(text)
                .expect("a timestamp")
                .with_timezone(&chrono::Utc);

            let stored = mark(instant);
            let said = serde_json::to_string(&instant).expect("a serialised timestamp");
            let said = said.trim_matches('"');

            assert_eq!(
                stored, said,
                "the stored mark and the response must be the same text for {text}"
            );
            assert_eq!(parse_mark(Some(stored)), Some(instant));
        }
    }

    /// The anchor derivation mirrors the Surreal record-id rule exactly:
    /// the account prefix before `~` is the tenant, the whole scoped id
    /// is the canonical resource, and an unscoped id falls back to
    /// `default`.
    #[test]
    fn the_policy_anchor_is_the_surreal_record_id_rule() {
        let scoped = IamAnchor::policy("acct-a~my-policy").expect("valid");
        assert_eq!(scoped.tenant, "acct-a");
        assert_eq!(
            scoped.canonical,
            "hrn:hodei:iam:default:Policy/acct-a~my-policy"
        );
        assert_eq!(
            scoped.key,
            "acct-a|hrn:hodei:iam:default:Policy/acct-a~my-policy"
        );

        let unscoped = IamAnchor::policy("legacy").expect("valid");
        assert_eq!(unscoped.tenant, "default");
        assert_eq!(unscoped.key, "default|hrn:hodei:iam:default:Policy/legacy");
    }

    /// Policies and principals under the same short id cannot collide:
    /// the kind is part of the canonical HRN, therefore of the key.
    #[test]
    fn the_kinds_cannot_collide_under_one_id() {
        let policy = IamAnchor::policy("acct|x").expect_err("the key separator is illegal");
        let _ = policy;
        let user = IamAnchor::new(KIND_USER, "acct-a", "alice").expect("valid");
        let group = IamAnchor::new(KIND_GROUP, "acct-a", "alice").expect("valid");
        assert_ne!(user.key, group.key);
    }

    /// The retry is bounded and never retries a non-retryable failure.
    #[test]
    fn the_cas_retry_is_bounded_and_backs_off() {
        assert!(retry_delay(1, true).is_some());
        assert!(retry_delay(MAX_ATTEMPTS - 1, true).is_some());
        assert!(retry_delay(MAX_ATTEMPTS, true).is_none());
        assert!(retry_delay(1, false).is_none());
    }

    /// A list replacement deletes the whole list and sets the new one in
    /// the same guarded block, so a stale list can never survive a
    /// committed update.
    #[test]
    fn attachment_replacement_deletes_before_setting() {
        let (set, delete) = iam_set_lines(None, None, None, None, Some(&["hrn:x".to_owned()]));
        assert!(delete.contains("<hodei.iam.attachment> *"));
        assert!(set.contains("<hodei.iam.attachment> \"hrn:x\""));
        // `None` leaves the list untouched.
        let (set, delete) = iam_set_lines(None, None, None, None, None);
        assert!(delete.is_empty());
        assert!(set.is_empty());
    }
}
