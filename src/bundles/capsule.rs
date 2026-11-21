//! La capsula de release como paquete OCI: el bundle, vestido de estandar.
//!
//! Un paquete de resolucion ya dice todo lo que hace falta decir —digests,
//! recibos, decisiones y ausencias explicitas— pero lo dice en un
//! directorio propio. Exportarlo como **OCI image layout** no le anade
//! verdad: le quita exotismo. El layout que escribe este modulo lo lee
//! `oras` o `skopeo` sin saber nada de Hodei, y dentro de el hay dos
//! capas con nombre: `release.json`, que son los bytes **exactos** del
//! manifiesto del bundle, y `capsule.json`, que es el estado de confianza
//! del paquete en el momento de exportarlo.
//!
//! La capsula de confianza se deriva del propio paquete, no de la fe:
//! cada ruta lleva el resultado de su recibo y la decision que la
//! respaldaba — o `null` si no la habia. Un `null` exportado como `null`
//! se importa como `null`: aqui nadie convierte una ausencia en un
//! veredificado, porque la autoridad la tiene el destino con sus propias
//! concesiones (la regla del paquete, repetida en el manifiesto).
//!
//! La firma es detached OpenPGP sobre los bytes exactos del manifiesto
//! de la capsula, y viaja como un segundo manifiesto cuyo `subject` es el
//! primero: el patron de referrers que el registro ya sirve por HTTP.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::BundleError;
use super::manifest::{BundleManifest, DecisionEvidence, MANIFEST_NAME, Origin, Withheld};
use crate::openpgp::MetadataSigner;

pub const LAYOUT_MEDIA_TYPE: &str = "application/vnd.oci.layout.header.v1+json";
pub const LAYOUT_VERSION: &str = "1.0.0";
pub const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
pub const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
pub const CAPSULE_ARTIFACT_TYPE: &str = "application/vnd.hodei.release-capsule.v1";
pub const SIGNATURE_ARTIFACT_TYPE: &str = "application/vnd.hodei.openpgp-signature.v1";
pub const CONFIG_MEDIA_TYPE: &str = "application/vnd.hodei.release-capsule.config.v1+json";
pub const RELEASE_MEDIA_TYPE: &str = "application/vnd.hodei.release.v1+json";
pub const TRUST_MEDIA_TYPE: &str = "application/vnd.hodei.trust-state.v1+json";
pub const BLOB_MEDIA_TYPE: &str = "application/vnd.hodei.bundle-blob.v1";
pub const SIGNATURE_MEDIA_TYPE: &str = "application/vnd.hodei.openpgp-signature.v1+json";

pub const LAYOUT_FILE: &str = "oci-layout";
pub const INDEX_FILE: &str = "index.json";
pub const LAYOUT_BLOBS: &str = "blobs/sha256";

pub const CAPSULE_NAME: &str = "hodei.release-capsule";
pub const CAPSULE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Descriptor {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub digest: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageManifest {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    #[serde(rename = "artifactType")]
    pub artifact_type: String,
    pub config: Descriptor,
    pub layers: Vec<Descriptor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<Descriptor>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutIndex {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub manifests: Vec<Descriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutHeader {
    #[serde(rename = "imageLayoutVersion")]
    pub image_layout_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustEntry {
    pub asset_path: String,
    pub digest: String,
    pub receipt_outcome: String,
    #[serde(default)]
    pub decision: Option<DecisionEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustState {
    pub capsule: String,
    pub version: u32,
    pub exported_at: DateTime<Utc>,
    pub origin: Origin,
    pub entries: Vec<TrustEntry>,
    pub withheld: Vec<Withheld>,
}

impl TrustState {
    /// El estado que se deriva de un manifiesto de paquete ya comprobado:
    /// misma entrada, misma capsula. No pregunta a nadie porque no hay
    /// nadie — el paquete ya cuenta lo que sabia.
    pub fn of(manifest: &BundleManifest) -> Self {
        Self {
            capsule: CAPSULE_NAME.to_string(),
            version: CAPSULE_VERSION,
            exported_at: manifest.exported_at,
            origin: manifest.origin.clone(),
            entries: manifest
                .assets
                .iter()
                .map(|entry| TrustEntry {
                    asset_path: entry.asset_path.clone(),
                    digest: entry.digest.clone(),
                    receipt_outcome: entry.receipt.outcome.as_str().to_string(),
                    decision: entry.decision.clone(),
                })
                .collect(),
            withheld: manifest.withheld.clone(),
        }
    }
}

fn descriptor(media_type: &str, bytes: &[u8]) -> Descriptor {
    Descriptor {
        media_type: media_type.to_string(),
        digest: digest_of(bytes),
        size: bytes.len() as u64,
        annotations: BTreeMap::new(),
    }
}

fn annotation(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// La ruta de un blob dentro del layout, segun el estandar.
pub fn layout_blob_path(digest: &str) -> PathBuf {
    let hex = digest
        .strip_prefix("sha256:")
        .expect("a layout digest is always sha256-prefixed");
    PathBuf::from(LAYOUT_BLOBS).join(hex)
}

fn write_layout_blob(
    dir: &Path,
    media_type: &str,
    bytes: &[u8],
) -> Result<Descriptor, BundleError> {
    let desc = descriptor(media_type, bytes);
    let path = dir.join(layout_blob_path(&desc.digest));
    if path.exists() {
        let existing = std::fs::read(&path)?;
        if existing != bytes {
            return Err(unsound(format!(
                "the layout already holds different bytes under {}",
                desc.digest
            )));
        }
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
    }
    Ok(desc)
}

fn unsound(what: impl Into<String>) -> BundleError {
    BundleError::Unsound(what.into())
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, BundleError> {
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Los bytes de cada descriptor que un manifiesto declara, comprobados
/// contra su descriptor. Un blob que falta es un error; uno que no
/// cuadra, tambien.
pub(super) fn read_declared_blobs(
    dir: &Path,
    manifest: &ImageManifest,
) -> Result<BTreeMap<String, Vec<u8>>, BundleError> {
    let mut blobs = BTreeMap::new();
    let mut declare = |desc: &Descriptor| -> Result<(), BundleError> {
        blobs.insert(desc.digest.clone(), read_layout_blob(dir, desc)?);
        Ok(())
    };
    declare(&manifest.config)?;
    for layer in &manifest.layers {
        declare(layer)?;
    }
    if let Some(subject) = &manifest.subject {
        declare(subject)?;
    }
    Ok(blobs)
}

pub(super) fn read_layout_blob(dir: &Path, desc: &Descriptor) -> Result<Vec<u8>, BundleError> {
    let path = dir.join(layout_blob_path(&desc.digest));
    let bytes = std::fs::read(&path).map_err(|_| {
        unsound(format!(
            "the index announces {} but the blob is not there",
            desc.digest
        ))
    })?;
    if bytes.len() as u64 != desc.size {
        return Err(unsound(format!(
            "{} announces {} bytes but holds {}",
            desc.digest,
            desc.size,
            bytes.len()
        )));
    }
    if digest_of(&bytes) != desc.digest {
        return Err(unsound(format!(
            "{} is not the content its digest names",
            desc.digest
        )));
    }
    Ok(bytes)
}

/// Lo que [`export_capsule`] dejo en el disco: el layout, el digest de
/// la capsula y los bytes exactos que se firmaron.
#[derive(Debug)]
pub struct CapsuleExport {
    pub layout_dir: PathBuf,
    pub image_digest: String,
    pub signature_digest: String,
    pub signed_manifest: Vec<u8>,
}

/// Exporta el bundle ya escrito bajo `dir` como imagen OCI firmada en
/// `layout`: el layout es un subdirectorio del paquete, de modo que el
/// bundle sigue intacto y `oras push` recibe la carpeta OCI a pelo.
///
/// Comprueba cada blob del paquete contra su digest declarado antes de
/// copiarlo como capa: la capsula no puede contener nada que el propio
/// paquete no respalde.
pub fn export_capsule(
    dir: &Path,
    layout: &Path,
    signer: &MetadataSigner,
) -> Result<CapsuleExport, BundleError> {
    if layout.exists() {
        return Err(BundleError::Refused(format!(
            "{} already exists: the export never overwrites a capsule",
            layout.display()
        )));
    }
    std::fs::create_dir_all(layout.join(LAYOUT_BLOBS))?;

    // El manifiesto que se exporta son los bytes del `release.json` que
    // `export` escribio: la capsula contiene el paquete, no una copia
    // re-serializada que cambiaria de digest por el orden de un mapa.
    // Quien exporta acaba de escribir el paquete en `dir`, asi que los
    // blobs del almacen se leen de su propia copia del paquete.
    let manifest_bytes = std::fs::read(dir.join(MANIFEST_NAME))?;
    let manifest: BundleManifest = read_json(&dir.join(MANIFEST_NAME))?;
    manifest.check_importable()?;
    let mut content: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for blob in &manifest.blobs {
        let path = dir.join(BundleManifest::blob_file(&blob.digest));
        let bytes = std::fs::read(&path).map_err(|_| {
            unsound(format!(
                "the bundle announces {} but its file is not there",
                blob.digest
            ))
        })?;
        if digest_of(&bytes) != blob.digest || bytes.len() as u64 != blob.length {
            return Err(unsound(format!(
                "{} is not the content the bundle declared",
                blob.digest
            )));
        }
        content.insert(blob.digest.clone(), bytes);
    }

    let config = write_layout_blob(layout, CONFIG_MEDIA_TYPE, b"{}")?;

    let mut layers = Vec::new();
    for entry in &manifest.assets {
        let bytes = content
            .get(&entry.digest)
            .ok_or_else(|| unsound(format!("the bundle lost its own blob {}", entry.digest)))?;
        // La capa de contenido vive en su blob del layout, como exige el
        // estandar: el descriptor puede anunciar el mismo digest que el
        // paquete, pero sin bytes no hay capa que un cliente OCI lea.
        let actual = write_layout_blob(layout, BLOB_MEDIA_TYPE, bytes)?;
        if actual.digest != entry.digest || actual.size != entry.size_bytes {
            return Err(unsound(format!(
                "{} is named for {} but the bytes digest as {}",
                entry.asset_path, entry.digest, actual.digest
            )));
        }
        layers.push(Descriptor {
            media_type: BLOB_MEDIA_TYPE.to_string(),
            digest: entry.digest.clone(),
            size: entry.size_bytes,
            annotations: annotation(&[("org.opencontainers.image.title", &entry.asset_path)]),
        });
    }

    let release = write_layout_blob(layout, RELEASE_MEDIA_TYPE, &manifest_bytes)?;
    let capsule_json = serde_json::to_vec_pretty(&TrustState::of(&manifest))?;
    let capsule_layer = write_layout_blob(layout, TRUST_MEDIA_TYPE, &capsule_json)?;
    // Las capas de contenido primero; el manifiesto del bundle y la
    // capsula de confianza al final, con nombre.
    layers.push(release);
    layers.push(capsule_layer);

    let mut image_annotations = BTreeMap::new();
    image_annotations.insert(
        "org.opencontainers.image.created".to_string(),
        manifest.exported_at.to_rfc3339(),
    );
    image_annotations.insert(
        "org.opencontainers.image.description".to_string(),
        format!(
            "capsule of {} exported from {}/{}",
            manifest.origin.upstream_id, manifest.origin.account, manifest.origin.repository
        ),
    );
    image_annotations.insert(
        "hodei.capsule".to_string(),
        format!("{}:{}", CAPSULE_NAME, CAPSULE_VERSION),
    );
    image_annotations.insert(
        "hodei.capsule.entries".to_string(),
        manifest.assets.len().to_string(),
    );
    image_annotations.insert(
        "hodei.capsule.withheld".to_string(),
        manifest.withheld.len().to_string(),
    );

    let image = ImageManifest {
        schema_version: 2,
        media_type: MANIFEST_MEDIA_TYPE.to_string(),
        artifact_type: CAPSULE_ARTIFACT_TYPE.to_string(),
        config,
        layers,
        subject: None,
        annotations: image_annotations,
    };
    let image_bytes = serde_json::to_vec_pretty(&image)?;
    let image_digest = digest_of(&image_bytes);

    let signature = signer
        .sign_detached(&image_bytes)
        .map_err(|e| unsound(format!("the capsule cannot be signed: {e}")))?;
    let signature_json = serde_json::to_vec_pretty(&serde_json::json!({
        "format": "openpgp-detached",
        "signer": signer.fingerprint_hex(),
        "signature": String::from_utf8_lossy(&signature),
    }))?;
    let signature_layer = write_layout_blob(layout, SIGNATURE_MEDIA_TYPE, &signature_json)?;
    let signature_image = ImageManifest {
        schema_version: 2,
        media_type: MANIFEST_MEDIA_TYPE.to_string(),
        artifact_type: SIGNATURE_ARTIFACT_TYPE.to_string(),
        config: descriptor(CONFIG_MEDIA_TYPE, b"{}"),
        layers: vec![signature_layer],
        subject: Some(Descriptor {
            media_type: MANIFEST_MEDIA_TYPE.to_string(),
            digest: image_digest.clone(),
            size: image_bytes.len() as u64,
            annotations: BTreeMap::new(),
        }),
        annotations: annotation(&[(
            "org.opencontainers.image.created",
            &manifest.exported_at.to_rfc3339(),
        )]),
    };
    let signature_bytes = serde_json::to_vec_pretty(&signature_image)?;
    let signature_digest = digest_of(&signature_bytes);

    let index = LayoutIndex {
        schema_version: 2,
        media_type: INDEX_MEDIA_TYPE.to_string(),
        manifests: vec![
            Descriptor {
                media_type: MANIFEST_MEDIA_TYPE.to_string(),
                digest: image_digest.clone(),
                size: image_bytes.len() as u64,
                annotations: annotation(&[("org.opencontainers.image.ref.name", "latest")]),
            },
            Descriptor {
                media_type: MANIFEST_MEDIA_TYPE.to_string(),
                digest: signature_digest.clone(),
                size: signature_bytes.len() as u64,
                annotations: annotation(&[("org.opencontainers.image.ref.name", "latest.sig")]),
            },
        ],
    };
    std::fs::write(layout.join(INDEX_FILE), serde_json::to_vec_pretty(&index)?)?;
    std::fs::write(
        layout.join(LAYOUT_FILE),
        serde_json::to_vec_pretty(&LayoutHeader {
            image_layout_version: LAYOUT_VERSION.to_string(),
        })?,
    )?;
    // Los dos manifiestos son contenido: viven en su blob, igual que en
    // el layout que `skopeo copy --format oci` escribe.
    std::fs::write(layout.join(layout_blob_path(&image_digest)), &image_bytes)?;
    std::fs::write(
        layout.join(layout_blob_path(&signature_digest)),
        &signature_bytes,
    )?;

    Ok(CapsuleExport {
        layout_dir: dir.to_path_buf(),
        image_digest,
        signature_digest,
        signed_manifest: image_bytes,
    })
}

/// El digest de contenido del estandar OCI: sha256 en **minusculas**.
/// `ContentDigest::of_bytes` lo escribe en mayusculas, que aqui no
/// vale: la ruta del blob y el digest anunciado tienen que coincidir
/// byte a byte con lo que cualquier cliente OCI va a recalcular.
fn digest_of(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let mut digest = String::from("sha256:");
    for byte in sha2::Sha256::digest(bytes) {
        write!(&mut digest, "{byte:02x}").expect("writing to a String never fails");
    }
    digest
}

/// El veredicto de [`verify_capsule`]: que el layout es una capsula
/// coherente y que la firma corresponde a esta capsula y a nadie mas.
#[derive(Debug)]
pub struct CapsuleVerification {
    pub image: ImageManifest,
    /// El manifiesto del bundle que la capsula contiene, byte a byte.
    pub manifest: BundleManifest,
    pub capsule: TrustState,
    pub signer: String,
}

/// Comprueba el layout: cada descriptor contra sus bytes, la capsula
/// contra lo que el manifiesto del bundle dice, y la firma detached
/// contra los bytes exactos del manifiesto de imagen.
///
/// La verificacion es estructural, no testimonial: un blob que falta o
/// que no cuadra se rechaza; una capsula que afirma una decision que el
/// manifiesto no tiene se rechaza; una firma valida sobre otra capsula
/// no sirve.
pub fn verify_capsule(
    dir: &Path,
    signer: &MetadataSigner,
) -> Result<CapsuleVerification, BundleError> {
    let header: LayoutHeader = read_json(&dir.join(LAYOUT_FILE))?;
    if header.image_layout_version != LAYOUT_VERSION {
        return Err(unsound(format!(
            "unsupported image layout version {}",
            header.image_layout_version
        )));
    }
    let index: LayoutIndex = read_json(&dir.join(INDEX_FILE))?;
    if index.media_type != INDEX_MEDIA_TYPE {
        return Err(unsound(format!("{} is not an OCI index", index.media_type)));
    }

    let image = read_manifest_of(dir, &index, CAPSULE_ARTIFACT_TYPE)?;
    let blobs = read_declared_blobs(dir, &image)?;

    let release = image
        .layers
        .iter()
        .find(|l| l.media_type == RELEASE_MEDIA_TYPE)
        .ok_or_else(|| unsound("the capsule has no release layer"))?;
    let manifest_bytes = blobs
        .get(&release.digest)
        .expect("read_declared_blobs collected every declared layer");
    if digest_of(manifest_bytes) != release.digest {
        return Err(unsound(
            "release.json is not the digest the manifest declares",
        ));
    }
    let manifest: BundleManifest = serde_json::from_slice(manifest_bytes)?;

    let capsule_layer = image
        .layers
        .iter()
        .find(|l| l.media_type == TRUST_MEDIA_TYPE)
        .ok_or_else(|| unsound("the capsule has no trust layer"))?;
    let capsule: TrustState =
        serde_json::from_slice(blobs.get(&capsule_layer.digest).expect("declared layer"))?;
    if TrustState::of(&manifest) != capsule {
        return Err(unsound(
            "the capsule does not state the decisions the bundle manifest holds",
        ));
    }

    // La firma vive en su propio manifiesto, con `subject` apuntando a
    // la capsula: el patron de referrers. Verificar exige el mismo
    // rigor que firmar: la firma es sobre los bytes del manifiesto de
    // imagen, y ese manifiesto tiene que ser el que el indice anuncia.
    let signature_image = read_manifest_of(dir, &index, SIGNATURE_ARTIFACT_TYPE)?;
    let subject = signature_image
        .subject
        .as_ref()
        .ok_or_else(|| unsound("the signature has no subject: it signs nothing"))?;
    let image_bytes = std::fs::read(dir.join(layout_blob_path(&subject.digest)))
        .map_err(|_| unsound("the signed manifest is announced but missing"))?;
    if digest_of(&image_bytes) != subject.digest
        || image_bytes.len() as u64 != subject.size
        || serde_json::from_slice::<ImageManifest>(&image_bytes)? != image
    {
        return Err(unsound("the signature subject does not name this capsule"));
    }
    let sig_layer = signature_image
        .layers
        .first()
        .ok_or_else(|| unsound("the signature image carries no signature layer"))?;
    let sig_bytes = read_layout_blob(dir, sig_layer)?;
    let sig_json: serde_json::Value = serde_json::from_slice(&sig_bytes)?;
    let signature = sig_json
        .get("signature")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let declared_signer = sig_json
        .get("signer")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if declared_signer != signer.fingerprint_hex() {
        return Err(unsound(format!(
            "the signature names key {}, not the key we verify with",
            declared_signer
        )));
    }
    let public_key_asc = signer
        .key_asc()
        .map_err(|e| unsound(format!("the signer has no public key: {e}")))?;
    MetadataSigner::verify_detached(&image_bytes, signature.as_bytes(), &public_key_asc)
        .map_err(|e| unsound(format!("the signature does not hold: {e}")))?;

    Ok(CapsuleVerification {
        image,
        manifest,
        capsule,
        signer: declared_signer,
    })
}

/// El manifiesto del layout cuyo `artifactType` es `want`, comprobando
/// que el blob bajo el digest anunciado digiere exactamente eso.
fn read_manifest_of(
    dir: &Path,
    index: &LayoutIndex,
    want: &str,
) -> Result<ImageManifest, BundleError> {
    for entry in &index.manifests {
        let bytes = read_layout_blob(dir, entry)?;
        let manifest: ImageManifest = serde_json::from_slice(&bytes)?;
        if manifest.artifact_type == want {
            return Ok(manifest);
        }
    }
    Err(unsound(format!(
        "the index has no manifest of artifact type {want}"
    )))
}
