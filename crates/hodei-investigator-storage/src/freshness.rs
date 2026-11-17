//! El comprobador de frescura, sobre el almacenamiento que ya existe.
//!
//! El verificador --la funcion pura-- vive en el dominio
//! (`hodei_investigator::freshness`). Aqui solo se **trae el mundo**: la
//! version de ahora, los digests de ahora y los cambios de ahora. El
//! almacenamiento no decide, y esta separation es la que hace que el
//! veredicto sea el mismo con cualquier material.
//!
//! # Sin motor de suscripciones
//!
//! La comprobacion es **bajo demanda**: quien tiene el informe pregunta,
//! y la respuesta se calcula con lo que hay. No hay un motor que empuje
//! invalidaciones, ni una cola, ni una tabla nueva: el registro de
//! deltas que ya existe es la historia, y se lee donde se quedo.
//!
//! # El aislamiento del tenant
//!
//! El registro se pregunta **por el tenant**, y el tenant es el de la
//! cuenta que se esta comprobando: una consulta de otra cuenta no llega
//! al registro. Y el sujeto tiene que ser **del tenant**: un sujeto de
//! otra cuenta no se consulta, y se dice que no se sabe en vez de
//! consultar lo que sea.

use std::collections::BTreeSet;
use std::sync::Arc;

use hodei_evidence::domain::delta::{
    RelationDeltaKind, RelationDeltaLedger, RelationDeltaSubscription,
};
use hodei_evidence::domain::graph::RelationType;
use hodei_evidence::ports::EvidenceStore;
use hodei_investigator::freshness::{
    ChangeKind, ChangedReference, CurrentWorld, EVIDENCE_SOURCE, Freshness, GRAPH_SOURCE, judge,
};
use hodei_investigator::read_set::{ReadKind, ReadSet};
use kernel::Hrn;

/// Cuantos deltas se leen de una vez.
const PAGE: usize = 512;

/// Cuantas paginas se leen antes de rendirse.
///
/// El limite es una puerta de **trabajo**, no de tiempo: leer los deltas
/// de una cuenta que cambio mucho es un trabajo acotado, y si no cabe
/// en el limite la respuesta no es "vigente" sino "no se sabe".
const MAX_PAGES: usize = 64;

/// El comprobador de frescura sobre el grafo y la evidencia.
///
/// El tenant es el de la cuenta que se comprueba, y las consultas que
/// hace --el registro de deltas y la evidencia del sujeto-- son de ese
/// tenant y de ninguno mas.
pub struct LedgerFreshness {
    evidence: Arc<dyn EvidenceStore>,
    deltas: Arc<dyn RelationDeltaLedger>,
    tenant: Hrn,
}

impl LedgerFreshness {
    /// El comprobador sobre el almacen de evidencia y el registro de
    /// deltas, para un tenant.
    pub fn new(
        evidence: Arc<dyn EvidenceStore>,
        deltas: Arc<dyn RelationDeltaLedger>,
        tenant: Hrn,
    ) -> Self {
        Self {
            evidence,
            deltas,
            tenant,
        }
    }

    /// La frescura del informe de una lectura.
    ///
    /// El manifiesto es el del informe entregado, y el sujeto es el
    /// mismo sujeto que el informe dice. Un sujeto de otra cuenta no se
    /// consulta: el veredicto es que no se sabe, no una respuesta sobre
    /// lo que no es suyo.
    pub async fn freshness_of(&self, report: &ReadSet, subject: &Hrn) -> Freshness {
        if subject.account_id() != self.tenant.account_id() {
            return Freshness::Unknown {
                reason: "the subject does not belong to the tenant of the check".to_string(),
            };
        }
        let world = self.the_world(report, subject).await;
        judge(report, &world)
    }

    /// El mundo de ahora, para lo que el manifiesto dice haber leido.
    async fn the_world(&self, report: &ReadSet, subject: &Hrn) -> CurrentWorld {
        let mut world = CurrentWorld::default();

        // La version del grafo: la posicion del tenant en su historia.
        match self.deltas.version_of(&self.tenant).await {
            Ok(version) => world.generations.push((GRAPH_SOURCE.to_string(), version)),
            Err(error) => {
                world.unreachable.push(GRAPH_SOURCE.to_string());
                tracing::warn!(%error, "the version of the graph could not be read");
            }
        }

        // Los cambios que hubo desde la version que el manifiesto dice
        // haber leido. Si la version no se movio, no hay nada que mirar.
        if let Some(after) = the_read_version(report, GRAPH_SOURCE)
            && let Some((_, current)) = world
                .generations
                .iter()
                .find(|(source, _)| source == GRAPH_SOURCE)
            && *current != after
        {
            match self.the_changes(report, after).await {
                Ok(changes) => world.changes = changes,
                Err(error) => {
                    world.unreachable.push(GRAPH_SOURCE.to_string());
                    tracing::warn!(%error, "the changes of the graph could not be read");
                }
            }
        }

        // Los digests de la evidencia que hay ahora: lo que se leyo se
        // contrasta con lo que hay.
        match self
            .evidence
            .evidence_of_subject(self.tenant.account_id(), subject)
            .await
        {
            Ok(records) => {
                for record in records {
                    world
                        .digests
                        .push((record.id().to_string(), record.content().digest.to_string()));
                }
            }
            Err(error) => {
                world.unreachable.push(EVIDENCE_SOURCE.to_string());
                tracing::warn!(%error, "the evidence of the subject could not be read");
            }
        }

        world
    }

    /// Los cambios del grafo desde la version que el manifiesto dice
    /// haber leido.
    ///
    /// Se leen por paginas hasta llegar a la version de ahora, y si no
    /// caben en el limite de trabajo la respuesta es que no se sabe: un
    /// "vigente" sin haber leido todo seria una garantia inventada.
    async fn the_changes(
        &self,
        report: &ReadSet,
        after: u64,
    ) -> Result<Vec<ChangedReference>, String> {
        let Some(types) = the_relation_types(report) else {
            return Err("the relations of the report name a type that is not known".to_string());
        };
        let subscription = RelationDeltaSubscription::new(self.tenant.clone(), types);
        let mut changes: Vec<ChangedReference> = Vec::new();
        let mut cursor = after;
        for _ in 0..MAX_PAGES {
            let (deltas, stopped) = self.deltas.pending(&subscription, cursor, PAGE).await?;
            for (_, delta) in &deltas {
                let reference = ChangedReference {
                    source: GRAPH_SOURCE.to_string(),
                    kind: ReadKind::Relation,
                    resource: delta.identity(),
                    evidence: delta.evidence.to_string(),
                    what: match delta.kind {
                        RelationDeltaKind::Added => ChangeKind::Added,
                        RelationDeltaKind::Removed => ChangeKind::Removed,
                        RelationDeltaKind::SupportChanged => ChangeKind::Changed,
                    },
                };
                if !changes.contains(&reference) {
                    changes.push(reference);
                }
            }
            if deltas.len() < PAGE || stopped <= cursor {
                // No hay mas deltas que leer: se llego al final.
                return Ok(changes);
            }
            cursor = stopped;
        }
        Err("the changes of the graph did not fit in the work of the check".to_string())
    }
}

/// La version que el manifiesto dice haber leido de una fuente.
fn the_read_version(report: &ReadSet, source: &str) -> Option<u64> {
    report
        .generations
        .iter()
        .find(|(name, _)| name == source)
        .map(|(_, version)| *version)
}

/// Los tipos de relacion que el manifiesto dice haber leido.
///
/// Se leen del propio manifiesto: la identidad del sello de una relacion
/// es "{tipo}|{desde}|{hasta}", y lo que el informe se apoya son sus
/// tipos. Preguntar por mas seria preguntar por lo que no es del
/// informe, y preguntar por menos seria no ver los cambios que lo
/// invalidan.
fn the_relation_types(report: &ReadSet) -> Option<BTreeSet<RelationType>> {
    let mut types = BTreeSet::new();
    for stamp in report
        .stamps
        .iter()
        .filter(|s| s.kind == ReadKind::Relation)
    {
        let name = stamp.resource.split('|').next()?;
        types.insert(RelationType::parse(name)?);
    }
    Some(types)
}
