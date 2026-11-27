//! The release capsule as an OCI package: the bundle, dressed in a standard.
//!
//! A resolution bundle already says everything that needs saying —digests,
//! receipts, decisions and explicit absences— but it says it in its own
//! directory. Exporting it as an **OCI image layout** does not add
//! truth to it: it removes the exoticism. The layout this module writes can be read by
//! `oras` or `skopeo` without knowing anything about Hodei, and inside it there are two
//! named layers: `release.json`, which is the **exact** bytes of the
//! bundle's manifest, and `capsule.json`, which is the bundle's trust
//! state at the moment of export.
//!
//! The trust capsule is derived from the bundle itself, not from faith:
//! each path carries the result of its receipt and the decision that
//! backed it — or `null` if there was none. A `null` exported as `null`
//! is imported as `null`: here nobody turns an absence into a
//! verified, because authority belongs to the destination with its own
//! grants (the bundle's rule, repeated in the manifest).
//!
//! The signature is a detached OpenPGP over the exact bytes of the capsule's
//! manifest, and travels as a second manifest whose `subject` is the
//! first: the referrers pattern that the registry already serves over HTTP.

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
    /// The state derived from an already-checked bundle manifest:
    /// same input, same capsule. It asks nobody because there is
    /// nobody — the bundle already tells what it knew.
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

pub(super) fn descriptor(media_type: &str, bytes: &[u8]) -> Descriptor {
    Descriptor {
        media_type: media_type.to_string(),
        digest: digest_of(bytes),
        size: bytes.len() as u64,
        annotations: BTreeMap::new(),
    }
}

pub(super) fn annotation(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// The path of a blob within the layout, according to the standard.
///
/// A digest that is not `sha256:` plus 64 lowercase hex digits never names a
/// file: it maps to a fixed name that no blob can have, so a layout written by
/// somebody else cannot steer a read out of `blobs/sha256` (`..`, separators)
/// and cannot make this function panic.
pub fn layout_blob_path(digest: &str) -> PathBuf {
    let well_formed = digest.strip_prefix("sha256:").filter(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    match well_formed {
        Some(hex) => PathBuf::from(LAYOUT_BLOBS).join(hex),
        None => PathBuf::from(LAYOUT_BLOBS).join("malformed-digest"),
    }
}

pub(super) fn write_layout_blob(
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

pub(super) fn unsound(what: impl Into<String>) -> BundleError {
    BundleError::Unsound(what.into())
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, BundleError> {
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// The bytes of each descriptor that a manifest declares, checked
/// against its descriptor. A missing blob is an error; one that does not
/// match is too.
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

/// What [`export_capsule`] left on disk: the layout, the digest of
/// the capsule and the exact bytes that were signed.
#[derive(Debug)]
pub struct CapsuleExport {
    pub layout_dir: PathBuf,
    pub image_digest: String,
    pub signature_digest: String,
    pub signed_manifest: Vec<u8>,
}

/// Exports the bundle already written under `dir` as a signed OCI image in
/// `layout`: the layout is a subdirectory of the bundle, so that the
/// bundle stays intact and `oras push` receives the OCI folder as is.
///
/// Checks each blob of the bundle against its declared digest before
/// copying it as a layer: the capsule cannot contain anything that the
/// bundle itself does not back.
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

    // The manifest that gets exported is the bytes of the `release.json` that
    // `export` wrote: the capsule contains the bundle, not a re-serialized
    // copy that would change digest because of the order of a map.
    // Whoever exports has just written the bundle in `dir`, so the
    // store's blobs are read from its own copy of the bundle.
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
        // The content layer lives in its layout blob, as the
        // standard requires: the descriptor may announce the same digest as the
        // bundle, but without bytes there is no layer that an OCI client can read.
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
    // The content layers first; the bundle manifest and the
    // trust capsule last, by name.
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
    // The two manifests are content: they live in their blob, just like in
    // the layout that `skopeo copy --format oci` writes.
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

/// The OCI standard's content digest: sha256 in **lowercase**.
/// `ContentDigest::of_bytes` writes it in uppercase, which is not
/// valid here: the blob path and the announced digest have to match
/// byte for byte with what any OCI client is going to recompute.
pub(super) fn digest_of(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let mut digest = String::from("sha256:");
    for byte in sha2::Sha256::digest(bytes) {
        write!(&mut digest, "{byte:02x}").expect("writing to a String never fails");
    }
    digest
}

/// The verdict of [`verify_capsule`]: that the layout is a coherent
/// capsule and that the signature corresponds to this capsule and to no other.
#[derive(Debug)]
pub struct CapsuleVerification {
    pub image: ImageManifest,
    /// The bundle manifest that the capsule contains, byte for byte.
    pub manifest: BundleManifest,
    pub capsule: TrustState,
    pub signer: String,
}

/// Checks the layout: each descriptor against its bytes, the capsule
/// against what the bundle manifest says, and the detached signature
/// against the exact bytes of the image manifest.
///
/// The verification is structural, not testimonial: a blob that is missing or
/// does not match is rejected; a capsule that claims a decision that the
/// manifest does not have is rejected; a valid signature over another capsule
/// is of no use.
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

    // The signature lives in its own manifest, with `subject` pointing to
    // the capsule: the referrers pattern. Verifying demands the same
    // rigor as signing: the signature is over the bytes of the image
    // manifest, and that manifest has to be the one the index announces.
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

/// The layout manifest whose `artifactType` is `want`, checking
/// that the blob under the announced digest digests exactly that.
pub(super) fn read_manifest_of(
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
