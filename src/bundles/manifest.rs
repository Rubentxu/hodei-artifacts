//! El manifiesto de un paquete de resolucion: lo que el paquete dice de si
//! mismo.
//!
//! Es lo ultimo que se escribe al exportar y lo primero que se lee al
//! importar. Como el de una copia, su comprobacion es **pura**: rechaza el
//! paquete entero antes de tocar nada si un manifiesto hostil o roto podria
//! hacer importar contenido sin respaldo, sobrescribir una ruta ajena o
//! escribir fuera del paquete. La regla esta tomada del manifiesto de
//! copias: un paquete que este binario no entiende se rechaza con un
//! mensaje, no se importa a medias.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use hodei_catalog::{ProvenanceReceipt, ReceiptOutcome};
use serde::{Deserialize, Serialize};

use super::BundleError;
use crate::recovery::manifest::hex_of;

/// La version del formato del paquete.
///
/// Sube cuando un paquete antiguo ya no se puede leer. Uno con un formato
/// que este binario no entiende se rechaza, no se importa a medias.
pub const FORMAT: u32 = 1;

/// El nombre del manifiesto dentro del paquete.
pub const MANIFEST_NAME: &str = "bundle.json";

/// El directorio de blobs dentro del paquete, con el mismo reparto que el
/// almacen: `<ab>/<cd>/<hex>`.
pub const BLOBS_DIRECTORY: &str = "blobs";

/// La cantidad maxima de rutas que un paquete puede declarar.
///
/// Es la cota honesta de la lectura de exportacion: un paquete con mas
/// entradas que las que el catalogo puede enumerar de una vez no pudo
/// exportarse completo, y no se importa lo que parece completo y no lo es.
pub const MAX_BUNDLE_ASSETS: usize = hodei_catalog::MAX_ASSETS_UNDER;

/// De donde salio el paquete.
///
/// No lleva la URL del upstream: una URL de configuracion puede llevar
/// credenciales en la parte de usuario, y un paquete no sale de casa con
/// secretos. La URL por archivo vive, sanitizada, en los recibos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// La cuenta del repositorio exportado.
    pub account: String,
    /// La clave del repositorio proxy.
    pub repository: String,
    /// El identificador del upstream con el que se resolvió.
    pub upstream_id: String,
    /// La version de la configuracion con la que se resolvio.
    pub config_version: u64,
}

/// La decision que respaldaba un digest al exportar.
///
/// Es **evidencia, no permiso**: al importar, el destino juzga la confianza
/// con sus propias concesiones. Se guarda para poder responder despues por
/// que el paquete traia lo que traia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionEvidence {
    /// La promocion que concedio el digest.
    pub decision_id: String,
    /// Quien promovio.
    pub actor: String,
    /// Cuando se concedio.
    pub granted_at: DateTime<Utc>,
}

/// Una ruta resuelta del paquete, con su procedencia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    /// La ruta dentro del repositorio.
    pub asset_path: String,
    /// El nombre del paquete tal y como lo publico la resolucion.
    pub package: String,
    /// La version publicada en la ruta.
    pub version: String,
    /// El contenido de la ruta, como `sha256:<hex>`.
    pub digest: String,
    /// Los bytes del contenido.
    pub size_bytes: u64,
    /// El recibo de procedencia que confirmaba la ruta al exportar.
    pub receipt: ProvenanceReceipt,
    /// La decision activa sobre ese digest al exportar, si existia.
    #[serde(default)]
    pub decision: Option<DecisionEvidence>,
}

/// Un blob del paquete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobEntry {
    /// El contenido, como `sha256:<hex>`.
    pub digest: String,
    /// Los bytes que ocupa.
    pub length: u64,
}

/// Una ruta que el paquete no trae, y por que.
///
/// Queda escrita para que la ausencia sea **explicable**: quien importa
/// ve que el origen tenia esa ruta y por que no vino.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withheld {
    /// La ruta retenida en el origen.
    pub asset_path: String,
    /// Por que no se exporto.
    pub reason: String,
}

/// Lo que un paquete dice de si mismo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    /// La version del formato.
    pub format: u32,
    /// Cuando se cerro el paquete.
    pub exported_at: DateTime<Utc>,
    /// De donde salio.
    pub origin: Origin,
    /// Las rutas resueltas, ordenadas por ruta.
    pub assets: Vec<AssetEntry>,
    /// Los blobs, ordenados por digest y sin repetirse.
    pub blobs: Vec<BlobEntry>,
    /// Las rutas que el origen tenia y el paquete no trae.
    pub withheld: Vec<Withheld>,
}

impl BundleManifest {
    /// Los bytes de blobs que lleva el paquete.
    pub fn blob_bytes(&self) -> u64 {
        self.blobs.iter().map(|blob| blob.length).sum()
    }

    /// Comprueba que el paquete se puede importar con este binario.
    ///
    /// No toca el disco: es la parte pura. Rechaza lo que, de aceptarse,
    /// dejaria que un paquete hostil hiciera visible contenido sin respaldo
    /// o escribiera fuera de sus propias reglas.
    pub fn check_importable(&self) -> Result<(), BundleError> {
        if self.format != FORMAT {
            return Err(BundleError::Unsound(format!(
                "the bundle has format {} and this build understands format {FORMAT}",
                self.format
            )));
        }
        for field in [
            ("account", &self.origin.account),
            ("repository", &self.origin.repository),
            ("upstream", &self.origin.upstream_id),
        ] {
            if field.1.is_empty() {
                return Err(BundleError::Unsound(format!(
                    "the origin {} of the bundle is empty",
                    field.0
                )));
            }
        }
        if self.assets.len() > MAX_BUNDLE_ASSETS {
            return Err(BundleError::Unsound(format!(
                "the bundle declares {} assets, more than the {} a single export can honestly \
                 enumerate",
                self.assets.len(),
                MAX_BUNDLE_ASSETS
            )));
        }

        let mut digests = Vec::with_capacity(self.assets.len());
        let mut previous: Option<&str> = None;
        for entry in &self.assets {
            check_path(&entry.asset_path)?;
            if previous.is_some_and(|last| last >= entry.asset_path.as_str()) {
                return Err(BundleError::Unsound(format!(
                    "the path {} is repeated or out of order",
                    entry.asset_path
                )));
            }
            previous = Some(&entry.asset_path);

            let hex = hex_of(&entry.digest).ok_or_else(|| {
                BundleError::Unsound(format!(
                    "the digest {} is not `sha256:` followed by 64 lowercase hexadecimal digits",
                    entry.digest
                ))
            })?;
            let receipt = &entry.receipt;
            if receipt.digest.value != hex || !receipt.digest.is_well_formed() {
                return Err(BundleError::Unsound(format!(
                    "the receipt of {} speaks of a different content than the entry: {}",
                    entry.asset_path, receipt.digest.value
                )));
            }
            if receipt.outcome == ReceiptOutcome::Quarantined {
                return Err(BundleError::Unsound(format!(
                    "the receipt of {} is a quarantine: those bytes were refused by the origin",
                    entry.asset_path
                )));
            }
            if receipt.account != self.origin.account
                || receipt.repository != self.origin.repository
                || receipt.asset_path != entry.asset_path
            {
                return Err(BundleError::Unsound(format!(
                    "the receipt of {} belongs to another path or repository",
                    entry.asset_path
                )));
            }
            if receipt.size_bytes != entry.size_bytes {
                return Err(BundleError::Unsound(format!(
                    "the receipt of {} disagrees about the size",
                    entry.asset_path
                )));
            }
            digests.push(entry.digest.as_str());
        }

        let mut previous: Option<&str> = None;
        let mut referenced = Vec::with_capacity(digests.len());
        for blob in &self.blobs {
            hex_of(&blob.digest).ok_or_else(|| {
                BundleError::Unsound(format!(
                    "the blob {} is not `sha256:` followed by 64 lowercase hexadecimal digits",
                    blob.digest
                ))
            })?;
            if previous.is_some_and(|last| last >= blob.digest.as_str()) {
                return Err(BundleError::Unsound(format!(
                    "the blob {} is repeated or out of order",
                    blob.digest
                )));
            }
            previous = Some(&blob.digest);
            // Un blob que no declara ninguna ruta no se importa: no hay a
            // que respaldarlo.
            match digests.iter().find(|digest| **digest == blob.digest) {
                None => {
                    return Err(BundleError::Unsound(format!(
                        "the blob {} belongs to no resolved path",
                        blob.digest
                    )));
                }
                // El tamano declarado tiene que coincidir con el de toda
                // ruta que lo comparte: un mismo contenido no puede medir
                // dos cosas.
                Some(_) => {
                    for entry in self
                        .assets
                        .iter()
                        .filter(|entry| entry.digest == blob.digest)
                    {
                        if entry.size_bytes != blob.length {
                            return Err(BundleError::Unsound(format!(
                                "the blob {} says {} bytes and the path {} says {}",
                                blob.digest, blob.length, entry.asset_path, entry.size_bytes
                            )));
                        }
                    }
                    referenced.push(blob.digest.as_str());
                }
            }
        }
        for digest in &digests {
            if !referenced.contains(digest) {
                return Err(BundleError::Unsound(format!(
                    "the path content {digest} has no blob in the bundle"
                )));
            }
        }

        let mut previous: Option<&str> = None;
        for withheld in &self.withheld {
            check_path(&withheld.asset_path)?;
            if withheld.reason.is_empty() {
                return Err(BundleError::Unsound(format!(
                    "the withheld path {} has no reason",
                    withheld.asset_path
                )));
            }
            if previous.is_some_and(|last| last >= withheld.asset_path.as_str()) {
                return Err(BundleError::Unsound(format!(
                    "the withheld path {} is repeated or out of order",
                    withheld.asset_path
                )));
            }
            previous = Some(&withheld.asset_path);
            if self
                .assets
                .iter()
                .any(|entry| entry.asset_path == withheld.asset_path)
            {
                return Err(BundleError::Unsound(format!(
                    "the path {} is exported and withheld at once",
                    withheld.asset_path
                )));
            }
        }
        Ok(())
    }

    /// La ruta del fichero de un blob dentro del paquete.
    ///
    /// Solo se llama con digests de un manifiesto ya comprobado.
    pub fn blob_file(digest: &str) -> PathBuf {
        let hex = hex_of(digest).expect("a blob path is built only on a checked manifest");
        PathBuf::from(BLOBS_DIRECTORY)
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(hex)
    }
}

/// Una ruta que no puede escribir fuera de su repositorio al importar.
fn check_path(path: &str) -> Result<(), BundleError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|segment| segment == ".." || segment.is_empty())
        || path.chars().any(|c| c.is_control())
    {
        return Err(BundleError::Unsound(format!(
            "the path {path:?} is not a repository-relative path"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_evidence::ContentDigest;

    fn digest(byte: char) -> ContentDigest {
        ContentDigest::of_bytes(byte.to_string().as_bytes())
    }

    fn receipt(path: &str, content: ContentDigest) -> ProvenanceReceipt {
        ProvenanceReceipt {
            id: format!("prv-{path}"),
            account: "acme".into(),
            repository: "mirror".into(),
            asset_path: path.into(),
            upstream_id: "central".into(),
            source_url: format!("https://repo.example.com/{path}"),
            config_version: 3,
            size_bytes: 1,
            checksum: hodei_catalog::ChecksumEvidence::ComputedOnly,
            status: 200,
            redirects: 0,
            etag: None,
            last_modified: None,
            outcome: ReceiptOutcome::Fetched,
            actor: "u".into(),
            correlation_id: "c".into(),
            fetched_at: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("time"),
            digest: content,
        }
    }

    fn entry(path: &str, byte: char) -> AssetEntry {
        let content = digest(byte);
        AssetEntry {
            asset_path: path.into(),
            package: path.into(),
            version: content.value.clone(),
            digest: format!("sha256:{}", content.value),
            size_bytes: 1,
            receipt: receipt(path, content),
            decision: None,
        }
    }

    fn manifest() -> BundleManifest {
        let entries = vec![entry("a/one", 'a'), entry("b/two", 'b')];
        let mut blobs: Vec<BlobEntry> = entries
            .iter()
            .map(|entry| BlobEntry {
                digest: entry.digest.clone(),
                length: entry.size_bytes,
            })
            .collect();
        blobs.sort_by(|a, b| a.digest.cmp(&b.digest));
        BundleManifest {
            format: FORMAT,
            exported_at: DateTime::<Utc>::from_timestamp(1_700_000_100, 0).expect("time"),
            origin: Origin {
                account: "acme".into(),
                repository: "mirror".into(),
                upstream_id: "central".into(),
                config_version: 3,
            },
            assets: entries,
            blobs,
            withheld: Vec::new(),
        }
    }

    #[test]
    fn a_well_formed_bundle_is_importable() {
        let bundle = manifest();
        bundle.check_importable().expect("sound");

        // Y el viaje por JSON no cambia nada: el manifiesto canonico es lo
        // que se escribe y lo que se lee.
        let text = serde_json::to_string(&bundle).expect("writes");
        let read: BundleManifest = serde_json::from_str(&text).expect("reads");
        assert_eq!(read, bundle);
        read.check_importable().expect("still sound");
    }

    #[test]
    fn a_format_this_build_does_not_understand_is_refused() {
        let mut bundle = manifest();
        bundle.format = FORMAT + 1;
        let error = bundle.check_importable().expect_err("refused");
        assert!(
            error
                .to_string()
                .contains(&format!("format {}", FORMAT + 1)),
            "{error}"
        );
    }

    #[test]
    fn hostile_paths_never_come_in() {
        for case in ["", "/abs", "a/../b", "..", "a\\b", "a//b", "a\nb"] {
            let mut bundle = manifest();
            bundle.assets[0].asset_path = case.to_string();
            bundle.assets[0].receipt.asset_path = case.to_string();
            assert!(
                bundle.check_importable().is_err(),
                "{case:?} should be refused"
            );
        }
    }

    #[test]
    fn paths_must_be_ordered_and_never_repeated() {
        let mut bundle = manifest();
        bundle.assets.reverse();
        assert!(bundle.check_importable().is_err());
        let mut bundle = manifest();
        bundle.assets[1] = bundle.assets[0].clone();
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn a_receipt_of_other_than_the_entry_is_not_provenance() {
        // El recibo habla de otro contenido.
        let mut bundle = manifest();
        bundle.assets[0].receipt.digest = digest('z');
        assert!(bundle.check_importable().is_err());

        // El recibo es de otra cuenta o de otro repositorio.
        let mut bundle = manifest();
        bundle.assets[0].receipt.account = "someone-else".into();
        assert!(bundle.check_importable().is_err());

        // El recibo cuenta una cuarentena: esos bytes fueron rechazados en
        // el origen y no viajan como resolucion.
        let mut bundle = manifest();
        bundle.assets[0].receipt.outcome = ReceiptOutcome::Quarantined;
        assert!(bundle.check_importable().is_err());

        // Y un tamano que no cuadra con su recibo.
        let mut bundle = manifest();
        bundle.assets[0].size_bytes += 1;
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn blobs_and_paths_have_to_cover_each_exactly() {
        // Falta el blob de una ruta.
        let mut bundle = manifest();
        bundle.blobs.remove(0);
        assert!(bundle.check_importable().is_err());

        // Sobra un blob que no es de ninguna ruta.
        let mut bundle = manifest();
        bundle.blobs.push(BlobEntry {
            digest: format!("sha256:{}", digest('q').value),
            length: 1,
        });
        bundle.blobs.sort_by(|a, b| a.digest.cmp(&b.digest));
        assert!(bundle.check_importable().is_err());

        // El blob dice unos bytes que la ruta no dice.
        let mut bundle = manifest();
        bundle.blobs[0].length += 1;
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn the_same_content_under_two_paths_is_one_blob_and_two_entries() {
        let mut bundle = manifest();
        let content = digest('a');
        bundle.assets[1] = AssetEntry {
            asset_path: "c/same".into(),
            package: "c/same".into(),
            version: content.value.clone(),
            digest: format!("sha256:{}", content.value),
            size_bytes: 1,
            receipt: receipt("c/same", content),
            decision: None,
        };
        bundle
            .blobs
            .retain(|blob| blob.digest == bundle.assets[0].digest);
        bundle.check_importable().expect("sound");
    }

    #[test]
    fn a_withheld_path_is_explained_and_never_also_exported() {
        let mut bundle = manifest();
        bundle.withheld.push(Withheld {
            asset_path: "d/absent".into(),
            reason: "the content was damaged in the origin".into(),
        });
        bundle.check_importable().expect("sound");

        bundle.withheld[0].reason = String::new();
        assert!(bundle.check_importable().is_err());

        let mut bundle = manifest();
        bundle.withheld.push(Withheld {
            asset_path: "a/one".into(),
            reason: "exported and withheld at once".into(),
        });
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn a_bundle_bigger_than_an_honest_export_is_refused() {
        let mut bundle = manifest();
        bundle.assets = (0..=MAX_BUNDLE_ASSETS)
            .map(|index| entry(&format!("many/{index:07}"), 'a'))
            .collect();
        bundle.blobs = vec![BlobEntry {
            digest: bundle.assets[0].digest.clone(),
            length: 1,
        }];
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn blob_files_live_in_the_same_layout_as_the_store() {
        let hex = digest('a').value;
        assert_eq!(
            BundleManifest::blob_file(&format!("sha256:{hex}")),
            PathBuf::from("blobs")
                .join(&hex[0..2])
                .join(&hex[2..4])
                .join(hex)
        );
    }

    #[test]
    fn the_well_formed_digest_of_a_receipt_is_its_own() {
        // Un digest del recibo mal formado (aunque la entrada cuadre) no
        // respaldo nada.
        let mut bundle = manifest();
        bundle.assets[0].receipt.digest.value = "../../etc/passwd".into();
        bundle.assets[0].digest = "sha256:../../etc/passwd".into();
        assert!(bundle.check_importable().is_err());
    }
}
