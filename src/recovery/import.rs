//! Import idempotente de autoridades persistentes (HSP-210 / HGS-28).
//!
//! La copia de [`super::backup`] es un espejo del motor: solo vuelve a un
//! destino **limpio** y trae los metadatos como SurrealQL opaco. Este
//! modulo es la otra mitad del contrato de recuperacion: el paquete
//! semantico portable que produce la herramienta de export (HSP-179,
//! doc-20) —filas JSON canonicas por tabla, en orden de dependencias, con
//! identidad canonica `tabla:clave`— se importa **contra una autoridad que
//! ya tiene datos**, y hacerlo dos veces no puede duplicar nada.
//!
//! Reglas, de mas a menos importantes:
//!
//! 1. **La identidad canonica decide, el UID fisico nunca.** La clave de
//!    una fila es su `id` canonicado (`tabla:clave`, la misma nocion que
//!    fija el export al escribir sus lotes): se busca por ella y se
//!    escribe con ella. Los UIDs internos del destino no se copian ni se
//!    reasignan: una fila que ya esta, con el mismo contenido, no se toca;
//!    una fila nueva se crea **con la identidad del origen**.
//! 2. **Manifest-first, fail-closed.** Antes de escribir una sola fila se
//!    comprueba el paquete entero: formato, autodigest, hashes de lote
//!    contra fichero, recuentos, orden, bordes de lote, identidades
//!    repetidas y digests de contenido contra el CAS. Un paquete que no
//!    cuadra aborta nombrando al culpable; nunca se importa "lo que habia".
//! 3. **El orden del manifiesto es el orden de aplicacion.** Las tablas
//!    viajan en orden de dependencias (repositorios antes que versiones y
//!    assets, el ledger despues de lo que referencia); los lotes viajan
//!    ordenados por identidad. El import conserva ese orden y no lo
//!    reordena.
//! 4. **Una fila divergente es una decision del operador, no un empate
//!    tecnico.** La misma identidad con distinto contenido es `Conflict`
//!    tipado y aborta la corrida. Solo una reescritura explicitamente
//!    autorizada (`overwrite`) la repara, y aun entonces la identidad
//!    canonica se conserva.
//! 5. **Toda corrida deja un registro resumible.** El progreso se escribe
//!    en atomico tras cada lote; un corte a mitad se reanuda verificando
//!    cada lote grabado contra su hash **y contra el destino** antes de
//!    saltarlo. Reanudar tras el corte y repetir la importacion entera
//!    convergen al mismo estado.
//!
//! La aplicacion vive detras de [`RowSink`]: el dominio decide (identidad,
//! orden, conflicto, progreso) y el adaptador toca el almacen. El adaptador
//! sobre el perfil Dgraph vigente es una costura registrada como gap en la
//! evidencia (`target/uat/hsp210/evidencia.md`); el nucleo aqui es
//! engine-agnostic y esta probado sobre un motor real en memoria.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::RecoveryError;

/// La version del formato del paquete importable.
///
/// Sube cuando un paquete antiguo deja de poder leerse. Un paquete con un
/// formato que este binario no entiende se **rechaza** nombrando ambos
/// numeros, igual que la copia de [`super::manifest`].
pub const IMPORT_FORMAT: u32 = 1;

/// El manifiesto dentro del directorio del paquete.
pub const IMPORT_MANIFEST_FILE: &str = "manifest.json";

/// El directorio de lotes dentro del paquete.
pub const IMPORT_BATCHES_DIR: &str = "batches";

/// El nombre canonico del marcador de progreso de una corrida a medias.
///
/// Vive junto a la autoridad, no junto al paquete: el paquete es
/// inmutable, el progreso es estado de la aplicacion. Su presencia
/// significa "corrida sin terminar"; al terminar se borra.
pub const IMPORT_PROGRESS_FILE: &str = ".import-progress.json";

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// De donde vino el paquete. Es metadato del origen: el import lo registra
/// en el informe pero nunca lo escribe en el destino.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSource {
    /// El motor del origen.
    pub engine: String,
    /// La version del contrato del motor.
    pub engine_version: String,
    /// El backend del origen, tal como lo declaro quien opera.
    pub backend: String,
    /// El namespace del origen.
    pub namespace: String,
    /// La base del origen.
    pub database: String,
}

/// La frontera de la fuente en el momento del export.
///
/// El import la conserva y la reporta: es parte de la firma de **que** se
/// importo. Dos importaciones del mismo paquete tienen la misma frontera.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFrontier {
    /// Recuento de filas por tabla.
    pub tables: BTreeMap<String, u64>,
    /// Filas del ledger de evidencia.
    pub ledger_rows: u64,
    /// La posicion mas alta del ledger, cuando no esta vacio.
    pub ledger_max_position: Option<u64>,
    /// Filas de la tabla de cabezales del ledger.
    pub head_rows: u64,
}

/// Un lote del paquete, y el hash que fija sus bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportBatch {
    /// El fichero, relativo al paquete (`batches/<tabla>-<i>.json`).
    pub file: String,
    /// El SHA-256 del fichero, en hexadecimal minusculo.
    pub sha256: String,
    /// Filas que lleva el lote.
    pub rows: u64,
    /// La identidad de la primera fila del lote.
    pub first_id: String,
    /// La identidad de la ultima fila del lote.
    pub last_id: String,
}

/// Los lotes de una tabla, en orden de aplicacion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportTable {
    /// La tabla.
    pub table: String,
    /// Filas totales de la tabla en la fuente.
    pub rows: u64,
    /// Los lotes, en orden.
    pub batches: Vec<ImportBatch>,
}

/// La politica de blobs declarada por el paquete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobPolicy {
    /// Por que los bytes no viajaron.
    pub policy: String,
    /// Si los bytes viajaron: nunca.
    pub copied: bool,
}

/// Lo que el paquete dice de si mismo.
///
/// Espejo de lectura del formato 1 que escribe la herramienta de export
/// (HSP-179): las claves serde coinciden, de modo que un export real se
/// importa sin traduccion. Las tablas viajan **en orden de dependencias**;
/// el orden del vector es contrato, no sugerencia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportManifest {
    /// La version del formato.
    pub format: u32,
    /// La version de la herramienta que lo produjo.
    pub tool_version: String,
    /// El origen de las filas.
    pub source: ExportSource,
    /// La frontera de la fuente.
    pub frontier: ExportFrontier,
    /// Las tablas, en orden de aplicacion.
    pub tables: Vec<ImportTable>,
    /// La politica de blobs: los digest viajaron, los bytes no.
    pub blobs: BlobPolicy,
    /// El SHA-256 del contenido entero: los hashes de lote concatenados
    /// en orden, cada uno seguido de un salto de linea.
    pub content_sha256: String,
}

impl ImportManifest {
    /// El autodigest recalculado desde los hashes de lote.
    pub fn content_digest(&self) -> String {
        let mut input = Vec::new();
        for table in &self.tables {
            for batch in &table.batches {
                input.extend_from_slice(batch.sha256.as_bytes());
                input.push(b'\n');
            }
        }
        sha256_hex(&input)
    }

    /// Los lotes de todas las tablas, aplanados en orden de aplicacion.
    pub fn all_batches(&self) -> Vec<(&str, &ImportBatch)> {
        self.tables
            .iter()
            .flat_map(|table| {
                table
                    .batches
                    .iter()
                    .map(move |batch| (table.table.as_str(), batch))
            })
            .collect()
    }

    /// Comprueba el paquete **antes** de tocar nada: formato, tablas sin
    /// repetir, lotes no vacios, orden de lotes por identidad y coherencia
    /// de recuentos y de la frontera. Cada problema es tipado.
    pub fn check(&self) -> Result<(), ImportFailure> {
        if self.format != IMPORT_FORMAT {
            return Err(ImportFailure::Format {
                found: self.format,
                expected: IMPORT_FORMAT,
            });
        }
        let mut names = BTreeSet::new();
        for table in &self.tables {
            if !names.insert(table.table.as_str()) {
                return Err(ImportFailure::Invalid(format!(
                    "the manifest repeats the table '{}'",
                    table.table
                )));
            }
            let mut counted = 0u64;
            let mut previous_last: Option<&str> = None;
            for batch in &table.batches {
                if batch.rows == 0 {
                    return Err(ImportFailure::Invalid(format!(
                        "the batch '{}' of '{}' is empty",
                        batch.file, table.table
                    )));
                }
                if let Some(previous) = previous_last {
                    if previous > batch.first_id.as_str() {
                        return Err(ImportFailure::Order {
                            table: table.table.clone(),
                            previous: previous.to_string(),
                            found: batch.first_id.clone(),
                        });
                    }
                }
                previous_last = Some(&batch.last_id);
                counted += batch.rows;
            }
            if counted != table.rows {
                return Err(ImportFailure::CountMismatch {
                    table: table.table.clone(),
                    declared: table.rows,
                    batches: counted,
                });
            }
        }
        let frontier_total: u64 = self.frontier.tables.values().sum();
        let batch_total: u64 = self.tables.iter().map(|t| t.rows).sum();
        if frontier_total != batch_total {
            return Err(ImportFailure::FrontierMismatch {
                frontier: frontier_total,
                batches: batch_total,
            });
        }
        if self.content_sha256 != self.content_digest() {
            return Err(ImportFailure::ContentDigest {
                declared: self.content_sha256.clone(),
                computed: self.content_digest(),
            });
        }
        Ok(())
    }
}

/// Lee y comprueba el manifiesto de un paquete de export.
pub fn read_import_manifest(directory: &Path) -> Result<ImportManifest, RecoveryError> {
    let text = std::fs::read(directory.join(IMPORT_MANIFEST_FILE)).map_err(|error| {
        RecoveryError::Unsound(format!(
            "{} cannot be read ({error}): the export is not finished or is not an export",
            IMPORT_MANIFEST_FILE
        ))
    })?;
    let manifest: ImportManifest = serde_json::from_slice(&text)
        .map_err(|error| RecoveryError::Unsound(format!("the export manifest is invalid: {error}")))?;
    manifest
        .check()
        .map_err(|failure| RecoveryError::Unsound(failure.to_string()))?;
    Ok(manifest)
}

/// La fila tal como viaja en un lote: un objeto JSON con su `id`.
pub type Row = serde_json::Map<String, serde_json::Value>;

/// El estado del destino que el nucleo necesita para decidir. Lo lee el
/// sink; el nucleo no toca el almacen.
#[async_trait]
pub trait RowSink: Send + Sync {
    /// Busca por identidad canonica completa (`tabla:clave`).
    async fn lookup(&self, identity: &str) -> Result<Option<Row>, RecoveryError>;

    /// Escribe la fila **con la identidad canonica dada** como clave. El
    /// adaptador no elige otra: conservar la clave es el contrato (AC1).
    async fn put(&self, identity: &str, row: &Row) -> Result<(), RecoveryError>;

    /// El digest canonico de una fila: la clave de la equivalencia.
    ///
    /// El default es la identidad estructural del JSON (claves ordenadas,
    /// sin el `id`, que ya es la clave). Un adaptador con representacion
    /// propia del origen puede igualarla aqui para que "mismo contenido"
    /// signifique lo mismo en origen y destino.
    fn fingerprint_row(&self, row: &Row) -> String {
        canonical_digest(row)
    }

    /// El resumen del destino por el que responde el AC: recuento por
    /// tabla y el digest global de las filas que hay.
    async fn state_summary(&self) -> Result<DestinationState, RecoveryError>;
}

/// El estado del destino, tal como lo ve el import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationState {
    /// Filas por tabla.
    pub tables: BTreeMap<String, u64>,
    /// El digest canonico de cada fila, ordenado como `tabla:id`.
    /// Es el invariante del fingerprint de autoridades: dos estados con
    /// el mismo mapa tienen las mismas autoridades, no "parecidas".
    pub rows: BTreeMap<String, String>,
}

impl DestinationState {
    /// El fingerprint de autoridades: un SHA-256 sobre el resumen
    /// canonico del destino. Es lo que dos importaciones del mismo
    /// paquete deben producir iguales (AC1).
    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        for (table, count) in &self.tables {
            hasher.update(format!("T {table} {count}\n").as_bytes());
        }
        for (key, digest) in &self.rows {
            hasher.update(format!("R {key} {digest}\n").as_bytes());
        }
        sha256_hex(&hasher.finalize())
    }
}

/// El digest canonico de una fila: serializacion con claves ordenadas
/// (el `Map` de serde_json serializa en el orden del parser; para el
/// contrato se reordena explicitamente) y sin el campo `id`, que es la
/// clave y se compara aparte.
pub fn canonical_digest(row: &Row) -> String {
    let mut content = Row::new();
    for (key, value) in row {
        if key == "id" {
            continue;
        }
        content.insert(key.clone(), canonical_value(value));
    }
    // BTreeMap da claves ordenadas: el digest no depende del orden de
    // las claves en el fichero de origen.
    let ordered: BTreeMap<&String, serde_json::Value> =
        content.iter().map(|(k, v)| (k, v.clone())).collect();
    let bytes = serde_json::to_vec(&ordered).unwrap_or_default();
    sha256_hex(&bytes)
}

/// Reordena objetos anidados para que el digest sea canonico de arriba
/// abajo: un origen que escribe `{"b":1,"a":2}` y un destino que lo leen
/// al reves son la misma autoridad.
fn canonical_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let ordered: BTreeMap<&String, serde_json::Value> =
                object.iter().map(|(k, v)| (k, canonical_value(v))).collect();
            serde_json::Value::Object(
                ordered
                    .into_iter()
                    .map(|(k, v)| (k.clone(), v))
                    .collect::<Row>(),
            )
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_value).collect())
        }
        other => other.clone(),
    }
}

/// El modo de la corrida.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Tamano maximo de pagina de escrituras por bloque. Acota el trabajo
    /// por transaccion del adaptador; el lote del paquete manda.
    pub batch_rows: usize,
    /// Reanudar en lugar de empezar: el progreso grabado en el destino
    /// manda, y cada lote ya aplicado se **verifica contra su hash y
    /// contra el destino** antes de saltarlo. Un progreso que no cuadra
    /// aborta.
    pub resume: bool,
    /// Reescribir filas divergentes conservando su identidad canonica.
    /// Sin esto, una divergencia aborta la corrida (AC2).
    pub overwrite: bool,
    /// El operador renuncio a la reconciliacion de digests contra el CAS.
    /// Sin reconciliador y sin esta bandera el import aborta antes de
    /// escribir: el contrato no se cumple a ciegas.
    pub allow_missing_blobs: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            batch_rows: 200,
            resume: false,
            overwrite: false,
            allow_missing_blobs: false,
        }
    }
}

/// Campos de las filas que llevan digests de contenido, por tabla. La
/// lista es deliberadamente corta y explicita: el reconciliador no
/// escanea valores adivinando, recorre estas claves.
pub const DIGEST_FIELDS: &[(&str, &[&str])] = &[
    ("catalog_asset", &["sha256"]),
    ("catalog_reference", &["sha256"]),
    ("provenance_receipt", &["digest"]),
    ("trust_grant", &["evidence"]),
    ("catalog_audit_outbox", &["payload_digest"]),
];

/// Que hizo una corrida de import, lote a lote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchOutcome {
    /// La tabla.
    pub table: String,
    /// El numero de lote (1-based).
    pub index: usize,
    /// El fichero del lote.
    pub file: String,
    /// Filas nuevas creadas con su identidad canonica.
    pub created: u64,
    /// Filas que ya estaban identicas: saltadas.
    pub skipped: u64,
    /// Filas divergentes reescritas con autorizacion (conservando id).
    pub overwritten: u64,
}

/// Lo que hizo una corrida de import.
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    /// El fingerprint de autoridades **antes** de la corrida.
    pub fingerprint_before: String,
    /// El fingerprint de autoridades **despues**.
    pub fingerprint_after: String,
    /// La frontera del paquete, copiada del manifiesto.
    pub frontier: ExportFrontier,
    /// El autodigest del paquete.
    pub content_sha256: String,
    /// Los lotes aplicados por esta corrida (los resumidos no aparecen).
    pub batches: Vec<BatchOutcome>,
    /// Totales por categoria.
    pub created: u64,
    pub skipped: u64,
    pub overwritten: u64,
    /// Si la corrida termino (borro el marcador de progreso).
    pub finished: bool,
    /// Cuando empezo (informativo; no entra en ningun digest).
    pub started_at: DateTime<Utc>,
    /// Cuanto tardo.
    pub elapsed_ms: u64,
}

/// El estado de una corrida a medias, grabado en el destino.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportProgress {
    /// La version del formato del marcador.
    pub format: u32,
    /// El autodigest del paquete que se esta aplicando. Un progreso de
    /// otro paquete aborta el resume: reanudar contra un paquete
    /// distinto no es reanudar.
    pub content_sha256: String,
    /// Los lotes aplicados, en orden.
    pub done: Vec<BatchOutcome>,
}

/// Por que aborta un import. Cada variante nombra al culpable (AC2).
#[derive(Debug, thiserror::Error)]
pub enum ImportFailure {
    /// El paquete declara un formato que este binario no puede leer.
    #[error("the export uses format {found}; this build reads format {expected}")]
    Format {
        /// El formato del paquete.
        found: u32,
        /// El formato que este binario escribe.
        expected: u32,
    },
    /// El autodigest del manifiesto no cuadra con sus lotes.
    #[error("the manifest content digest is {declared} but its batches hash to {computed}")]
    ContentDigest {
        /// Lo que declara el manifiesto.
        declared: String,
        /// Lo que calculan sus lotes.
        computed: String,
    },
    /// La suma de filas de los lotes de una tabla no es la declarada.
    #[error("the table {table} declares {declared} rows but its batches carry {batches}")]
    CountMismatch {
        /// La tabla.
        table: String,
        /// Lo declarado.
        declared: u64,
        /// Lo que suman los lotes.
        batches: u64,
    },
    /// La frontera y los lotes no cuentan lo mismo.
    #[error("the frontier counts {frontier} rows but the batches carry {batches}")]
    FrontierMismatch {
        /// Lo que cuenta la frontera.
        frontier: u64,
        /// Lo que cargan los lotes.
        batches: u64,
    },
    /// Los lotes no vienen ordenados por identidad.
    #[error("the batches of {table} are out of order: '{previous}' then '{found}'")]
    Order {
        /// La tabla.
        table: String,
        /// El ultimo id del lote anterior.
        previous: String,
        /// El primer id que lo viola.
        found: String,
    },
    /// Una fila del lote no puede canonicarse.
    #[error("the row '{id}' of {table} is invalid: {detail}")]
    RowInvalid {
        /// La tabla.
        table: String,
        /// El id declarado o inferido.
        id: String,
        /// Por que no vale.
        detail: String,
    },
    /// Una identidad fuera del rango de bordes de su lote.
    #[error(
        "the row '{id}' of batch '{file}' is outside the batch range '{first}'..='{last}'"
    )]
    RowOutOfBatch {
        /// La identidad culpable.
        id: String,
        /// El fichero del lote.
        file: String,
        /// El borde inferior declarado.
        first: String,
        /// El borde superior declarado.
        last: String,
    },
    /// La misma identidad canonica aparece dos veces en el paquete.
    #[error("the export repeats the canonical identity '{key}' of table {table}")]
    DuplicateRow {
        /// La tabla.
        table: String,
        /// La identidad repetida.
        key: String,
    },
    /// Una identidad canonica ya existe en el destino con otro contenido
    /// y la reescritura no esta autorizada. Fail-closed.
    #[error(
        "the destination already has '{key}' in {table} with different content \
         (package digest {package_digest}, destination digest {destination_digest}); pass \
         overwrite to repair it keeping the canonical identity"
    )]
    Conflict {
        /// La tabla.
        table: String,
        /// La identidad divergente.
        key: String,
        /// El digest canonico de la fila del paquete.
        package_digest: String,
        /// El digest canonico de la fila del destino.
        destination_digest: String,
    },
    /// El paquete referencia un digest de contenido que el CAS no tiene.
    #[error(
        "the CAS lacks {missing} referenced blob(s); first is {first} referenced by \
         {referenced_by}"
    )]
    MissingBlobs {
        /// Cuantos faltan.
        missing: usize,
        /// El primero.
        first: String,
        /// Quien lo referencia.
        referenced_by: String,
    },
    /// El progreso grabado no pertenece a este paquete.
    #[error(
        "the destination holds progress for content {progress} and this package is {package}; \
         resume would apply the wrong batches"
    )]
    ProgressMismatch {
        /// El progreso que hay.
        progress: String,
        /// El paquete que se trae.
        package: String,
    },
    /// El progreso promete un lote que el paquete o el destino no respaldan.
    #[error("the progress for batch '{file}' of {table} is not backed: {detail}")]
    ProgressNotBacked {
        /// La tabla.
        table: String,
        /// El fichero.
        file: String,
        /// Que falta o en que no cuadra.
        detail: String,
    },
    /// El paquete no declara la politica de blobs que exige el contrato.
    #[error("the export does not declare the referenced-by-digest policy (policy '{policy}')")]
    BlobPolicy {
        /// Lo que declara.
        policy: String,
    },
    /// Una peticion invalida de quien llama.
    #[error("{0}")]
    Invalid(String),
    /// El almacen fallo.
    #[error("the store failed: {0}")]
    Store(#[from] RecoveryError),
}

/// Canonicar la identidad de una fila: `tabla:clave`, la misma nocion que
/// fija el export al escribir sus lotes.
///
/// Acepta las dos formas en que un id llega de un JSON:
/// - la cadena `tabla:clave` (forma canonica, la que fija el contrato);
/// - el objeto `{tb, id}` en que algun cliente serializa un Thing.
///
/// Sin `id` **no se inventa**: la identidad tiene que viajar en la fila;
/// aborta con `RowInvalid`.
pub fn canonicalize_id(table: &str, row: &Row) -> Result<String, ImportFailure> {
    let declared = row.get("id").ok_or_else(|| ImportFailure::RowInvalid {
        table: table.to_string(),
        id: String::new(),
        detail: "the row carries no id: a canonical identity cannot be invented".to_string(),
    })?;
    match declared {
        serde_json::Value::String(text) => {
            let (prefix, key) = text.split_once(':').ok_or_else(|| ImportFailure::RowInvalid {
                table: table.to_string(),
                id: text.clone(),
                detail: "the id is not a canonical 'table:key' identity".to_string(),
            })?;
            if prefix != table {
                return Err(ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text.clone(),
                    detail: format!("the id belongs to table '{prefix}'"),
                });
            }
            if key.is_empty() {
                return Err(ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text.clone(),
                    detail: "the id has an empty key".to_string(),
                });
            }
            Ok(text.clone())
        }
        serde_json::Value::Object(object) => {
            let text = text_of(declared);
            let prefix = object
                .get("tb")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text.clone(),
                    detail: "the id object has no table".to_string(),
                })?;
            if prefix != table {
                return Err(ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text,
                    detail: format!("the id belongs to table '{prefix}'"),
                });
            }
            let key = object
                .get("id")
                .map(|key| match key {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .ok_or_else(|| ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text,
                    detail: "the id object has no key".to_string(),
                })?;
            if key.is_empty() {
                return Err(ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text_of(declared),
                    detail: "the id object has an empty key".to_string(),
                });
            }
            Ok(format!("{table}:{key}"))
        }
        _ => Err(ImportFailure::RowInvalid {
            table: table.to_string(),
            id: text_of(declared),
            detail: "the id is neither a string nor an object".to_string(),
        }),
    }
}

fn text_of(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Fija la identidad canonica en la fila antes de escribir: el `id` es
/// siempre `tabla:clave`, nunca el UID del origen ni el que el destino
/// quiera asignar.
fn with_canonical_id(mut row: Row, identity: &str) -> Row {
    row.insert(
        "id".to_string(),
        serde_json::Value::String(identity.to_string()),
    );
    row
}

/// Lee y verifica un lote del paquete: el fichero tiene que existir y
/// cuadrar con el hash del manifiesto, y su contenido tiene que ser una
/// lista de objetos del recuento declarado.
pub fn read_batch(package: &Path, batch: &ImportBatch) -> Result<Vec<Row>, RecoveryError> {
    let path = package.join(&batch.file);
    let bytes = std::fs::read(&path).map_err(|error| {
        RecoveryError::Metadata(format!("the batch '{}' cannot be read: {error}", batch.file))
    })?;
    let hash = sha256_hex(&bytes);
    if hash != batch.sha256 {
        return Err(RecoveryError::Unsound(format!(
            "the batch '{}' is truncated or altered: its bytes hash to {hash}, the manifest \
             says {}",
            batch.file, batch.sha256
        )));
    }
    let rows: Vec<Row> = serde_json::from_slice(&bytes).map_err(|error| {
        RecoveryError::Unsound(format!(
            "the batch '{}' is not a JSON array of rows: {error}",
            batch.file
        ))
    })?;
    if rows.len() as u64 != batch.rows {
        return Err(RecoveryError::Unsound(format!(
            "the batch '{}' carries {} rows, the manifest says {}",
            batch.file,
            rows.len(),
            batch.rows
        )));
    }
    Ok(rows)
}

/// Los digests de contenido que referencia una fila, segun
/// [`DIGEST_FIELDS`].
fn referenced_digests(table: &str, row: &Row) -> Vec<String> {
    let mut digests = Vec::new();
    for (name, fields) in DIGEST_FIELDS {
        if *name != table {
            continue;
        }
        for field in *fields {
            if let Some(value) = row.get(*field).and_then(serde_json::Value::as_str) {
                if !value.is_empty() {
                    digests.push(value.to_string());
                }
            }
        }
    }
    digests
}

/// La reconciliacion previa: todo lote existe, cuadra con su hash, sus
/// filas se canonican, no hay identidades repetidas, cada identidad cae
/// dentro de los bordes de su lote y cada digest referenciado esta en el
/// CAS. Nada de esto toca el destino; es el "manifest-first" del modulo.
async fn preflight(
    package: &Path,
    manifest: &ImportManifest,
    reconciler: Option<&Arc<dyn DigestReconciler>>,
) -> Result<(), ImportFailure> {
    for table in &manifest.tables {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for batch in &table.batches {
            let rows = read_batch(package, batch).map_err(ImportFailure::Store)?;
            for row in &rows {
                let identity = canonicalize_id(&table.table, row)?;
                if !seen.insert(identity.clone()) {
                    return Err(ImportFailure::DuplicateRow {
                        table: table.table.clone(),
                        key: identity,
                    });
                }
                if identity < batch.first_id || identity > batch.last_id {
                    return Err(ImportFailure::RowOutOfBatch {
                        id: identity,
                        file: batch.file.clone(),
                        first: batch.first_id.clone(),
                        last: batch.last_id.clone(),
                    });
                }
                if let Some(reconciler) = reconciler {
                    for digest in referenced_digests(&table.table, row) {
                        if !reconciler.has(&digest).await? {
                            return Err(ImportFailure::MissingBlobs {
                                missing: 1,
                                first: digest,
                                referenced_by: identity,
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Verifica que un lote marcado como aplicado en el progreso esta de
/// verdad en el destino, fila por fila y digest a digest.
async fn verify_done_batch(
    sink: &Arc<dyn RowSink>,
    package: &Path,
    table: &str,
    outcome: &BatchOutcome,
    manifest: &ImportManifest,
) -> Result<(), ImportFailure> {
    let entry = manifest
        .tables
        .iter()
        .find(|t| t.table == outcome.table)
        .and_then(|t| t.batches.get(outcome.index - 1))
        .ok_or_else(|| ImportFailure::ProgressNotBacked {
            table: table.to_string(),
            file: outcome.file.clone(),
            detail: "the progress names a batch this package does not carry".to_string(),
        })?;
    if entry.file != outcome.file {
        return Err(ImportFailure::ProgressNotBacked {
            table: table.to_string(),
            file: outcome.file.clone(),
            detail: format!("the progress slot holds '{}'", entry.file),
        });
    }
    let rows = read_batch(package, entry).map_err(ImportFailure::Store)?;
    if outcome.created + outcome.skipped + outcome.overwritten != rows.len() as u64 {
        return Err(ImportFailure::ProgressNotBacked {
            table: table.to_string(),
            file: outcome.file.clone(),
            detail: format!(
                "the progress says {} rows applied, the batch carries {}",
                outcome.created + outcome.skipped + outcome.overwritten,
                rows.len()
            ),
        });
    }
    for row in &rows {
        let identity = canonicalize_id(&outcome.table, row)?;
        let row = with_canonical_id(row.clone(), &identity);
        let expected = sink.fingerprint_row(&row);
        match sink.lookup(&identity).await? {
            None => {
                return Err(ImportFailure::ProgressNotBacked {
                    table: table.to_string(),
                    file: outcome.file.clone(),
                    detail: format!("'{identity}' is not in the destination"),
                });
            }
            Some(stored) => {
                if sink.fingerprint_row(&stored) != expected {
                    return Err(ImportFailure::ProgressNotBacked {
                        table: table.to_string(),
                        file: outcome.file.clone(),
                        detail: format!("'{identity}' differs from what the batch declares"),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Importa un paquete de export en una autoridad. Idempotente por
/// identidad canonica, resumible por lote, fail-closed por defecto.
///
/// - Comprueba el manifiesto y concilia el paquete entero antes de la
///   primera escritura (formato, autodigest, hashes, orden, identidades,
///   digests contra el CAS).
/// - Si hay progreso sin `resume`, aborta: reanudar sin decirlo podria
///   solaparse con una corrida que no murio. Con `resume`, cada lote
///   grabado se verifica contra su hash **y contra el destino**.
/// - Aplica lote a lote en el orden del manifiesto: identidad ausente se
///   crea con la identidad del origen; identidad presente con el mismo
///   digest canonico se salta; identidad presente con otro digest aborta
///   (`Conflict`) salvo `overwrite`, que reescribe conservando la
///   identidad.
/// - Graba el progreso tras cada lote en atomico y lo borra al terminar.
pub async fn import_export(
    package: &Path,
    sink: Arc<dyn RowSink>,
    reconciler: Option<Arc<dyn DigestReconciler>>,
    options: &ImportOptions,
    progress_path: &Path,
) -> Result<ImportReport, ImportFailure> {
    let started = std::time::Instant::now();
    let manifest = read_import_manifest(package).map_err(ImportFailure::Store)?;
    if manifest.blobs.policy != "referenced-by-digest" || manifest.blobs.copied {
        return Err(ImportFailure::BlobPolicy {
            policy: manifest.blobs.policy.clone(),
        });
    }
    if options.batch_rows == 0 {
        return Err(ImportFailure::Invalid(
            "the page size cannot be zero".to_string(),
        ));
    }
    if reconciler.is_none() && !options.allow_missing_blobs {
        return Err(ImportFailure::Invalid(
            "the import requires a CAS digest reconciler or an explicit \
             allow_missing_blobs waiver (doc-20: a digest the CAS lacks is the \
             import's failure to raise, not its silence)"
                .to_string(),
        ));
    }
    // Manifest-first: nada se aplica hasta que todo el paquete cuadra.
    preflight(package, &manifest, reconciler.as_ref()).await?;

    let fingerprint_before = sink.state_summary().await?.fingerprint();

    let mut progress: Option<ImportProgress> = None;
    match std::fs::read(progress_path) {
        Ok(text) => {
            let on_disk: ImportProgress = serde_json::from_slice(&text).map_err(|error| {
                ImportFailure::Invalid(format!("the import progress is unreadable: {error}"))
            })?;
            if on_disk.content_sha256 != manifest.content_sha256 {
                return Err(ImportFailure::ProgressMismatch {
                    progress: on_disk.content_sha256,
                    package: manifest.content_sha256.clone(),
                });
            }
            if !options.resume {
                return Err(ImportFailure::Invalid(
                    "the destination holds an unfinished import; pass resume to continue it \
                     (each done batch is re-verified against its hash and the destination \
                     first)"
                        .to_string(),
                ));
            }
            for outcome in &on_disk.done {
                verify_done_batch(&sink, package, &outcome.table, outcome, &manifest).await?;
            }
            progress = Some(on_disk);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ImportFailure::Invalid(format!(
                "the import progress cannot be read: {error}"
            )));
        }
    }

    let done: BTreeSet<(String, usize)> = progress
        .as_ref()
        .map(|state| {
            state
                .done
                .iter()
                .map(|batch| (batch.table.clone(), batch.index))
                .collect()
        })
        .unwrap_or_default();

    let mut report = ImportReport {
        fingerprint_before,
        fingerprint_after: String::new(),
        frontier: manifest.frontier.clone(),
        content_sha256: manifest.content_sha256.clone(),
        batches: Vec::new(),
        created: 0,
        skipped: 0,
        overwritten: 0,
        finished: false,
        started_at: Utc::now(),
        elapsed_ms: 0,
    };
    let mut applied: Vec<BatchOutcome> = progress
        .as_ref()
        .map(|state| state.done.clone())
        .unwrap_or_default();

    for table in &manifest.tables {
        for (index, batch) in table.batches.iter().enumerate() {
            if done.contains(&(table.table.clone(), index + 1)) {
                continue;
            }
            let rows = read_batch(package, batch).map_err(ImportFailure::Store)?;
            let mut outcome = BatchOutcome {
                table: table.table.clone(),
                index: index + 1,
                file: batch.file.clone(),
                created: 0,
                skipped: 0,
                overwritten: 0,
            };
            let mut page: Vec<(String, Row)> = Vec::with_capacity(options.batch_rows);
            for row in rows {
                let identity = canonicalize_id(&table.table, &row)?;
                let row = with_canonical_id(row, &identity);
                match sink.lookup(&identity).await? {
                    None => {
                        page.push((identity, row));
                        outcome.created += 1;
                    }
                    Some(existing) => {
                        let source_digest = sink.fingerprint_row(&row);
                        let destination_digest = sink.fingerprint_row(&existing);
                        if source_digest == destination_digest {
                            outcome.skipped += 1;
                        } else if options.overwrite {
                            sink.put(&identity, &row).await?;
                            outcome.overwritten += 1;
                        } else {
                            return Err(ImportFailure::Conflict {
                                table: table.table.clone(),
                                key: identity,
                                package_digest: source_digest,
                                destination_digest,
                            });
                        }
                    }
                }
                if page.len() >= options.batch_rows {
                    flush(&sink, std::mem::take(&mut page)).await?;
                }
            }
            if !page.is_empty() {
                flush(&sink, page).await?;
            }
            report.created += outcome.created;
            report.skipped += outcome.skipped;
            report.overwritten += outcome.overwritten;
            report.batches.push(outcome.clone());
            applied.push(outcome);
            write_progress(
                progress_path,
                &ImportProgress {
                    format: IMPORT_FORMAT,
                    content_sha256: manifest.content_sha256.clone(),
                    done: applied.clone(),
                },
            )?;
        }
    }

    report.fingerprint_after = sink.state_summary().await?.fingerprint();
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    match std::fs::remove_file(progress_path) {
        Ok(()) => report.finished = true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => report.finished = true,
        Err(error) => {
            return Err(ImportFailure::Invalid(format!(
                "the import finished but its progress marker could not be removed: {error}"
            )));
        }
    }
    Ok(report)
}

async fn flush(sink: &Arc<dyn RowSink>, page: Vec<(String, Row)>) -> Result<(), ImportFailure> {
    for (identity, row) in page {
        sink.put(&identity, &row).await?;
    }
    Ok(())
}

fn write_progress(path: &Path, state: &ImportProgress) -> Result<(), ImportFailure> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ImportFailure::Invalid(format!("the progress directory cannot be created: {error}"))
        })?;
    }
    let temporary = path.with_file_name(".import-progress.part");
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| ImportFailure::Invalid(format!("the progress is unwritable: {error}")))?;
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|error| {
            ImportFailure::Invalid(format!("the import progress cannot be written: {error}"))
        })
}

/// El sink de memoria para el contrato y las pruebas: un mapa de
/// `tabla:identidad -> fila`, con la misma semantica que exige el
/// adaptador de grafo (la identidad canonica es la clave, los UIDs no
/// existen).
#[derive(Debug, Default)]
pub struct MemorySink {
    rows: tokio::sync::Mutex<BTreeMap<String, Row>>,
    fingerprint_override: Option<fn(&Row) -> String>,
}

impl MemorySink {
    /// Uno vacio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Con una funcion de digest propia: el adaptador que normaliza la
    /// representacion del origen iguala aqui su nocion de "mismo
    /// contenido".
    pub fn with_fingerprint(digest: fn(&Row) -> String) -> Self {
        Self {
            rows: tokio::sync::Mutex::new(BTreeMap::new()),
            fingerprint_override: Some(digest),
        }
    }

    /// Las filas actuales, por identidad canonica.
    pub async fn snapshot(&self) -> BTreeMap<String, Row> {
        self.rows.lock().await.clone()
    }

    /// Cuantas filas hay.
    pub async fn len(&self) -> usize {
        self.rows.lock().await.len()
    }

    /// Si el destino esta vacio.
    pub async fn is_empty(&self) -> bool {
        self.rows.lock().await.is_empty()
    }
}

#[async_trait]
impl RowSink for MemorySink {
    async fn lookup(&self, identity: &str) -> Result<Option<Row>, RecoveryError> {
        Ok(self.rows.lock().await.get(identity).cloned())
    }

    async fn put(&self, identity: &str, row: &Row) -> Result<(), RecoveryError> {
        self.rows
            .lock()
            .await
            .insert(identity.to_string(), row.clone());
        Ok(())
    }

    fn fingerprint_row(&self, row: &Row) -> String {
        match self.fingerprint_override {
            Some(custom) => custom(row),
            None => canonical_digest(row),
        }
    }

    async fn state_summary(&self) -> Result<DestinationState, RecoveryError> {
        let guard = self.rows.lock().await;
        let mut tables: BTreeMap<String, u64> = BTreeMap::new();
        let mut rows = BTreeMap::new();
        for (key, row) in guard.iter() {
            let table = key.split(':').next().unwrap_or(key).to_string();
            *tables.entry(table).or_default() += 1;
            rows.insert(key.clone(), self.fingerprint_row(row));
        }
        Ok(DestinationState { tables, rows })
    }
}

/// Reconcilia los digests que las filas referencian contra el CAS.
#[async_trait]
pub trait DigestReconciler: Send + Sync {
    /// `true` si el digest existe en el almacen con su hash verificado.
    async fn has(&self, digest: &str) -> Result<bool, RecoveryError>;
}

/// El reconciliador de prueba y del rehearsal local: el conjunto de
/// digests que el almacen tiene.
#[derive(Debug, Default)]
pub struct SetReconciler {
    digests: BTreeSet<String>,
}

impl SetReconciler {
    /// Uno que conoce exactamente estos digests.
    pub fn new<I: IntoIterator<Item = String>>(digests: I) -> Self {
        Self {
            digests: digests.into_iter().collect(),
        }
    }
}

#[async_trait]
impl DigestReconciler for SetReconciler {
    async fn has(&self, digest: &str) -> Result<bool, RecoveryError> {
        Ok(self.digests.contains(digest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(table: &str, key: &str, fields: &[(&str, serde_json::Value)]) -> Row {
        let mut row = Row::new();
        row.insert(
            "id".to_string(),
            serde_json::Value::String(format!("{table}:{key}")),
        );
        for (name, value) in fields {
            row.insert((*name).to_string(), value.clone());
        }
        row
    }

    fn number(value: i64) -> serde_json::Value {
        serde_json::Value::Number(value.into())
    }

    #[test]
    fn a_canonical_string_id_is_the_identity() {
        let row = row("catalog_tag", "repo|v1", &[]);
        assert_eq!(
            canonicalize_id("catalog_tag", &row).unwrap(),
            "catalog_tag:repo|v1"
        );
    }

    #[test]
    fn a_ledger_composite_key_survives_canonicalization() {
        // El contrato del ledger: la identidad es `stream,pos` dentro del
        // id; el import la conserva literal, no la reordena ni la reescribe.
        let row = row("event_ledger", "stream_a,0", &[("position", number(0))]);
        assert_eq!(
            canonicalize_id("event_ledger", &row).unwrap(),
            "event_ledger:stream_a,0"
        );
    }

    #[test]
    fn an_object_id_is_canonicalized_to_the_same_text() {
        let mut row = Row::new();
        let mut id = serde_json::Map::new();
        id.insert(
            "tb".to_string(),
            serde_json::Value::String("event_ledger".to_string()),
        );
        id.insert("id".to_string(), number(7));
        row.insert("id".to_string(), serde_json::Value::Object(id));
        assert_eq!(
            canonicalize_id("event_ledger", &row).unwrap(),
            "event_ledger:7"
        );
    }

    #[test]
    fn a_row_without_an_id_is_refused_not_invented() {
        let mut row = Row::new();
        row.insert(
            "note".to_string(),
            serde_json::Value::String("x".to_string()),
        );
        assert!(matches!(
            canonicalize_id("t", &row),
            Err(ImportFailure::RowInvalid { .. })
        ));
    }

    #[test]
    fn an_id_from_another_table_is_refused() {
        let row = row("catalog_tag", "x", &[]);
        assert!(matches!(
            canonicalize_id("catalog_asset", &row),
            Err(ImportFailure::RowInvalid { .. })
        ));
    }

    #[test]
    fn the_content_digest_pins_the_batch_order() {
        let a = ImportBatch {
            file: "batches/x-000001.json".into(),
            sha256: "a".repeat(64),
            rows: 1,
            first_id: "x:1".into(),
            last_id: "x:1".into(),
        };
        let b = ImportBatch {
            file: "batches/x-000002.json".into(),
            sha256: "b".repeat(64),
            rows: 1,
            first_id: "x:2".into(),
            last_id: "x:2".into(),
        };
        let table = |batches: Vec<ImportBatch>| ImportTable {
            table: "x".into(),
            rows: 2,
            batches,
        };
        let forward = manifest_of(vec![table(vec![a.clone(), b.clone()])]);
        let backward = manifest_of(vec![table(vec![b, a])]);
        assert_ne!(forward.content_digest(), backward.content_digest());
    }

    fn manifest_of(tables: Vec<ImportTable>) -> ImportManifest {
        let mut frontier_tables = BTreeMap::new();
        for table in &tables {
            frontier_tables.insert(table.table.clone(), table.rows);
        }
        ImportManifest {
            format: IMPORT_FORMAT,
            tool_version: "test".into(),
            source: ExportSource {
                engine: "surrealdb".into(),
                engine_version: "2.3.9".into(),
                backend: "mem".into(),
                namespace: "hodei".into(),
                database: "artifacts".into(),
            },
            frontier: ExportFrontier {
                tables: frontier_tables,
                ledger_rows: 0,
                ledger_max_position: None,
                head_rows: 0,
            },
            tables,
            blobs: BlobPolicy {
                policy: "referenced-by-digest".into(),
                copied: false,
            },
            content_sha256: String::new(),
        }
    }

    #[test]
    fn an_altered_self_digest_is_refused() {
        let mut manifest = manifest_of(vec![]);
        manifest.content_sha256 = manifest.content_digest();
        manifest
            .check()
            .expect("an empty manifest with its own digest is sound");
        manifest.content_sha256 = "f".repeat(64);
        assert!(matches!(
            manifest.check(),
            Err(ImportFailure::ContentDigest { .. })
        ));
    }

    #[test]
    fn unordered_batches_are_refused() {
        let batch = |index: usize, first: &str, last: &str| ImportBatch {
            file: format!("batches/x-{index:06}.json"),
            sha256: "0".repeat(64),
            rows: 1,
            first_id: first.into(),
            last_id: last.into(),
        };
        let manifest = manifest_of(vec![ImportTable {
            table: "x".into(),
            rows: 2,
            batches: vec![batch(1, "x:2", "x:2"), batch(2, "x:1", "x:1")],
        }]);
        assert!(matches!(
            manifest.check(),
            Err(ImportFailure::Order { .. })
        ));
    }

    #[tokio::test]
    async fn the_memory_sink_fingerprint_is_insertion_order_independent() {
        let sink = MemorySink::new();
        sink.put("t:a", &row("t", "a", &[("v", number(1))]))
            .await
            .unwrap();
        sink.put("t:b", &row("t", "b", &[("v", number(2))]))
            .await
            .unwrap();
        let first = sink.state_summary().await.unwrap().fingerprint();
        let other = MemorySink::new();
        other
            .put("t:b", &row("t", "b", &[("v", number(2))]))
            .await
            .unwrap();
        other
            .put("t:a", &row("t", "a", &[("v", number(1))]))
            .await
            .unwrap();
        assert_eq!(first, other.state_summary().await.unwrap().fingerprint());
    }

    #[tokio::test]
    async fn a_changed_value_changes_the_fingerprint() {
        let sink = MemorySink::new();
        sink.put("t:a", &row("t", "a", &[("v", number(1))]))
            .await
            .unwrap();
        let before = sink.state_summary().await.unwrap().fingerprint();
        sink.put("t:a", &row("t", "a", &[("v", number(2))]))
            .await
            .unwrap();
        assert_ne!(before, sink.state_summary().await.unwrap().fingerprint());
    }

    #[test]
    fn key_order_inside_a_row_does_not_change_its_digest() {
        let mut first = Row::new();
        first.insert("b".to_string(), number(2));
        first.insert("a".to_string(), number(1));
        let mut second = Row::new();
        second.insert("a".to_string(), number(1));
        second.insert("b".to_string(), number(2));
        assert_eq!(canonical_digest(&first), canonical_digest(&second));
    }

    #[test]
    fn the_digest_ignores_the_identity_itself() {
        let with_id = row("t", "a", &[("v", number(1))]);
        let mut content = row("t", "ignored", &[("v", number(1))]);
        content.remove("id");
        assert_eq!(canonical_digest(&with_id), canonical_digest(&content));
    }

    #[test]
    fn a_nested_object_orders_its_keys_in_the_digest() {
        let mut outer = Row::new();
        let mut inner = Row::new();
        inner.insert("y".to_string(), number(2));
        inner.insert("x".to_string(), number(1));
        outer.insert("meta".to_string(), serde_json::Value::Object(inner));
        let mut shuffled = Row::new();
        let mut inner = Row::new();
        inner.insert("x".to_string(), number(1));
        inner.insert("y".to_string(), number(2));
        shuffled.insert("meta".to_string(), serde_json::Value::Object(inner));
        assert_eq!(canonical_digest(&outer), canonical_digest(&shuffled));
    }

    #[test]
    fn referenced_digests_follow_the_declared_fields() {
        let asset = row(
            "catalog_asset",
            "a",
            &[("sha256", serde_json::Value::String("sha256:aa".into()))],
        );
        assert_eq!(
            referenced_digests("catalog_asset", &asset),
            vec!["sha256:aa".to_string()]
        );
        assert!(referenced_digests("catalog_tag", &asset).is_empty());
    }
}
