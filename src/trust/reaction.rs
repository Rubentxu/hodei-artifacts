//! El ciclo reactivo completo en el servicio real: advisory entra,
//! reevaluación corre, distribución bloquea (HSP-137).
//!
//! El motor (`hodei-reactive`) y sus adaptadores de dominio
//! (`PromotionDecisionRequester`, `DomainIntentExecutor`, el outbox
//! duradero `SurrealIntentStore`) existían y estaban probados, pero solo
//! vivían en los tests de sus crates. Este módulo es la composition root
//! de ese ciclo: los conecta a las piezas que el servicio ya construye
//! (el grafo, el catálogo, el plano de confianza, los hechos de riesgo) y
//! añade lo único que no existía en ninguna parte:
//!
//! - una **entrada soportada** de advisories que reutiliza la proyección
//!   del dominio (`hodei_risk::projection`) —no un motor nuevo— y despacha
//!   el `VulnerabilityAccepted` por el camino planificado (intent→recibo);
//! - un **radio de impacto multi-cuenta**: la lectura del crate fija un
//!   tenant al construirse, y el servicio es multi-cuenta, así que aquí el
//!   tenant se resuelve por la cuenta del asunto, con la MISMA plantilla
//!   (`investigation.graph_tenant_template`) que usan promociones y
//!   riesgos. La autoridad sigue siendo el HRN canónico: una cuenta no
//!   lee el grafo de otra;
//! - el **contexto real de reevaluación**: la reevaluación decide contra
//!   la misma cápsula y los mismos hechos de riesgo que decidió la
//!   promoción original. Con un contexto vacío toda reevaluación negaría
//!   por hueco propio y arrancaría concesiones ajenas al hecho — justo lo
//!   que la AC2 prohíbe;
//! - los **barridores de vigencia**: la concesión caduca (excepción
//!   agotada o política sustituida) y eso se reevalúa, no se descubre
//!   solo al servir;
//! - la **reconciliación tras crash**: los intents que un `kill -9` dejó
//!   dichos se persiguen con `resume` al arrancar, sin repetir el efecto
//!   cuyo recibo ya está cerrado.
//!
//! # Por qué la vigencia necesita planificadores propios
//!
//! El motor cierra el mundo una vez por clave de efecto: un recibo
//! `applied` bajo una idempotencia ya no se toca, nunca, aunque llegue un
//! evento nuevo (`runtime.rs`: la supresión consulta `applied_effects()`
//! por idempotencia a secas). Eso es correcto para la entrega duplicada y
//! letal para el paso del tiempo: `PlannedExceptionExpiryReaction` fija su
//! clave en `decision:expired:{subject}`, y la segunda caducidad del mismo
//! sujeto se suprimiría para siempre. Aquí cada barrido dice un evento
//! nuevo por diseño —identificador fresco, clave de efecto fresca— cuando
//! hay concesiones nuevas que vencer, y el identificador de la pasada
//! forma parte del nombre: un crash a mitad deja los intents dichos y el
//! `resume` los persigue; los efectos cerrados se suprimen, que es
//! exactamente lo que la AC3 pide — reconverger sin duplicar recibos.
//!
//! Lo que este módulo deliberately NO hace: no decide nada. La decisión
//! la toma `PromotionService` con la política activa; aquí solo se dicen
//! los hechos, se piden las reevaluaciones y se persiguen los
//! vencimientos.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use kernel::Hrn;
use serde::Serialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use hodei_catalog::TrustLedger;
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::application::projector::evidence_hrn;
use hodei_evidence::domain::graph::{GraphChangeSet, GraphNode, NodeKind, RelationType};
use hodei_evidence::ports::graph::GraphWriter;
use hodei_evidence::standards::inventory::read_cyclonedx_17;
use hodei_evidence::{ContentDigest, EvidenceId, RelationDelta, RelationDeltaKind};
use hodei_reactive::behavior::{BehaviorError, GuardOutcome};
use hodei_reactive::events::{Event, EventKind, Subscription};
use hodei_reactive::intent::{
    Intent, IntentExecutor, IntentOrigin, IntentOutbox, Plan, PlanningBehavior, ReadScope,
};
use hodei_reactive::reactions::{
    BlastRadiusSource, DecisionRequester, DomainIntentExecutor, ImpactedSubject,
    PlannedVulnerabilityReaction, ReasonCode, SecurityCaseSink,
};
use hodei_reactive::runtime::{DispatchReport, ReactiveRuntime};
use hodei_risk::osv::read_osv;
use hodei_risk::projection::{project, vulnerability_hrn};
use hodei_risk_storage::blast_radius;
use hodei_risk_storage::impact_view::SurrealImpactView;
use hodei_risk_storage::reactions::SurrealSecurityCaseStore;
use hodei_risk_storage::subject_risk::RiskFactsReader;
use hodei_trust_policy::PromotionService;
use hodei_trust_policy::context::TrustContext;
use hodei_trust_policy::ports::DecisionStore;
use hodei_trust_policy_storage::SurrealDecisionStore;
use hodei_trust_policy_storage::intents::SurrealIntentStore;
use hodei_trust_policy_storage::reactions::{PromotionDecisionRequester, TrustContextSource};

use crate::config::InvestigationConfig;
use crate::evidence::EvidenceReader;
use crate::risk_facts::to_trust_facts;
use crate::trust::promotion::{TRUSTED_DISTRIBUTION, TrustPlane, TrustedChannelTargets};

/// Por qué el ciclo no pudo recorrer un tramo.
///
/// El fallo es un valor: la entrada la provoca un cliente (un documento
/// malformado, una cuenta ajena al grafo) y un pánico en el hilo del
/// servidor no es una respuesta honesta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReactorError {
    /// The advisory or the inventory is not a document this service reads.
    BadRequest(String),
    /// A dependency (graph, ledger, database) did not answer.
    Unavailable(String),
}

impl std::fmt::Display for ReactorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadRequest(reason) => write!(formatter, "the request is not readable: {reason}"),
            Self::Unavailable(reason) => {
                write!(formatter, "the reactive cycle could not run: {reason}")
            }
        }
    }
}

impl std::error::Error for ReactorError {}

/// What the cycle says about one advisory it just admitted.
///
/// `admitted_at_wall` and `elapsed_millis` are the admission stamp the SLO
/// needs: the moment the event entered and what the admission itself cost.
/// The time from admission to the first blocked pull is measured by the
/// reader of this answer, against the real serving path.
#[derive(Debug, Clone, Serialize)]
pub struct AdvisoryAdmission {
    /// The advisory as the graph names it (its canonical HRN).
    pub advisory: String,
    /// The event that carries it: deterministic, so a redelivery is the
    /// same event and the cycle says so instead of pretending twice.
    pub event_id: String,
    /// Whether this exact event had already run.
    pub duplicate: bool,
    /// The behaviours the event ran.
    pub behaviors_ran: usize,
    /// The receipts the cycle closed, applied or suppressed.
    pub receipts: usize,
    /// The chain was cut by the budget, and this is why.
    pub cut: Option<String>,
    /// Wall clock of the admission, for the reader's measurement.
    pub admitted_at_wall: DateTime<Utc>,
    /// Millis the admission itself spent inside the cycle.
    pub elapsed_millis: u128,
}

/// Lo que un barrido tocó.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct SweepCounters {
    /// Concesiones cuya vigencia se agotó y fueron reevaluadas.
    pub expired_reevaluated: usize,
    /// Concesiones cuya versión de política ya no es la activa.
    pub superseded_reevaluated: usize,
    /// Concesiones renovadas: la reevaluación volvió a concederlas.
    pub regenerated: usize,
    /// Pasadas que ya estaban dichas: el barrido no repite efectos.
    pub already_said: usize,
}

/// El tenant de la composición estampado en los intents.
///
/// No es una identidad de cuenta: la autoridad multi-cuenta del ciclo es
/// el HRN del asunto, que el radio de impacto y el peticionario de
/// decisiones validan en cada llamada. La marca existe porque el contrato
/// del intent la exige (un intent sin dueño no se reconcilia), y el
/// ejecutor la comprueba contra la composición que sirve.
const COMPOSITION_MARK: &str = "hodei";

/// El techo de sujetos que un solo evento de drift puede nombrar.
///
/// Un barrido que encontrara más deja el resto para el siguiente: decir
/// mil intents en un evento no es más honesto que decirlos en dos
/// pasadas, y el presupuesto del motor tiene que quedar holgado para la
/// cadena que cada sujeto dispara.
const MAX_DRIFT_SUBJECTS: usize = 1024;

/// El techo de filas de deltas que una pasada de reconciliación lee.
const MAX_RECONCILE_DELTAS: usize = 512;

/// The blast radius of the service: the tenant of every question comes
/// from the subject's own account and region, through the same template
/// the promotion and the risk facts already use.
///
/// The crate's own reader binds one tenant at construction and its
/// contract refuses any other account — which is right for it and too
/// narrow for a multi-account service. This adapter does not widen that
/// contract: it resolves the tenant per event, with exactly the authority
/// the crate asserts (a canonical HRN whose account must own the
/// advisory), and delegates the walk itself to the crate, unchanged.
pub struct GraphBlastRadius {
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<Db>>,
    limits: InvestigationConfig,
}

impl GraphBlastRadius {
    /// The radius over the graph and the template of the composition.
    pub fn new(
        graph: Arc<hodei_evidence_storage::SurrealGraphStore<Db>>,
        limits: InvestigationConfig,
    ) -> Self {
        Self { graph, limits }
    }
}

#[async_trait::async_trait]
impl BlastRadiusSource for GraphBlastRadius {
    async fn impacted(
        &self,
        account: &str,
        advisory: &str,
    ) -> Result<Vec<ImpactedSubject>, String> {
        let Some(vulnerability) = Hrn::from_string(advisory) else {
            return Err(format!("the advisory {advisory} is not a canonical HRN"));
        };
        // The same identity rule the crate enforces, kept verbatim: the
        // event's account claim has to match the advisory's own account,
        // and a request without an account is a closed door.
        if account.trim().is_empty() {
            return Err("the request does not say which account it belongs to".to_string());
        }
        if account != vulnerability.account_id() {
            return Err(format!(
                "the request names account {account} and the advisory belongs to {}",
                vulnerability.account_id()
            ));
        }
        let Some(tenant) = self.limits.the_graph_tenant(&vulnerability) else {
            return Err("the tenant template does not name an HRN".to_string());
        };

        let radius = blast_radius(self.graph.as_ref(), &tenant, &vulnerability)
            .await
            .map_err(|error| error.to_string())?;

        Ok(radius
            .depths
            .into_iter()
            // Only subjects below the component decide: the entry at
            // depth zero is a fact about the artifact, not the subject
            // of a concession.
            .filter(|entry| entry.depth >= 1)
            .map(|entry| ImpactedSubject {
                subject: entry.subject,
                deployment: entry.deployments.first().cloned(),
                depth: entry.depth,
            })
            .collect())
    }
}

/// The context of the reevaluation: the real one.
///
/// It assembles the capsule from the evidence that exists and the risk
/// facts from the impact graph, exactly the way the promotion endpoint
/// does — because a reevaluation that decided with a thinner context
/// than the decision it replaces would deny for its own holes, and would
/// revoke concessions a CVE never touched.
pub struct ContextFromEvidence {
    evidence: Arc<dyn EvidenceReader>,
    risk_facts: Arc<dyn RiskFactsReader>,
    limits: InvestigationConfig,
}

impl ContextFromEvidence {
    /// The source over the service's readers.
    pub fn new(
        evidence: Arc<dyn EvidenceReader>,
        risk_facts: Arc<dyn RiskFactsReader>,
        limits: InvestigationConfig,
    ) -> Self {
        Self {
            evidence,
            risk_facts,
            limits,
        }
    }
}

#[async_trait::async_trait]
impl TrustContextSource for ContextFromEvidence {
    async fn context_of(
        &self,
        subject: &str,
        _critical_reachable: u32,
        at: DateTime<Utc>,
    ) -> Result<TrustContext, String> {
        let Some(subject_hrn) = Hrn::from_string(subject) else {
            return Err(format!("the subject {subject} is not a canonical HRN"));
        };
        let account = subject_hrn.account_id();
        let digest = digest_of_subject(&subject_hrn)?;

        let records = self
            .evidence
            .evidence_of_subject(account, &subject_hrn)
            .await
            .map_err(|error| error.to_string())?;
        let capsule = CapsuleAssembler::new().assemble(&subject_hrn, &records, at);

        // The facts of the risk come from the graph — including the
        // advisory that triggered this reevaluation, which was projected
        // before the event was said and which the reconciliation pass
        // materializes into the view (HSP-134 left that seam open;
        // closing it here is what makes AC2's "no revokes a concession
        // nobody touched" checkable instead of lucky). The count the
        // reaction carries is NOT used to fabricate the facts: the graph
        // is the authority, and an answer that cannot be measured stays
        // UNKNOWN, which is what makes the policy refuse rather than
        // pretend.
        let facts = match self.limits.the_graph_tenant(&subject_hrn) {
            Some(tenant) => self
                .risk_facts
                .risk_facts(&tenant, &subject_hrn)
                .await
                .map_err(|error| error.to_string())?,
            None => hodei_risk_storage::subject_risk::SubjectFacts::unknown(),
        };

        Ok(TrustContext::builder(&capsule)
            .with_digest(digest)
            .with_risk(to_trust_facts(&facts))
            .assembled_at(at)
            .build())
    }
}

/// The digest a subject HRN names: `artifact/<repo>/<alg>-<hex>`.
fn digest_of_subject(subject: &Hrn) -> Result<String, String> {
    let resource = subject.resource_id();
    let tail = resource
        .rsplit('/')
        .next()
        .ok_or_else(|| format!("the subject {subject} names no digest"))?;
    let (algorithm, value) = tail
        .split_once('-')
        .ok_or_else(|| format!("the subject {subject} names no algorithm-digest"))?;
    Ok(format!("{algorithm}:{value}"))
}

/// An identifier safe for a Surreal record id and for an event name.
///
/// The HRNs of the cycle are full of `:` and `/`, and a raw HRN inside an
/// id would either fail to store or collide with the parser of another
/// component. The sanitization is lossy on purpose: the name of the event
/// only has to be unique for its inputs, and the full HRN travels inside
/// the payload, where it is read back whole.
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            _ => '.',
        })
        .collect()
}

/// A concession the sweep found, with everything the renewal needs.
#[derive(Debug, Clone, serde::Deserialize)]
struct StaleGrantRow {
    decision_id: String,
    account: String,
    repository: String,
    digest_algorithm: String,
    digest: String,
    subject: Option<String>,
    policy_version: Option<String>,
    valid_until: Option<DateTime<Utc>>,
}

/// The planned reevaluation of a concession whose validity lapsed.
///
/// It is the local twin of `PlannedExceptionExpiryReaction` and it
/// cannot be that planner: its effect key is fixed in the subject
/// (`decision:expired:{subject}`) and the engine closes the world once
/// per key, so the second expiry of a subject would never reevaluate
/// again. Here the key carries the pass: the same pass says nothing
/// twice — its execution record already exists — and a new lapse says
/// what it has to say. The reason stays `ExceptionExpired` because that
/// is the truth the decision trail records: the validity the concession
/// carried is done.
pub struct PlannedGrantExpiryReaction;

#[async_trait::async_trait]
impl PlanningBehavior for PlannedGrantExpiryReaction {
    fn name(&self) -> &str {
        "grant_expiry"
    }

    fn subscription(&self) -> Subscription {
        Subscription::to(EventKind::ExceptionExpired)
    }

    fn guard(&self, event: &Event) -> GuardOutcome {
        let named = event
            .payload
            .get("subject")
            .and_then(|value| value.as_str())
            .is_some_and(|subject| !subject.trim().is_empty());
        let passed = event
            .payload
            .get("pass")
            .and_then(|value| value.as_str())
            .is_some_and(|pass| !pass.trim().is_empty());
        if named && passed {
            GuardOutcome::Pass
        } else {
            GuardOutcome::Skip("the event names no subject and no pass".to_string())
        }
    }

    async fn plan(&self, event: &Event, scope: &ReadScope) -> Result<Plan, BehaviorError> {
        let Some(subject) = event
            .payload
            .get("subject")
            .and_then(|value| value.as_str())
        else {
            return Ok(Plan::default());
        };
        // The subject must be a canonical HRN or the requester will
        // refuse it at the account split: refuse it here, where the
        // reason can be named.
        if Hrn::from_string(subject).is_none() {
            return Err(BehaviorError::permanent(format!(
                "the lapse names {subject}, which is not a canonical HRN"
            )));
        }
        let pass = event
            .payload
            .get("pass")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        Ok(Plan {
            intents: vec![Intent::new(
                IntentOrigin::of(self.name(), event, &scope.origin.causation),
                scope.tenant.clone(),
                "decision_reevaluation",
                format!("decision:expired:{subject}:{pass}"),
                scope.origin.event_id.clone(),
                serde_json::json!({
                    "subject": subject,
                    "reason": ReasonCode::ExceptionExpired,
                    "at": scope.at.to_rfc3339(),
                }),
            )],
            events: Vec::new(),
        })
    }
}

/// The planned reevaluation of a concession whose policy version is no
/// longer the active one.
///
/// Same reasoning as the expiry planner and the same trap to avoid: the
/// effect key carries the version being drifted to, so one drift is said
/// once and a later drift says itself again. The subjects travel in the
/// event itself: which concessions lapsed under which version is a
/// question for the ledger, and this half of the cycle reads the event
/// rather than reaching back into the catalog.
pub struct PlannedPolicyDriftReaction;

#[async_trait::async_trait]
impl PlanningBehavior for PlannedPolicyDriftReaction {
    fn name(&self) -> &str {
        "policy_drift"
    }

    fn subscription(&self) -> Subscription {
        Subscription::to(EventKind::VulnerabilityAccepted)
    }

    fn guard(&self, event: &Event) -> GuardOutcome {
        // The version is what makes the drift new, and the subjects are
        // what it reaches; an event without either is not this
        // behaviour's to answer.
        let named = event
            .payload
            .get("policy_version")
            .and_then(|value| value.as_str())
            .is_some_and(|version| !version.trim().is_empty());
        let reaches = event
            .payload
            .get("subjects")
            .and_then(|value| value.as_array())
            .is_some_and(|subjects| !subjects.is_empty());
        if named && reaches {
            GuardOutcome::Pass
        } else {
            GuardOutcome::Skip("the event names no policy version and no subjects".to_string())
        }
    }

    async fn plan(&self, event: &Event, scope: &ReadScope) -> Result<Plan, BehaviorError> {
        let Some(version) = event
            .payload
            .get("policy_version")
            .and_then(|value| value.as_str())
        else {
            return Ok(Plan::default());
        };
        let Some(subjects) = event
            .payload
            .get("subjects")
            .and_then(|value| value.as_array())
        else {
            return Ok(Plan::default());
        };
        let mut intents = Vec::new();
        for entry in subjects.iter().take(MAX_DRIFT_SUBJECTS) {
            let Some(subject) = entry.as_str() else {
                continue;
            };
            if Hrn::from_string(subject).is_none() {
                return Err(BehaviorError::permanent(format!(
                    "the drift names {subject}, which is not a canonical HRN"
                )));
            }
            intents.push(Intent::new(
                IntentOrigin::of(self.name(), event, &scope.origin.causation),
                scope.tenant.clone(),
                "decision_reevaluation",
                format!("decision:drift:{subject}:{version}"),
                scope.origin.event_id.clone(),
                serde_json::json!({
                    "subject": subject,
                    // The truth of a drift is a lapsed validity under a
                    // superseded policy: the engine's reason vocabulary
                    // says exactly that.
                    "reason": ReasonCode::ExceptionExpired,
                    "at": scope.at.to_rfc3339(),
                }),
            ));
        }
        Ok(Plan {
            intents,
            events: Vec::new(),
        })
    }
}

/// The cycle itself: the runtime, its durable outbox, and the sweeps
/// that keep the concessions honest after the admission that opened
/// them.
///
/// One instance serves every account: the tenant of the composition
/// marks the intents, and the account of every subject is read from the
/// subject HRN itself. A concession is never reevaluated against another
/// account's graph: both the radius and the requester enforce it.
pub struct VulnerabilityReactor {
    runtime: ReactiveRuntime,
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<Db>>,
    view: SurrealImpactView<Db>,
    db: Arc<Surreal<Db>>,
    plane: Arc<TrustPlane>,
    decisions: Arc<SurrealDecisionStore<Db>>,
    ledger: Arc<dyn TrustLedger>,
    limits: InvestigationConfig,
}

impl VulnerabilityReactor {
    /// Builds the cycle over the parts the composition root already has.
    ///
    /// Nothing here invents a decision: the policy registry and the
    /// store of decisions are shared with the plane the promotion
    /// endpoint uses — the registry is handles over the same inner
    /// state, so a policy activated later is seen by both halves. The
    /// service cannot be cloned, so the reactor builds its own over
    /// them.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<Surreal<Db>>,
        graph: Arc<hodei_evidence_storage::SurrealGraphStore<Db>>,
        view: SurrealImpactView<Db>,
        plane: Arc<TrustPlane>,
        ledger: Arc<dyn TrustLedger>,
        evidence: Arc<dyn EvidenceReader>,
        risk_facts: Arc<dyn RiskFactsReader>,
        limits: InvestigationConfig,
    ) -> Self {
        let intents = Arc::new(SurrealIntentStore::new(db.clone()));
        let cases = Arc::new(SurrealSecurityCaseStore::new(db.clone()));
        let decisions = Arc::new(SurrealDecisionStore::new(db.clone()));
        let requester_service = Arc::new(PromotionService::new(
            decisions.clone() as Arc<dyn DecisionStore>,
            Arc::new(TrustedChannelTargets),
            Arc::new(plane.registry()),
        ));
        let radius = Arc::new(GraphBlastRadius::new(graph.clone(), limits.clone()));
        let contexts: Arc<dyn TrustContextSource> = Arc::new(ContextFromEvidence::new(
            evidence,
            risk_facts,
            limits.clone(),
        ));
        let requester: Arc<dyn DecisionRequester> = Arc::new(
            PromotionDecisionRequester::new(
                requester_service,
                decisions.clone() as Arc<dyn DecisionStore>,
                contexts,
                plane.policy_id().to_string(),
                TRUSTED_DISTRIBUTION,
                "reactor",
            )
            .with_ledger(ledger.clone()),
        );
        let executor: Arc<dyn IntentExecutor> = Arc::new(DomainIntentExecutor::new(
            COMPOSITION_MARK,
            cases as Arc<dyn SecurityCaseSink>,
            requester,
        ));

        let mut runtime =
            ReactiveRuntime::new(intents.clone(), Default::default()).with_tenant(COMPOSITION_MARK);
        runtime.with_outbox(intents as Arc<dyn IntentOutbox>);
        runtime.register_planned(
            Arc::new(PlannedVulnerabilityReaction::new(radius)),
            executor.clone(),
        );
        runtime.register_planned(Arc::new(PlannedGrantExpiryReaction), executor.clone());
        runtime.register_planned(Arc::new(PlannedPolicyDriftReaction), executor);

        Self {
            runtime,
            graph,
            view,
            db,
            plane,
            decisions,
            ledger,
            limits,
        }
    }

    /// Admits one OSV advisory and runs the cycle.
    ///
    /// The inventory travels with the advisory when the caller has it:
    /// the projection of the domain (`hodei_risk::projection::project`)
    /// is what writes components and the `affected_by` edge into the
    /// graph, and the advisory alone against an already-projected
    /// inventory still finds its subjects. The entry does not
    /// re-implement the matching: it feeds the same pure projection the
    /// crates use.
    ///
    /// The changeset carries its cause — the event that will be said —
    /// and that is what makes the graph emit the deltas the impact view
    /// reads: a projection without cause is the rebuild's shape, and the
    /// live graph does not talk about it.
    pub async fn admit_advisory(
        &self,
        account: &str,
        region: &str,
        repository: &str,
        digest: &ContentDigest,
        advisory_document: &str,
        inventory_document: Option<&str>,
    ) -> Result<AdvisoryAdmission, ReactorError> {
        let started = std::time::Instant::now();
        let at = Utc::now();

        let advisory = read_osv(advisory_document)
            .map_err(|error| ReactorError::BadRequest(error.to_string()))?;
        let subject = crate::trust::hrn::artifact_hrn(region, account, repository, digest);
        let tenant = self.limits.the_graph_tenant(&subject).ok_or_else(|| {
            ReactorError::BadRequest("the tenant template does not name an HRN".to_string())
        })?;
        let advisory_hrn = vulnerability_hrn(&tenant, &advisory.id);

        // The evidence that supports every node this admission writes.
        // It is deterministic on the document: the same advisory
        // ingested twice is the same evidence and the same graph, not a
        // new observation nobody asked for.
        let evidence_id = EvidenceId::new(format!(
            "advisory-{}",
            ContentDigest::of_bytes(advisory_document.as_bytes()).value
        ));
        // The event is the cause of the projection and the name of the
        // cycle: one name for the same fact, said by both halves of it.
        let event_id = format!(
            "evt-cve-{}-{}",
            sanitize(&tenant.to_string()),
            sanitize(&advisory_hrn.to_string())
        );

        let changes = match inventory_document {
            Some(document) => {
                let inventory = read_cyclonedx_17(document)
                    .map_err(|error| ReactorError::BadRequest(error.to_string()))?;
                project(
                    &tenant,
                    &subject,
                    &inventory,
                    Some(&advisory),
                    evidence_id.clone(),
                    at,
                )
            }
            None => {
                // Without an inventory the only facts this admission
                // vouches for are the advisory and the evidence that
                // says it. The components and the edges to them belong
                // to the inventory that projected them, and this entry
                // does not invent them — and does not draw an
                // `affected_by` edge either, because the graph would
                // (rightly) refuse to credit a relation nobody
                // observed.
                GraphChangeSet::empty()
                    .with_node(GraphNode::new(
                        evidence_hrn(&tenant, &evidence_id),
                        tenant.clone(),
                        NodeKind::Evidence,
                        at,
                    ))
                    .with_node(GraphNode::new(
                        advisory_hrn.clone(),
                        tenant.clone(),
                        NodeKind::Vulnerability,
                        at,
                    ))
            }
        };
        self.graph
            .apply(changes.with_cause(event_id.clone()))
            .await
            .map_err(|error| ReactorError::Unavailable(error.to_string()))?;

        // The view is the authority of the reevaluation's context, and
        // the graph just said the relation that reaches the subject. A
        // reevaluation against a lagging — or simply absent — route for
        // this advisory would answer UNKNOWN, and the policy would
        // refuse by the gap, not by the CVE. Materialize THIS advisory
        // first: the delta reconciliation cannot do it (a new advisory
        // has no route to invalidate), and `rebuild_advisory` is the
        // crate's own idempotent primitive — it walks the graph and
        // stamps the watermark at the version the ledger has reached,
        // which is exactly what makes the view current for the
        // context. The crash in the middle leaves the route one pass
        // behind; the next admission or the sweep rebuilds it.
        self.view
            .rebuild_advisory(&tenant, &advisory_hrn)
            .await
            .map_err(|error| ReactorError::Unavailable(error.to_string()))?;

        let event = Event::new(
            event_id,
            EventKind::VulnerabilityAccepted,
            serde_json::json!({
                "advisory": advisory_hrn.to_string(),
                "account": account,
            }),
            at,
        );
        let event_id = event.id.clone();

        let report = self
            .runtime
            .dispatch_planned(event, at)
            .await
            .map_err(|error| ReactorError::Unavailable(error.to_string()))?;

        Ok(AdvisoryAdmission {
            advisory: advisory_hrn.to_string(),
            duplicate: !report.duplicates.is_empty(),
            event_id,
            behaviors_ran: report.executions.len(),
            receipts: report.applied.len() + report.suppressed.len(),
            cut: report.cut,
            admitted_at_wall: at,
            elapsed_millis: started.elapsed().as_millis(),
        })
    }

    /// Reconciles what a crash left behind: pending intents and claimed
    /// ones whose lease died. An effect with a closed receipt is not
    /// touched again — the receipt says what is true.
    pub async fn resume(&self, now: DateTime<Utc>) -> Result<DispatchReport, ReactorError> {
        self.runtime
            .resume(now)
            .await
            .map_err(|error| ReactorError::Unavailable(error.to_string()))
    }

    /// Sweeps the concessions whose validity ended: the exception or the
    /// evidence behind them expired, or the policy that granted them is
    /// no longer the active one.
    ///
    /// The serving path already refuses a stale concession (HSP-135):
    /// the sweep is not what protects the wire, it is what makes the
    /// truth reevaluated — a new decision on the current policy and
    /// facts. The denial is enforced by the requester itself: a
    /// reevaluation that refuses revokes the concession it supersedes.
    /// The concession the reevaluation wants to KEEP is renewed here,
    /// because the requester knows the subject and the ledger does not
    /// name a repository: the sweep holds the row, and the row is where
    /// the repository, the digest and the horizon of the old concession
    /// live. Renewal is one grant for one digest — this path never
    /// widens what a concession already covered.
    pub async fn sweep_stale_grants(
        &self,
        now: DateTime<Utc>,
    ) -> Result<SweepCounters, ReactorError> {
        let rows: Vec<StaleGrantRow> = self
            .db
            .query(
                "SELECT decision_id, account, repository, digest_algorithm, digest, subject, \
                 policy_version, valid_until FROM trust_grant \
                 WHERE revoked_at = NONE \
                 ORDER BY account, repository, digest \
                 LIMIT 4096;",
            )
            .await
            .map_err(|error| ReactorError::Unavailable(error.to_string()))?
            .take(0)
            .map_err(|error| ReactorError::Unavailable(error.to_string()))?;

        let active = self.plane.active_policy_version();
        let mut counters = SweepCounters::default();
        // Concessions whose own horizon lapsed, grouped by subject.
        let mut lapsed: BTreeMap<String, Vec<&StaleGrantRow>> = BTreeMap::new();
        // Concessions decided under a version that is no longer active,
        // grouped by account: the drift event is per account and
        // version.
        let mut drifted: BTreeMap<String, Vec<&StaleGrantRow>> = BTreeMap::new();

        for row in &rows {
            let Some(subject) = row.subject.as_deref() else {
                // A concession without the vigencia contract belongs to
                // a subject this cycle cannot name: the serving path
                // hides it, and there is no HRN to reevaluate.
                continue;
            };
            if Hrn::from_string(subject).is_none() {
                continue;
            }
            if row.valid_until.is_some_and(|until| until <= now) {
                lapsed.entry(subject.to_string()).or_default().push(row);
            } else if active
                .as_deref()
                .is_some_and(|version| row.policy_version.as_deref() != Some(version))
            {
                drifted.entry(row.account.clone()).or_default().push(row);
            }
        }

        for (subject, rows) in lapsed {
            // The pass is the rounded minute: a sweep every minute says
            // each lapse once, and a redelivered pass finds its own
            // execution record and does nothing. The effect key carries
            // the same pass, which is what lets a LATER lapse of the
            // same subject be said again (see the module note).
            let pass = (now.timestamp() / 60).to_string();
            let event = Event::new(
                format!("evt-expiry-{}-{pass}", sanitize(&subject)),
                EventKind::ExceptionExpired,
                serde_json::json!({ "subject": subject, "pass": pass }),
                now,
            );
            let report = self
                .runtime
                .dispatch_planned(event, now)
                .await
                .map_err(|error| ReactorError::Unavailable(error.to_string()))?;
            if report.duplicates.is_empty() {
                counters.expired_reevaluated += 1;
            } else {
                counters.already_said += 1;
            }
            self.renew_concessions(&rows, now, &mut counters).await?;
        }

        if let Some(version) = active {
            for (account, rows) in drifted {
                let subjects: BTreeSet<String> = rows
                    .iter()
                    .filter_map(|row| row.subject.clone())
                    .take(MAX_DRIFT_SUBJECTS)
                    .collect();
                if subjects.is_empty() {
                    continue;
                }
                // The set the version is said for is part of the name:
                // the same set says the same event and runs once; a
                // subject that went stale in a later pass makes a new
                // set, and a later pass of the same complete set is the
                // same pass again.
                let joined: Vec<&str> = subjects.iter().map(String::as_str).collect();
                let set_digest = ContentDigest::of_bytes(joined.join("\n").as_bytes()).value;
                let event = Event::new(
                    format!(
                        "evt-drift-{}-{}-{set_digest}",
                        sanitize(&account),
                        sanitize(&version)
                    ),
                    EventKind::VulnerabilityAccepted,
                    serde_json::json!({
                        "policy_version": version,
                        "account": account,
                        "subjects": Vec::from_iter(subjects),
                    }),
                    now,
                );
                let report = self
                    .runtime
                    .dispatch_planned(event, now)
                    .await
                    .map_err(|error| ReactorError::Unavailable(error.to_string()))?;
                if report.duplicates.is_empty() {
                    counters.superseded_reevaluated += 1;
                } else {
                    counters.already_said += 1;
                }
                self.renew_concessions(&rows, now, &mut counters).await?;
            }
        }

        Ok(counters)
    }

    /// Gives a concession back to the decision that still wants it.
    ///
    /// It reads the newest decision of each swept subject — the one the
    /// dispatch that just ran wrote — and grants for it, with the
    /// horizon the plane computes. A subject whose newest decision is
    /// not a concession gets nothing: its old concession was already
    /// revoked by the requester, and inventing a grant here would be
    /// deciding, which this module does not do.
    async fn renew_concessions(
        &self,
        rows: &[&StaleGrantRow],
        now: DateTime<Utc>,
        counters: &mut SweepCounters,
    ) -> Result<(), ReactorError> {
        let mut newest: BTreeMap<String, hodei_trust_policy::promotion::PromotionDecision> =
            BTreeMap::new();
        for row in rows {
            let Some(subject) = row.subject.as_deref() else {
                continue;
            };
            if newest.contains_key(subject) {
                continue;
            }
            let decisions = self
                .decisions
                .list_for_subject(&row.account, subject)
                .await
                .map_err(|error| ReactorError::Unavailable(error.to_string()))?;
            if let Some(decision) = decisions.into_iter().next() {
                newest.insert(subject.to_string(), decision);
            }
        }
        for row in rows {
            let Some(subject) = row.subject.as_deref() else {
                continue;
            };
            let Some(decision) = newest.get(subject) else {
                continue;
            };
            if !decision.applied || decision.id.as_str() == row.decision_id {
                // Not a concession, or the very decision that already
                // granted it: nothing to renew, nothing to say twice.
                continue;
            }
            let Ok(digest) = digest_of_row(row) else {
                continue;
            };
            let grant = hodei_catalog::TrustGrant {
                account: row.account.clone(),
                repository: row.repository.clone(),
                digest,
                decision_id: decision.id.as_str().to_string(),
                actor: "reactor".to_string(),
                granted_at: now,
                revoked_at: None,
                revoked_by: None,
                revoke_reason: None,
                subject: Some(subject.to_string()),
                policy_version: Some(decision.trust.policy_version.clone()),
                capsule_hash: decision.trust.capsule_hash.clone(),
                risk_version: Some(decision.trust.risk_version),
                valid_until: Some(self.plane.validity_of(decision, now)),
            };
            self.ledger
                .grant(std::slice::from_ref(&grant))
                .await
                .map_err(|error| ReactorError::Unavailable(error.to_string()))?;
            counters.regenerated += 1;
        }
        Ok(())
    }

    /// Materializes the impact view from the deltas the live projections
    /// have said (HSP-134 left the view without a consumer in the
    /// service). A delta is applied through the view's own machinery,
    /// which is idempotent per (route, delta): the pass converges no
    /// matter where it resumed from, and a crash in the middle just
    /// leaves deltas for the next pass.
    pub async fn reconcile_impact_view(&self) -> Result<usize, ReactorError> {
        // The ledger's own reading port is per subscription and per
        // tenant, and a reconciler of the whole store has to see every
        // tenant's story: the same table, read straight, is the honest
        // question here. The quota bounds the pass; the deltas beyond
        // it are the next pass's, and applying one twice is a no-op by
        // the view's own watermark.
        let rows: Vec<hodei_evidence_storage::DeltaRow> = self
            .db
            .query(format!(
                "SELECT * FROM type::table($table) ORDER BY tenant, seq LIMIT {MAX_RECONCILE_DELTAS};"
            ))
            .bind(("table", hodei_evidence_storage::DELTA_TABLE))
            .await
            .map_err(|error| ReactorError::Unavailable(error.to_string()))?
            .take(0)
            .map_err(|error| ReactorError::Unavailable(error.to_string()))?;

        let mut done = 0;
        for row in rows {
            let Some(delta) = delta_of_row(&row) else {
                continue;
            };
            // `invalidated_by` says which routes the delta touches; the
            // apply is the view's own step, and a route that already
            // digested the graph's current truth says nothing here.
            let touched = self
                .view
                .invalidated_by(&delta)
                .await
                .map_err(|error| ReactorError::Unavailable(error.to_string()))?;
            if touched.is_empty() {
                continue;
            }
            self.view
                .apply_delta(&delta)
                .await
                .map_err(|error| ReactorError::Unavailable(error.to_string()))?;
            done += touched.len();
        }
        Ok(done)
    }

    /// The template of the graph tenant, for the wiring's diagnostics.
    pub fn the_limits(&self) -> &InvestigationConfig {
        &self.limits
    }
}

/// Rebuilds a delta from the row the ledger stored.
///
/// A row that does not parse is skipped, not failed: the
/// reconciliation is a converger, and one unreadable row of a long pass
/// must not stop the other nine hundred. It mirrors the storage crate's
/// own row shape (its helper is private): the same fields, the same
/// kinds, the same leniency.
fn delta_of_row(row: &hodei_evidence_storage::DeltaRow) -> Option<RelationDelta> {
    Some(RelationDelta {
        kind: match row.kind.as_str() {
            "added" => RelationDeltaKind::Added,
            "removed" => RelationDeltaKind::Removed,
            "support_changed" => RelationDeltaKind::SupportChanged,
            _ => return None,
        },
        tenant: Hrn::from_string(&row.tenant)?,
        relation: RelationType::parse(&row.relation)?,
        from: Hrn::from_string(&row.from)?,
        to: Hrn::from_string(&row.to)?,
        evidence: EvidenceId::new(row.evidence.clone()),
        cause: row.cause.clone(),
        observed_at: row.observed_at,
    })
}

/// The digest a grant row names.
fn digest_of_row(row: &StaleGrantRow) -> Result<ContentDigest, String> {
    let algorithm = match row.digest_algorithm.as_str() {
        "sha256" => hodei_evidence::DigestAlgorithm::Sha256,
        "sha512" => hodei_evidence::DigestAlgorithm::Sha512,
        other => return Err(format!("the grant names an unknown algorithm {other}")),
    };
    let digest = ContentDigest {
        algorithm,
        value: row.digest.clone(),
    };
    if !digest.is_well_formed() {
        return Err("the grant names a digest that is not well formed".to_string());
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hrn(text: &str) -> Hrn {
        Hrn::from_string(text).expect("a canonical HRN")
    }

    fn subject() -> Hrn {
        hrn(
            "hrn:hodei:artifacts:eu:acme:artifact/releases-sha256-0000000000000000000000000000000000000000000000000000000000000000",
        )
    }

    fn scope(event: &Event) -> ReadScope {
        ReadScope {
            tenant: COMPOSITION_MARK.to_string(),
            origin: hodei_reactive::intent::EventOrigin::new(event.id.clone(), Vec::new()),
            at: event.happened_at,
        }
    }

    // --- sanitize: the lossy name keeps identity, sheds the syntax. ---

    #[test]
    fn two_different_inputs_do_not_share_a_sanitized_name() {
        assert_ne!(
            sanitize("hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-a"),
            sanitize("hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-b"),
            "a collision here would say two advisories as one event"
        );
    }

    #[test]
    fn the_sanitized_name_keeps_only_what_an_id_can_carry() {
        let name = sanitize("hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-0001");
        assert!(!name.contains(':'));
        assert!(!name.contains('/'));
        assert!(name.contains("GHSA-0001"));
    }

    // --- digest_of_subject: the subject's tail is the concession's key. ---

    #[test]
    fn the_digest_is_read_back_from_the_subject_it_was_stamped_in() {
        let digest = ContentDigest::of_bytes(b"a body");
        let subject = crate::trust::hrn::artifact_hrn("eu", "acme", "releases", &digest);
        let read = digest_of_subject(&subject).expect("the subject names a digest");
        assert_eq!(read, format!("sha256:{}", digest.value));
    }

    #[test]
    fn a_subject_whose_tail_has_no_algorithm_dash_is_an_error_not_a_guess() {
        // The shape the cycle writes is `artifact/<repo>/<alg>-<hex>`; a
        // resource whose last segment carries no dash names no digest,
        // and reading one would be inventing the concession's key.
        let stranger = hrn("hrn:hodei:artifacts:eu:acme:artifact/releases");
        assert!(digest_of_subject(&stranger).is_err());
    }

    // --- delta_of_row: a row that does not parse is skipped, not fatal. ---

    fn row(kind: &str) -> hodei_evidence_storage::DeltaRow {
        hodei_evidence_storage::DeltaRow {
            seq: 1,
            kind: kind.to_string(),
            tenant: "hrn:hodei:artifacts:eu:acme:artifact/acme".to_string(),
            relation: "affected_by".to_string(),
            from: subject().to_string(),
            to: "hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-0001".to_string(),
            evidence: "ev-1".to_string(),
            cause: "c".to_string(),
            observed_at: Utc::now(),
        }
    }

    #[test]
    fn the_three_known_kinds_round_trip_and_anything_else_is_skipped() {
        assert!(delta_of_row(&row("added")).is_some());
        assert!(delta_of_row(&row("removed")).is_some());
        assert!(delta_of_row(&row("support_changed")).is_some());
        assert!(
            delta_of_row(&row("exploded")).is_none(),
            "an unknown kind must not stop a long pass"
        );
        let broken = row("added");
        let broken = hodei_evidence_storage::DeltaRow {
            tenant: "not-an-hrn".to_string(),
            ..broken
        };
        assert!(delta_of_row(&broken).is_none());
    }

    // --- the planners: the pass in the key is what lets time be said twice. ---

    #[tokio::test]
    async fn a_lapse_without_a_pass_is_skipped_and_a_lapse_says_one_intent_with_the_pass_in_its_key()
     {
        let event = Event::new(
            "evt-expiry-x",
            EventKind::ExceptionExpired,
            serde_json::json!({ "subject": subject().to_string() }),
            Utc::now(),
        );
        let planner = PlannedGrantExpiryReaction;
        assert!(
            matches!(planner.guard(&event), GuardOutcome::Skip(_)),
            "a lapse without a pass cannot be deduplicated by key"
        );

        let named = Event::new(
            "evt-expiry-y",
            EventKind::ExceptionExpired,
            serde_json::json!({ "subject": subject().to_string(), "pass": "2900000" }),
            Utc::now(),
        );
        assert!(matches!(planner.guard(&named), GuardOutcome::Pass));
        let plan = planner
            .plan(&named, &scope(&named))
            .await
            .expect("the plan is said");
        assert_eq!(plan.intents.len(), 1);
        assert!(
            plan.intents[0].idempotency_key.contains("2900000"),
            "the pass belongs in the key: without it the second lapse of a subject would be suppressed forever"
        );
    }

    #[tokio::test]
    async fn a_lapse_that_names_a_non_canonical_subject_is_refused_where_the_reason_can_be_named() {
        let event = Event::new(
            "evt-expiry-z",
            EventKind::ExceptionExpired,
            serde_json::json!({ "subject": "app-1.0.0", "pass": "1" }),
            Utc::now(),
        );
        let error = PlannedGrantExpiryReaction
            .plan(&event, &scope(&event))
            .await
            .expect_err("a stranger's subject is refused, not planned");
        assert!(
            !error.retryable,
            "a bad HRN is a bad input: retrying it would retry the same mistake"
        );
    }

    #[tokio::test]
    async fn the_drift_planner_says_one_intent_per_named_subject_and_refuses_a_stranger() {
        let one = subject();
        let event = Event::new(
            "evt-drift",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({
                "policy_version": "2",
                "subjects": [one.to_string()],
            }),
            Utc::now(),
        );
        let planner = PlannedPolicyDriftReaction;
        assert!(matches!(planner.guard(&event), GuardOutcome::Pass));
        let plan = planner
            .plan(&event, &scope(&event))
            .await
            .expect("the drift is planned");
        assert_eq!(plan.intents.len(), 1);
        assert!(plan.intents[0].idempotency_key.contains(":drift:"));

        let polluted = Event::new(
            "evt-drift-bad",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({
                "policy_version": "2",
                "subjects": ["not-an-hrn"],
            }),
            Utc::now(),
        );
        assert!(
            PlannedPolicyDriftReaction
                .plan(&polluted, &scope(&polluted))
                .await
                .is_err()
        );

        let empty = Event::new(
            "evt-drift-empty",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({ "policy_version": "2", "subjects": [] }),
            Utc::now(),
        );
        assert!(
            matches!(
                PlannedPolicyDriftReaction.guard(&empty),
                GuardOutcome::Skip(_)
            ),
            "a drift that reaches nobody is not this behaviour's to answer"
        );
    }

    // --- GraphBlastRadius: the identity gate, before any read. ---

    #[tokio::test]
    async fn the_radius_refuses_a_stranger_before_reading_the_graph() {
        // The gate is a property of the adapter's contract, and it is
        // exercised without a seeded graph: nothing is asked, nothing is
        // reached. An adapter whose gate needs a live store could not
        // be tested where it matters -- the account that does not match.
        let scratch: Surreal<Db> = Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .expect("an in-memory scratch store");
        let radius = GraphBlastRadius {
            graph: Arc::new(hodei_evidence_storage::SurrealGraphStore::new(Arc::new(
                scratch,
            ))),
            limits: InvestigationConfig::default(),
        };
        let advisory = hrn("hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-0001").to_string();
        assert!(radius.impacted("", &advisory).await.is_err());
        assert!(radius.impacted("other", &advisory).await.is_err());
        assert!(radius.impacted("acme", "not-an-hrn").await.is_err());
    }

    // --- the admission's own shape: what the cycle answers. ---

    #[test]
    fn a_displayed_reactor_error_names_the_truce_it_broke() {
        let text = ReactorError::BadRequest("the document is unreadable".to_string()).to_string();
        assert!(text.contains("unreadable"));
        let text = ReactorError::Unavailable("the graph did not answer".to_string()).to_string();
        assert!(text.contains("not answer") || text.contains("did not answer"));
    }
}
