//! The real retriever: the subject's subgraph, with its evidence and its
//! paths.
//!
//! The retriever reads **three** things about the subject, and all three are evidence
//! with its identifier:
//!
//! - the subject's **evidence pieces**, with their real content (which
//!   is third-party text, and therefore data);
//! - the **graph relations** that touch it, with the path;
//! - and the **reachability evidence**, if there is any.
//!
//! And everything it returns fits in the scope: if it does not fit, the
//! investigator rejects it before asking the model.
//!
//! # The source is a port, not a store
//!
//! The retriever core ([`InvestigationRetriever`]) asks a
//! [`GraphSource`] for the world generation and the subject's relations;
//! it does not know what store is behind. That is what makes the read-set
//! **reproducible**: two reads against the same watermark
//! of the delta history seal the same manifest, because the
//! manifest is a function of the state and of the stable keys (the
//! evidence identifier and the relation identity), never
//! of the order in which the store responded.

pub mod freshness;

use std::sync::Arc;

use async_trait::async_trait;
use hodei_evidence::domain::graph::{GraphRelation, NodeKind};
use hodei_evidence::ports::graph::GraphQuery;
use hodei_evidence::ports::{ContentStore, EvidenceStore};
use hodei_evidence_storage::DgraphGraphStore;
use hodei_investigator::investigator::{EvidenceRetriever, ReachabilityFacts};
use hodei_investigator::read_set::{ReadSet, ReadStamp};
use hodei_investigator::scope::{
    Coverage, EvidenceReadAuthorizer, FactStatus, RetrievalError, RetrievalScope, RetrievedContext,
    ScopedFact, TrustLevel,
};
use kernel::Hrn;
use kernel::application::ports::{EvaluationDecision, EvaluationRequest, IamPolicyEvaluator};

/// The retriever over the graph authority.
///
/// It is the Dgraph port of the investigation read-set (HSP-206): the
/// core (the same prior authorization, the same
/// budget, the same stable manifest keys) against the
/// native graph and its delta history. The generation with which the
/// read is sealed is the watermark of the tenant's deltas, the cursor
/// that the freshness check asks again: that is why two
/// reads at the same watermark reproduce the read-set byte for byte.
pub struct DgraphEvidenceRetriever {
    inner: InvestigationRetriever<DgraphSource>,
}

/// The Dgraph source: the native graph, with its generation read from the
/// delta history that the boundary projected. The history is the SAME
/// port that the freshness checker queries ([`freshness::LedgerFreshness`]):
/// the watermark with which the read-set is sealed and the watermark against
/// which it is checked are the same question to the same material, and therefore
/// a support withdrawal is seen as stale and not as a falsified current.
pub struct DgraphSource {
    graph: Arc<DgraphGraphStore>,
    deltas: Arc<dyn hodei_evidence::domain::delta::RelationDeltaLedger>,
}

impl DgraphSource {
    /// The source over the graph and the delta history of a validated
    /// connection.
    pub fn new(
        graph: Arc<DgraphGraphStore>,
        deltas: Arc<dyn hodei_evidence::domain::delta::RelationDeltaLedger>,
    ) -> Self {
        Self { graph, deltas }
    }
}

#[async_trait]
impl GraphSource for DgraphSource {
    async fn generation(&self, tenant: &Hrn) -> Result<u64, String> {
        self.deltas.version_of(tenant).await
    }

    async fn relations_from(&self, tenant: &Hrn, node: &Hrn) -> Result<Vec<GraphRelation>, String> {
        GraphQuery::relations_from(self.graph.as_ref(), tenant, node)
            .await
            .map_err(|error| error.to_string())
    }

    async fn relations_to(&self, tenant: &Hrn, node: &Hrn) -> Result<Vec<GraphRelation>, String> {
        GraphQuery::relations_to(self.graph.as_ref(), tenant, node)
            .await
            .map_err(|error| error.to_string())
    }
}

impl DgraphEvidenceRetriever {
    /// The retriever over the evidence, the content, the native graph
    /// and its delta history, with the shared Authorizer.
    pub fn new(
        evidence: Arc<dyn EvidenceStore>,
        content: Arc<dyn ContentStore>,
        graph: Arc<DgraphGraphStore>,
        deltas: Arc<dyn hodei_evidence::domain::delta::RelationDeltaLedger>,
        authorizer: Arc<dyn EvidenceReadAuthorizer>,
        tenant: Hrn,
    ) -> Self {
        Self {
            inner: InvestigationRetriever::new(
                evidence,
                content,
                DgraphSource::new(graph, deltas),
                authorizer,
                tenant,
            ),
        }
    }

    /// The retriever that also knows about reachability.
    pub fn with_reachability(mut self, reachability: Arc<dyn ReachabilityFacts>) -> Self {
        self.inner = self.inner.with_reachability(reachability);
        self
    }
}

#[async_trait]
impl EvidenceRetriever for DgraphEvidenceRetriever {
    async fn retrieve(&self, scope: &RetrievalScope) -> Result<RetrievedContext, RetrievalError> {
        self.inner.retrieve(scope).await
    }
}

/// The two read-set questions to the graph, stated as a port.
///
/// An adapter that cannot answer them closes the read: the
/// missing generation is not invented as zero, and the relations that
/// fail are not handed over as absence. That is what separates a mute
/// graph from a graph that declares it has nothing to say.
#[async_trait]
pub trait GraphSource: Send + Sync {
    /// The version of the tenant's delta history: the watermark
    /// against which the read-set is sealed and reproduced.
    async fn generation(&self, tenant: &Hrn) -> Result<u64, String>;

    /// The relations that start at the node, for the account.
    async fn relations_from(&self, tenant: &Hrn, node: &Hrn) -> Result<Vec<GraphRelation>, String>;

    /// The relations that end at the node, for the account.
    async fn relations_to(&self, tenant: &Hrn, node: &Hrn) -> Result<Vec<GraphRelation>, String>;
}

/// The retriever core: the sealed read over a source.
///
/// It lives apart from the adapter because the guarantee that matters (the
/// reproduction of the read-set at the same watermark, the declared
/// budget and the door closed on failure) belongs to the procedure,
/// not to the store. Surreal and Dgraph are two materials of the same
/// procedure.
pub struct InvestigationRetriever<G: GraphSource> {
    evidence: Arc<dyn EvidenceStore>,
    content: Arc<dyn ContentStore>,
    graph: G,
    authorizer: Arc<dyn EvidenceReadAuthorizer>,
    reachability: Option<Arc<dyn ReachabilityFacts>>,
    tenant: Hrn,
}

impl<G: GraphSource> InvestigationRetriever<G> {
    /// The core over a graph source and the shared Authorizer.
    pub fn new(
        evidence: Arc<dyn EvidenceStore>,
        content: Arc<dyn ContentStore>,
        graph: G,
        authorizer: Arc<dyn EvidenceReadAuthorizer>,
        tenant: Hrn,
    ) -> Self {
        Self {
            evidence,
            content,
            graph,
            authorizer,
            reachability: None,
            tenant,
        }
    }

    /// The retriever that also knows about reachability.
    pub fn with_reachability(mut self, reachability: Arc<dyn ReachabilityFacts>) -> Self {
        self.reachability = Some(reachability);
        self
    }

    /// The subject's evidence pieces, with their real content, within
    /// the budget.
    ///
    /// The order is a STABLE KEY (the evidence identifier)
    /// and not the store's order: two retrievals of the same snapshot
    /// and the same limits deliver the SAME selection. The budget
    /// is applied BEFORE reading the content: the size is asked and a
    /// blob that does not fit is NOT downloaded to be truncated afterwards.
    ///
    /// And absences are typed, not invented: content that is not
    /// there is `Missing`, content that cannot be decoded is
    /// `Undecodable`, content that the store did not deliver is `Unavailable`, and
    /// none of them carries invented text.
    async fn the_evidence(
        &self,
        scope: &RetrievalScope,
        used_bytes: &mut u64,
        omitted: &mut usize,
        read_set: &mut ReadSet,
    ) -> Result<Vec<ScopedFact>, String> {
        let account = scope.subject.account_id();
        let subject = &scope.subject;
        let budget = scope.budget.max_objects;
        let bytes_budget = scope.budget.max_bytes;
        let mut records = self
            .evidence
            .evidence_of_subject(account, subject)
            .await
            .map_err(|error| error.to_string())?;
        records.sort_by_key(|record| record.id().to_string());

        let mut facts = Vec::new();
        let mut loaded = 0usize;
        for record in records {
            if loaded >= budget {
                // The object budget is spent: the evidence is
                // DECLARED as omitted, and its content is NOT downloaded.
                *omitted += 1;
                facts.push(ScopedFact {
                    evidence_id: record.id().to_string(),
                    text: String::new(),
                    path: vec![subject.clone()],
                    kind: NodeKind::Evidence,
                    trust: TrustLevel::Untrusted,
                    status: FactStatus::OmittedByBudget,
                });
                continue;
            }

            // The read seal: the content digest is what
            // allows the origin to be checked, and the record carries it,
            // regardless of whether the content was read.
            read_set.read(ReadStamp::evidence(
                record.id().to_string(),
                record.content().digest.to_string(),
            ));

            let size = self
                .content
                .size(&record.content().digest)
                .await
                .map_err(|error| error.to_string())?;
            let fits = match size {
                Some(size) => *used_bytes + size <= bytes_budget,
                None => false,
            };
            if size.is_some() && !fits {
                // The blob does NOT fit in the byte budget: the omission is declared
                // and the content is not downloaded, because a whole
                // blob is not downloaded to be truncated afterwards.
                *omitted += 1;
                facts.push(ScopedFact {
                    evidence_id: record.id().to_string(),
                    text: String::new(),
                    path: vec![subject.clone()],
                    kind: NodeKind::Evidence,
                    trust: TrustLevel::Untrusted,
                    status: FactStatus::OmittedByBudget,
                });
                continue;
            }

            let bytes = self.content.get(&record.content().digest).await;
            let fact = match bytes {
                Err(_) => ScopedFact {
                    evidence_id: record.id().to_string(),
                    text: String::new(),
                    path: vec![subject.clone()],
                    kind: NodeKind::Evidence,
                    trust: TrustLevel::Untrusted,
                    status: FactStatus::Unavailable,
                },
                Ok(None) => ScopedFact {
                    evidence_id: record.id().to_string(),
                    text: String::new(),
                    path: vec![subject.clone()],
                    kind: NodeKind::Evidence,
                    trust: TrustLevel::Untrusted,
                    status: FactStatus::Missing,
                },
                Ok(Some(bytes)) => match String::from_utf8(bytes) {
                    Err(_) => ScopedFact {
                        evidence_id: record.id().to_string(),
                        text: String::new(),
                        path: vec![subject.clone()],
                        kind: NodeKind::Evidence,
                        trust: TrustLevel::Untrusted,
                        status: FactStatus::Undecodable,
                    },
                    Ok(text) => {
                        loaded += 1;
                        if let Some(size) = size {
                            *used_bytes += size;
                        }
                        ScopedFact {
                            evidence_id: record.id().to_string(),
                            text: hodei_investigator::redact::redact(&text),
                            path: vec![subject.clone()],
                            kind: NodeKind::Evidence,
                            trust: TrustLevel::Untrusted,
                            status: FactStatus::Loaded,
                        }
                    }
                },
            };
            facts.push(fact);
        }
        Ok(facts)
    }

    /// The graph relations that touch the subject, with their path,
    /// within the relations budget.
    ///
    /// The order is the relation identity, a stable key: the
    /// same view of the same graph gives the same list.
    async fn the_relations(
        &self,
        subject: &Hrn,
        budget: usize,
        omitted: &mut usize,
        read_set: &mut ReadSet,
    ) -> Result<Vec<ScopedFact>, String> {
        let mut facts = Vec::new();

        let mut relations = self.graph.relations_from(&self.tenant, subject).await?;
        relations.extend(
            self.graph
                .relations_to(&self.tenant, subject)
                .await
                .map_err(|error| error.to_string())?,
        );
        // The stable key: the relation identity, which does not depend
        // on the order in which the graph gave it.
        relations.sort_by_key(|relation| format!("{:?}", relation.identity()));

        let mut kept = 0usize;
        for relation in relations {
            // The relation seal: its identity and the evidence that
            // supports it, which is what allows the origin to be checked.
            read_set.read(ReadStamp::relation(
                relation.identity(),
                relation.evidence_id.to_string(),
            ));
            let path = if relation.from == *subject {
                vec![subject.clone(), relation.to.clone()]
            } else {
                vec![subject.clone(), relation.from.clone()]
            };
            if kept >= budget {
                *omitted += 1;
                facts.push(ScopedFact {
                    evidence_id: relation.evidence_id.to_string(),
                    text: String::new(),
                    path,
                    kind: NodeKind::Artifact,
                    trust: TrustLevel::Untrusted,
                    status: FactStatus::OmittedByBudget,
                });
                continue;
            }
            kept += 1;
            facts.push(ScopedFact {
                evidence_id: relation.evidence_id.to_string(),
                text: format!(
                    "{} {} {}",
                    relation.from,
                    relation.relation.as_str(),
                    relation.to
                ),
                path,
                kind: NodeKind::Artifact,
                trust: TrustLevel::Untrusted,
                status: FactStatus::Loaded,
            });
        }
        Ok(facts)
    }
}

#[async_trait]
impl<G: GraphSource> EvidenceRetriever for InvestigationRetriever<G> {
    async fn retrieve(&self, scope: &RetrievalScope) -> Result<RetrievedContext, RetrievalError> {
        // Authorization goes BEFORE any read: whoever asks
        // must be able to read the subject, and a denial or an error
        // from the Authorizer is a closed door. The subject's account is NOT
        // inferred from the scope text: it is its canonical component.
        self.authorizer
            .can_read(&scope.actor, &scope.subject)
            .await
            .map_err(|reason| RetrievalError::NotAuthorized {
                actor: scope.actor.to_string(),
                resource: format!("{} ({reason})", scope.subject),
            })?;

        // The world generation BEFORE reading: the consistency check
        // repeats it at the end, and a world that changed
        // while being read is NOT presented as a coherent snapshot.
        // The generation belongs to the ACCOUNT (the graph's tenant), not to the
        // subject: it is the account's delta history that says whether
        // the world advanced.
        let generation_before = self
            .graph
            .generation(&self.tenant)
            .await
            .map_err(RetrievalError::Unavailable)?;
        let mut read_set = ReadSet::empty();
        read_set.generation("graph", generation_before);

        let mut omitted = 0usize;
        let mut used_bytes = 0u64;
        let mut facts = self
            .the_evidence(scope, &mut used_bytes, &mut omitted, &mut read_set)
            .await
            .map_err(RetrievalError::Unavailable)?;
        facts.extend(
            self.the_relations(
                &scope.subject,
                scope.budget.max_relations,
                &mut omitted,
                &mut read_set,
            )
            .await
            .map_err(RetrievalError::Unavailable)?,
        );
        if let Some(reachability) = &self.reachability {
            facts.extend(
                reachability
                    .facts_of(&scope.subject)
                    .await
                    .map_err(RetrievalError::Unavailable)?,
            );
        }

        // Paths that touch a resource of ANOTHER account are not
        // delivered: the asker does not receive them, even if the graph
        // has them. The investigator's check remains as the last
        // line: a fact that slips in here does not pass.
        let account = scope.subject.account_id().to_string();
        facts.retain(|fact| {
            fact.path
                .iter()
                .all(|node| node.account_id() == account.as_str())
        });

        // And the scope is respected here too: a retriever that
        // returns too much cannot slip anything through, because the investigator
        // checks it again.
        facts.truncate(scope.max_nodes);

        // Consistency is checked AGAINST THE WORLD: if the graph
        // generation changed while being read, there is no coherent snapshot
        // to present, and the read is declared obsolete.
        let generation_after = self
            .graph
            .generation(&self.tenant)
            .await
            .map_err(RetrievalError::Unavailable)?;
        if generation_after != generation_before {
            return Err(RetrievalError::Stale {
                source: "graph".to_string(),
                expected: generation_before,
                found: generation_after,
            });
        }

        // Coverage is DECLARED: what did not fit in the budget is not
        // hidden. A known but incomplete scope is not presented
        // as an exhaustive answer.
        let coverage = if omitted == 0 {
            Coverage::Complete
        } else {
            Coverage::Partial { omitted }
        };
        Ok(RetrievedContext {
            facts,
            coverage,
            read_set,
        })
    }
}

/// The shared Authorizer, through the kernel's port.
///
/// The decision is taken by the deterministic policy engine (the same one
/// that authorizes the rest of the system), not the scope text and not the
/// model. An evaluation error closes: if the
/// permission could not be checked, it is not read.
pub struct EvaluatorReadAuthorizer<E: IamPolicyEvaluator> {
    evaluator: Arc<E>,
}

impl<E: IamPolicyEvaluator> EvaluatorReadAuthorizer<E> {
    /// The authorizer over an IAM policy evaluator.
    pub fn new(evaluator: Arc<E>) -> Self {
        Self { evaluator }
    }
}

#[async_trait]
impl<E: IamPolicyEvaluator> EvidenceReadAuthorizer for EvaluatorReadAuthorizer<E> {
    async fn can_read(&self, actor: &Hrn, resource: &Hrn) -> Result<(), String> {
        let decision: EvaluationDecision = self
            .evaluator
            .evaluate_iam_policies(EvaluationRequest {
                principal_hrn: actor.clone(),
                action_name: "read".to_string(),
                resource_hrn: resource.clone(),
            })
            .await
            .map_err(|error| format!("the authorization could not be evaluated: {error}"))?;
        if decision.decision {
            Ok(())
        } else {
            Err(decision.reason)
        }
    }
}
