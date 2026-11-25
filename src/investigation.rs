//! The investigation, as the service serves it.
//!
//! The investigation is a **read**: it explains what the service knows
//! about a subject, and it does not change anything. There is no method
//! here that writes, and no method that decides: the report describes.
//!
//! # One policy, one decision
//!
//! The retriever of the domain authorizes the subject BEFORE it reads, and
//! every resource it brings with it. That authorization is the **same**
//! Cedar decision the HTTP boundary takes: the authorizer below asks the
//! same use case with the same action and the same verified context. Two
//! policies deciding about the same thing could disagree, and the one that
//! is more permissive would win by accident.
//!
//! # The model is optional
//!
//! The investigation the service serves is the one of the **tools**: the
//! facts the graph and the evidence give, with their citations, and the
//! uncertainties that are declared. There is no commercial API in the
//! request path, and a deployment without a model still answers.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
// HSP-213: el puerto de la investigacion y sus tipos viajan sin la feature
// `legacy` --el handler HTTP los necesita tambien en el binario
// productivo--, y solo la composicion sobre el almacem embebido queda
// detras del corte.
use hodei_authz::VerifiedContext;
use hodei_evidence::ports::{ContentStore, StoreError};
use hodei_hub::blob::BlobError;
use hodei_hub::blob::BlobStore;
use hodei_investigator::freshness::Freshness;
use hodei_investigator::investigator::{Investigation, InvestigationError};
use hodei_investigator::read_set::ReadSet;
use hodei_investigator::scope::RetrievalScope;

use hodei_authz::AuthorizationOutcome;
#[cfg(feature = "legacy")]
use hodei_evidence::domain::delta::RelationDeltaLedger;
#[cfg(feature = "legacy")]
use hodei_evidence::ports::EvidenceStore;
#[cfg(feature = "legacy")]
use hodei_investigator::Investigator;
#[cfg(feature = "legacy")]
use hodei_investigator::guard::CapabilityGuard;
use hodei_investigator::scope::EvidenceReadAuthorizer;
#[cfg(feature = "legacy")]
use hodei_investigator_storage::GraphEvidenceRetriever;
#[cfg(feature = "legacy")]
use hodei_investigator_storage::freshness::LedgerFreshness;
use kernel::Hrn;
#[cfg(feature = "legacy")]
use surrealdb::Connection;

use crate::actions;
use crate::authorization::AuthorizeResourceUseCase;
#[cfg(feature = "legacy")]
use crate::config::InvestigationConfig;

/// The port of the investigation.
///
/// The context of the verified principal is part of the question, not of
/// the subject: the same subject is a different question for a different
/// principal, and the authorization depends on who asks.
#[async_trait]
pub trait InvestigationPort: Send + Sync {
    /// Investigates a subject for a verified principal.
    async fn investigate(
        &self,
        investigation: &str,
        context: &VerifiedContext,
        scope: &RetrievalScope,
        question: &str,
        now: DateTime<Utc>,
    ) -> Result<Investigation, InvestigationError>;

    /// Says whether a delivered report still rests on the world.
    ///
    /// The manifest is the one of the report the client holds: the check
    /// is under demand, and the client is the one that says what it read.
    /// The answer is of three: still current, stale, or unknown.
    async fn freshness_of(
        &self,
        scope: &RetrievalScope,
        report: &ReadSet,
    ) -> Result<Freshness, InvestigationError>;
}

#[cfg(feature = "legacy")]
/// The investigation over the graph and the evidence of the deployment.
pub struct GraphInvestigation<C: Connection> {
    evidence: Arc<dyn EvidenceStore>,
    content: Arc<dyn ContentStore>,
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
    deltas: Arc<dyn RelationDeltaLedger>,
    authorize_resource: Arc<AuthorizeResourceUseCase>,
    limits: InvestigationConfig,
}

#[cfg(feature = "legacy")]
impl<C: Connection> GraphInvestigation<C> {
    /// The investigation over the stores of the deployment.
    pub fn new(
        evidence: Arc<dyn EvidenceStore>,
        content: Arc<dyn ContentStore>,
        graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
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

    /// The tenant of the graph for a subject, or the reason there is
    /// none.
    fn the_tenant(&self, subject: &Hrn) -> Result<Hrn, InvestigationError> {
        self.limits.the_graph_tenant(subject).ok_or_else(|| {
            InvestigationError::NoAnswer(
                "the tenant of the graph is not a canonical HRN".to_string(),
            )
        })
    }
}

#[cfg(feature = "legacy")]
#[async_trait]
impl<C: Connection> InvestigationPort for GraphInvestigation<C> {
    async fn investigate(
        &self,
        investigation: &str,
        context: &VerifiedContext,
        scope: &RetrievalScope,
        question: &str,
        now: DateTime<Utc>,
    ) -> Result<Investigation, InvestigationError> {
        // The tenant of the graph: the account the investigation is about.
        // A graph written under another name has no relations to show, and
        // that is said in the coverage of the report rather than hidden.
        let tenant = self.the_tenant(&scope.subject)?;
        // The authorizer carries the context of THIS request: the decision
        // is taken for the principal that asked, and not for whoever asked
        // last.
        let retriever = GraphEvidenceRetriever::new(
            self.evidence.clone(),
            self.content.clone(),
            self.graph.clone(),
            Arc::new(CedarReadAuthorizer {
                authorize_resource: self.authorize_resource.clone(),
                context: context.clone(),
            }),
            tenant,
        );
        let investigator =
            Investigator::without_a_model(Arc::new(retriever), CapabilityGuard::empty());
        investigator
            .investigate_without_a_model(investigation, scope, question, now)
            .await
    }

    async fn freshness_of(
        &self,
        scope: &RetrievalScope,
        report: &ReadSet,
    ) -> Result<Freshness, InvestigationError> {
        // El tenant es el de la cuenta del sujeto, y el comprobador
        // consulta SOLO ese tenant: una comprobacion de otra cuenta no
        // se hace por el camino de un tenant ajeno.
        let tenant = self.the_tenant(&scope.subject)?;
        let checker = LedgerFreshness::new(self.evidence.clone(), self.deltas.clone(), tenant);
        Ok(checker.freshness_of(report, &scope.subject).await)
    }
}

/// The authorizer of the reads, over Cedar.
///
/// It asks the **same** use case the HTTP boundary asks, with the same
/// action: the decision that opens the door is the decision that opened
/// it, and not a second one that could say something else.
///
/// A decision that could not be taken is not a permission: an authorizer
/// that fails closes, because a read that nobody could forbid is a read
/// that nobody could explain.
pub(crate) struct CedarReadAuthorizer {
    pub(crate) authorize_resource: Arc<AuthorizeResourceUseCase>,
    pub(crate) context: VerifiedContext,
}

#[async_trait]
impl EvidenceReadAuthorizer for CedarReadAuthorizer {
    async fn can_read(&self, _actor: &Hrn, resource: &Hrn) -> Result<(), String> {
        let outcome = self
            .authorize_resource
            .execute_canonical(self.context.clone(), actions::investigate(), resource)
            .map_err(|error| format!("the authorizer could not be asked: {error}"))?;
        match outcome {
            AuthorizationOutcome::Permit => Ok(()),
            AuthorizationOutcome::Deny(reason) => Err(format!("denied: {reason:?}")),
            AuthorizationOutcome::Indeterminate(reason) => {
                Err(format!("the authorizer could not decide: {reason:?}"))
            }
        }
    }
}

/// The content of the evidence, over the store of content of the service.
///
/// The evidence content is content-addressed, and the store of content of
/// the service is too: the digest of the evidence IS the address. What is
/// not there is not an error: the absence is the answer, and the
/// investigation declares it instead of inventing it.
///
/// The store of content is a **write** store, and this adapter does not
/// write: the investigation surface does not publish evidence, and a port
/// that pretends to write would be a way to write by accident.
pub struct CasContentStore {
    blobs: Arc<dyn BlobStore>,
}

impl CasContentStore {
    /// The content store over the store of content of the service.
    pub fn new(blobs: Arc<dyn BlobStore>) -> Self {
        Self { blobs }
    }
}

#[async_trait]
impl ContentStore for CasContentStore {
    async fn put(&self, _content: Vec<u8>) -> Result<hodei_evidence::ContentDigest, StoreError> {
        Err(StoreError::InvalidRecord(
            "the investigation surface does not publish content".to_string(),
        ))
    }

    async fn get(
        &self,
        digest: &hodei_evidence::ContentDigest,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        match self.blobs.get(digest).await {
            Ok(bytes) => Ok(Some(bytes)),
            // The content is not there: the absence is an answer, not a
            // failure.
            Err(BlobError::NotFound) => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error.to_string())),
        }
    }

    async fn size(
        &self,
        digest: &hodei_evidence::ContentDigest,
    ) -> Result<Option<u64>, StoreError> {
        match self.blobs.length(digest).await {
            Ok(length) => Ok(Some(length)),
            Err(BlobError::NotFound) => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error.to_string())),
        }
    }
}
