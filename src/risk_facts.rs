//! The risk facts the promotion decision measures against.
//!
//! The graph of impact (the materialized view of advisories reaching
//! subjects) is the only authority for these facts; this module holds the
//! fallback used when a deployment has no graph, and the translation from
//! the graph's answer to the contract of the trust policy.

use std::collections::BTreeMap;

use hodei_risk_storage::subject_risk::{
    AdvisorySeverity, MemoryCatalog, RiskFactsError, RiskFactsReader, SubjectFacts,
};
use hodei_trust_policy::context::{Exposure, RiskFacts};
use kernel::Hrn;

/// The minimum severity catalog an operator can give the deployment: a
/// JSON object mapping advisory HRN to `"critical"` or `"other"`.
///
/// A name the catalog does not carry is not smoothed into `"other"`: it
/// keeps the answer incomplete, and an incomplete answer establishes no
/// risk.
pub fn load_risk_catalog(path: &str) -> Result<MemoryCatalog, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("the risk catalog of {path} could not be read: {error}"))?;
    let entries: BTreeMap<String, String> = serde_json::from_str(&text)
        .map_err(|error| format!("the risk catalog of {path} is not a JSON object: {error}"))?;
    let severities: Vec<(String, AdvisorySeverity)> = entries
        .into_iter()
        .filter_map(|(advisory, name)| {
            let severity = match name.to_ascii_lowercase().as_str() {
                "critical" => AdvisorySeverity::Critical,
                "other" => AdvisorySeverity::Other,
                other => {
                    tracing::warn!(
                        advisory = %advisory,
                        severity = other,
                        "the risk catalog names a severity nobody knows: the advisory stays unknown"
                    );
                    return None;
                }
            };
            Some((advisory, severity))
        })
        .collect();
    Ok(MemoryCatalog::new(severities))
}

/// The reader of a deployment without risk graph: it answers, and what it
/// says is that nothing was measured.
///
/// It is not a refusal and not a zero: the facts it produces are
/// `established: false` with version zero, and a policy that requires the
/// risk does not grant on them.
pub struct UnmeasuredRiskFacts;

#[async_trait::async_trait]
impl RiskFactsReader for UnmeasuredRiskFacts {
    async fn risk_facts(
        &self,
        _tenant: &Hrn,
        _subject: &Hrn,
    ) -> Result<SubjectFacts, RiskFactsError> {
        Ok(SubjectFacts::unknown())
    }
}

/// The graph's answer, in the words the trust policy understands.
///
/// The exposure and the VEX are not persisted by the graph yet, and the
/// translation says so instead of inventing them: an exposure nobody
/// measured is `Unknown`, and no VEX statement was accepted without being
/// recorded.
pub fn to_trust_facts(facts: &SubjectFacts) -> RiskFacts {
    RiskFacts {
        critical_known: facts.critical_known,
        critical_reachable: facts.critical_reachable,
        exposure: Exposure::Unknown,
        vex_accepted: 0,
        established: facts.established,
        risk_version: facts.version,
    }
}

/// The risk facts of the promotion on the graph profile (HSP-314 F3a): the
/// reader is [`DgraphRiskFacts`] over the impact view of the SAME alpha, with
/// the severity catalog the operator gave the deployment
/// (`trust.risk_catalog_file`, the same rule as the embedded profile: an
/// unreadable catalog leaves the facts UNKNOWN, it does not stop the boot).
///
/// The impact view this reader consults is maintained by the reactor wired
/// in [`crate::reactor_dgraph`] (HSP-314 F3b-3): until an advisory has been
/// accepted the tenant has no materialized routes, the view is never
/// "current", and the facts stay `established: false` ("not measured"),
/// always fail-closed.
pub async fn wire_graph_risk_facts(
    state: crate::app_state::AppState,
    config: &crate::config::AppConfig,
    graph: &crate::profile::GraphAuthority,
) -> Result<crate::app_state::AppState, Box<dyn std::error::Error + Send + Sync>> {
    use hodei_risk_storage::dgraph_view::DgraphImpactView;
    use hodei_risk_storage::subject_risk::{AdvisoryCatalog, DgraphRiskFacts, EmptyCatalog};
    use std::sync::Arc;
    let catalog: Arc<dyn AdvisoryCatalog> = match config.trust.risk_catalog_file.as_deref() {
        Some(path) => match crate::risk_facts::load_risk_catalog(path) {
            Ok(catalog) => Arc::new(catalog),
            Err(error) => {
                tracing::warn!("{error}: the risk facts will stay UNKNOWN");
                Arc::new(EmptyCatalog)
            }
        },
        None => Arc::new(EmptyCatalog),
    };
    let store = Arc::new(hodei_evidence_storage::DgraphGraphStore::new(
        graph.client.clone(),
    ));
    let frontier = Arc::new(
        hodei_evidence_storage::DgraphProjectionFrontier::open(
            graph.client.clone(),
            graph.tenant.as_str(),
        )
        .await
        .map_err(|error| {
            format!("the projection frontier of the risk view could not open: {error}")
        })?,
    );
    let view = Arc::new(DgraphImpactView::new(graph.client.clone(), store, frontier));
    Ok(state.with_risk_facts(Arc::new(DgraphRiskFacts::new(view, catalog))))
}
