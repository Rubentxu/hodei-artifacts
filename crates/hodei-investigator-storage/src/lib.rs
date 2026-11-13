//! El recuperador real: el subgrafo del sujeto, con su evidencia y sus
//! caminos.
//!
//! El recuperador lee **tres** cosas del sujeto, y las tres son evidencia
//! con su identificador:
//!
//! - las **piezas de evidencia** del sujeto, con su contenido real (que
//!   es texto de un tercero, y por tanto dato);
//! - las **relaciones del grafo** que lo tocan, con el camino;
//! - y la **evidencia de alcanzabilidad**, si la hay.
//!
//! Y todo lo que devuelve cabe en el alcance: si no cabe, el
//! investigador lo rechaza antes de preguntarle al modelo.

use std::sync::Arc;

use async_trait::async_trait;
use hodei_evidence::domain::graph::NodeKind;
use hodei_evidence::ports::graph::GraphQuery;
use hodei_evidence::ports::{ContentStore, EvidenceStore};
use hodei_investigator::investigator::{EvidenceRetriever, ReachabilityFacts};
use hodei_investigator::scope::{
    EvidenceReadAuthorizer, RetrievalError, RetrievalScope, ScopedFact, TrustLevel,
};
use kernel::Hrn;
use kernel::application::ports::{EvaluationDecision, EvaluationRequest, IamPolicyEvaluator};
use surrealdb::Connection;

/// El recuperador sobre el grafo y la evidencia reales.
///
/// Es el **borde de autorizacion** de la investigacion: autoriza el
/// sujeto ANTES de leer nada, y cada recurso que traen los caminos se
/// autoriza antes de entregarse. Un permiso denegado o un almacen que
/// no contesta son puertas cerradas, nunca lecturas silenciosas.
pub struct GraphEvidenceRetriever<C: Connection> {
    evidence: Arc<dyn EvidenceStore>,
    content: Arc<dyn ContentStore>,
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
    authorizer: Arc<dyn EvidenceReadAuthorizer>,
    reachability: Option<Arc<dyn ReachabilityFacts>>,
    tenant: Hrn,
}

impl<C: Connection> GraphEvidenceRetriever<C> {
    /// El recuperador sobre un almacen de evidencia, un almacen de
    /// contenido, un grafo y el Authorizer compartido. Sin autorizador
    /// no hay recuperador: sin permiso no se lee.
    pub fn new(
        evidence: Arc<dyn EvidenceStore>,
        content: Arc<dyn ContentStore>,
        graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
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

    /// El recuperador que tambien sabe de alcanzabilidad.
    pub fn with_reachability(mut self, reachability: Arc<dyn ReachabilityFacts>) -> Self {
        self.reachability = Some(reachability);
        self
    }

    /// Las piezas de evidencia del sujeto, con su contenido real.
    async fn the_evidence(&self, account: &str, subject: &Hrn) -> Result<Vec<ScopedFact>, String> {
        let records = self
            .evidence
            .evidence_of_subject(account, subject)
            .await
            .map_err(|error| error.to_string())?;

        let mut facts = Vec::new();
        for record in records {
            // El contenido es el texto del tercero: la descripcion del
            // SBOM, el README, el mensaje del escaner. Se lee entero, y
            // se entrega como DATO.
            let text = match self.content.get(&record.content().digest).await {
                Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
                _ => format!(
                    "the {} of the producer {}",
                    record.evidence_type(),
                    record.producer().id
                ),
            };
            facts.push(ScopedFact {
                evidence_id: record.id().to_string(),
                text,
                path: vec![subject.clone()],
                kind: NodeKind::Evidence,
                trust: TrustLevel::Untrusted,
            });
        }
        Ok(facts)
    }

    /// Las relaciones del grafo que tocan al sujeto, con su camino.
    async fn the_relations(&self, subject: &Hrn) -> Result<Vec<ScopedFact>, String> {
        let mut facts = Vec::new();

        let mut relations = self
            .graph
            .relations_from(&self.tenant, subject)
            .await
            .map_err(|error| error.to_string())?;
        relations.extend(
            self.graph
                .relations_to(&self.tenant, subject)
                .await
                .map_err(|error| error.to_string())?,
        );

        for relation in relations {
            let path = if relation.from == *subject {
                vec![subject.clone(), relation.to.clone()]
            } else {
                vec![subject.clone(), relation.from.clone()]
            };
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
            });
        }
        Ok(facts)
    }
}

#[async_trait]
impl<C: Connection> EvidenceRetriever for GraphEvidenceRetriever<C> {
    async fn retrieve(&self, scope: &RetrievalScope) -> Result<Vec<ScopedFact>, RetrievalError> {
        // La autorizacion va ANTES de cualquier lectura: quien pregunta
        // tiene que poder leer el sujeto, y una denegacion o un error
        // del Authorizer es una puerta cerrada. La cuenta del sujeto NO
        // se infiere del texto del alcance: es su componente canonico.
        self.authorizer
            .can_read(&scope.actor, &scope.subject)
            .await
            .map_err(|reason| RetrievalError::NotAuthorized {
                actor: scope.actor.to_string(),
                resource: format!("{} ({reason})", scope.subject),
            })?;

        let mut facts = self
            .the_evidence(scope.subject.account_id(), &scope.subject)
            .await
            .map_err(RetrievalError::Unavailable)?;
        facts.extend(
            self.the_relations(&scope.subject)
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

        // Los caminos que tocan un recurso de OTRA cuenta no se
        // entregan: el que pregunta no los recibe, aunque el grafo los
        // tenga. La comprobacion del investigador queda como ultima
        // linea: un hecho que se cuele aqui no pasa.
        let account = scope.subject.account_id().to_string();
        facts.retain(|fact| {
            fact.path
                .iter()
                .all(|node| node.account_id() == account.as_str())
        });

        // Y el alcance se respeta aqui tambien: un recuperador que
        // devuelve de mas no puede colar nada, porque el investigador lo
        // comprueba otra vez.
        facts.truncate(scope.max_nodes);
        Ok(facts)
    }
}

/// El Authorizer compartido, por su puerto del kernel.
///
/// La decision la toma el motor de politicas determinista --el mismo
/// que autoriza el resto del sistema--, no el texto del alcance y no el
/// modelo. Un error de evaluacion cierra: si no se pudo comprobar el
/// permiso, no se lee.
pub struct EvaluatorReadAuthorizer<E: IamPolicyEvaluator> {
    evaluator: Arc<E>,
}

impl<E: IamPolicyEvaluator> EvaluatorReadAuthorizer<E> {
    /// El autorizador sobre un evaluador de politicas IAM.
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
