//! Las condiciones de una petición: el `ETag` y lo que el cliente dice
//! sobre el.
//!
//! El `ETag` de un contenido es su **digest**: la identidad del
//! contenido, que es la unica que se puede comparar sin leer los bytes.
//!
//! Las condiciones se aplican con la regla de RFC 9110, que distingue
//! entre leer y escribir:
//!
//! - Al **leer**, un `If-None-Match` que coincide quiere decir "no me
//!   lo mandes otra vez": la respuesta es 304. Un `If-Match` que no
//!   coincide quiere decir "solo si es ese": la respuesta es 412.
//! - Al **escribir**, las dos condiciones que fallan se responden con
//!   412: la petición no se hace. Un 304 sería mentira, porque 304 dice
//!   "no hay nada nuevo", y aquí si hay algo nuevo: lo que el cliente
//!   quiere escribir.

use hodei_catalog::Precondition;
use hodei_evidence::domain::content::ContentDigest;

/// La etiqueta de un contenido.
///
/// Va entre comillas porque es una etiqueta de entidad, y el digest va
/// dentro porque es lo que la distingue de otra.
pub fn the_etag(digest: &ContentDigest) -> String {
    format!("\"{}\"", digest)
}

/// Si la cabecera dice que el contenido es **este**, con comparación fuerte.
///
/// `*` dice "el que hay", y una lista de etiquetas dice "uno de estos".
///
/// La comparación es **fuerte**, que es lo que RFC 9110 pide para
/// `If-Match`: una etiqueta débil `W/"..."` no es la etiqueta que se
/// generó, y no puede decir que el contenido es ese.
pub fn the_strong_match(header: &str, etag: &str) -> bool {
    let header = header.trim();
    if header == "*" {
        return true;
    }
    let wanted = etag.trim();
    header.split(',').any(|candidate| {
        let candidate = candidate.trim();
        // Una etiqueta débil no es la etiqueta que se generó: el digest
        // es fuerte, y decir que dos contenidos con el mismo digest son
        // el mismo contenido es exactamente lo que se quiere.
        !candidate.starts_with("W/") && candidate == wanted
    })
}

/// Si la cabecera dice que el contenido es **este**, con comparación débil.
///
/// `*` dice "el que hay", y una lista de etiquetas dice "uno de estos".
///
/// La comparación es **débil**, que es lo que RFC 9110 pide para
/// `If-None-Match`: lo que se compara es la **etiqueta opaca**, sin el
/// prefijo `W/`. Un `W/"..."` y un `"..."` son la misma entidad para esta
/// comparación, que es justo lo que necesita un cliente que revalida
/// contra una caché.
pub fn the_weak_match(header: &str, etag: &str) -> bool {
    let header = header.trim();
    if header == "*" {
        return true;
    }
    let wanted = the_opaque_tag(etag.trim());
    header.split(',').any(|candidate| {
        let candidate = the_opaque_tag(candidate.trim());
        !candidate.is_empty() && candidate == wanted
    })
}

/// La etiqueta opaca de una etiqueta de entidad: lo que queda sin el
/// `W/` de delante.
fn the_opaque_tag(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

/// La cabecera `If-Match` de una petición.
pub fn the_if_match(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// La cabecera `If-None-Match` de una petición.
pub fn the_if_none_match(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// Lo que decide una condición de **lectura**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    /// No había condición, o la condición se cumple: se sirve el
    /// contenido.
    Serve,
    /// El cliente ya tiene ese contenido: la respuesta es 304, que no
    /// es un fallo, sino la respuesta a "dame esto solo si ha cambiado".
    NotModified,
    /// La condición no se cumple: la respuesta es 412.
    Failed,
}

/// Lo que decide una condición de **escritura**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// No había condición, o la condición se cumple: se escribe.
    Write,
    /// La condición no se cumple: la respuesta es 412 y **no** se
    /// escribe nada. Ni los bytes, ni el asset.
    Failed,
}

/// La condición de una **lectura**.
///
/// El `etag` es `None` cuando en esa dirección no hay nada: entonces un
/// `If-Match` no puede cumplirse, porque no hay nada que comparar.
pub fn the_read_condition(
    if_match: Option<&str>,
    if_none_match: Option<&str>,
    etag: Option<&str>,
) -> ReadOutcome {
    if let Some(header) = if_match {
        let matches = etag.is_some_and(|etag| the_strong_match(header, etag));
        if !matches {
            return ReadOutcome::Failed;
        }
    }
    if let Some(header) = if_none_match
        && let Some(etag) = etag
        && the_weak_match(header, etag)
    {
        // El contenido es el que el cliente ya tiene.
        return ReadOutcome::NotModified;
    }
    ReadOutcome::Serve
}

/// La condición de una **escritura**.
///
/// Las dos condiciones que fallan se responden con 412, y la petición no
/// se hace: ni se escribe el contenido, ni se toca el asset.
///
/// Y un `If-Match` sobre una dirección **vacia** falla: `*` quiere decir
/// "el recurso que hay", y si no hay recurso, no hay nada que
/// comparar.
pub fn the_write_condition(
    if_match: Option<&str>,
    if_none_match: Option<&str>,
    etag: Option<&str>,
) -> WriteOutcome {
    if let Some(header) = if_match {
        let matches = etag.is_some_and(|etag| the_strong_match(header, etag));
        if !matches {
            return WriteOutcome::Failed;
        }
    }
    if let Some(header) = if_none_match
        && let Some(etag) = etag
        && the_weak_match(header, etag)
    {
        // El contenido ya está: una escritura que dice "solo si no
        // existe" no se hace.
        return WriteOutcome::Failed;
    }
    WriteOutcome::Write
}

/// La condición que el cliente exige sobre lo que hay ahora.
///
/// Es lo que se transporta al catálogo. La comprobación de la frontera
/// es una **cortesía**: dice pronto lo que va a pasar. La que vale es la
/// del catálogo, porque ocurre **dentro de la transacción** que escribe,
/// y ahí no hay ventana entre el comprobar y el escribir.
///
/// Lo que no se entiende no viaja: una etiqueta que no es un digest no
/// se puede comparar con nada, y una condición que no se puede comparar
/// no es una condición.
pub fn the_precondition(
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> Option<Precondition> {
    if let Some(header) = if_match {
        let header = header.trim();
        if header == "*" {
            return Some(Precondition::Exists);
        }
        let digests = the_digests_of(header);
        if !digests.is_empty() {
            return Some(Precondition::Present(digests));
        }
    }
    if let Some(header) = if_none_match {
        let header = header.trim();
        if header == "*" {
            return Some(Precondition::Absent);
        }
        let digests = the_digests_of(header);
        if !digests.is_empty() {
            return Some(Precondition::Other(digests));
        }
    }
    None
}

/// Los digests de una lista de etiquetas.
///
/// Una etiqueta es un digest entre comillas, y la lista se separa por
/// comas. Lo que no es un digest se deja fuera: una lista con una
/// entrada que no se entiende es una lista que no se puede comparar.
fn the_digests_of(header: &str) -> Vec<ContentDigest> {
    header
        .split(',')
        .filter_map(|tag| {
            let tag = tag.trim();
            let tag = tag.strip_prefix("W/").unwrap_or(tag).trim();
            let tag = tag.trim_matches('"');
            let (algorithm, value) = tag.split_once(':')?;
            if !algorithm.eq_ignore_ascii_case("sha256") {
                return None;
            }
            let digest = ContentDigest::sha256(value);
            if digest.is_well_formed() {
                Some(digest)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El digest de un contenido cualquiera.
    fn a_digest() -> ContentDigest {
        ContentDigest::of_bytes(b"content")
    }

    #[test]
    fn the_etag_is_the_quoted_digest() {
        let digest = a_digest();
        let etag = the_etag(&digest);
        assert!(etag.starts_with('"') && etag.ends_with('"'));
        assert!(etag.contains(&digest.value));
    }

    #[test]
    fn the_header_matches_the_etag() {
        let etag = the_etag(&a_digest());
        assert!(the_strong_match(&etag, &etag));
        assert!(the_strong_match("*", &etag));
        assert!(!the_strong_match("\"other\"", &etag));
    }

    #[test]
    fn a_list_of_tags_matches_if_one_of_them_does() {
        let etag = the_etag(&a_digest());
        assert!(the_strong_match(&format!("\"other\", {etag}"), &etag));
    }

    #[test]
    fn a_weak_tag_is_not_the_tag_that_was_generated() {
        let etag = the_etag(&a_digest());
        let weak = format!("W/{etag}");
        assert!(!the_strong_match(&weak, &etag));
    }

    #[test]
    fn a_weak_tag_is_the_same_entity_for_the_weak_comparison() {
        // RFC 9110: `If-None-Match` se compara con la función **débil**,
        // y un `W/` delante dice "comparte la representación". Un cliente
        // que revalida contra una caché manda la etiqueta así.
        let etag = the_etag(&a_digest());
        let weak = format!("W/{etag}");
        assert!(the_weak_match(&weak, &etag));
        assert!(the_weak_match(&etag, &etag));
        assert!(!the_weak_match("\"other\"", &etag));
    }

    #[test]
    fn a_weak_read_with_a_matching_if_none_match_is_not_modified() {
        let etag = the_etag(&a_digest());
        let weak = format!("W/{etag}");
        assert_eq!(
            the_read_condition(None, Some(&weak), Some(&etag)),
            ReadOutcome::NotModified
        );
    }

    #[test]
    fn the_precondition_of_an_if_match_is_the_content_it_names() {
        let digest = a_digest();
        let etag = the_etag(&digest);
        assert_eq!(
            the_precondition(Some(&etag), None),
            Some(Precondition::Present(vec![digest.clone()]))
        );
    }

    #[test]
    fn the_precondition_of_a_star_is_the_absence_or_the_presence() {
        assert_eq!(
            the_precondition(Some("*"), None),
            Some(Precondition::Exists)
        );
        assert_eq!(
            the_precondition(None, Some("*")),
            Some(Precondition::Absent)
        );
    }

    #[test]
    fn a_condition_that_cannot_be_compared_does_not_travel() {
        // Una etiqueta que no es un digest no se puede comparar con
        // nada, y una condición que no se puede comparar no es una
        // condición.
        assert_eq!(the_precondition(Some("\"other\""), None), None);
        assert_eq!(the_precondition(None, Some("\"other\"")), None);
    }

    #[test]
    fn a_read_with_a_matching_if_none_match_is_not_modified() {
        let etag = the_etag(&a_digest());
        assert_eq!(
            the_read_condition(None, Some(&etag), Some(&etag)),
            ReadOutcome::NotModified
        );
    }

    #[test]
    fn a_read_with_a_wrong_if_match_fails() {
        let etag = the_etag(&a_digest());
        assert_eq!(
            the_read_condition(Some("\"other\""), None, Some(&etag)),
            ReadOutcome::Failed
        );
    }

    #[test]
    fn a_read_of_something_that_is_not_there_fails_an_if_match() {
        assert_eq!(
            the_read_condition(Some("*"), None, None),
            ReadOutcome::Failed
        );
    }

    #[test]
    fn a_write_with_a_matching_if_none_match_fails() {
        let etag = the_etag(&a_digest());
        assert_eq!(
            the_write_condition(None, Some(&etag), Some(&etag)),
            WriteOutcome::Failed
        );
    }

    #[test]
    fn a_write_with_a_matching_if_match_goes_on() {
        let etag = the_etag(&a_digest());
        assert_eq!(
            the_write_condition(Some(&etag), None, Some(&etag)),
            WriteOutcome::Write
        );
    }

    #[test]
    fn a_write_with_an_if_match_on_an_empty_path_fails() {
        // `*` quiere decir "el recurso que hay", y si no hay recurso, no
        // hay nada que comparar.
        assert_eq!(
            the_write_condition(Some("*"), None, None),
            WriteOutcome::Failed
        );
    }

    #[test]
    fn a_write_with_an_if_none_match_on_an_empty_path_goes_on() {
        assert_eq!(
            the_write_condition(None, Some("*"), None),
            WriteOutcome::Write
        );
    }
}
