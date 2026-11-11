//! The projection of the ledger into the graph, with su checkpoint.
//!
//! The projector is run over the ledger, and it has to be able to fail: a
//! projection that cannot fail is a projection that cannot be recovered.
//! When it fails, the events stay where they are (they are durable), the
//! checkpoint does not move (so the lag is visible), and a later run picks
//! up where it left off.
//!
//! The checkpoint says **the next position to project**. It moves only when
//! the change set has been applied, so an event is never applied twice by
//! the same consumer and never skipped.

use crate::GraphChangeSet;
use crate::ports::graph::{GraphError, GraphProjector, GraphWriter};
use crate::ports::{EventLedger, LedgerCursor, LedgerError};
use std::sync::Arc;

/// How many events are read at a time.
const PAGE: usize = 256;

/// The error of a projection that could not be carried out.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectionError {
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

/// What a projection did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectionReport {
    /// The events that were read.
    pub events_read: usize,
    /// The events that changed the graph.
    pub events_applied: usize,
    /// The events that did not change anything.
    pub events_ignored: usize,
    /// The position the projection is at now.
    pub checkpoint: u64,
}

/// The checkpoints of the projections.
///
/// One per consumer and stream: two consumers of the same stream are two
/// projections, and each one has to say where it is.
#[async_trait::async_trait]
pub trait ProjectionCheckpoints: Send + Sync {
    /// Where a consumer is, or nothing when it has not started.
    async fn checkpoint(&self, consumer: &str, stream_id: &str) -> Result<Option<u64>, GraphError>;

    /// Says where a consumer is.
    async fn set_checkpoint(
        &self,
        consumer: &str,
        stream_id: &str,
        position: u64,
    ) -> Result<(), GraphError>;
}

/// Projects a stream into the graph, from where the checkpoint left it.
pub async fn project_stream<L, W, C>(
    ledger: &L,
    projector: &dyn GraphProjector,
    writer: &W,
    checkpoints: &C,
    consumer: &str,
    stream_id: &str,
) -> Result<ProjectionReport, ProjectionError>
where
    L: EventLedger + ?Sized,
    W: GraphWriter + ?Sized,
    C: ProjectionCheckpoints + ?Sized,
{
    let mut report = ProjectionReport::default();

    let start = checkpoints
        .checkpoint(consumer, stream_id)
        .await
        .map_err(graph_error)?
        .unwrap_or(0);
    report.checkpoint = start;

    let mut cursor = LedgerCursor::from_position(stream_id.to_string(), start);

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

            // A projector that fails leaves the event where it is: the
            // event is durable, and the checkpoint does not move, so the
            // lag is visible and a later run will try again.
            let changes = projector.project(event).map_err(projector_error)?;

            if changes.is_empty() {
                report.events_ignored += 1;
            } else {
                writer.apply(changes).await.map_err(graph_error)?;
                report.events_applied += 1;
            }

            // The checkpoint moves once the event has been dealt with.
            let next = event.stream_position() + 1;
            checkpoints
                .set_checkpoint(consumer, stream_id, next)
                .await
                .map_err(graph_error)?;
            report.checkpoint = next;
        }

        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = next;
    }

    Ok(report)
}

/// The error of a ledger that could not be read.
fn ledger_error(error: LedgerError) -> ProjectionError {
    ProjectionError::Ledger(error.to_string())
}

/// The error of a graph that could not be written.
fn graph_error(error: GraphError) -> ProjectionError {
    ProjectionError::Graph(error.to_string())
}

/// The error of a projector that could not project.
fn projector_error(error: crate::ports::graph::ProjectionError) -> ProjectionError {
    ProjectionError::Projector(error.to_string())
}

/// A graph writer and a checkpoint store that are the same thing.
///
/// It is a convenience for the wiring, not a rule: the ports stay separate,
/// so a deployment can keep the checkpoints somewhere else.
pub struct GraphProjection {
    ledger: Arc<dyn crate::ports::EventLedger>,
    projector: Arc<dyn GraphProjector>,
    writer: Arc<dyn GraphWriter>,
    checkpoints: Arc<dyn ProjectionCheckpoints>,
}

impl GraphProjection {
    /// The projection, over the four things it needs.
    pub fn new(
        ledger: Arc<dyn crate::ports::EventLedger>,
        projector: Arc<dyn GraphProjector>,
        writer: Arc<dyn GraphWriter>,
        checkpoints: Arc<dyn ProjectionCheckpoints>,
    ) -> Self {
        Self {
            ledger,
            projector,
            writer,
            checkpoints,
        }
    }

    /// Projects a stream.
    pub async fn project(
        &self,
        consumer: &str,
        stream_id: &str,
    ) -> Result<ProjectionReport, ProjectionError> {
        project_stream(
            self.ledger.as_ref(),
            self.projector.as_ref(),
            self.writer.as_ref(),
            self.checkpoints.as_ref(),
            consumer,
            stream_id,
        )
        .await
    }
}

/// A change set that is empty.
pub fn empty_changes() -> GraphChangeSet {
    GraphChangeSet::empty()
}
