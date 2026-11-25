//! The investigation of the graph profile (HSP-314 F3c).
//!
//! The counterpart of the embedded `GraphInvestigation`: the same retriever
//! core, the same authorizer (the Cedar decision the HTTP boundary takes),
//! the same freshness verdict, over the Dgraph adapters. The graph facts, the
//! evidence and the delta history all come from the SAME alpha the rest of
//! the profile uses, and the content of the evidence from the content store
//! of the deployment.
//!
//! The composition is fail-closed twice. At boot, a missing port or an alpha
//! that does not answer stops the service instead of leaving the surface
//! answering as if there were nothing to find. At serving time, a store that
//! cannot answer is an error of the investigator (the handler answers `503`),
//! never an empty report.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_authz::VerifiedContext;
use hodei_evidence::domain::delta::RelationDeltaLedger;
use hodei_evidence::ports::EvidenceStore;
use hodei_evidence_storage::{
    DgraphDeltaLedger, DgraphEvidenceStore, DgraphGraphStore, DgraphProjectionFrontier,
};
use hodei_hub::blob::BlobStore;
use hodei_investigator::Investigator;
use hodei_investigator::freshness::Freshness;
use hodei_investigator::guard::CapabilityGuard;
use hodei_investigator::investigator::{Investigation, InvestigationError};
use hodei_investigator::read_set::ReadSet;
use hodei_investigator::scope::RetrievalScope;
use hodei_investigator_storage::DgraphEvidenceRetriever;
use hodei_investigator_storage::freshness::LedgerFreshness;

use crate::app_state::AppState;
use crate::authorization::AuthorizeResourceUseCase;
use crate::config::{AppConfig, InvestigationConfig};
use crate::investigation::{CasContentStore, CedarReadAuthorizer, InvestigationPort};

/// The investigation over the Dgraph stores of the profile.
pub struct DgraphInvestigation {
    evidence: Arc<dyn EvidenceStore>,
    content: Arc<CasContentStore>,
    graph: Arc<DgraphGraphStore>,
    deltas: Arc<dyn RelationDeltaLedger>,
    authorize_resource: Arc<AuthorizeResourceUseCase>,
    limits: InvestigationConfig,
}

impl DgraphInvestigation {
    /// The investigation over the stores of the profile.
    pub fn new(
        evidence: Arc<dyn EvidenceStore>,
        content: Arc<CasContentStore>,
        graph: Arc<DgraphGraphStore>,
        deltas: Arc<dyn RelationDeltaLedger>,
        authorize_resource: Arc<AuthorizeResourceUseCase>,
        limits: InvestigationConfig,
    ) -> Self {
        Self {
            evidence,
            content,
            graph,
            deltas,
            authorize_resource,
            limits,
        }
    }

    /// The tenant of the graph for a subject, or the reason there is none.
    fn the_tenant(&self, subject: &kernel::Hrn) -> Result<kernel::Hrn, InvestigationError> {
        self.limits.the_graph_tenant(subject).ok_or_else(|| {
            InvestigationError::NoAnswer(
                "the tenant of the graph is not a canonical HRN".to_string(),
            )
        })
    }
}

#[async_trait]
impl InvestigationPort for DgraphInvestigation {
    async fn investigate(
        &self,
        investigation: &str,
        context: &VerifiedContext,
        scope: &RetrievalScope,
        question: &str,
        now: DateTime<Utc>,
    ) -> Result<Investigation, InvestigationError> {
        let tenant = self.the_tenant(&scope.subject)?;
        // The authorizer carries the context of THIS request: the decision
        // is taken for the principal that asked, and not for whoever asked
        // last.
        let retriever = DgraphEvidenceRetriever::new(
            self.evidence.clone(),
            self.content.clone(),
            self.graph.clone(),
            self.deltas.clone(),
            Arc::new(CedarReadAuthorizer {
                authorize_resource: self.authorize_resource.clone(),
                context: context.clone(),
            }),
            tenant,
        );
        Investigator::without_a_model(Arc::new(retriever), CapabilityGuard::empty())
            .investigate_without_a_model(investigation, scope, question, now)
            .await
    }

    async fn freshness_of(
        &self,
        scope: &RetrievalScope,
        report: &ReadSet,
    ) -> Result<Freshness, InvestigationError> {
        // The tenant is the one of the account of the subject, and the
        // checker asks ONLY that tenant.
        let tenant = self.the_tenant(&scope.subject)?;
        let checker = LedgerFreshness::new(self.evidence.clone(), self.deltas.clone(), tenant);
        Ok(checker.freshness_of(report, &scope.subject).await)
    }
}

/// Wires the investigation of the graph profile.
///
/// Fail-closed: the content store and the authorizer must already be in the
/// state, and the evidence store and the delta frontier are opened through
/// the versioned schema guard of the alpha. Any of them missing stops the
/// boot; none degrades to an investigation that finds nothing.
pub async fn wire_graph_investigation(
    state: AppState,
    config: &AppConfig,
    graph: &crate::profile::GraphAuthority,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    let content: Arc<dyn BlobStore> = state.content_store.clone().ok_or(
        "the investigation of the graph profile needs the content store of the deployment",
    )?;
    let evidence = Arc::new(
        DgraphEvidenceStore::open(graph.client.clone())
            .await
            .map_err(|error| {
                format!("the evidence store of the investigation could not open: {error}")
            })?,
    );
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(graph.client.clone(), graph.tenant.as_str())
            .await
            .map_err(|error| {
                format!("the delta history of the investigation could not open: {error}")
            })?,
    );
    let investigation = Arc::new(DgraphInvestigation::new(
        evidence,
        Arc::new(CasContentStore::new(content)),
        Arc::new(DgraphGraphStore::new(graph.client.clone())),
        Arc::new(DgraphDeltaLedger::new(frontier)),
        state.authorize_resource.clone(),
        config.investigation.clone(),
    ));
    Ok(state
        .with_investigation(investigation)
        .with_investigation_limits(config.investigation.clone()))
}
