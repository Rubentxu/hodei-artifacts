//! El subconjunto de OCI soportado, y lo que no lo esta.
//!
//! El SPIKE-010 pregunta si Hodei debe hablar OCI/Maven/npm o integrarse
//! con los registros que ya existen, y su salida es declarar el conjunto
//! minimo de protocolos y los no-objetivos explicitos.
//!
//! La conclusion es: **OCI primero, y solo el subconjunto que Hodei puede
//! verificar**. Un hub que habla un protocolo entero sin verificar los
//! bytes es un hub que repite el problema que vino a resolver.

use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};

/// El tipo de medio del manifiesto de OCI.
pub const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

/// El tipo de medio de la configuracion de OCI.
pub const CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";

/// Lo que Hodei soporta de OCI.
///
/// Se declara aqui, en el codigo, para que no sea una promesa de la
/// documentacion: lo que no esta en esta lista no esta soportado, y
/// escribirlo en la lista es cambiar el comportamiento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OciSupport {
    /// El manifiesto de una imagen, con su configuracion y sus capas.
    ImageManifest,
    /// Una referencia a un artefacto de un registro externo.
    ExternalReference,
}

/// Los tipos de medio que Hodei NO soporta todavia.
///
/// La lista es corta a proposito: un indice multi-plataforma, los
/// referrers, las firmas y los artefactos de artefactos. Hodei no los
/// habla, y un cliente que los envie recibe un error claro en vez de una
///interpretacion silenciosa.
pub const UNSUPPORTED: &[&str] = &[
    "application/vnd.oci.image.index.v1+json",
    "application/vnd.oci.artifact.manifest.v1+json",
    "application/vnd.cncf.notary.signature",
];

/// Por que una peticion OCI no se puede servir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OciError {
    /// El tipo de medio no esta soportado.
    UnsupportedMediaType(String),
    /// El manifiesto no se puede leer.
    Unreadable(String),
}

impl std::fmt::Display for OciError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedMediaType(media_type) => write!(
                formatter,
                "the media type {media_type} is not supported: the supported subset is the image \
                 manifest, its config and its layers"
            ),
            Self::Unreadable(reason) => write!(formatter, "the manifest is not readable: {reason}"),
        }
    }
}

/// El digest de un texto hexadecimal.
///
/// Un digest que no esta bien formado no es un digest, y un digest mal
/// formado en un manifiesto es un manifiesto que no se puede servir.
pub fn the_digest_of(reference: &str) -> Option<ContentDigest> {
    // La referencia de OCI es "sha256:<hexadecimal>", y el digest de
    // Hodei es el algoritmo y el hexadecimal.
    let hex = reference.strip_prefix("sha256:").unwrap_or(reference);
    let digest = ContentDigest::sha256(hex);
    digest.is_well_formed().then_some(digest)
}

/// El manifiesto de una imagen OCI, en lo que Hodei necesita de el.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciManifest {
    /// La version del esquema.
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    /// El tipo de medio del manifiesto.
    #[serde(rename = "mediaType")]
    pub media_type: String,
    /// La configuracion de la imagen, por su digest.
    pub config: OciDescriptor,
    /// Las capas de la imagen.
    pub layers: Vec<OciDescriptor>,
}

/// Un descriptor: el tipo de medio y el digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciDescriptor {
    /// El tipo de medio.
    #[serde(rename = "mediaType")]
    pub media_type: String,
    /// El digest del contenido.
    pub digest: String,
}

impl OciManifest {
    /// Lee un manifiesto y comprueba que es del subconjunto soportado.
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

    /// Los digests de todo lo que el manifiesto referencia.
    ///
    /// La configuracion y las capas: todo lo que Hodei tiene que tener
    /// para poder servir la imagen por su digest.
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
