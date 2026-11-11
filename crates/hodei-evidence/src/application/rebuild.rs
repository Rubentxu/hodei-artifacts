//! The reconstruction of the graph from the ledger.
//!
//! The graph is a projection, and a projection that cannot be rebuilt is a
//! second truth pretending to be a first one. The rebuild here proves the
//! opposite: it replays the ledger through the same projector and checks
//! that the graph it produces says the same things as the one in use.
//!
//! Two rules:
//!
//! - **The rebuild never touches the graph in use.** It builds into a
//!   projection of its own, compares, and the promotion is somebody else's
//!   decision.
//! - **The comparison is semantic.** Two graphs are the same when they hold
//!   the same nodes and the same relations with the same support, not when
//!   their bytes are equal: two stores write the same facts in a different
//!   order all the time.

use crate::domain::event::EventEnvelope;
use crate::ports::graph::{GraphError, GraphProjector, GraphQuery, GraphWriter};
use crate::ports::{EventLedger, LedgerCursor, LedgerError};
use kernel::Hrn;
use std::collections::BTreeSet;

/// How many events are read at a time.
const PAGE: usize = 256;

/// What a reconstruction did and what it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildReport {
    /// The events that were read.
    pub events_read: usize,
    /// The events that changed the graph.
    pub events_projected: usize,
    /// The nodes that were written.
    pub nodes_written: usize,
    /// The relations that were written.
    pub relations_written: usize,
    /// Whether the rebuilt graph says the same things as the one in use.
    pub equivalent: bool,
    /// What differs, when it differs.
    pub differences: Vec<String>,
}

/// The error of a reconstruction that could not be carried out.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RebuildError {
    /// The ledger could not be read.
    #[error("the ledger could not be read: {0}")]
    Ledger(String),
    /// The graph could not be written.
    #[error("the graph could not be written: {0}")]
    Graph(String),
    /// The projector could not project an event.
    #[error("the projector failed: {0}")]
    Projector(String),
}

/// Replays the ledger into a projection and compares it with the one in use.
///
/// The events are replayed through the same projector that built the graph
/// in the first place, because that is the only way the comparison means
/// something: a rebuild that used a different projector would prove that a
/// different graph can be built, not that this one can.
pub async fn rebuild_graph<L, W, Q>(
    ledger: &L,
    projector: &impl GraphProjector,
    target: &W,
    tenant: &Hrn,
    in_use: Option<&Q>,
) -> Result<RebuildReport, RebuildError>
where
    L: EventLedger + ?Sized,
    W: GraphWriter + GraphQuery + ?Sized,
    Q: GraphQuery + ?Sized,
{
    let mut report = RebuildReport {
        events_read: 0,
        events_projected: 0,
        nodes_written: 0,
        relations_written: 0,
        equivalent: true,
        differences: Vec::new(),
    };

    // The projection that is being built starts empty. The one in use is
    // never emptied: it is the only copy of the truth until the rebuilt one
    // has been checked.
    target.clear().await.map_err(graph_error)?;

    let stream = format!("tenant-{}", tenant.account_id());
    let mut cursor = LedgerCursor::start_of(stream.clone());

    loop {
        let page = ledger
            .read(cursor.clone(), PAGE)
            .await
            .map_err(ledger_error)?;
        if page.events.is_empty() {
            break;
        }

        for event in &page.events {
            report.events_read += 1;
            let changes = projector
                .project(event)
                .map_err(|error| RebuildError::Projector(error.to_string()))?;
            if changes.is_empty() {
                continue;
            }
            report.events_projected += 1;
            report.nodes_written += changes.nodes.len();
            report.relations_written += changes.relations.len();
            target.apply(changes).await.map_err(graph_error)?;
        }

        // The page says whether there is more to read.
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = next;
    }

    // The comparison, when there is a graph in use to compare with.
    if let Some(in_use) = in_use {
        let original = fingerprint(in_use, tenant).await.map_err(graph_error)?;
        let rebuilt = fingerprint(target, tenant).await.map_err(graph_error)?;
        let differences = compare(&original, &rebuilt);
        report.equivalent = differences.is_empty();
        report.differences = differences;
    }

    Ok(report)
}

/// What a graph holds, said in a way that can be compared.
///
/// It is a set, so the order in which the store returned the rows does not
/// matter: two graphs that hold the same facts are the same graph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphFingerprint {
    /// The nodes, as "kind|identity".
    pub nodes: BTreeSet<String>,
    /// The relations, as "type|from|to|evidence".
    pub relations: BTreeSet<String>,
}

/// The fingerprint of a graph, of an account.
pub async fn fingerprint<Q: GraphQuery + ?Sized>(
    graph: &Q,
    tenant: &Hrn,
) -> Result<GraphFingerprint, GraphError> {
    use crate::domain::graph::NodeKind;

    let mut fingerprint = GraphFingerprint::default();

    for kind in [
        NodeKind::Repository,
        NodeKind::SourceRevision,
        NodeKind::Build,
        NodeKind::Artifact,
        NodeKind::Deployment,
        NodeKind::Evidence,
    ] {
        for node in graph.nodes(tenant, kind).await? {
            fingerprint
                .nodes
                .insert(format!("{}|{}", node.kind.as_str(), node.id));
        }
    }

    // The relations are reached from the nodes: every relation starts or
    // ends at one of them.
    let mut seen = BTreeSet::new();
    for node in &fingerprint.nodes {
        let identity = node.split_once('|').map(|(_, id)| id).unwrap_or(node);
        let hrn = Hrn::from_string(identity).ok_or_else(|| {
            GraphError::Rejected(format!("the identity of a node is not an HRN: {identity}"))
        })?;
        for relation in graph.relations_from(tenant, &hrn).await? {
            seen.insert(format!(
                "{}|{}|{}|{}",
                relation.relation.as_str(),
                relation.from,
                relation.to,
                relation.evidence_id.as_str()
            ));
        }
        for relation in graph.relations_to(tenant, &hrn).await? {
            seen.insert(format!(
                "{}|{}|{}|{}",
                relation.relation.as_str(),
                relation.from,
                relation.to,
                relation.evidence_id.as_str()
            ));
        }
    }
    fingerprint.relations = seen;

    Ok(fingerprint)
}

/// The differences between two graphs, said one by one.
fn compare(original: &GraphFingerprint, rebuilt: &GraphFingerprint) -> Vec<String> {
    let mut differences = Vec::new();

    for node in original.nodes.difference(&rebuilt.nodes) {
        differences.push(format!("the rebuilt graph is missing the node {node}"));
    }
    for node in rebuilt.nodes.difference(&original.nodes) {
        differences.push(format!(
            "the rebuilt graph has a node that the original does not: {node}"
        ));
    }
    for relation in original.relations.difference(&rebuilt.relations) {
        differences.push(format!(
            "the rebuilt graph is missing the relation {relation}"
        ));
    }
    for relation in rebuilt.relations.difference(&original.relations) {
        differences.push(format!(
            "the rebuilt graph has a relation that the original does not: {relation}"
        ));
    }

    differences
}

/// The error of a ledger that could not be read.
fn ledger_error(error: LedgerError) -> RebuildError {
    RebuildError::Ledger(error.to_string())
}

/// The error of a graph that could not be written.
fn graph_error(error: GraphError) -> RebuildError {
    RebuildError::Graph(error.to_string())
}

/// The events of a stream, for the caller that wants to read them.
pub async fn read_events<L: EventLedger + ?Sized>(
    ledger: &L,
    stream: &str,
) -> Result<Vec<EventEnvelope>, RebuildError> {
    let mut events = Vec::new();
    let mut cursor = LedgerCursor::start_of(stream.to_string());
    loop {
        let page = ledger
            .read(cursor.clone(), PAGE)
            .await
            .map_err(ledger_error)?;
        if page.events.is_empty() {
            break;
        }
        events.extend(page.events.iter().cloned());
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = next;
    }
    Ok(events)
}
