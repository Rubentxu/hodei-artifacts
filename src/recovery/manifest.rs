//! El manifiesto de una copia: lo que la copia dice de si misma.
//!
//! Es lo ultimo que se escribe, y lo que permite comprobar la copia entera
//! sin restaurarla: el hash de los metadatos, y el digest y la longitud de
//! cada blob. Una copia sin manifiesto no esta terminada y no se restaura.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::RecoveryError;

/// La version del formato de la copia.
///
/// Sube cuando un formato antiguo ya no se puede leer. Una copia con un
/// formato que este binario no entiende se **rechaza** con un mensaje, en
/// lugar de restaurarse a medias.
pub const FORMAT: u32 = 1;

/// El nombre del manifiesto dentro de la copia.
pub const MANIFEST_FILE: &str = "manifest.json";

/// El fichero de metadatos (SurrealQL) dentro de la copia.
pub const METADATA_FILE: &str = "metadata.surql";

/// El directorio de blobs dentro de la copia, con el mismo reparto que el
/// almacen: `<ab>/<cd>/<digest hex>`.
pub const BLOBS_DIR: &str = "blobs";

/// Los metadatos exportados.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataEntry {
    /// El fichero, relativo a la copia.
    pub file: String,
    /// El SHA-256 del fichero, en hexadecimal.
    pub sha256: String,
    /// Los bytes que ocupa.
    pub bytes: u64,
}

/// Un blob de la copia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobEntry {
    /// El contenido, como `sha256:<hex>`.
    pub digest: String,
    /// Los bytes que ocupa.
    pub length: u64,
}

/// Un blob que existia y no se copio, y por que.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Excluded {
    /// El contenido, como `sha256:<hex>`.
    pub digest: String,
    /// Por que no se copio.
    pub reason: String,
}

/// Lo que una copia dice de si misma.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    /// La version del formato.
    pub format: u32,
    /// La version del servicio que la hizo.
    pub service_version: String,
    /// El namespace de SurrealDB.
    pub namespace: String,
    /// La base de SurrealDB.
    pub database: String,
    /// Cuando empezo la copia.
    pub started_at: DateTime<Utc>,
    /// El instante del que son los metadatos: todo lo confirmado antes de
    /// este instante esta en la copia, y nada de lo confirmado despues.
    ///
    /// Es la base del RPO: lo que se escribio entre este instante y un
    /// fallo posterior se pierde.
    pub metadata_snapshot_at: DateTime<Utc>,
    /// Cuando termino la copia.
    pub finished_at: DateTime<Utc>,
    /// Los metadatos.
    pub metadata: MetadataEntry,
    /// Los blobs copiados, ordenados por digest.
    pub blobs: Vec<BlobEntry>,
    /// Los blobs que no se copiaron porque ya estaban danados.
    #[serde(default)]
    pub excluded: Vec<Excluded>,
}

impl BackupManifest {
    /// Los bytes de blobs que lleva la copia.
    pub fn blob_bytes(&self) -> u64 {
        self.blobs.iter().map(|blob| blob.length).sum()
    }

    /// Comprueba que la copia se puede restaurar con este binario.
    ///
    /// No toca el disco: es la parte pura. Rechaza un formato que no se
    /// entiende y todo lo que, de aceptarse, dejaria que un manifiesto
    /// hostil escribiera fuera de la copia o del almacen.
    pub fn check_restorable(&self) -> Result<(), RecoveryError> {
        if self.format != FORMAT {
            return Err(RecoveryError::Unsound(format!(
                "the backup has format {} and this build understands format {FORMAT}",
                self.format
            )));
        }
        if self.metadata.file != METADATA_FILE {
            return Err(RecoveryError::Unsound(format!(
                "the metadata file must be {METADATA_FILE}, not {}",
                self.metadata.file
            )));
        }
        if !is_hex(&self.metadata.sha256, 64) {
            return Err(RecoveryError::Unsound(
                "the hash of the metadata is not a sha256".to_string(),
            ));
        }
        let mut previous: Option<&str> = None;
        for blob in &self.blobs {
            if hex_of(&blob.digest).is_none() {
                return Err(RecoveryError::Unsound(format!(
                    "the digest {} is not `sha256:` followed by 64 lowercase hexadecimal digits",
                    blob.digest
                )));
            }
            if previous.is_some_and(|last| last >= blob.digest.as_str()) {
                return Err(RecoveryError::Unsound(format!(
                    "the blob {} is repeated or out of order",
                    blob.digest
                )));
            }
            previous = Some(&blob.digest);
        }
        Ok(())
    }
}

/// El hexadecimal de un digest `sha256:<hex>` bien formado.
pub fn hex_of(digest: &str) -> Option<&str> {
    let hex = digest.strip_prefix("sha256:")?;
    is_hex(hex, 64).then_some(hex)
}

fn is_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: char) -> String {
        format!("sha256:{}", seed.to_string().repeat(64))
    }

    fn manifest() -> BackupManifest {
        let now = Utc::now();
        BackupManifest {
            format: FORMAT,
            service_version: "0.1.0".into(),
            namespace: "hodei".into(),
            database: "artifacts".into(),
            started_at: now,
            metadata_snapshot_at: now,
            finished_at: now,
            metadata: MetadataEntry {
                file: METADATA_FILE.into(),
                sha256: "a".repeat(64),
                bytes: 10,
            },
            blobs: vec![
                BlobEntry {
                    digest: digest('a'),
                    length: 3,
                },
                BlobEntry {
                    digest: digest('b'),
                    length: 4,
                },
            ],
            excluded: vec![],
        }
    }

    #[test]
    fn a_well_formed_manifest_is_restorable() {
        assert!(manifest().check_restorable().is_ok());
        assert_eq!(manifest().blob_bytes(), 7);
    }

    #[test]
    fn a_format_this_build_does_not_understand_is_refused_with_both_numbers() {
        let mut other = manifest();
        other.format = FORMAT + 1;
        let error = other.check_restorable().unwrap_err().to_string();
        assert!(error.contains(&format!("format {}", FORMAT + 1)), "{error}");
        assert!(error.contains(&format!("format {FORMAT}")), "{error}");
    }

    #[test]
    fn a_digest_that_could_escape_the_store_is_refused() {
        for hostile in ["sha256:../../etc/passwd", "sha256:AB", "md5:abc", "../x"] {
            let mut other = manifest();
            other.blobs[0].digest = hostile.into();
            assert!(
                other.check_restorable().is_err(),
                "{hostile} must be refused"
            );
        }
    }

    #[test]
    fn a_repeated_or_unordered_blob_is_refused() {
        let mut repeated = manifest();
        repeated.blobs[1].digest = digest('a');
        assert!(repeated.check_restorable().is_err());
        let mut unordered = manifest();
        unordered.blobs.reverse();
        assert!(unordered.check_restorable().is_err());
    }

    #[test]
    fn the_metadata_file_cannot_point_somewhere_else() {
        let mut other = manifest();
        other.metadata.file = "../outside.surql".into();
        assert!(other.check_restorable().is_err());
    }

    #[test]
    fn the_manifest_survives_a_round_trip_through_json() {
        let original = manifest();
        let text = serde_json::to_string_pretty(&original).unwrap();
        let back: BackupManifest = serde_json::from_str(&text).unwrap();
        assert_eq!(back, original);
    }
}
