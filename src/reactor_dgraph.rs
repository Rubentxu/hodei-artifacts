//! The reactive CVE cycle of the graph profile (HSP-314 F3b-3).
//!
//! This is the composition root of [`VulnerabilityReactor`] over the Dgraph
//! adapters, the counterpart of the embedded profile's wiring: the same
//! cycle, the same order of start. The reactor exists only when the trust
//! plane exists (without policies and decisions there is nothing to
//! reevaluate, and a cycle that decides on its own would be a second trust
//! plane in disguise), and `resume` runs BEFORE the state is handed to the
//! traffic: it is what makes a crash in the middle of an effect converge on
//! restart, and running it afterwards would leave a window in which the
//! service answers with the intents of the crash still said and not pursued.

use std::sync::Arc;
use std::time::Duration;

use hodei_trust_policy_storage::{DgraphDecisionStore, DgraphTrustStore};

use crate::app_state::AppState;
use crate::config::AppConfig;
use crate::trust::reaction::{DgraphReactorParts, VulnerabilityReactor};

/// Wires the reactive cycle of the graph profile.
///
/// The stores of trust are opened over the SAME alpha the profile already
/// uses (the plane and the serving ledger hold their own handles over it).
/// The concessions the cycle revokes are written through the ledger the
/// service serves from, so the cache of admissions sweeps the entry in the
/// same call: the pull after the revocation is refused, not "refused when
/// the entry expires".
///
/// # What is NOT covered
///
/// - Dgraph's `GraphWriter::apply` emits no relation deltas (only
///   `apply_and_checkpoint` does); the view is kept current by the
///   immediate `rebuild_advisory` of the advisory entry, and the
///   reconciliation pass reads whatever deltas the frontier holds.
/// - The reconciliation reads at most 512 deltas per pass with no cursor,
///   and the validity sweep at most 4096 rows (see the HSP-314 notes).
pub async fn wire_graph_reactor(
    state: AppState,
    config: &AppConfig,
    graph: &crate::profile::GraphAuthority,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    let (Some(plane), Some(ledger)) = (state.trust_plane.clone(), state.trust_ledger.clone())
    else {
        return Ok(state);
    };
    let trust = Arc::new(
        DgraphTrustStore::open(graph.client.clone(), graph.tenant.clone())
            .await
            .map_err(|error| format!("the trust store of the reactor could not open: {error}"))?,
    );
    let decisions = Arc::new(
        DgraphDecisionStore::open(graph.client.clone())
            .await
            .map_err(|error| {
                format!("the decision store of the reactor could not open: {error}")
            })?,
    );
    let reactor = Arc::new(
        VulnerabilityReactor::over_dgraph_with_ledger(
            DgraphReactorParts {
                client: graph.client.clone(),
                namespace: graph.tenant.as_str().to_string(),
                trust,
                decisions,
                plane,
                evidence: state.evidence_reader.clone(),
                risk_facts: state.risk_facts.clone(),
                limits: config.investigation.clone(),
            },
            ledger,
        )
        .await
        .map_err(|error| format!("the reactive cycle could not be built: {error}"))?,
    );
    let resume = reactor.resume(chrono::Utc::now()).await;
    // The periodic sweep: the validity that lapses and the policy that is
    // superseded are reevaluated here, not at serving time. `0` disables
    // the loop.
    if config.trust.sweep_interval_secs > 0 {
        let sweeper = reactor.clone();
        let interval = Duration::from_secs(config.trust.sweep_interval_secs);
        tokio::spawn(async move {
            let mut clock = tokio::time::interval(interval);
            clock.tick().await;
            loop {
                clock.tick().await;
                if let Err(error) = sweeper.sweep_stale_grants(chrono::Utc::now()).await {
                    tracing::warn!(%error, "the validity sweep could not run");
                }
                if let Err(error) = sweeper.reconcile_impact_view().await {
                    tracing::warn!(%error, "the impact view could not be reconciled");
                }
            }
        });
    }
    resume.map_err(|error| format!("the reactive cycle could not resume: {error}"))?;
    Ok(state.with_reactor(reactor))
}
