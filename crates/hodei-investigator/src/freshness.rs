//! La frescura de una investigacion entregada.
//!
//! Un informe dice que leyo, y lo dice con su manifiesto: los sellos de
//! lo que leyo y la version del mundo en que lo leyo. Ese informe es una
//! afirmacion con fecha, y la pregunta es si sigue siendo cierta.
//!
//! La respuesta es de TRES, no de dos: `Current`, `Stale` y `Unknown`.
//! Un informe puede seguir vigente, puede haber caducado, y puede que no
//! haya forma de saberlo. Lo tercero NO es lo primero: un almacen que no
//! contesta no dice que el mundo no se movio, dice que nadie lo sabe. Un
//! comprobador que ante la duda contestara "vigente" fabricaria una
//! garantia que no tiene.
//!
//! # Lo que caduca y lo que no
//!
//! Caduca lo que el informe **sostiene**: el apoyo de la relacion que
//! leyo, y la evidencia con el digest que leyo. NO caduca lo que paso en
//! el mundo y el informe no leyo: una relacion nueva de otro sujeto no
//! invalida un informe que no la mencionaba, y el apoyo que se anadio a
//! una relacion que el informe leyo sigue dejando en pie lo que el
//! informe dijo de ella.
//!
//! # Donde esta la decision
//!
//! Aqui hay una funcion PURA y nada mas: `judge` compara un manifiesto
//! con un mundo y dice un veredicto. El mundo --las versiones de ahora,
//! los digests de ahora, los cambios de ahora-- lo trae el
//! almacenamiento, y el almacenamiento no decide: decide esta funcion.
//! Por eso el mismo manifiesto y el mismo mundo dan el mismo veredicto,
//! siempre.

use serde::{Deserialize, Serialize};

use crate::read_set::{ReadKind, ReadSet};

/// La fuente de la que sale el grafo: la version que el manifiesto dice
/// haber leido.
pub const GRAPH_SOURCE: &str = "graph";

/// La fuente de la que sale la evidencia: los digests que el manifiesto
/// dice haber leido.
pub const EVIDENCE_SOURCE: &str = "evidence";

/// Lo que le paso a una referencia del mundo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// La referencia se anadio.
    Added,
    /// La referencia se retiro: lo que la sostenia ya no esta.
    Removed,
    /// La referencia ya no esta: no hay nada donde lo que se leyo.
    Gone,
    /// La referencia cambio: lo que la sostenia ya no es lo mismo.
    Changed,
}

/// Una referencia del mundo que se movio.
///
/// Lleva **el apoyo que se movio**, porque la identidad de la relacion no
/// basta: una relacion puede tener varios apoyos, y lo que deja de
/// sostenerse es UN apoyo, no la relacion entera.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedReference {
    /// La fuente que se movio.
    pub source: String,
    /// Que clase de referencia es.
    pub kind: ReadKind,
    /// La identidad de la referencia.
    pub resource: String,
    /// El apoyo que se movio: la evidencia que la sostenia.
    pub evidence: String,
    /// Que le paso.
    pub what: ChangeKind,
}

/// El mundo de ahora, dicho como lo que el almacenamiento pudo contestar.
///
/// Lo que no esta aqui es lo que no se pudo consultar: una fuente que
/// no contesta se dice con `unreachable`, y lo que se dice con
/// `unreachable` NO se cuenta como "igual que antes".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CurrentWorld {
    /// La generacion de cada fuente ahora.
    pub generations: Vec<(String, u64)>,
    /// El digest de ahora de cada evidencia.
    pub digests: Vec<(String, String)>,
    /// Lo que se movio desde la generacion que el manifiesto dice haber
    /// leido.
    pub changes: Vec<ChangedReference>,
    /// Las fuentes que no se pudieron consultar.
    pub unreachable: Vec<String>,
}

impl CurrentWorld {
    /// El mundo de una fuente que no se pudo consultar.
    pub fn unreachable(source: impl Into<String>) -> Self {
        Self {
            unreachable: vec![source.into()],
            ..Self::default()
        }
    }
}

/// Si una investigacion entregada sigue apoyandose en el mundo.
///
/// `Stale` dice **que** cambio, porque un informe que caduca sin decir
/// por que no es accionable: el que lo pidio tiene que saber que volver
/// a leer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    /// El mundo no se movio bajo lo que el informe dice.
    Current,
    /// El mundo se movio, y lo que se movio toca lo que el informe
    /// sostiene.
    Stale {
        /// Las referencias que cambiaron.
        changed: Vec<ChangedReference>,
    },
    /// No se pudo saber: una fuente no se pudo consultar.
    Unknown {
        /// Por que no se pudo saber.
        reason: String,
    },
}

impl Freshness {
    /// Si el informe sigue vigente.
    pub fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }

    /// Si el informe caduco.
    pub fn is_stale(&self) -> bool {
        matches!(self, Self::Stale { .. })
    }

    /// Si no se pudo saber.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }
}

/// Dice si la investigacion que el manifiesto sostiene sigue vigente.
///
/// Es una funcion **pura**: el mismo manifiesto y el mismo mundo dan el
/// mismo veredicto, siempre. Y el veredicto es de tres: lo que se sabe
/// que caducado, lo que se sabe que sigue vigente, y lo que no se sabe.
///
/// Un `Stale` probado gana a un `Unknown`: si se sabe que el mundo se
/// movio bajo el informe, el informe esta obsoleto, y que ademas no se
/// pueda comprobar el resto no lo salva.
pub fn judge(report: &ReadSet, world: &CurrentWorld) -> Freshness {
    let mut changed: Vec<ChangedReference> = Vec::new();
    let mut unknown: Option<String> = None;

    // Un manifiesto que no dice de que mundo hablo no se puede comprobar.
    if report.generations.is_empty() {
        unknown = Some("the report does not say which version of the world it read".to_string());
    }

    // Las fuentes del manifiesto: cada una dice en que version se leyo, y
    // cada una se contrasta con la version de ahora.
    for (source, expected) in &report.generations {
        if world
            .unreachable
            .iter()
            .any(|unreachable| unreachable == source)
        {
            unknown.get_or_insert_with(|| format!("the source {source} could not be consulted"));
            continue;
        }
        let Some((_, current)) = world.generations.iter().find(|(name, _)| name == source) else {
            unknown.get_or_insert_with(|| format!("the source {source} could not be consulted"));
            continue;
        };
        if current == expected {
            // La fuente no se movio: lo que se leyo sigue siendo lo que
            // hay.
            continue;
        }
        // La fuente se movio. Lo que se movio puede no tener nada que ver
        // con lo que el informe dice: lo que el informe NO leyo no puede
        // caducar lo que leyo.
        for change in world
            .changes
            .iter()
            .filter(|change| &change.source == source)
        {
            let rests_on_it = report.stamps.iter().any(|stamp| {
                stamp.kind == change.kind
                    && stamp.resource == change.resource
                    && stamp.verified_by == change.evidence
            });
            if rests_on_it && !changed.contains(change) {
                changed.push(change.clone());
            }
        }
    }

    // La evidencia: el digest de ahora tiene que ser el de entonces. Lo
    // que no esta ya no se puede leer, y un informe que se apoya en algo
    // que no esta, esta obsoleto.
    let evidence_stamps: Vec<_> = report
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Evidence)
        .collect();
    if !evidence_stamps.is_empty() {
        if world
            .unreachable
            .iter()
            .any(|unreachable| unreachable == EVIDENCE_SOURCE)
        {
            unknown.get_or_insert_with(|| {
                format!("the source {EVIDENCE_SOURCE} could not be consulted")
            });
        } else {
            for stamp in evidence_stamps {
                let moved = match world.digests.iter().find(|(id, _)| *id == stamp.resource) {
                    Some((_, digest)) if *digest == stamp.verified_by => None,
                    Some(_) => Some(ChangeKind::Changed),
                    None => Some(ChangeKind::Gone),
                };
                if let Some(what) = moved {
                    let reference = ChangedReference {
                        source: EVIDENCE_SOURCE.to_string(),
                        kind: ReadKind::Evidence,
                        resource: stamp.resource.clone(),
                        evidence: stamp.resource.clone(),
                        what,
                    };
                    if !changed.contains(&reference) {
                        changed.push(reference);
                    }
                }
            }
        }
    }

    if !changed.is_empty() {
        Freshness::Stale { changed }
    } else if let Some(reason) = unknown {
        Freshness::Unknown { reason }
    } else {
        Freshness::Current
    }
}
