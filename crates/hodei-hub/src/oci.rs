//! The supported OCI subset, and what is not.
//!
//! SPIKE-010 asks whether Hodei should speak OCI/Maven/npm or integrate
//! with the registries that already exist, and its output is to declare the
//! minimal set of protocols and the explicit non-goals.
//!
//! The conclusion is: **OCI first, and only the subset that Hodei can
//! verify**. A hub that speaks an entire protocol without verifying the
//! bytes is a hub that repeats the problem it came to solve.

use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};

/// The media type of the OCI manifest.
pub const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

/// The media type of a Docker image manifest (schema 2).
pub const DOCKER_MANIFEST_MEDIA_TYPE: &str = "application/vnd.docker.distribution.manifest.v2+json";

/// The media type of the OCI multi-platform image index.
pub const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";

/// The media type of the Docker multi-platform manifest list.
///
/// It is the same as the OCI index under another name: a list of
/// child manifests, one per platform. A registry that speaks one speaks
/// the other, and a container client uses them interchangeably.
pub const DOCKER_MANIFEST_LIST_MEDIA_TYPE: &str =
    "application/vnd.docker.distribution.manifest.list.v2+json";

/// The media type of the OCI artifact manifest.
///
/// The artifact manifest is the one that carries a `subject`: the manifest
/// of a signature or an SBOM that hangs from another manifest without touching it.
pub const ARTIFACT_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.artifact.manifest.v1+json";

/// The media type of the OCI configuration.
pub const CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";

/// The media type of the **empty** descriptor.
///
/// It is the descriptor that says "this manifest has no configuration":
/// an `artifactType` and nothing else. The bytes it names are a two-byte `{}`,
/// and a registry need not store them: the descriptor is
/// a convention, not content.
pub const EMPTY_MEDIA_TYPE: &str = "application/vnd.oci.empty.v1+json";

/// The digest of the **empty** descriptor.
pub const EMPTY_DIGEST: &str =
    "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";

/// Whether a descriptor is the empty descriptor.
///
/// An artifact manifest says it has no configuration with this
/// descriptor, and a registry that required it in the store would reject a
/// manifest that the protocol says is valid.
///
/// The empty descriptor is **one**: the one that says both things at once,
/// the media type and the digest of the two bytes. With only one of the two
/// it is not the empty descriptor: it is a descriptor that says "there is no
/// configuration" and names a content that is not there, and a registry that
/// took it for the empty one would stop requiring a content that the
/// manifest names.
pub fn is_the_empty_descriptor(media_type: &str, digest: &str) -> bool {
    media_type == EMPTY_MEDIA_TYPE && digest == EMPTY_DIGEST
}

/// The manifest media types that this registry publishes and serves.
///
/// It is declared here, in code, so that it is not a promise of the
/// documentation: what is not in this list is not supported, and
/// writing it into the list is changing the behavior.
///
/// The five are the ones a real client uses: the OCI image manifest
/// and the Docker one (schema 2), the OCI multi-platform index and the
/// Docker list, and the artifact manifest with its `subject`.
pub const SUPPORTED_MANIFESTS: &[&str] = &[
    MANIFEST_MEDIA_TYPE,
    DOCKER_MANIFEST_MEDIA_TYPE,
    INDEX_MEDIA_TYPE,
    DOCKER_MANIFEST_LIST_MEDIA_TYPE,
    ARTIFACT_MANIFEST_MEDIA_TYPE,
];

/// What Hodei supports of OCI.
///
/// It is declared here, in code, so that it is not a promise of the
/// documentation: what is not in this list is not supported, and
/// writing it into the list is changing the behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OciSupport {
    /// The manifest of an image, with its configuration and its layers.
    ImageManifest,
    /// A reference to an artifact of an external registry.
    ExternalReference,
}

/// The media types that Hodei does NOT support.
///
/// The list is short on purpose, and now more so: the multi-platform index
/// and the artifact manifest are already spoken (HAR-08), so what
/// remains is the notary signature, a type that a registry with referrers does
/// not need. A client that sends it receives a clear error instead of a
/// silent reinterpretation.
pub const UNSUPPORTED: &[&str] = &["application/vnd.cncf.notary.signature"];

/// The class of a manifest.
///
/// The class decides **which descriptors must be required**: an image
/// manifest requires its configuration and its layers, an index requires its children,
/// and an artifact manifest requires nothing because its blobs are
/// optional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestKind {
    /// An image manifest: configuration and layers.
    Image,
    /// A multi-platform index: the list of the child manifests.
    Index,
    /// An artifact manifest: an `artifactType` and, optionally, blobs.
    Artifact,
}

/// The class of a manifest media type.
///
/// `None` is a type that this registry does not speak, and a type that is not
/// spoken is neither published nor served.
pub fn the_kind_of(media_type: &str) -> Option<ManifestKind> {
    match media_type {
        MANIFEST_MEDIA_TYPE | DOCKER_MANIFEST_MEDIA_TYPE => Some(ManifestKind::Image),
        INDEX_MEDIA_TYPE | DOCKER_MANIFEST_LIST_MEDIA_TYPE => Some(ManifestKind::Index),
        ARTIFACT_MANIFEST_MEDIA_TYPE => Some(ManifestKind::Artifact),
        _ => None,
    }
}

/// Why an OCI request cannot be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OciError {
    /// The media type is not supported.
    UnsupportedMediaType(String),
    /// The manifest cannot be read.
    Unreadable(String),
}

impl std::fmt::Display for OciError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedMediaType(media_type) => write!(
                formatter,
                "the media type {media_type} is not supported: the supported subset is the one \
                 declared in SUPPORTED_MANIFESTS"
            ),
            Self::Unreadable(reason) => write!(formatter, "the manifest is not readable: {reason}"),
        }
    }
}

/// The digest of a hexadecimal text.
///
/// A digest that is not well formed is not a digest, and a malformed digest
/// in a manifest is a manifest that cannot be served.
pub fn the_digest_of(reference: &str) -> Option<ContentDigest> {
    // The OCI reference is "sha256:<hexadecimal>", and the digest of
    // Hodei is the algorithm and the hexadecimal.
    let hex = reference.strip_prefix("sha256:").unwrap_or(reference);
    let digest = ContentDigest::sha256(hex);
    digest.is_well_formed().then_some(digest)
}

/// The manifest of an OCI image, in what Hodei needs from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciManifest {
    /// The version of the schema.
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    /// The media type of the manifest.
    #[serde(rename = "mediaType")]
    pub media_type: String,
    /// The configuration of the image, by its digest.
    pub config: OciDescriptor,
    /// The layers of the image.
    pub layers: Vec<OciDescriptor>,
}

/// A descriptor: the media type and the digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciDescriptor {
    /// The media type.
    #[serde(rename = "mediaType")]
    pub media_type: String,
    /// The digest of the content.
    pub digest: String,
}

impl OciManifest {
    /// Reads a manifest and checks that it is of the supported subset.
    pub fn read(bytes: &[u8]) -> Result<Self, OciError> {
        let manifest: OciManifest = serde_json::from_slice(bytes)
            .map_err(|error| OciError::Unreadable(error.to_string()))?;

        if manifest.media_type != MANIFEST_MEDIA_TYPE {
            return Err(OciError::UnsupportedMediaType(manifest.media_type));
        }
        if manifest.config.media_type != CONFIG_MEDIA_TYPE {
            return Err(OciError::UnsupportedMediaType(manifest.config.media_type));
        }
        for unsupported in UNSUPPORTED {
            if manifest.media_type == *unsupported {
                return Err(OciError::UnsupportedMediaType(manifest.media_type));
            }
        }
        Ok(manifest)
    }

    /// The digests of everything the manifest references.
    ///
    /// The configuration and the layers: everything Hodei must have
    /// in order to serve the image by its digest.
    pub fn the_digests(&self) -> Vec<ContentDigest> {
        let mut digests = Vec::new();
        if let Some(digest) = the_digest_of(&self.config.digest) {
            digests.push(digest);
        }
        for layer in &self.layers {
            if let Some(digest) = the_digest_of(&layer.digest) {
                digests.push(digest);
            }
        }
        digests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both lists must say the same thing.
    ///
    /// The push decides with `the_kind_of` and the pull decides with
    /// `SUPPORTED_MANIFESTS`: if one of the two falls behind, the
    /// registry accepts publishing a type that it then does not serve, and the
    /// client receives a 201 and a 404 for the same manifest.
    #[test]
    fn the_supported_manifests_are_exactly_the_ones_the_kind_knows() {
        for media_type in SUPPORTED_MANIFESTS {
            assert!(
                the_kind_of(media_type).is_some(),
                "{media_type} is supported but its kind is unknown"
            );
        }
        for media_type in [
            MANIFEST_MEDIA_TYPE,
            DOCKER_MANIFEST_MEDIA_TYPE,
            INDEX_MEDIA_TYPE,
            DOCKER_MANIFEST_LIST_MEDIA_TYPE,
            ARTIFACT_MANIFEST_MEDIA_TYPE,
        ] {
            assert!(
                SUPPORTED_MANIFESTS.contains(&media_type),
                "{media_type} has a kind but is not supported"
            );
        }
    }

    /// The empty descriptor is **one**: the one that says both things.
    #[test]
    fn the_empty_descriptor_is_the_one_that_says_both() {
        assert!(is_the_empty_descriptor(EMPTY_MEDIA_TYPE, EMPTY_DIGEST));
        assert!(
            !is_the_empty_descriptor(EMPTY_MEDIA_TYPE, "sha256:aa"),
            "un descriptor que dice 'vacio' y nombra otro contenido no es el vacio"
        );
        assert!(
            !is_the_empty_descriptor("application/vnd.oci.image.layer.v1.tar", EMPTY_DIGEST),
            "un descriptor que nombra el digest del vacio con otro tipo no es el vacio"
        );
    }
}
