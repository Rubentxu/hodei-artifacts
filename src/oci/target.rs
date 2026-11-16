//! El reparto del path de OCI, y las reglas del nombre y del digest.
//!
//! El path de la distribucion se lee **en crudo**, del URI como llego:
//! `/v2/<name>/blobs/<digest>`, `/v2/<name>/manifests/<referencia>`,
//! `/v2/<name>/blobs/uploads/<uuid>`.
//!
//! Y hay un detalle que el protocolo deja claro y que muchas
//! implementaciones no cumplen: el nombre del repositorio puede
//! contener `/`. Un repositorio se llama `team/demo`, y su path es
//! `/v2/team/demo/blobs/sha256:...`.
//!
//! Por eso el reparto no busca el **primer** `blobs` o `manifests`, sino el
//! **ultimo**: lo que va antes de el es el nombre, y lo que va despues es
//! la clase de recurso y la referencia. Un repositorio que se llamara
//! `blobs/app` seguiria siendo un nombre valido, no una clase de
//! recurso.
//!
//! El path se lee en crudo a proposito: un `%2e%2e` ya decodificado no
//! se puede refutar con un extractor que decodifica, y un traversal es un
//! traversal.

use crate::oci::failure::OciFailure;

/// Lo que el path nombra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Un blob del repositorio, por digest.
    Blob {
        /// El nombre del repositorio.
        name: String,
        /// La referencia: el digest, con su algoritmo.
        reference: String,
    },
    /// Un manifiesto del repositorio, por digest o por etiqueta.
    Manifest {
        /// El nombre del repositorio.
        name: String,
        /// La referencia: el digest o la etiqueta.
        reference: String,
    },
    /// La lista de etiquetas del repositorio: `<name>/tags/list`.
    Tags {
        /// El nombre del repositorio.
        name: String,
    },
    /// Los referrers de un manifiesto: `<name>/referrers/<digest>`.
    Referrers {
        /// El nombre del repositorio.
        name: String,
        /// El digest del manifiesto al que se pregunta.
        digest: String,
    },
    /// El **inicio** de una subida: `<name>/blobs/uploads/`.
    ///
    /// Aqui no hay sesion todavia: este path es el que abre una.
    UploadStart {
        /// El nombre del repositorio.
        name: String,
    },
    /// Una **sesion** de subida en curso: `<name>/blobs/uploads/<uuid>`.
    UploadSession {
        /// El nombre del repositorio.
        name: String,
        /// El identificador opaco de la sesion.
        session: String,
    },
}

impl Target {
    /// El nombre del repositorio, sea cual sea la clase de recurso.
    pub fn the_name(&self) -> &str {
        match self {
            Self::Blob { name, .. }
            | Self::Manifest { name, .. }
            | Self::Tags { name }
            | Self::Referrers { name, .. }
            | Self::UploadStart { name }
            | Self::UploadSession { name, .. } => name,
        }
    }
}

/// El prefijo que lleva la superficie de distribucion.
const PREFIX: &str = "/v2/";

/// Lo que el path nombra, sin el prefijo de la superficie.
///
/// El prefijo se quita y el resto se parte en niveles. Un nivel vacio al
/// final es la barra final de `/blobs/uploads/`, y no es un nivel.
fn the_levels(path: &str) -> Option<Vec<&str>> {
    let rest = path.strip_prefix(PREFIX)?;
    let mut levels: Vec<&str> = rest.split('/').collect();
    // La barra final no es un nivel: `/blobs/uploads/` y `/blobs/uploads`
    // son la misma peticion.
    while levels.last().is_some_and(|level| level.is_empty()) {
        levels.pop();
    }
    if levels.iter().any(|level| level.is_empty()) {
        // Un nivel vacio en medio (`team//demo`) no es un nombre.
        return None;
    }
    Some(levels)
}

/// El indice del nivel que nombra la clase de recurso.
///
/// Es el **ultimo** nivel que sea `blobs`, `manifests`, `tags` o
/// `referrers` y que tenga algo detras: lo que va despues de el es la
/// referencia.
fn the_kind_index(levels: &[&str]) -> Option<usize> {
    levels
        .iter()
        .enumerate()
        .rev()
        .find(|(index, level)| {
            matches!(**level, "blobs" | "manifests" | "tags" | "referrers")
                && *index + 1 < levels.len()
        })
        .map(|(index, _)| index)
}

/// Lo que el path nombra.
///
/// El nombre se valida ANTES de mirar el catalogo: un `..` no llega a
/// ser una consulta, y un nombre mal formado es un error del cliente.
pub fn the_target(path: &str) -> Result<Target, OciFailure> {
    let levels = the_levels(path)
        .ok_or_else(|| OciFailure::name_invalid("the path is not an OCI resource path"))?;
    let index = the_kind_index(&levels).ok_or_else(|| {
        OciFailure::name_invalid("the path does not name a resource kind and a reference")
    })?;
    let name = levels[..index].join("/");
    the_name(&name)?;
    let rest = &levels[index + 1..];

    match levels[index] {
        "manifests" => {
            let reference = rest.join("/");
            if reference.is_empty() {
                return Err(OciFailure::name_invalid(
                    "the path does not name a reference",
                ));
            }
            Ok(Target::Manifest { name, reference })
        }
        // La lista de etiquetas: `<name>/tags/list`. El `list` es parte
        // del protocolo y no un nombre de recurso, asi que sin el no es
        // la lista de etiquetas: es un path que no nombra nada.
        "tags" => {
            if rest != ["list"] {
                return Err(OciFailure::name_invalid(
                    "the path does not name the list of tags",
                ));
            }
            Ok(Target::Tags { name })
        }
        // Los referrers: `<name>/referrers/<digest>`. El digest se
        // comprueba aqui, con la misma regla que el de un blob: un
        // digest con otra sintaxis es un error del cliente, y no una
        // busqueda sin resultado.
        "referrers" => {
            if rest.len() != 1 {
                return Err(OciFailure::name_invalid(
                    "the path does not name a manifest to ask the referrers of",
                ));
            }
            // La sintaxis se comprueba aqui: un digest con otra sintaxis
            // es un error del cliente, y el protocolo lo dice con un
            // `400`, no con una lista vacia.
            the_digest_of_reference(rest[0])?;
            Ok(Target::Referrers {
                name,
                digest: rest[0].to_string(),
            })
        }
        "blobs" => {
            // La superficie de subida vive bajo `blobs/uploads`: es lo
            // que dice que esto no es un blob que se llama `uploads`.
            if rest.first() == Some(&"uploads") {
                return match rest.len() {
                    1 => Ok(Target::UploadStart { name }),
                    2 => {
                        the_session_id(rest[1])?;
                        Ok(Target::UploadSession {
                            name,
                            session: rest[1].to_string(),
                        })
                    }
                    _ => Err(OciFailure::name_invalid(
                        "the upload path names neither a session nor its start",
                    )),
                };
            }
            let reference = rest.join("/");
            if reference.is_empty() {
                return Err(OciFailure::name_invalid(
                    "the path does not name a reference",
                ));
            }
            Ok(Target::Blob { name, reference })
        }
        other => Err(OciFailure::unsupported(format!(
            "the resource kind {other:?} is not part of the distribution API"
        ))),
    }
}

/// El identificador de una sesion de subida.
///
/// Es un UUID: el cliente lo recibe en el `Location` y lo devuelve tal
/// cual. Que sea un UUID no es un capricho: es lo que hace que el
/// identificador **no se pueda adivinar**, y una sesion que se puede
/// adivinar es una sesion que otro puede escribir.
///
/// Y lo que no se puede adivinar tampoco se puede colar: un
/// identificador que no es un UUID no es una sesion, y se responde como
/// si no existiera.
pub fn the_session_id(session: &str) -> Result<(), OciFailure> {
    let parsed = uuid::Uuid::parse_str(session)
        .map_err(|_| OciFailure::blob_upload_unknown("the upload session does not exist"))?;
    // El UUID canonico es el que se devuelve: un cliente que manda la
    // version en mayusculas o con corchetes recibe el mismo error que uno
    // que manda cualquier otra cosa.
    if parsed.hyphenated().to_string() != session {
        return Err(OciFailure::blob_upload_unknown(
            "the upload session does not exist",
        ));
    }
    Ok(())
}

/// El nombre del repositorio, con sus reglas.
///
/// Un nombre OCI es minusculas, digitos y separadores, con niveles
/// separados por barra. Un `..` no es un nivel: es un traversal, y se
/// rechaza antes de mirar el catalogo.
pub fn the_name(name: &str) -> Result<(), OciFailure> {
    if name.is_empty() || name.len() > 255 {
        return Err(OciFailure::name_invalid(
            "the repository name is empty or too long",
        ));
    }
    if name.starts_with('/') || name.ends_with('/') {
        return Err(OciFailure::name_invalid(
            "the repository name does not start or end with a separator",
        ));
    }
    for segment in name.split('/') {
        if segment.is_empty() {
            return Err(OciFailure::name_invalid(
                "the repository name has an empty level",
            ));
        }
        if segment == "." || segment == ".." {
            return Err(OciFailure::name_invalid(
                "the repository name does not carry navigation levels",
            ));
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        {
            return Err(OciFailure::name_invalid(
                "the repository name only admits lowercase letters, digits, '.', '_' and '-'",
            ));
        }
    }
    Ok(())
}

/// El digest que la referencia nombra.
///
/// Una referencia de blob o de manifest por digest lleva el algoritmo y
/// el hexadecimal (`sha256:<64 hex>`). Sin el prefijo, la referencia es
/// una **etiqueta**, que es mutable: el digest pelado no se acepta como
/// digest, porque un cliente que cree que pide por digest y recibe
/// contenido mutable se lleva un manifiesto que puede haber cambiado
/// entre la peticion y el uso.
///
/// Lo que no cuadre es un error del cliente, y el codigo OCI lo dice.
pub fn the_digest_of_reference(reference: &str) -> Result<String, OciFailure> {
    if !reference.starts_with("sha256:") {
        return Err(OciFailure::digest_invalid(format!(
            "{reference:?} does not name a digest: a digest carries its algorithm"
        )));
    }
    let digest = hodei_hub::oci::the_digest_of(reference)
        .ok_or_else(|| {
            OciFailure::digest_invalid(format!(
                "{reference:?} is not a content digest the registry understands"
            ))
        })?
        .value;
    Ok(digest)
}

/// Si la referencia es un digest o una etiqueta.
///
/// Una referencia con `:` es un digest (el algoritmo lo lleva); sin el,
/// es una etiqueta, que es mutable.
pub fn is_a_digest(reference: &str) -> bool {
    reference.contains(':')
}

/// El nombre de una etiqueta, con sus reglas.
///
/// El protocolo dice que una etiqueta es una palabra: empieza por un
/// alfanumerico o un guion bajo, y sigue con alfanumericos, puntos,
/// guiones y guiones bajos, hasta 128 caracteres.
///
/// La regla no es un capricho: la etiqueta es una **clave** en el
/// catalogo, y una clave con barras, espacios o caracteres de control no
/// es una etiqueta: es una ruta que nadie ha pedido. Y el nombre que se
/// acepta es el que el protocolo acepta, para que un cliente real no
/// tenga que aprender dos reglas.
pub fn the_tag_name(tag: &str) -> Result<(), OciFailure> {
    let mut characters = tag.chars();
    let first = characters
        .next()
        .ok_or_else(|| OciFailure::manifest_invalid("the tag of the manifest is empty"))?;
    let starts_well = first.is_ascii_alphanumeric() || first == '_';
    if !starts_well {
        return Err(OciFailure::manifest_invalid(format!(
            "the tag {tag:?} does not start with a letter, a digit or an underscore"
        )));
    }
    if tag.len() > 128 {
        return Err(OciFailure::manifest_invalid(format!(
            "the tag {tag:?} is longer than the 128 characters the protocol admits"
        )));
    }
    let rest_well = characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'));
    if !rest_well {
        return Err(OciFailure::manifest_invalid(format!(
            "the tag {tag:?} carries a character that is not a letter, a digit, a '.', a '_' or a '-'"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El nombre puede tener niveles: el ultimo `blobs` o `manifests` es
    /// el que manda, no el primero.
    #[test]
    fn the_last_kind_wins_and_the_name_keeps_its_levels() {
        let target = the_target("/v2/team/demo/blobs/sha256:abc").expect("the target");
        assert_eq!(
            target,
            Target::Blob {
                name: "team/demo".to_string(),
                reference: "sha256:abc".to_string(),
            }
        );

        // Un repositorio que se llama `blobs/app` sigue siendo un nombre.
        let target = the_target("/v2/blobs/app/manifests/v1").expect("the target");
        assert_eq!(
            target,
            Target::Manifest {
                name: "blobs/app".to_string(),
                reference: "v1".to_string(),
            }
        );

        // Y el ultimo manda cuando los dos aparecen.
        let target = the_target("/v2/team/manifests/blobs/sha256:abc").expect("the target");
        assert_eq!(
            target,
            Target::Blob {
                name: "team/manifests".to_string(),
                reference: "sha256:abc".to_string(),
            }
        );
    }

    /// La superficie de subida distingue el inicio de la sesion.
    #[test]
    fn the_upload_path_tells_the_start_from_the_session() {
        let start = the_target("/v2/team/demo/blobs/uploads/").expect("the start");
        assert_eq!(
            start,
            Target::UploadStart {
                name: "team/demo".to_string()
            }
        );

        // Sin la barra final es la misma peticion.
        let start = the_target("/v2/team/demo/blobs/uploads").expect("the start");
        assert_eq!(
            start,
            Target::UploadStart {
                name: "team/demo".to_string()
            }
        );

        let session = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";
        let target =
            the_target(&format!("/v2/team/demo/blobs/uploads/{session}")).expect("the session");
        assert_eq!(
            target,
            Target::UploadSession {
                name: "team/demo".to_string(),
                session: session.to_string(),
            }
        );
    }

    /// Un identificador de sesion que no es un UUID no es una sesion.
    #[test]
    fn a_session_id_that_is_not_a_uuid_is_not_a_session() {
        let error = the_target("/v2/team/demo/blobs/uploads/..%2Fetc%2Fpasswd")
            .expect_err("the traversal is not a session");
        assert_eq!(error.the_code(), "BLOB_UPLOAD_UNKNOWN");
    }

    /// El traversal se rechaza antes de tocar el catalogo.
    #[test]
    fn a_traversal_name_is_refused_before_anything_else() {
        let error = the_target("/v2/..%2Fescape/manifests/v1").expect_err("the traversal");
        assert_eq!(error.the_code(), "NAME_INVALID");
    }

    /// Un digest sin su algoritmo no es un digest.
    #[test]
    fn a_digest_without_its_algorithm_is_refused() {
        let error = the_digest_of_reference("not-a-digest").expect_err("the digest");
        assert_eq!(error.the_code(), "DIGEST_INVALID");
    }
}
