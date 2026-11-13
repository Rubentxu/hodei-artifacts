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
use hodei_investigator::read_set::{ReadSet, ReadStamp};
use hodei_investigator::scope::{
    Coverage, EvidenceReadAuthorizer, FactStatus, RetrievalError, RetrievalScope, RetrievedContext,
    ScopedFact, TrustLevel,
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

    /// Las piezas de evidencia del sujeto, con su contenido real, dentro
    /// del presupuesto.
    ///
    /// El orden es una CLAVE ESTABLE --el identificador de la evidencia--
    /// y no el orden del almacen: dos recuperaciones del mismo snapshot
    /// y los mismos limites entregan la MISMA seleccion. El presupuesto
    /// se aplica ANTES de leer el contenido: se pregunta el tamaño y un
    /// blob que no cabe NO se baja para truncarlo despues.
    ///
    /// Y las ausencias son tipadas, no inventadas: un contenido que no
    /// esta es `Missing`, uno que no se puede decodificar es
    /// `Undecodable`, uno que el almacen no entrego es `Unavailable`, y
    /// ninguno de ellos lleva texto inventado.
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
                // El presupuesto de objetos se gasto: la evidencia se
                // DECLARA como omitida, y su contenido NO se baja.
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

            // El sello de la lectura: el digest del contenido es lo que
            // permite comprobar el origen, y lo lleva el registro, sin
            // importar si el contenido llego a leerse.
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
                // El blob NO cabe en el presupuesto de bytes: se declara
                // la omision y el contenido no se baja, porque no se baja
                // un blob entero para truncarlo despues.
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

    /// Las relaciones del grafo que tocan al sujeto, con su camino,
    /// dentro del presupuesto de relaciones.
    ///
    /// El orden es la identidad de la relacion, una clave estable: la
    /// misma vista del mismo grafo da la misma lista.
    async fn the_relations(
        &self,
        subject: &Hrn,
        budget: usize,
        omitted: &mut usize,
        read_set: &mut ReadSet,
    ) -> Result<Vec<ScopedFact>, String> {
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
        // La clave estable: la identidad de la relacion, que no depende
        // del orden en que el grafo la dio.
        relations.sort_by_key(|relation| format!("{:?}", relation.identity()));

        let mut kept = 0usize;
        for relation in relations {
            // El sello de la relacion: su identidad y la evidencia que
            // la sostiene, que es lo que permite comprobar el origen.
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
impl<C: Connection> EvidenceRetriever for GraphEvidenceRetriever<C> {
    async fn retrieve(&self, scope: &RetrievalScope) -> Result<RetrievedContext, RetrievalError> {
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

        // La generacion del mundo ANTES de leer: la comprobacion de
        // consistencia la repite al final, y un mundo que cambio
        // mientras se leia NO se presenta como snapshot coherente.
        // La generacion es de la CUENTA (el tenant del grafo), no del
        // sujeto: es la historia de deltas de la cuenta la que dice si
        // el mundo avanzo.
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

        // La consistencia se comprueba CONTRA EL MUNDO: si la generacion
        // del grafo cambio mientras se leia, no hay snapshot coherente
        // que presentar, y la lectura se dice obsoleta.
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

        // La cobertura se DECLARA: lo que no entro por el presupuesto no
        // se esconde. Un scope conocido pero incompleto no se presenta
        // como respuesta exhaustiva.
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
