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

/// El tipo de medio del manifiesto de una imagen Docker (schema 2).
pub const DOCKER_MANIFEST_MEDIA_TYPE: &str = "application/vnd.docker.distribution.manifest.v2+json";

/// El tipo de medio del indice de imagenes multi-plataforma de OCI.
pub const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";

/// El tipo de medio de la lista de manifiestos multi-plataforma de Docker.
///
/// Es lo mismo que el indice de OCI con otro nombre: una lista de
/// manifiestos hijos, uno por plataforma. Un registro que habla uno habla
/// el otro, y un cliente de contenedores los usa indistintamente.
pub const DOCKER_MANIFEST_LIST_MEDIA_TYPE: &str =
    "application/vnd.docker.distribution.manifest.list.v2+json";

/// El tipo de medio del manifiesto de artefacto de OCI.
///
/// El manifiesto de artefacto es el que lleva un `subject`: el manifiesto
/// de una firma o de un SBOM que se cuelga de otro manifiesto sin tocarlo.
pub const ARTIFACT_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.artifact.manifest.v1+json";

/// El tipo de medio de la configuracion de OCI.
pub const CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";

/// El tipo de medio del descriptor **vacio**.
///
/// Es el descriptor que dice "este manifiesto no tiene configuracion":
/// un `artifactType` y nada mas. Los bytes que nombra son un `{}` de dos
/// bytes, y un registro no tiene por que guardarlos: el descriptor es
/// una convencion, no un contenido.
pub const EMPTY_MEDIA_TYPE: &str = "application/vnd.oci.empty.v1+json";

/// El digest del descriptor **vacio**.
pub const EMPTY_DIGEST: &str =
    "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";

/// Si un descriptor es el descriptor vacio.
///
/// Un manifiesto de artefacto dice que no tiene configuracion con este
/// descriptor, y un registro que lo exigiera en el almacen rechazaria un
/// manifiesto que el protocolo dice que es valido.
///
/// El descriptor vacio es **uno**: el que dice las dos cosas a la vez,
/// el tipo de medio y el digest de los dos bytes. Con una sola de las dos
/// no es el descriptor vacio: es un descriptor que dice "no hay
/// configuracion" y nombra un contenido que no esta, y un registro que
/// lo tomara por el vacio dejaria de exigir un contenido que el
/// manifiesto nombra.
pub fn is_the_empty_descriptor(media_type: &str, digest: &str) -> bool {
    media_type == EMPTY_MEDIA_TYPE && digest == EMPTY_DIGEST
}

/// Los tipos de medio de manifiesto que este registro publica y sirve.
///
/// Se declara aqui, en el codigo, para que no sea una promesa de la
/// documentacion: lo que no esta en esta lista no esta soportado, y
/// escribirlo en la lista es cambiar el comportamiento.
///
/// Los cinco son los que un cliente real usa: el manifiesto de imagen de
/// OCI y el de Docker (schema 2), el indice multi-plataforma de OCI y la
/// lista de Docker, y el manifiesto de artefacto con su `subject`.
pub const SUPPORTED_MANIFESTS: &[&str] = &[
    MANIFEST_MEDIA_TYPE,
    DOCKER_MANIFEST_MEDIA_TYPE,
    INDEX_MEDIA_TYPE,
    DOCKER_MANIFEST_LIST_MEDIA_TYPE,
    ARTIFACT_MANIFEST_MEDIA_TYPE,
];

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

/// Los tipos de medio que Hodei NO soporta.
///
/// La lista es corta a proposito, y ahora mas: del indice multi-plataforma
/// y del manifiesto de artefacto ya se habla (HAR-08), asi que lo que
/// queda es la firma de notary, un tipo que un registro con referrers no
/// necesita. Un cliente que lo envie recibe un error claro en vez de una
/// reinterpretacion silenciosa.
pub const UNSUPPORTED: &[&str] = &["application/vnd.cncf.notary.signature"];

/// La clase de un manifiesto.
///
/// La clase decide **que descriptors hay que exigir**: un manifiesto de
/// imagen exige su configuracion y sus capas, un indice exige sus hijos,
/// y un manifiesto de artefacto no exige nada porque sus blobs son
/// opcionales.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestKind {
    /// Un manifiesto de imagen: configuracion y capas.
    Image,
    /// Un indice multi-plataforma: la lista de los manifiestos hijos.
    Index,
    /// Un manifiesto de artefacto: un `artifactType` y, si quiere, blobs.
    Artifact,
}

/// La clase de un tipo de medio de manifiesto.
///
/// `None` es un tipo que este registro no habla, y un tipo que no se
/// habla no se publica ni se sirve.
pub fn the_kind_of(media_type: &str) -> Option<ManifestKind> {
    match media_type {
        MANIFEST_MEDIA_TYPE | DOCKER_MANIFEST_MEDIA_TYPE => Some(ManifestKind::Image),
        INDEX_MEDIA_TYPE | DOCKER_MANIFEST_LIST_MEDIA_TYPE => Some(ManifestKind::Index),
        ARTIFACT_MANIFEST_MEDIA_TYPE => Some(ManifestKind::Artifact),
        _ => None,
    }
}

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
                "the media type {media_type} is not supported: the supported subset is the one \
                 declared in SUPPORTED_MANIFESTS"
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Las dos listas tienen que decir lo mismo.
    ///
    /// El push decide con `the_kind_of` y el pull decide con
    /// `SUPPORTED_MANIFESTS`: si una de las dos se queda atras, el
    /// registro acepta publicar un tipo que luego no sirve, y el
    /// cliente recibe un 201 y un 404 del mismo manifiesto.
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

    /// El descriptor vacio es **uno**: el que dice las dos cosas.
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
