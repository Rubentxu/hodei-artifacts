//! Rebuild de proyecciones y reconciliación semántica para el cutover (HSP-211).
//!
//! El import de [`super::import`] responde a *qué filas hay*: la identidad
//! canónica manda y el fingerprint de autoridades es el oráculo. Este módulo
//! responde a la pregunta del cutover: **origen y destino equivalen por
//! cuenta**, no "por conteo". Dos estados con el mismo número de filas pueden
//! no equivaler nada — una etiqueta cruzada, un grant sin evidencia en el
//! CAS, un cabezal de ledger que miente — y un cutover autorizado por conteos
//! aislados es justo el falso verde que doc-20 prohíbe.
//!
//! Reglas, de más a menos importantes:
//!
//! 1. **La equivalencia es agregada por clave lógica.** Cada proyección
//!    ([`SemanticProjection`]) se calcula por cuenta, grant vigente, tag,
//!    camino de artefacto, digest referenciado y cursor de ledger. Ninguna
//!    comparación usa recuentos globales: un desplazamiento entre cuentas es
//!    divergencia, no compensación.
//! 2. **Falta y corrupción jamás escalan a verificado.** [`CutoverToken`]
//!    solo lo fabrica [`gate_cutover`] cuando cada proyección cuadra y cada
//!    digest referenciado está en el CAS. No hay constructor público, no hay
//!    booleano suelto: el estado se llama `Blocked` o `Verified`, y `Blocked`
//!    nombra la cuenta o fila culpable.
//! 3. **El fingerprint de autoridades es uno solo.** El oráculo es
//!    [`super::import::DestinationState::fingerprint`]; este módulo lo
//!    reutiliza para la equivalencia global y añade la semántica por encima,
//!    sin inventar una segunda firma canónica.
//! 4. **El rebuild es idempotente.** [`prepare`] es una función pura sobre
//!    el mapa de filas: reconstruir dos veces el mismo estado produce la
//!    misma versión semántica y el mismo resultado de gate (AC2).
//! 5. **El cabezal no miente.** Si la posición máxima de `event_ledger` de un
//!    stream no coincide con su `event_ledger_head`, el estado está corrupto
//!    y el rebuild aborta nombrando el stream: un cursor divergente impide
//!    el cutover, nunca se "normaliza".

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest as _, Sha256};

use super::RecoveryError;
use super::import::{
    DIGEST_FIELDS, DestinationState, DigestReconciler, ImportFailure, ImportManifest, Row, RowSink,
    canonical_digest, canonicalize_id, read_batch, read_import_manifest,
};

/// Tablas cuyas filas cuelgan de una cuenta. `iam_account` declara la cuenta
/// por su `handle`; el resto, por el campo `account`.
pub const ACCOUNT_TABLES: &[&str] = &[
    "catalog_repository",
    "catalog_version",
    "catalog_asset",
    "catalog_reference",
    "catalog_tag",
];

/// Tablas que definen un camino de artefacto dentro de una cuenta y
/// repositorio.
pub const PATH_TABLES: &[&str] = &["catalog_asset", "catalog_reference"];

/// La tabla de grants vigentes.
pub const GRANT_TABLE: &str = "trust_grant";

/// La tabla de etiquetas.
pub const TAG_TABLE: &str = "catalog_tag";

/// La tabla de cabezales de ledger (cursores).
pub const HEAD_TABLE: &str = "event_ledger_head";

/// La tabla de eventos del ledger.
pub const LEDGER_TABLE: &str = "event_ledger";

/// El rebuild falla: el estado no es proyectable tal como está.
#[derive(Debug, thiserror::Error)]
pub enum ReconcileError {
    /// Una fila obligatoria para la proyección no lleva su campo clave, o lo
    /// lleva con otra forma. Corrupción: fail-closed, no se ignora.
    #[error("the row '{identity}' is not projectable: {detail}")]
    Corrupt {
        /// La identidad canónica culpable.
        identity: String,
        /// Por qué no cuadra.
        detail: String,
    },
    /// El cabezal del ledger no respalda la posición más alta de sus eventos.
    #[error("the ledger head of stream '{stream}' says {head}, its events reach {events}")]
    CursorMismatch {
        /// El stream culpable.
        stream: String,
        /// Lo que dice el cabezal.
        head: u64,
        /// Lo que alcanzan los eventos.
        events: u64,
    },
    /// El paquete de origen no se puede leer o no es íntegro.
    #[error("the origin package failed: {0}")]
    Package(#[from] RecoveryError),
    /// El origen no se puede canonicar (filas del paquete inválidas).
    #[error("the origin package is invalid: {0}")]
    Origin(#[from] ImportFailure),
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{byte:02x}"));
    }
    out
}

fn table_of(identity: &str) -> &str {
    identity.split(':').next().unwrap_or(identity)
}

fn str_field(row: &Row, identity: &str, field: &str) -> Result<String, ReconcileError> {
    let value = row.get(field).ok_or_else(|| ReconcileError::Corrupt {
        identity: identity.to_string(),
        detail: format!("the row carries no '{field}'"),
    })?;
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ReconcileError::Corrupt {
            identity: identity.to_string(),
            detail: format!("the '{field}' is not a string"),
        })
}

fn num_field(row: &Row, identity: &str, field: &str) -> Result<u64, ReconcileError> {
    let value = row.get(field).ok_or_else(|| ReconcileError::Corrupt {
        identity: identity.to_string(),
        detail: format!("the row carries no '{field}'"),
    })?;
    value.as_u64().ok_or_else(|| ReconcileError::Corrupt {
        identity: identity.to_string(),
        detail: format!("the '{field}' is not a non-negative integer"),
    })
}

/// La proyección semántica del estado: cada agregado por **su** clave
/// lógica, nunca un censo global. Es la unidad con la que origen y destino
/// se comparan; su [`Self::version`] es la firma del rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticProjection {
    /// Cuenta -> digest sobre el haz completo de sus filas
    /// (`tabla:identidad = digest` ordenado). Cubre repositorios, versiones,
    /// assets, referencias y etiquetas de la cuenta.
    pub accounts: BTreeMap<String, String>,
    /// Grant vigente (identidad canónica) -> digest de su contenido.
    pub grants: BTreeMap<String, String>,
    /// Etiqueta (identidad canónica) -> digest de su contenido.
    pub tags: BTreeMap<String, String>,
    /// Camino `cuenta|repositorio|ruta` -> digest de la fila que lo ocupa.
    pub paths: BTreeMap<String, String>,
    /// Digest de contenido referenciado -> identidades que lo referencian,
    /// unidas por coma en orden. Es el mapa que se confronta con el CAS.
    pub digests: BTreeMap<String, String>,
    /// Stream del ledger -> posición que declara su cabezal.
    pub cursors: BTreeMap<String, u64>,
}

impl SemanticProjection {
    /// La versión semántica del estado reconstruido: un SHA-256 sobre los
    /// seis agregados en orden de clave. Reconstruir el mismo estado dos
    /// veces da la misma versión (AC2); una divergencia en cualquier clave
    /// la cambia.
    pub fn version(&self) -> String {
        let mut hasher = Sha256::new();
        for (account, digest) in &self.accounts {
            hasher.update(format!("A {account} {digest}\n").as_bytes());
        }
        for (grant, digest) in &self.grants {
            hasher.update(format!("G {grant} {digest}\n").as_bytes());
        }
        for (tag, digest) in &self.tags {
            hasher.update(format!("T {tag} {digest}\n").as_bytes());
        }
        for (path, digest) in &self.paths {
            hasher.update(format!("P {path} {digest}\n").as_bytes());
        }
        for (digest, refs) in &self.digests {
            hasher.update(format!("D {digest} {refs}\n").as_bytes());
        }
        for (stream, position) in &self.cursors {
            hasher.update(format!("C {stream} {position}\n").as_bytes());
        }
        sha256_hex(&hasher.finalize())
    }
}

/// Un estado listo para comparar: las filas canónicas, su proyección
/// semántica y el fingerprint de autoridades del oráculo de HSP-210.
#[derive(Debug, Clone)]
pub struct ReconcileSubject {
    /// Las filas por identidad canónica.
    pub rows: BTreeMap<String, Row>,
    /// La proyección semántica reconstruida desde esas filas.
    pub projection: SemanticProjection,
    /// El fingerprint de autoridades (el único oráculo, el de
    /// [`DestinationState::fingerprint`]).
    pub fingerprint: String,
}

impl ReconcileSubject {
    /// La versión semántica del estado.
    pub fn version(&self) -> String {
        self.projection.version()
    }
}

/// El rebuild puro: de `tabla:identidad -> fila` a la proyección semántica
/// y al fingerprint de autoridades. Es una sola pasada determinista;
/// aplicarlo dos veces sobre el mismo mapa preserva versión y resultados.
pub fn prepare(rows: BTreeMap<String, Row>) -> Result<ReconcileSubject, ReconcileError> {
    let mut projection = SemanticProjection {
        accounts: BTreeMap::new(),
        grants: BTreeMap::new(),
        tags: BTreeMap::new(),
        paths: BTreeMap::new(),
        digests: BTreeMap::new(),
        cursors: BTreeMap::new(),
    };

    // Haz por cuenta: iam_account declara la cuenta por su handle; las
    // tablas de catálogo cuelgan del campo `account`.
    let mut bundles: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut heads: BTreeMap<String, u64> = BTreeMap::new();
    let mut ledger_max: BTreeMap<String, u64> = BTreeMap::new();

    for (identity, row) in &rows {
        let table = table_of(identity);
        let digest = canonical_digest(row);

        let account = match table {
            "iam_account" => Some(str_field(row, identity, "handle")?),
            t if ACCOUNT_TABLES.contains(&t) => Some(str_field(row, identity, "account")?),
            _ => None,
        };
        if let Some(account) = account {
            bundles
                .entry(account)
                .or_default()
                .push(format!("{identity}={digest}"));
        }

        if table == GRANT_TABLE {
            projection.grants.insert(identity.clone(), digest.clone());
        }
        if table == TAG_TABLE {
            projection.tags.insert(identity.clone(), digest.clone());
        }
        if PATH_TABLES.contains(&table) {
            let account = str_field(row, identity, "account")?;
            let repository = str_field(row, identity, "repository")?;
            let path = str_field(row, identity, "asset_path")?;
            projection
                .paths
                .insert(format!("{account}|{repository}|{path}"), digest.clone());
        }

        for (name, fields) in DIGEST_FIELDS {
            if *name != table {
                continue;
            }
            for field in *fields {
                if let Some(value) = row.get(*field).and_then(serde_json::Value::as_str)
                    && !value.is_empty()
                {
                    let refs = projection.digests.entry(value.to_string()).or_default();
                    if refs.is_empty() {
                        refs.push_str(identity);
                    } else {
                        let _ = std::fmt::Write::write_fmt(refs, format_args!(",{identity}"));
                    }
                }
            }
        }

        if table == HEAD_TABLE {
            let stream = str_field(row, identity, "stream")?;
            let last = num_field(row, identity, "last_position")?;
            heads.insert(stream, last);
        }
        if table == LEDGER_TABLE {
            let stream = str_field(row, identity, "stream")?;
            let position = num_field(row, identity, "position")?;
            let entry = ledger_max.entry(stream).or_default();
            *entry = (*entry).max(position);
        }
    }

    for (account, mut facts) in bundles {
        facts.sort();
        let joined = facts.join("\n");
        projection
            .accounts
            .insert(account, sha256_hex(joined.as_bytes()));
    }

    // El cabezal no miente: su posición tiene que ser exactamente la más
    // alta de sus eventos. Un stream sin eventos no puede declarar cabeza.
    for (stream, head) in &heads {
        let events = ledger_max.get(stream).copied().unwrap_or(0);
        if *head != events {
            return Err(ReconcileError::CursorMismatch {
                stream: stream.clone(),
                head: *head,
                events,
            });
        }
    }
    projection.cursors = heads;

    let mut tables: BTreeMap<String, u64> = BTreeMap::new();
    let mut row_digests = BTreeMap::new();
    for (identity, row) in &rows {
        *tables.entry(table_of(identity).to_string()).or_default() += 1;
        row_digests.insert(identity.clone(), canonical_digest(row));
    }
    let fingerprint = DestinationState {
        tables,
        rows: row_digests,
    }
    .fingerprint();

    Ok(ReconcileSubject {
        rows,
        projection,
        fingerprint,
    })
}

/// El estado de una comparación origen↔destino. Explícito por construcción:
/// no existe un booleano "ok" suelto que falta y corrupción puedan escalar.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Divergence {
    /// Una clave lógica del origen no existe en el destino. **Missing**:
    /// impide el cutover; jamás se conforma en verificado.
    #[error("missing on the destination: {what}")]
    Missing {
        /// Qué agregado y qué clave.
        what: String,
    },
    /// Una clave lógica del destino no existe en el origen.
    #[error("extra on the destination: {what}")]
    Extra {
        /// Qué agregado y qué clave.
        what: String,
    },
    /// La misma clave lógica, distinto contenido canónico.
    #[error("{what} differs: origin {origin}, destination {destination}")]
    Content {
        /// Qué agregado y qué clave.
        what: String,
        /// Digest del origen.
        origin: String,
        /// Digest del destino.
        destination: String,
    },
    /// La posición del cursor no equivale.
    #[error(
        "the ledger cursor of stream '{stream}' is at origin {origin}, destination {destination}"
    )]
    Cursor {
        /// El stream culpable.
        stream: String,
        /// La posición de origen.
        origin: u64,
        /// La posición del destino.
        destination: u64,
    },
    /// El CAS no tiene el digest que una fila referencia. **Missing/corrupt
    /// de contenido**: fail-closed, nombrando al referente.
    #[error("the CAS lacks the digest {digest} referenced by {referenced_by}")]
    MissingDigest {
        /// El digest ausente.
        digest: String,
        /// La primera fila (origen o destino) que lo referencia.
        referenced_by: String,
    },
    /// El fingerprint de autoridades no equivale aunque las proyecciones
    /// cuadren: hay filas fuera de los agregados (decisiones, outbox,
    /// ledger) que divergen.
    #[error("the authority fingerprint differs: origin {origin}, destination {destination}")]
    Authority {
        /// Fingerprint del origen.
        origin: String,
        /// Fingerprint del destino.
        destination: String,
    },
}

/// El resultado del gate. [`CutoverDecision::Verified`] lleva el token
/// único que autoriza el corte; [`CutoverDecision::Blocked`] enumera cada
/// divergencia con su culpable nombrado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CutoverDecision {
    /// Origen y destino equivalen por cuenta, supports, tags, digests,
    /// caminos y cursores, y el CAS respalda cada digest referenciado.
    Verified(CutoverToken),
    /// Algo diverge. El cutover está impedido; nada de advertencias.
    Blocked(Vec<Divergence>),
}

impl CutoverDecision {
    /// El token si y solo si la decisión es verificada.
    pub fn token(&self) -> Option<&CutoverToken> {
        match self {
            Self::Verified(token) => Some(token),
            Self::Blocked(_) => None,
        }
    }
}

/// La prueba de que el gate pasó. **No se puede fabricar desde fuera**:
/// su constructor está privado y sus campos son privados. Un `Verified`
/// sin token no existe; un missing o corrupt no llega a este tipo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutoverToken {
    origin_fingerprint: String,
    destination_fingerprint: String,
    version: String,
}

impl CutoverToken {
    /// El fingerprint de autoridades que se verificó en el origen.
    pub fn origin_fingerprint(&self) -> &str {
        &self.origin_fingerprint
    }

    /// El fingerprint de autoridades que se verificó en el destino.
    pub fn destination_fingerprint(&self) -> &str {
        &self.destination_fingerprint
    }

    /// La versión semántica común a ambos estados.
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// El rebuild del destino desde un sink del contrato de import: mismo
/// mapa `tabla:identidad -> fila` que ve el import, misma proyección.
pub async fn prepare_from_sink(sink: &dyn RowSink) -> Result<ReconcileSubject, ReconcileError> {
    let summary = sink.state_summary().await?;
    let mut rows = BTreeMap::new();
    for identity in summary.rows.keys() {
        let row = sink
            .lookup(identity)
            .await?
            .ok_or_else(|| ReconcileError::Corrupt {
                identity: identity.clone(),
                detail: "the state summary lists a row the sink cannot return".to_string(),
            })?;
        rows.insert(identity.clone(), row);
    }
    prepare(rows)
}

/// Las filas del paquete de origen, canonicadas con la misma identidad que
/// fija el import. Reutiliza la verificación manifest-first de HSP-210:
/// un lote alterado o fuera de bordes aborta antes de proyectar nada.
pub fn package_rows(package: &Path) -> Result<BTreeMap<String, Row>, ReconcileError> {
    let manifest: ImportManifest =
        read_import_manifest(package).map_err(ReconcileError::Package)?;
    let mut rows = BTreeMap::new();
    for table in &manifest.tables {
        for batch in &table.batches {
            for row in read_batch(package, batch).map_err(ReconcileError::Package)? {
                let identity =
                    canonicalize_id(&table.table, &row).map_err(ReconcileError::Origin)?;
                if rows.insert(identity.clone(), row).is_some() {
                    return Err(ReconcileError::Origin(ImportFailure::DuplicateRow {
                        table: table.table.clone(),
                        key: identity,
                    }));
                }
            }
        }
    }
    Ok(rows)
}

/// El gate del cutover. Reconstruye la comparación sobre las proyecciones
/// ya preparadas de origen y destino y confronta cada digest referenciado
/// contra el CAS. Devuelve `Blocked` con la lista completa de culpables;
/// `Verified` exige que no haya ninguno (AC1 fail-closed) y que las dos
/// proyecciones tengan la misma versión.
pub async fn gate_cutover(
    origin: &ReconcileSubject,
    destination: &ReconcileSubject,
    cas: &dyn DigestReconciler,
) -> Result<CutoverDecision, RecoveryError> {
    let mut divergences = Vec::new();
    compare_map(
        "account",
        &origin.projection.accounts,
        &destination.projection.accounts,
        &mut divergences,
    );
    compare_map(
        "grant",
        &origin.projection.grants,
        &destination.projection.grants,
        &mut divergences,
    );
    compare_map(
        "tag",
        &origin.projection.tags,
        &destination.projection.tags,
        &mut divergences,
    );
    compare_map(
        "path",
        &origin.projection.paths,
        &destination.projection.paths,
        &mut divergences,
    );
    compare_map(
        "digest-reference",
        &origin.projection.digests,
        &destination.projection.digests,
        &mut divergences,
    );
    for stream in origin
        .projection
        .cursors
        .keys()
        .chain(destination.projection.cursors.keys())
    {
        match (
            origin.projection.cursors.get(stream),
            destination.projection.cursors.get(stream),
        ) {
            (Some(from), Some(to)) if from != to => divergences.push(Divergence::Cursor {
                stream: stream.clone(),
                origin: *from,
                destination: *to,
            }),
            (Some(_), None) => divergences.push(Divergence::Missing {
                what: format!("cursor stream '{stream}'"),
            }),
            (None, Some(_)) => divergences.push(Divergence::Extra {
                what: format!("cursor stream '{stream}'"),
            }),
            _ => {}
        }
    }

    if origin.fingerprint != destination.fingerprint {
        divergences.push(Divergence::Authority {
            origin: origin.fingerprint.clone(),
            destination: destination.fingerprint.clone(),
        });
    }

    // CAS: cada digest que cualquier lado referencia tiene que estar. El
    // "first referencing" viaja en el error para nombrar al culpable.
    let mut digests: BTreeMap<&String, &String> = BTreeMap::new();
    for (digest, refs) in origin.projection.digests.iter() {
        digests.insert(digest, refs);
    }
    for (digest, refs) in destination.projection.digests.iter() {
        digests.entry(digest).or_insert(refs);
    }
    for (digest, referenced_by) in digests {
        if !cas.has(digest).await? {
            divergences.push(Divergence::MissingDigest {
                digest: digest.clone(),
                referenced_by: referenced_by.clone(),
            });
        }
    }

    if !divergences.is_empty() {
        return Ok(CutoverDecision::Blocked(divergences));
    }
    let version = origin.version();
    if version != destination.version() {
        return Ok(CutoverDecision::Blocked(vec![Divergence::Content {
            what: "projection version".to_string(),
            origin: version,
            destination: destination.version(),
        }]));
    }
    Ok(CutoverDecision::Verified(CutoverToken {
        origin_fingerprint: origin.fingerprint.clone(),
        destination_fingerprint: destination.fingerprint.clone(),
        version,
    }))
}

fn compare_map(
    what: &str,
    origin: &BTreeMap<String, String>,
    destination: &BTreeMap<String, String>,
    divergences: &mut Vec<Divergence>,
) {
    let keys: std::collections::BTreeSet<&String> =
        origin.keys().chain(destination.keys()).collect();
    for key in keys {
        match (origin.get(key), destination.get(key)) {
            (Some(from), Some(to)) if from != to => divergences.push(Divergence::Content {
                what: format!("{what} '{key}'"),
                origin: from.clone(),
                destination: to.clone(),
            }),
            (Some(_), None) => divergences.push(Divergence::Missing {
                what: format!("{what} '{key}'"),
            }),
            (None, Some(_)) => divergences.push(Divergence::Extra {
                what: format!("{what} '{key}'"),
            }),
            _ => {}
        }
    }
}
