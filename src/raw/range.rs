//! La petición de un tramo del contenido.
//!
//! RFC 9110 define el tramado de bytes. Lo que este modulo decide es
//! **que se sirve**: un tramo válido se sirve con 206 y su
//! `Content-Range`, un tramo que no se puede satisfacer se dice con 416
//! y el `Content-Range` que dice cuanto tiene el contenido, y un `Range`
//! que no se entiende se ignora, que es lo que el RFC permite.
//!
//! Lo que NO se hace es servir un tramo a medias: un 206 con menos
//! bytes de los que pedia el cliente es un 206 que no significa nada.

/// Lo que el cliente pide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeRequest {
    /// No hay tramo: se sirve el contenido entero.
    Whole,
    /// Un tramo, con el primer byte y el último byte **incluidos**.
    Single { first: u64, last: u64 },
}

/// Por que un tramo no se puede servir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeError {
    /// El tramo no cabe en el contenido, o no esta bien escrito.
    Unsatisfiable,
}

impl RangeError {
    /// Lo que se le dice al cliente.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Unsatisfiable => "the requested range is not satisfiable",
        }
    }
}

/// Que tramo pide la cabecera `Range`.
///
/// El contenido entero se responde con `Whole`. Un `Range` con otra
/// unidad, o con varios tramos, se **ignora**: RFC 9110 lo permite, y
/// un servidor que no sabe hacer lo que le piden puede decir que no
/// sabe, pero no puede decir una verdad a medias.
pub fn the_range(header: Option<&str>, length: u64) -> Result<RangeRequest, RangeError> {
    let Some(header) = header else {
        return Ok(RangeRequest::Whole);
    };
    let header = header.trim();
    // La unidad no distingue mayúsculas (RFC 9110): `Bytes=0-9` es un
    // tramo, no otra unidad. Lo que se compara es el nombre de la
    // unidad, no cómo está escrito.
    let Some((unit, specification)) = header.split_once('=') else {
        // Ni siquiera tiene la forma de una unidad: no se entiende, y lo
        // que no se entiende se ignora.
        return Ok(RangeRequest::Whole);
    };
    if !unit.trim().eq_ignore_ascii_case("bytes") {
        // Otra unidad: no se entiende, y lo que no se entiende se ignora.
        return Ok(RangeRequest::Whole);
    }
    let specification = specification.trim();
    if specification.contains(',') {
        // Varios tramos: esta release sirve uno. Se ignora la petición y
        // se sirve el contenido entero, que es lo que permite el RFC.
        return Ok(RangeRequest::Whole);
    }
    let Some((first, last)) = specification.split_once('-') else {
        return Err(RangeError::Unsatisfiable);
    };
    let first = first.trim();
    let last = last.trim();

    if first.is_empty() {
        // `bytes=-N`: los últimos N bytes.
        let suffix: u64 = last.parse().map_err(|_| RangeError::Unsatisfiable)?;
        if suffix == 0 || length == 0 {
            return Err(RangeError::Unsatisfiable);
        }
        let first = length.saturating_sub(suffix);
        return Ok(RangeRequest::Single {
            first,
            last: length - 1,
        });
    }

    let first: u64 = first.parse().map_err(|_| RangeError::Unsatisfiable)?;
    let last: u64 = if last.is_empty() {
        // `bytes=N-`: desde N hasta el final.
        if length == 0 {
            return Err(RangeError::Unsatisfiable);
        }
        length - 1
    } else {
        last.parse().map_err(|_| RangeError::Unsatisfiable)?
    };

    if first > last || first >= length {
        // El RFC dice que un rango invalido puede ignorarse, y uno que no
        // se puede satisfacer se responde con 416. Los dos casos se
        // responden igual, porque en los dos el cliente pidió algo que
        // no existe.
        return Err(RangeError::Unsatisfiable);
    }
    Ok(RangeRequest::Single {
        first,
        last: last.min(length - 1),
    })
}

/// La cabecera `Content-Range` de un tramo.
pub fn the_content_range(first: u64, last: u64, length: u64) -> String {
    format!("bytes {first}-{last}/{length}")
}

/// La cabecera `Content-Range` de un contenido que no se puede servir
/// el tramo pedido.
pub fn the_unsatisfiable_range(length: u64) -> String {
    format!("bytes */{length}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_range_the_whole_content_is_served() {
        assert_eq!(the_range(None, 100), Ok(RangeRequest::Whole));
        assert_eq!(
            the_range(Some("bytes=0-9"), 100),
            Ok(RangeRequest::Single { first: 0, last: 9 })
        );
    }

    #[test]
    fn the_unit_of_the_range_does_not_distinguish_capitals() {
        // RFC 9110: los nombres de las unidades no distinguen mayúsculas.
        assert_eq!(
            the_range(Some("Bytes=0-9"), 100),
            Ok(RangeRequest::Single { first: 0, last: 9 })
        );
        assert_eq!(
            the_range(Some("BYTES=0-9"), 100),
            Ok(RangeRequest::Single { first: 0, last: 9 })
        );
        // Pero otra unidad sigue siendo otra unidad.
        assert_eq!(the_range(Some("items=0-9"), 100), Ok(RangeRequest::Whole));
    }

    #[test]
    fn a_range_that_starts_and_ends_is_served() {
        assert_eq!(
            the_range(Some("bytes=10-19"), 100),
            Ok(RangeRequest::Single {
                first: 10,
                last: 19
            })
        );
    }

    #[test]
    fn a_range_without_an_end_goes_to_the_end() {
        assert_eq!(
            the_range(Some("bytes=90-"), 100),
            Ok(RangeRequest::Single {
                first: 90,
                last: 99
            })
        );
    }

    #[test]
    fn a_suffix_range_takes_the_last_bytes() {
        assert_eq!(
            the_range(Some("bytes=-10"), 100),
            Ok(RangeRequest::Single {
                first: 90,
                last: 99
            })
        );
    }

    #[test]
    fn a_range_that_past_the_end_is_not_satisfiable() {
        assert_eq!(
            the_range(Some("bytes=100-200"), 100),
            Err(RangeError::Unsatisfiable)
        );
        assert_eq!(
            the_range(Some("bytes=100-"), 100),
            Err(RangeError::Unsatisfiable)
        );
    }

    #[test]
    fn an_invalid_range_is_not_satisfiable() {
        assert_eq!(
            the_range(Some("bytes=abc"), 100),
            Err(RangeError::Unsatisfiable)
        );
        assert_eq!(
            the_range(Some("bytes=10-5"), 100),
            Err(RangeError::Unsatisfiable)
        );
    }

    #[test]
    fn a_range_in_another_unit_is_ignored() {
        assert_eq!(the_range(Some("items=0-9"), 100), Ok(RangeRequest::Whole));
    }

    #[test]
    fn several_ranges_are_ignored_and_the_whole_is_served() {
        assert_eq!(
            the_range(Some("bytes=0-9,20-29"), 100),
            Ok(RangeRequest::Whole)
        );
    }

    #[test]
    fn the_content_range_says_what_is_served() {
        assert_eq!(the_content_range(0, 9, 100), "bytes 0-9/100");
        assert_eq!(the_unsatisfiable_range(100), "bytes */100");
    }
}
