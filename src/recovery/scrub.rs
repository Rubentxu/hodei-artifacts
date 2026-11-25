//! El scrub: comprueba que el catalogo y el almacen se dicen la verdad.
//!
//! La recoleccion de basura ya sabe que blobs hay y que raices los
//! referencian; dejo para el scrub lo que ella no hace: **leer los bytes**.
//! Detecta tres males, y los tres se informan con la causa y con lo que se
//! puede hacer, sin tocar nada:
//!
//! - **corrupto**: los bytes del blob ya no tienen el digest que su nombre
//!   dice;
//! - **referencia rota**: una fila del catalogo apunta a un contenido que no
//!   esta en el almacen;
//! - **huerfano**: un blob que nadie referencia (lo reclama la recoleccion
//!   de basura, no el scrub).
//!
//! El scrub **no repara por su cuenta**. Reparar un blob es devolverle los
//! bytes correctos, y los unicos bytes correctos son los de una copia cuyo
//! hash coincide. Inventar bytes, o
//! borrar una referencia para que el informe salga limpio, seria esconder
//! el dano.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hodei_catalog::{ContentLedger, ContentRoot};
use hodei_hub::{BlobError, BlobStore, StoredBlob};
use serde::Serialize;

use crate::gc::plan::{Plan, Verdict};
use crate::gc::{GcError, GcService};

use super::RecoveryError;

/// Una fila del catalogo que referencia un contenido.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct RootRef {
    /// De que fila: asset, referencia, version o subject.
    pub root: &'static str,
    /// La cuenta.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// La ruta o la version.
    pub subject: String,
}

/// Que se puede hacer con un hallazgo.
///
/// Es una sugerencia para la persona que opera, no una accion que el scrub
/// ejecute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction {
    /// Devolver los bytes desde una copia cuyo hash coincide
    /// (`hodei-recovery repair --from <copia>`).
    RestoreFromBackup,
    /// No hay copia que valga: volver a publicar o a sincronizar el
    /// contenido desde su origen, o retirar la version afectada.
    RepublishOrRetire,
    /// Nadie lo referencia: lo recoge la recoleccion de basura.
    ReclaimWithGc,
}

/// Un hallazgo del scrub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    /// Los bytes no tienen el digest de su nombre.
    Corrupt {
        /// El contenido que se esperaba, `sha256:<hex>`.
        digest: String,
        /// El contenido que tienen los bytes que hay.
        found: String,
        /// Los bytes que ocupa.
        length: u64,
        /// Que filas lo referencian: si esta vacio, nadie lo usa.
        referenced_by: Vec<RootRef>,
        /// Que se puede hacer.
        repair: RepairAction,
    },
    /// Una fila del catalogo apunta a un contenido que no esta.
    BrokenReference {
        /// El contenido que falta, `sha256:<hex>`.
        digest: String,
        /// Que filas lo referencian.
        referenced_by: Vec<RootRef>,
        /// Que se puede hacer.
        repair: RepairAction,
    },
    /// Un blob que nadie referencia.
    Orphan {
        /// El contenido, `sha256:<hex>`.
        digest: String,
        /// Los bytes que ocupa.
        length: u64,
        /// Si sigue dentro del periodo de gracia: un contenido recien
        /// subido que aun no se ha publicado es un huerfano legitimo.
        in_grace: bool,
        /// Que se puede hacer.
        repair: RepairAction,
    },
}

impl Finding {
    /// El contenido al que se refiere.
    pub fn digest(&self) -> &str {
        match self {
            Self::Corrupt { digest, .. }
            | Self::BrokenReference { digest, .. }
            | Self::Orphan { digest, .. } => digest,
        }
    }

    /// Si el hallazgo es un dano de datos (corrupto o referencia rota),
    /// y no un huerfano, que es solo espacio.
    pub fn is_damage(&self) -> bool {
        !matches!(self, Self::Orphan { .. })
    }
}

/// Los totales de un scrub.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScrubSummary {
    /// Blobs con los bytes cambiados.
    pub corrupt: u64,
    /// Referencias a contenido que no esta.
    pub broken_references: u64,
    /// Blobs que nadie referencia y ya no estan en gracia.
    pub orphans: u64,
    /// Blobs que nadie referencia y siguen en gracia.
    pub orphans_in_grace: u64,
}

/// El informe de un scrub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScrubReport {
    /// Cuantos blobs se leyeron y se comprobaron.
    pub blobs_checked: u64,
    /// Cuantos bytes se leyeron.
    pub bytes_checked: u64,
    /// Cuantas filas del catalogo se comprobaron.
    pub roots_checked: u64,
    /// Lo que se encontro, ordenado por digest.
    pub findings: Vec<Finding>,
    /// Los totales.
    pub summary: ScrubSummary,
    /// Cuanto tardo, en milisegundos.
    pub elapsed_ms: u64,
}

impl ScrubReport {
    /// Si no hay ningun dano: huerfanos no cuentan, son espacio.
    pub fn is_sound(&self) -> bool {
        self.summary.corrupt == 0 && self.summary.broken_references == 0
    }

    /// Si no hay ningun hallazgo, ni siquiera huerfanos.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Lo que la lectura de un blob dijo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// Los bytes tienen el digest que dice el nombre.
    Sound,
    /// Los bytes tienen otro digest.
    Corrupt {
        /// El digest de los bytes que hay.
        found: String,
    },
    /// El blob ya no estaba cuando se fue a leer.
    Gone,
}

/// Todo lo que el informe necesita saber, ya leido.
pub struct Evidence<'a> {
    /// Los blobs del almacen.
    pub blobs: &'a [StoredBlob],
    /// Lo que la lectura de cada uno dijo, por `sha256:<hex>`.
    pub checks: &'a BTreeMap<String, Check>,
    /// Las raices del catalogo.
    pub roots: &'a [ContentRoot],
    /// El plan de la recoleccion de basura: de el salen los huerfanos.
    pub plan: &'a Plan,
    /// Los contenidos que se comprobo de nuevo que **no** estan: solo estos
    /// pueden ser referencia rota. Un contenido publicado entre el listado y
    /// la lectura de las raices no esta aqui y no se acusa.
    pub confirmed_missing: &'a BTreeSet<String>,
}

fn root_ref(root: &ContentRoot) -> RootRef {
    RootRef {
        root: root.kind.as_str(),
        account: root.account.clone(),
        repository: root.repository.clone(),
        subject: root.subject.clone(),
    }
}

/// Calcula el informe. Es una funcion pura: no lee ni escribe nada.
pub fn assess(evidence: &Evidence<'_>, elapsed: Duration) -> ScrubReport {
    // Quien referencia cada contenido.
    let mut referenced: BTreeMap<String, BTreeSet<RootRef>> = BTreeMap::new();
    for root in evidence.roots {
        referenced
            .entry(root.digest.to_string())
            .or_default()
            .insert(root_ref(root));
    }

    let mut findings: Vec<Finding> = Vec::new();
    let mut summary = ScrubSummary::default();
    let mut corrupt: BTreeSet<&str> = BTreeSet::new();

    for blob in evidence.blobs {
        let digest = blob.digest.to_string();
        if let Some(Check::Corrupt { found }) = evidence.checks.get(&digest) {
            let owners: Vec<RootRef> = referenced
                .get(&digest)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default();
            let repair = if owners.is_empty() {
                RepairAction::ReclaimWithGc
            } else {
                RepairAction::RestoreFromBackup
            };
            summary.corrupt += 1;
            findings.push(Finding::Corrupt {
                digest,
                found: found.clone(),
                length: blob.length,
                referenced_by: owners,
                repair,
            });
        }
    }
    for finding in &findings {
        corrupt.insert(finding.digest());
    }
    let corrupt: BTreeSet<String> = corrupt.into_iter().map(str::to_string).collect();

    for (digest, owners) in &referenced {
        if evidence.confirmed_missing.contains(digest) {
            summary.broken_references += 1;
            findings.push(Finding::BrokenReference {
                digest: digest.clone(),
                referenced_by: owners.iter().cloned().collect(),
                repair: RepairAction::RestoreFromBackup,
            });
        }
    }

    for entry in &evidence.plan.entries {
        let in_grace = match entry.verdict {
            Verdict::InGrace { .. } => true,
            Verdict::Reclaimable { .. } => false,
            Verdict::Retained { .. } => continue,
            // El censo del scrub es completo: un desconocido aqui solo
            // puede ser una degradacion del paginado, y eso nunca es
            // huerfano.
            Verdict::Unknown { .. } => continue,
        };
        // Un huerfano danado ya esta en la lista como corrupto.
        if corrupt.contains(&entry.digest) {
            continue;
        }
        if in_grace {
            summary.orphans_in_grace += 1;
        } else {
            summary.orphans += 1;
        }
        findings.push(Finding::Orphan {
            digest: entry.digest.clone(),
            length: entry.length,
            in_grace,
            repair: RepairAction::ReclaimWithGc,
        });
    }

    findings.sort_by(|a, b| a.digest().cmp(b.digest()));

    ScrubReport {
        blobs_checked: evidence
            .checks
            .values()
            .filter(|check| !matches!(check, Check::Gone))
            .count() as u64,
        bytes_checked: evidence
            .blobs
            .iter()
            .filter(|blob| {
                !matches!(
                    evidence.checks.get(&blob.digest.to_string()),
                    Some(Check::Gone)
                )
            })
            .map(|blob| blob.length)
            .sum(),
        roots_checked: evidence.roots.len() as u64,
        findings,
        summary,
        elapsed_ms: elapsed.as_millis() as u64,
    }
}

/// El servicio de scrub: lee el almacen y el catalogo y arma el informe.
pub struct Scrubber {
    ledger: Arc<dyn ContentLedger>,
    store: Arc<dyn BlobStore>,
    gc: GcService,
    /// Un scrub lee todo el almacen: solo hay uno a la vez en el servicio.
    running: tokio::sync::Mutex<()>,
}

impl Scrubber {
    /// Crea el servicio. Falla si el almacen no sabe listar sus blobs.
    pub fn new(
        ledger: Arc<dyn ContentLedger>,
        store: Arc<dyn BlobStore>,
        grace: Duration,
    ) -> Result<Self, RecoveryError> {
        let gc = GcService::new(ledger.clone(), store.clone(), grace)
            .map_err(|error| RecoveryError::Store(error.to_string()))?;
        Ok(Self {
            ledger,
            store,
            gc,
            running: tokio::sync::Mutex::new(()),
        })
    }

    /// Como [`Self::run`], pero devuelve `None` si ya hay un scrub en curso.
    ///
    /// Es lo que usa el servicio: leer todo el almacen dos veces a la vez
    /// no aporta nada y duplica la carga de disco.
    pub async fn try_run(&self) -> Result<Option<ScrubReport>, RecoveryError> {
        let Ok(_guard) = self.running.try_lock() else {
            return Ok(None);
        };
        self.run().await.map(Some)
    }

    /// Hace el scrub. Solo lee: no escribe, no mueve y no borra nada.
    pub async fn run(&self) -> Result<ScrubReport, RecoveryError> {
        let started = Instant::now();
        // El censo lista los blobs primero y lee las raices despues: asi lo
        // que se publica mientras tanto solo puede protegerse de mas, nunca
        // de menos.
        let (plan, blobs) = self
            .gc
            .census_with(crate::gc::service::ManifestReads::Lenient)
            .await
            .map_err(|error| match error {
                GcError::Unknown(message) | GcError::Store(message) => {
                    RecoveryError::Store(message)
                }
                other => RecoveryError::Store(other.to_string()),
            })?;
        let roots = self
            .ledger
            .content_roots()
            .await
            .map_err(|error| RecoveryError::Store(error.to_string()))?;

        let mut checks: BTreeMap<String, Check> = BTreeMap::new();
        for blob in &blobs {
            let check = match self.store.verify(&blob.digest).await {
                Ok(()) => Check::Sound,
                Err(BlobError::Corrupted { found, .. }) => Check::Corrupt {
                    found: found.to_string(),
                },
                Err(BlobError::NotFound) => Check::Gone,
                Err(error) => return Err(RecoveryError::Store(error.to_string())),
            };
            checks.insert(blob.digest.to_string(), check);
        }

        // Una raiz cuyo contenido no esta en el listado puede ser una
        // referencia rota **o** algo publicado despues del listado: solo se
        // acusa lo que sigue sin estar cuando se vuelve a preguntar.
        let listed: BTreeSet<String> = blobs.iter().map(|blob| blob.digest.to_string()).collect();
        let mut confirmed_missing: BTreeSet<String> = BTreeSet::new();
        for root in &roots {
            let digest = root.digest.to_string();
            if listed.contains(&digest) || confirmed_missing.contains(&digest) {
                continue;
            }
            let present = self
                .store
                .exists(&root.digest)
                .await
                .map_err(|error| RecoveryError::Store(error.to_string()))?;
            if !present {
                confirmed_missing.insert(digest);
            }
        }

        Ok(assess(
            &Evidence {
                blobs: &blobs,
                checks: &checks,
                roots: &roots,
                plan: &plan,
                confirmed_missing: &confirmed_missing,
            },
            started.elapsed(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::plan::{Entry, Summary};
    use hodei_catalog::RootKind;
    use hodei_evidence::ContentDigest;

    fn digest(seed: char) -> ContentDigest {
        ContentDigest::sha256(seed.to_string().repeat(64))
    }

    fn blob(seed: char, length: u64) -> StoredBlob {
        StoredBlob {
            digest: digest(seed),
            length,
            age: Duration::from_secs(3600),
        }
    }

    fn root(seed: char, subject: &str) -> ContentRoot {
        ContentRoot {
            kind: RootKind::Asset,
            account: "acme".into(),
            repository: "images".into(),
            subject: subject.into(),
            digest: digest(seed),
        }
    }

    fn plan_of(entries: Vec<Entry>) -> Plan {
        Plan {
            grace_secs: 60,
            summary: Summary::default(),
            entries,
        }
    }

    fn orphan(seed: char, in_grace: bool) -> Entry {
        Entry {
            digest: digest(seed).to_string(),
            length: 5,
            verdict: if in_grace {
                Verdict::InGrace {
                    age_secs: 1,
                    remaining_secs: 59,
                }
            } else {
                Verdict::Reclaimable { age_secs: 3600 }
            },
        }
    }

    fn report(
        blobs: &[StoredBlob],
        checks: &[(char, Check)],
        roots: &[ContentRoot],
        entries: Vec<Entry>,
        missing: &[char],
    ) -> ScrubReport {
        let checks: BTreeMap<String, Check> = checks
            .iter()
            .map(|(seed, check)| (digest(*seed).to_string(), check.clone()))
            .collect();
        let missing: BTreeSet<String> = missing.iter().map(|s| digest(*s).to_string()).collect();
        assess(
            &Evidence {
                blobs,
                checks: &checks,
                roots,
                plan: &plan_of(entries),
                confirmed_missing: &missing,
            },
            Duration::from_millis(7),
        )
    }

    #[test]
    fn a_healthy_store_has_no_findings() {
        let result = report(
            &[blob('a', 3)],
            &[('a', Check::Sound)],
            &[root('a', "blobs/a")],
            vec![],
            &[],
        );
        assert!(result.is_clean() && result.is_sound());
        assert_eq!(result.blobs_checked, 1);
        assert_eq!(result.bytes_checked, 3);
        assert_eq!(result.roots_checked, 1);
    }

    #[test]
    fn a_corrupt_blob_names_what_it_has_and_who_uses_it() {
        let result = report(
            &[blob('a', 3)],
            &[(
                'a',
                Check::Corrupt {
                    found: digest('f').to_string(),
                },
            )],
            &[root('a', "blobs/a"), root('a', "manifests/a")],
            vec![],
            &[],
        );
        assert!(!result.is_sound());
        assert_eq!(result.summary.corrupt, 1);
        let Finding::Corrupt {
            found,
            referenced_by,
            repair,
            ..
        } = &result.findings[0]
        else {
            panic!("a corrupt finding was expected");
        };
        assert_eq!(found, &digest('f').to_string());
        assert_eq!(referenced_by.len(), 2);
        assert_eq!(*repair, RepairAction::RestoreFromBackup);
    }

    #[test]
    fn a_corrupt_blob_nobody_uses_is_left_to_the_collector() {
        let result = report(
            &[blob('a', 3)],
            &[(
                'a',
                Check::Corrupt {
                    found: digest('f').to_string(),
                },
            )],
            &[],
            vec![orphan('a', false)],
            &[],
        );
        // Aparece una sola vez: como corrupto, no tambien como huerfano.
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.summary.orphans, 0);
        let Finding::Corrupt { repair, .. } = &result.findings[0] else {
            panic!("a corrupt finding was expected");
        };
        assert_eq!(*repair, RepairAction::ReclaimWithGc);
    }

    #[test]
    fn a_row_that_points_at_nothing_is_a_broken_reference_with_every_owner() {
        let result = report(
            &[],
            &[],
            &[root('b', "blobs/b"), root('b', "manifests/b")],
            vec![],
            &['b'],
        );
        assert_eq!(result.summary.broken_references, 1);
        let Finding::BrokenReference { referenced_by, .. } = &result.findings[0] else {
            panic!("a broken reference was expected");
        };
        assert_eq!(referenced_by.len(), 2);
        assert!(!result.is_sound());
    }

    #[test]
    fn content_that_was_published_after_the_listing_is_not_accused() {
        // La raiz existe, el blob no se listo, pero al volver a preguntar
        // estaba: no esta en `confirmed_missing` y no es un hallazgo.
        let result = report(&[], &[], &[root('c', "blobs/c")], vec![], &[]);
        assert!(result.is_clean());
    }

    #[test]
    fn an_orphan_is_space_not_damage_and_the_grace_is_kept_apart() {
        let result = report(
            &[blob('d', 4), blob('e', 6)],
            &[('d', Check::Sound), ('e', Check::Sound)],
            &[],
            vec![orphan('d', false), orphan('e', true)],
            &[],
        );
        assert!(result.is_sound());
        assert!(!result.is_clean());
        assert_eq!(result.summary.orphans, 1);
        assert_eq!(result.summary.orphans_in_grace, 1);
        assert!(result.findings.iter().all(|finding| !finding.is_damage()));
    }

    #[test]
    fn a_blob_that_vanished_while_reading_is_not_counted_as_checked() {
        let result = report(&[blob('a', 3)], &[('a', Check::Gone)], &[], vec![], &[]);
        assert_eq!(result.blobs_checked, 0);
        assert_eq!(result.bytes_checked, 0);
        assert!(result.is_clean());
    }

    #[test]
    fn the_findings_are_ordered_by_digest_and_serialize_with_their_kind() {
        let result = report(
            &[blob('b', 1), blob('a', 1)],
            &[
                (
                    'a',
                    Check::Corrupt {
                        found: digest('f').to_string(),
                    },
                ),
                ('b', Check::Sound),
            ],
            &[root('a', "blobs/a"), root('c', "blobs/c")],
            vec![orphan('b', false)],
            &['c'],
        );
        let digests: Vec<&str> = result.findings.iter().map(Finding::digest).collect();
        let mut sorted = digests.clone();
        sorted.sort();
        assert_eq!(digests, sorted);
        let json = serde_json::to_value(&result).unwrap();
        let kinds: Vec<&str> = json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| finding["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["corrupt", "orphan", "broken_reference"]);
    }
}
