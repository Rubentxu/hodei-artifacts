//! La copia coherente, su verificacion, la restauracion y la reparacion.
//!
//! Todo lo que toca el disco vive aqui. Los blobs se copian **siempre**
//! con el hash a la vista: se lee el origen, se calcula el SHA-256 mientras
//! se escribe, y el destino solo existe si el hash coincide con el digest.
//! Un blob corrupto no se copia, se informa.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use hodei_hub::blob::FileBlobStore;
use hodei_hub::{BlobReclaim, BlobStore};
use serde::Serialize;
use sha2::{Digest, Sha256};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

use super::RecoveryError;
use super::manifest::{
    BLOBS_DIR, BackupManifest, BlobEntry, Excluded, FORMAT, MANIFEST_FILE, METADATA_FILE,
    MetadataEntry, hex_of,
};
use super::scrub::{Finding, ScrubReport, Scrubber};

/// El directorio, dentro del almacen, donde se aparta lo danado.
///
/// No lo lista el almacen (su nombre no es un prefijo de digest), asi que lo
/// apartado no cuenta como contenido ni lo recoge la recoleccion de basura:
/// queda para que la persona que opera lo examine.
pub const QUARANTINE_DIR: &str = "quarantine";

const CHUNK: usize = 256 * 1024;

fn hex_string(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Donde vive un blob dentro de un directorio con el reparto del almacen.
fn blob_file(root: &Path, hex: &str) -> PathBuf {
    root.join(&hex[..2]).join(&hex[2..4]).join(hex)
}

/// El SHA-256 y la longitud de un fichero, leido por trozos.
fn hash_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok((hex_string(&hasher.finalize()), total))
}

/// Por que no se pudo copiar un blob.
#[derive(Debug)]
enum CopyError {
    /// El origen no esta.
    Missing,
    /// El origen no tiene el hash esperado.
    Mismatch { found: String },
    /// El disco fallo.
    Io(std::io::Error),
}

impl From<std::io::Error> for CopyError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Copia `src` a `dst` calculando el hash, y solo deja `dst` si coincide.
///
/// Se escribe en un fichero temporal junto al destino y se renombra: nadie
/// ve nunca un blob a medias, y un corte deja, como mucho, un `.part` que el
/// almacen no reconoce como contenido.
fn copy_verified(src: &Path, dst: &Path, expected_hex: &str) -> Result<u64, CopyError> {
    let mut input = match std::fs::File::open(src) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CopyError::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = dst.with_file_name(format!(".{expected_hex}.part"));
    let outcome = (|| -> Result<u64, CopyError> {
        let mut output = std::fs::File::create(&temporary)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read])?;
            total += read as u64;
        }
        output.sync_all()?;
        let found = hex_string(&hasher.finalize());
        if found != expected_hex {
            return Err(CopyError::Mismatch { found });
        }
        Ok(total)
    })();
    match outcome {
        Ok(total) => {
            std::fs::rename(&temporary, dst)?;
            Ok(total)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            Err(error)
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, RecoveryError> + Send + 'static,
) -> Result<T, RecoveryError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| RecoveryError::Io(error.to_string()))?
}

/// Importa los metadatos en una base en memoria que se descarta.
///
/// Es el ensayo de la restauracion: si el export no se importa, la copia no
/// se publica. Cuesta memoria en proporcion al catalogo.
async fn rehearse_import(
    metadata: &Path,
    namespace: &str,
    database: &str,
) -> Result<(), RecoveryError> {
    let scratch: Surreal<Db> = Surreal::new::<surrealdb::engine::local::Mem>(())
        .await
        .map_err(|error| RecoveryError::Metadata(error.to_string()))?;
    scratch
        .use_ns(namespace)
        .use_db(database)
        .await
        .map_err(|error| RecoveryError::Metadata(error.to_string()))?;
    scratch.import(metadata).await.map_err(|error| {
        RecoveryError::Metadata(format!(
            "the exported metadata cannot be imported back, so the backup would not be \
             restorable: {error}"
        ))
    })
}

/// Abre la base embebida y selecciona namespace y base.
///
/// El motor bloquea el directorio mientras el servicio vive: si no se puede
/// abrir, el mensaje dice la causa probable.
pub async fn open_database(
    path: &str,
    namespace: &str,
    database: &str,
) -> Result<Surreal<Db>, RecoveryError> {
    let db: Surreal<Db> = Surreal::new::<RocksDb>(path).await.map_err(|error| {
        RecoveryError::Metadata(format!(
            "cannot open the database {path}: {error}; the service must be stopped, \
             it holds the database while it runs"
        ))
    })?;
    db.use_ns(namespace)
        .use_db(database)
        .await
        .map_err(|error| RecoveryError::Metadata(error.to_string()))?;
    Ok(db)
}

/// Lo que hizo una copia.
#[derive(Debug, Clone, Serialize)]
pub struct BackupReport {
    /// Donde quedo.
    pub path: PathBuf,
    /// Lo que la copia dice de si misma.
    pub manifest: BackupManifest,
    /// Lo que tardo exportar los metadatos, en milisegundos.
    pub metadata_ms: u64,
    /// Lo que tardo copiar los blobs, en milisegundos.
    pub blobs_ms: u64,
    /// Lo que tardo en total, en milisegundos.
    pub elapsed_ms: u64,
}

/// Hace una copia coherente de los metadatos y del almacen.
///
/// Primero se exportan los metadatos y despues se copian los blobs: los
/// blobs de la copia son un superconjunto de lo que los metadatos
/// referencian. Todo se escribe en un directorio temporal junto al destino
/// y se publica con un renombrado.
pub async fn backup(
    db: &Surreal<Db>,
    cas: &Path,
    dest: &Path,
    namespace: &str,
    database: &str,
) -> Result<BackupReport, RecoveryError> {
    let started = Instant::now();
    let started_at = Utc::now();
    if dest.exists() {
        return Err(RecoveryError::Invalid(format!(
            "the destination {} already exists: a backup is never overwritten",
            dest.display()
        )));
    }
    if !cas.is_dir() {
        return Err(RecoveryError::Invalid(format!(
            "the content store {} does not exist",
            cas.display()
        )));
    }
    let parent = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(RecoveryError::Invalid(format!(
            "the directory {} does not exist",
            parent.display()
        )));
    }
    let name = dest
        .file_name()
        .ok_or_else(|| RecoveryError::Invalid("the destination has no name".to_string()))?
        .to_string_lossy()
        .into_owned();
    let staging = parent.join(format!(".{name}.partial-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(staging.join(BLOBS_DIR))?;

    let outcome = write_backup(
        db, cas, &staging, dest, namespace, database, started, started_at,
    )
    .await;
    if outcome.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    outcome
}

#[allow(clippy::too_many_arguments)]
async fn write_backup(
    db: &Surreal<Db>,
    cas: &Path,
    staging: &Path,
    dest: &Path,
    namespace: &str,
    database: &str,
    started: Instant,
    started_at: chrono::DateTime<Utc>,
) -> Result<BackupReport, RecoveryError> {
    // 1. Los metadatos, primero.
    let metadata_path = staging.join(METADATA_FILE);
    let metadata_snapshot_at = Utc::now();
    let metadata_started = Instant::now();
    let raw_path = staging.join(format!("{METADATA_FILE}.raw"));
    db.export(raw_path.clone())
        .await
        .map_err(|error| RecoveryError::Metadata(error.to_string()))?;
    let metadata_file = metadata_path.clone();
    // El export del motor no se puede importar tal cual (ver `export`).
    let (sha256, bytes) = blocking(move || {
        super::export::drop_implied_elements(&raw_path, &metadata_file)?;
        std::fs::remove_file(&raw_path)?;
        Ok(hash_file(&metadata_file)?)
    })
    .await?;
    // Ensayo: lo exportado tiene que importarse. Una copia que se escribe
    // bien y no se puede restaurar es la peor forma de fallar, y el filtro
    // del export solo conoce las formas que conoce.
    rehearse_import(&metadata_path, namespace, database).await?;
    let metadata_ms = metadata_started.elapsed().as_millis() as u64;

    // 2. Los blobs, despues.
    let blobs_started = Instant::now();
    let store = FileBlobStore::new(cas).map_err(|error| RecoveryError::Store(error.to_string()))?;
    let listed = store
        .list_blobs()
        .await
        .map_err(|error| RecoveryError::Store(error.to_string()))?;
    let source = cas.to_path_buf();
    let target = staging.join(BLOBS_DIR);
    let (blobs, excluded) = blocking(move || {
        let mut blobs: Vec<BlobEntry> = Vec::new();
        let mut excluded: Vec<Excluded> = Vec::new();
        for blob in listed {
            let digest = blob.digest.to_string();
            let hex = blob.digest.value.as_str();
            match copy_verified(&blob_file(&source, hex), &blob_file(&target, hex), hex) {
                Ok(length) => blobs.push(BlobEntry { digest, length }),
                // Se borro mientras se copiaba: ya no es contenido.
                Err(CopyError::Missing) => {}
                Err(CopyError::Mismatch { found }) => excluded.push(Excluded {
                    digest,
                    reason: format!(
                        "the bytes have the digest sha256:{found}, they were not copied"
                    ),
                }),
                Err(CopyError::Io(error)) => return Err(error.into()),
            }
        }
        blobs.sort_by(|a, b| a.digest.cmp(&b.digest));
        excluded.sort_by(|a, b| a.digest.cmp(&b.digest));
        Ok((blobs, excluded))
    })
    .await?;
    let blobs_ms = blobs_started.elapsed().as_millis() as u64;

    // 3. El manifiesto, al final, y la publicacion atomica.
    let manifest = BackupManifest {
        format: FORMAT,
        service_version: env!("CARGO_PKG_VERSION").to_string(),
        namespace: namespace.to_string(),
        database: database.to_string(),
        started_at,
        metadata_snapshot_at,
        finished_at: Utc::now(),
        metadata: MetadataEntry {
            file: METADATA_FILE.to_string(),
            sha256,
            bytes,
        },
        blobs,
        excluded,
    };
    let staging = staging.to_path_buf();
    let destination = dest.to_path_buf();
    let to_write = manifest.clone();
    blocking(move || {
        let text = serde_json::to_vec_pretty(&to_write)
            .map_err(|error| RecoveryError::Io(error.to_string()))?;
        let mut file = std::fs::File::create(staging.join(MANIFEST_FILE))?;
        file.write_all(&text)?;
        file.sync_all()?;
        std::fs::rename(&staging, &destination)?;
        // Que el renombrado sobreviva a un corte: se sincroniza el padre.
        if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty())
            && let Ok(directory) = std::fs::File::open(parent)
        {
            let _ = directory.sync_all();
        }
        Ok(())
    })
    .await?;

    Ok(BackupReport {
        path: dest.to_path_buf(),
        manifest,
        metadata_ms,
        blobs_ms,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// El resultado de comprobar una copia sin restaurarla.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyOutcome {
    /// Lo que la copia dice de si misma.
    pub manifest: BackupManifest,
    /// Lo que no cuadra. Vacio si la copia es integra.
    pub problems: Vec<String>,
    /// Cuanto tardo, en milisegundos.
    pub elapsed_ms: u64,
}

impl VerifyOutcome {
    /// Si la copia es integra.
    pub fn is_sound(&self) -> bool {
        self.problems.is_empty()
    }
}

fn read_manifest(backup: &Path) -> Result<BackupManifest, RecoveryError> {
    let text = std::fs::read(backup.join(MANIFEST_FILE)).map_err(|error| {
        RecoveryError::Unsound(format!(
            "{} cannot be read ({error}): the backup is not finished or is not a backup",
            MANIFEST_FILE
        ))
    })?;
    let manifest: BackupManifest = serde_json::from_slice(&text)
        .map_err(|error| RecoveryError::Unsound(format!("the manifest is not valid: {error}")))?;
    manifest.check_restorable()?;
    Ok(manifest)
}

/// Comprueba una copia entera: el hash de los metadatos y el digest, la
/// longitud y la presencia de **cada** blob. No restaura nada.
pub async fn inspect(backup: &Path) -> Result<VerifyOutcome, RecoveryError> {
    let backup = backup.to_path_buf();
    blocking(move || {
        let started = Instant::now();
        let manifest = read_manifest(&backup)?;
        let mut problems = Vec::new();
        match hash_file(&backup.join(&manifest.metadata.file)) {
            Ok((hash, bytes)) => {
                if hash != manifest.metadata.sha256 || bytes != manifest.metadata.bytes {
                    problems.push(format!(
                        "the metadata does not match its manifest (sha256 {hash}, {bytes} bytes)"
                    ));
                }
            }
            Err(error) => problems.push(format!("the metadata cannot be read: {error}")),
        }
        let root = backup.join(BLOBS_DIR);
        for blob in &manifest.blobs {
            let hex = hex_of(&blob.digest).expect("a restorable manifest has well-formed digests");
            match hash_file(&blob_file(&root, hex)) {
                Ok((hash, length)) => {
                    if hash != hex {
                        problems.push(format!("{} has the bytes of sha256:{hash}", blob.digest));
                    } else if length != blob.length {
                        problems.push(format!(
                            "{} is {length} bytes long, the manifest says {}",
                            blob.digest, blob.length
                        ));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    problems.push(format!("{} is missing from the backup", blob.digest));
                }
                Err(error) => problems.push(format!("{} cannot be read: {error}", blob.digest)),
            }
        }
        Ok(VerifyOutcome {
            manifest,
            problems,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    })
    .await
}

/// Como [`inspect`], pero una copia que no es integra es un error.
pub async fn verify(backup: &Path) -> Result<BackupManifest, RecoveryError> {
    let outcome = inspect(backup).await?;
    if outcome.is_sound() {
        return Ok(outcome.manifest);
    }
    const SHOWN: usize = 10;
    let total = outcome.problems.len();
    let mut message = outcome
        .problems
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if total > SHOWN {
        message.push_str(&format!("; and {} more", total - SHOWN));
    }
    Err(RecoveryError::Unsound(message))
}

/// Lo que hizo una restauracion.
#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    /// Los blobs que volvieron al almacen.
    pub blobs_restored: u64,
    /// Los bytes de esos blobs.
    pub bytes_restored: u64,
    /// El scrub que se hizo al terminar: lo restaurado se comprueba.
    pub scrub: ScrubReport,
    /// Lo que tardo, en milisegundos. Es el RTO de esta restauracion.
    pub elapsed_ms: u64,
}

/// Si la base no tiene nada definido: ni tablas, ni funciones, ni
/// parametros, ni accesos. Una respuesta que no se entiende no es «vacia»:
/// falla del lado seguro y la restauracion se niega.
async fn database_is_empty(db: &Surreal<Db>) -> Result<bool, RecoveryError> {
    let mut response = db
        .query("INFO FOR DB")
        .await
        .map_err(|error| RecoveryError::Metadata(error.to_string()))?;
    let info: Option<serde_json::Value> = response
        .take(0)
        .map_err(|error| RecoveryError::Metadata(error.to_string()))?;
    let Some(info) = info.as_ref().and_then(|info| info.as_object()) else {
        return Ok(false);
    };
    // `tables` tiene que estar y ser un objeto; lo demas, si esta, vacio.
    if !info.get("tables").is_some_and(|tables| tables.is_object()) {
        return Ok(false);
    }
    let empty = |value: &serde_json::Value| match value {
        serde_json::Value::Object(map) => map.is_empty(),
        serde_json::Value::Array(list) => list.is_empty(),
        serde_json::Value::Null => true,
        _ => false,
    };
    Ok(["tables", "functions", "params", "accesses", "analyzers"]
        .iter()
        .all(|key| info.get(*key).is_none_or(empty)))
}

/// Restaura una copia en una instalacion **limpia**.
///
/// Antes de escribir nada comprueba la copia entera y que el destino --la
/// base y el almacen-- este vacio. Despues importa los metadatos, devuelve
/// los blobs con el hash a la vista y termina con un scrub.
pub async fn restore(
    db: &Surreal<Db>,
    backup: &Path,
    cas: &Path,
    grace: Duration,
) -> Result<RestoreReport, RecoveryError> {
    let started = Instant::now();
    let manifest = verify(backup).await?;

    if cas.exists() {
        let mut entries = std::fs::read_dir(cas)?;
        if entries.next().is_some() {
            return Err(RecoveryError::NotClean(format!(
                "the content store {} is not empty",
                cas.display()
            )));
        }
    }
    if !database_is_empty(db).await? {
        return Err(RecoveryError::NotClean(
            "the database already has tables".to_string(),
        ));
    }

    db.import(backup.join(&manifest.metadata.file))
        .await
        .map_err(|error| RecoveryError::Metadata(error.to_string()))?;

    std::fs::create_dir_all(cas)?;
    let source = backup.join(BLOBS_DIR);
    let target = cas.to_path_buf();
    let blobs = manifest.blobs.clone();
    let (blobs_restored, bytes_restored) = blocking(move || {
        let mut bytes = 0u64;
        for blob in &blobs {
            let hex = hex_of(&blob.digest).expect("a verified manifest has well-formed digests");
            match copy_verified(&blob_file(&source, hex), &blob_file(&target, hex), hex) {
                Ok(length) => bytes += length,
                Err(CopyError::Missing) => {
                    return Err(RecoveryError::Unsound(format!(
                        "{} disappeared from the backup while restoring",
                        blob.digest
                    )));
                }
                Err(CopyError::Mismatch { found }) => {
                    return Err(RecoveryError::Unsound(format!(
                        "{} changed while restoring: it has the bytes of sha256:{found}",
                        blob.digest
                    )));
                }
                Err(CopyError::Io(error)) => return Err(error.into()),
            }
        }
        Ok((blobs.len() as u64, bytes))
    })
    .await?;

    let scrub = scrubber_for(db, cas, grace).await?.run().await?;
    Ok(RestoreReport {
        blobs_restored,
        bytes_restored,
        scrub,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// El servicio de scrub sobre una base abierta y un almacen.
pub async fn scrubber_for(
    db: &Surreal<Db>,
    cas: &Path,
    grace: Duration,
) -> Result<Scrubber, RecoveryError> {
    let catalog = Arc::new(hodei_catalog_storage::SurrealCatalogStore::new(db.clone()));
    catalog
        .initialize()
        .await
        .map_err(|error| RecoveryError::Store(error.to_string()))?;
    let store: Arc<dyn BlobStore> =
        Arc::new(FileBlobStore::new(cas).map_err(|error| RecoveryError::Store(error.to_string()))?);
    let ledger: Arc<dyn hodei_catalog::ContentLedger> = catalog;
    Scrubber::new(ledger, store, grace)
}

/// Lo que hizo una reparacion.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RepairReport {
    /// Los blobs a los que se devolvieron los bytes desde la copia.
    pub restored: Vec<String>,
    /// Los ficheros danados que se apartaron, no se borraron.
    pub quarantined: Vec<PathBuf>,
    /// Los blobs que la copia no tiene: nadie los puede reparar desde aqui.
    pub not_in_backup: Vec<String>,
    /// Los blobs que la copia tiene pero con otros bytes: tampoco valen.
    pub backup_damaged: Vec<String>,
}

/// Repara los hallazgos de dano de un scrub con bytes de una copia.
///
/// **Nunca inventa bytes**: un blob solo se sustituye si la copia tiene un
/// fichero cuyo hash es exactamente el digest que se busca. Un blob corrupto
/// se aparta a `quarantine/` antes de sustituirlo, no se borra. Los huerfanos
/// no se tocan: son de la recoleccion de basura.
pub async fn repair_from_backup(
    cas: &Path,
    backup: &Path,
    report: &ScrubReport,
) -> Result<RepairReport, RecoveryError> {
    let cas = cas.to_path_buf();
    let source = backup.join(BLOBS_DIR);
    let wanted: Vec<(String, bool)> = report
        .findings
        .iter()
        .filter(|finding| finding.is_damage())
        .map(|finding| {
            (
                finding.digest().to_string(),
                matches!(finding, Finding::Corrupt { .. }),
            )
        })
        .collect();
    blocking(move || {
        let mut outcome = RepairReport::default();
        for (digest, corrupt) in wanted {
            let Some(hex) = hex_of(&digest) else {
                continue;
            };
            let from = blob_file(&source, hex);
            match hash_file(&from) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    outcome.not_in_backup.push(digest);
                    continue;
                }
                Err(error) => return Err(error.into()),
                Ok((hash, _)) if hash != hex => {
                    outcome.backup_damaged.push(digest);
                    continue;
                }
                Ok(_) => {}
            }
            let to = blob_file(&cas, hex);
            if corrupt && to.exists() {
                let apart = cas.join(QUARANTINE_DIR);
                std::fs::create_dir_all(&apart)?;
                let name = format!("{hex}.{}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
                let destination = apart.join(name);
                std::fs::rename(&to, &destination)?;
                outcome.quarantined.push(destination);
            }
            match copy_verified(&from, &to, hex) {
                Ok(_) => outcome.restored.push(digest),
                Err(CopyError::Missing) => outcome.not_in_backup.push(digest),
                Err(CopyError::Mismatch { .. }) => outcome.backup_damaged.push(digest),
                Err(CopyError::Io(error)) => return Err(error.into()),
            }
        }
        Ok(outcome)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_verified_only_leaves_a_destination_with_the_expected_hash() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::write(&source, b"hello").unwrap();
        let (hex, _) = hash_file(&source).unwrap();

        let good = dir.path().join("a/b/good");
        assert_eq!(copy_verified(&source, &good, &hex).unwrap(), 5);
        assert_eq!(std::fs::read(&good).unwrap(), b"hello");

        let bad = dir.path().join("a/b/bad");
        let wrong = "0".repeat(64);
        assert!(matches!(
            copy_verified(&source, &bad, &wrong),
            Err(CopyError::Mismatch { .. })
        ));
        assert!(!bad.exists(), "a mismatching copy leaves no destination");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("a/b"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "no partial file is left: {leftovers:?}");
    }

    #[test]
    fn copy_verified_reports_a_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let result = copy_verified(
            &dir.path().join("nothing"),
            &dir.path().join("x"),
            &"0".repeat(64),
        );
        assert!(matches!(result, Err(CopyError::Missing)));
    }
}
