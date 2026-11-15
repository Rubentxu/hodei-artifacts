//! Las reglas de la ruta de un contenido crudo.
//!
//! Una ruta es lo que dice el cliente, y lo que dice el cliente es lo
//! que va a acabar en el disco. Por eso las reglas son **estrictas** y
//! se aplican antes de tocar nada:
//!
//! - La ruta tiene que estar bajo `/repository/`.
//! - La clave del repositorio tiene que ser un nombre, no una ruta.
//! - Cada segmento tiene que ser un `pchar` de RFC 3986: ni barras, ni
//!   barras invertidas, ni los delimitadores que le cambian el sentido a
//!   la referencia, ni el espacio, y sin los segmentos `.` y `..`.
//! - Un segmento se decodifica **una** vez, y la forma decodificada tiene
//!   que ser un segmento válido: como el símbolo `%` no es un `pchar`, un
//!   doble codificado es una forma que todavía lleva codificación, y una
//!   forma que todavía lleva codificación no es un nombre.
//!
//! Lo que se acepta es exactamente lo que se puede nombrar sin
//! ambigüedad. Una ruta que dos sistemas interpretan de manera distinta
//! es una ruta que un día Bytes y otro día Es un fichero distinto, y un
//! registro de artefactos que guarda una ruta que no puede comparar es un
//! registro con diferencias silenciosas.

/// Lo que se rechaza de una ruta, y por que.
///
/// Los errores son **distintos** a propósito: quién recibe un 400 sabe
/// si la ruta no era de un repositorio crudo, si el nombre del
/// repositorio no era válido, o si el problema estaba en un segmento.
/// Un error que no dice nada es un error que hay que adivinar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawPathError {
    /// La ruta no está bajo `/repository/`.
    NotRaw,
    /// El nombre del repositorio no es un nombre.
    InvalidRepository,
    /// No hay ruta de contenido dentro del repositorio.
    EmptyPath,
    /// La ruta es más larga de lo que se admite.
    TooLong,
    /// Un segmento no es un segmento válido.
    InvalidSegment,
    /// La ruta no está bien codificada.
    BadEncoding,
}

impl RawPathError {
    /// Lo que se le dice al cliente.
    ///
    /// No dice **dónde** está el problema en la ruta: un cliente que
    /// recibe el error sabe que su ruta no es válida, y no recibe una
    /// description de cómo está escrita su ruta para que la pruebe con
    /// otras variantes.
    pub fn message(&self) -> &'static str {
        match self {
            Self::NotRaw => "the path is not a raw repository path",
            Self::InvalidRepository => "the name of the repository is not valid",
            Self::EmptyPath => "the path of the content is empty",
            Self::TooLong => "the path of the content is too long",
            Self::InvalidSegment => "the path of the content is not valid",
            Self::BadEncoding => "the path of the content is not correctly encoded",
        }
    }
}

/// Donde vive un contenido: el repositorio y la ruta dentro de el.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTarget {
    /// La clave del repositorio.
    pub repository: String,
    /// La ruta dentro del repositorio.
    pub asset_path: String,
}

/// El prefijo de la superficie de contenido crudo.
const RAW_PREFIX: &str = "/repository/";

/// Cuántos segmentos como máximo tiene una ruta.
const MAX_SEGMENTS: usize = 64;

/// Cuántos bytes como máximo tiene un segmento.
const MAX_SEGMENT_BYTES: usize = 255;

/// Cuántos bytes como máximo tiene la ruta entera.
const MAX_PATH_BYTES: usize = 1024;

/// La ruta pedida, leída del URI crudo.
///
/// El URI **crudo** es el que llegó, sin decodificar: si se usará el
/// decodificado, un `%2e%2e` ya habría pasado a ser `..` y no se podría
/// ni ver ni refutar.
pub fn the_target(raw_path: &str) -> Result<RawTarget, RawPathError> {
    let rest = raw_path
        .strip_prefix(RAW_PREFIX)
        .ok_or(RawPathError::NotRaw)?;
    if rest.is_empty() {
        return Err(RawPathError::EmptyPath);
    }

    let raw_segments: Vec<&str> = rest.split('/').collect();
    let (raw_repository, raw_asset) = raw_segments.split_first().ok_or(RawPathError::EmptyPath)?;

    // El nombre del repositorio es un **nombre**: no lleva barra, ni
    // escapado, ni nada que lo convierta en otra cosa. Un nombre no
    // admite `%`, así que no hay nada que decodificar: se comprueba tal
    // como llega.
    if !valid_repository_name(raw_repository) {
        return Err(RawPathError::InvalidRepository);
    }
    let repository = raw_repository.to_string();

    // Una barra final no es un segmento: es una ruta sin contenido.
    if raw_asset.len() == 1 && raw_asset[0].is_empty() {
        return Err(RawPathError::EmptyPath);
    }
    if raw_asset.len() > MAX_SEGMENTS {
        return Err(RawPathError::TooLong);
    }

    let mut segments = Vec::with_capacity(raw_asset.len());
    for raw_segment in raw_asset {
        let segment = decode_segment(raw_segment, valid_segment)?;
        segments.push(segment);
    }

    let asset_path = segments.join("/");
    if asset_path.len() > MAX_PATH_BYTES {
        return Err(RawPathError::TooLong);
    }

    Ok(RawTarget {
        repository,
        asset_path,
    })
}

/// Un segmento de la ruta: un `pchar` de RFC 3986.
///
/// Son los caracteres que pueden ir en un segmento de una URI sin
/// codificar: los no reservados, los sub-delimitadores, y `:` y `@`.
///
/// Ni barra ni barra invertida: las dos son separadores en algun
/// sistema, y un nombre que es un separador en un sistema es un
/// traversal en ese sistema. Ni `#` ni `?`: son los delimitadores que
/// le cambian el sentido a la referencia, y un `Location` que apunta a
/// otro recurso es peor que un `Location` que no existe. Ni el espacio:
/// el mismo nombre escrito de dos maneras es dos nombres.
///
/// La lista no es arbitraria: es exactamente la que hace que la ruta sea
/// una **referencia URI** sin ambigüedad.
fn valid_segment(segment: &str) -> bool {
    if segment.is_empty() || segment.len() > MAX_SEGMENT_BYTES {
        return false;
    }
    if segment == "." || segment == ".." {
        return false;
    }
    segment.chars().all(is_a_pchar)
}

/// Un `pchar` de RFC 3986.
///
/// Un `pchar` es un carácter no reservado, un sub-delimitador, `:` o
/// `@`. Los no reservados son alfanuméricos y `-._~`, y los
/// sub-delimitadores son `!$&'()*+,;=`.
fn is_a_pchar(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '-' | '.'
                | '_'
                | '~'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
                | ':'
                | '@'
        )
}

/// Un nombre de repositorio.
///
/// El mismo criterio que usa la superficie administrativa: un nombre es
/// un nombre, y un nombre con barras es una ruta disfrazada.
pub fn valid_repository_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

/// Decodifica un segmento, y comprueba que no esconde nada.
///
/// Un segmento llega **codificado**: `%2f` es una barra, `%20` es un
/// espacio y `%25` es un símbolo. El nombre que se guarda es el
/// **decodificado**, y ese nombre tiene que ser un `pchar` entero: ni
/// una barra, ni un espacio, ni un símbolo que otro sistema pueda
/// descodificar otra vez.
///
/// Se decodifica **una** vez, y solo una. Como el símbolo `%` no es un
/// `pchar`, una forma que todavía lleva codificación —un doble
/// codificado— es una forma que **no** es un nombre, y se rechaza:
/// `%252e%252e%252f` se decodifica a `%2e%2e%2f`, y ahí se para.
///
/// Esa es la regla: lo que se guarda es siempre un nombre literal, y un
/// nombre literal no se puede leer de dos maneras.
fn decode_segment(raw: &str, valid: fn(&str) -> bool) -> Result<String, RawPathError> {
    let decoded = decode_once(raw)?;
    if !valid(&decoded) {
        return Err(RawPathError::InvalidSegment);
    }
    Ok(decoded)
}

/// Una pasada de decodificación percent.
///
/// Solo se acepta `%XX` con dos dígitos hexadecimales. El `+` de una
/// cadena de consulta **no** es un espacio aquí: en una ruta es un
/// carácter más.
fn decode_once(text: &str) -> Result<String, RawPathError> {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1).ok_or(RawPathError::BadEncoding)?;
            let low = *bytes.get(index + 2).ok_or(RawPathError::BadEncoding)?;
            let high = hex_value(high).ok_or(RawPathError::BadEncoding)?;
            let low = hex_value(low).ok_or(RawPathError::BadEncoding)?;
            out.push(high * 16 + low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| RawPathError::BadEncoding)
}

/// El valor de un dígito hexadecimal.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Atajo para leer el resultado como texto de error.
    fn the_error(path: &str) -> RawPathError {
        the_target(path).expect_err("the path is refused")
    }

    #[test]
    fn a_well_formed_path_is_read() {
        let target = the_target("/repository/releases/path/to/file.bin").expect("the path is read");
        assert_eq!(target.repository, "releases");
        assert_eq!(target.asset_path, "path/to/file.bin");
    }

    #[test]
    fn a_path_outside_the_raw_surface_is_refused() {
        assert_eq!(the_error("/api/v1/repositories"), RawPathError::NotRaw);
        assert_eq!(the_error("/repository"), RawPathError::NotRaw);
    }

    #[test]
    fn the_traversal_is_refused() {
        assert_eq!(
            the_error("/repository/releases/../etc/passwd"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/a/../../b"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/./a"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_double_decoding_is_refused() {
        // `%252e%252e%252f` es `../` con una capa de más.
        assert_eq!(
            the_error("/repository/releases/%252e%252e%252fetc"),
            RawPathError::InvalidSegment
        );
        // Y la barra codificada también: un segmento que al decodificar
        // trae una barra es dos segmentos, y el cliente no pidió eso.
        assert_eq!(
            the_error("/repository/releases/a%2fb"),
            RawPathError::InvalidSegment
        );
        // Y la barra codificada dos veces: `a%252fb` es `a/b` con una
        // capa de más. La forma intermedia (`a%2fb`) es un segmento
        // válido, y por eso el Segmento tiene que comprobar **cada**
        // forma, no solo la última.
        assert_eq!(
            the_error("/repository/releases/a%252fb"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_control_characters_are_refused() {
        assert_eq!(
            the_error("/repository/releases/a%00b"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/a%09b"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_backslash_is_refused() {
        assert_eq!(
            the_error("/repository/releases/a%5cb"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_ambiguous_unicode_is_refused() {
        // Una ruta con caracteres fuera de ASCII se puede escribir de
        // varias maneras, y no todas dicen lo mismo.
        assert_eq!(
            the_error("/repository/releases/caf%C3%A9"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_bad_encoding_is_refused() {
        assert_eq!(
            the_error("/repository/releases/a%zzb"),
            RawPathError::BadEncoding
        );
        assert_eq!(
            the_error("/repository/releases/a%2"),
            RawPathError::BadEncoding
        );
    }

    #[test]
    fn the_empty_path_is_refused() {
        assert_eq!(the_error("/repository/releases/"), RawPathError::EmptyPath);
    }

    #[test]
    fn the_invalid_repository_name_is_refused() {
        assert_eq!(
            the_error("/repository/rel%2Fases/a"),
            RawPathError::InvalidRepository
        );
        assert_eq!(
            the_error("/repository/re leases/a"),
            RawPathError::InvalidRepository
        );
    }

    #[test]
    fn the_too_long_path_is_refused() {
        let long = "a".repeat(300);
        assert_eq!(
            the_error(&format!("/repository/releases/{long}")),
            RawPathError::InvalidSegment
        );
        let many = vec!["a"; 70].join("/");
        assert_eq!(
            the_error(&format!("/repository/releases/{many}")),
            RawPathError::TooLong
        );
    }

    #[test]
    fn the_encoded_space_is_refused() {
        // El espacio no es un carácter imprimible ASCII válido en una
        // ruta de contenido: el mismo nombre escrito de dos maneras es
        // dos nombres.
        assert_eq!(
            the_error("/repository/releases/a%20b"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_delimiters_that_change_the_meaning_are_refused() {
        // Un `#` o un `?` en un segmento hacen que la ruta sea otra cosa
        // cuando se lee como una referencia: `/repository/repos/a?b` es
        // `/repository/repos/a` con una consulta. Y un `Location` que
        // apunta a otro recurso es peor que un `Location` que no existe.
        assert_eq!(
            the_error("/repository/releases/a%23b"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/a%3fb"),
            RawPathError::InvalidSegment
        );
        // Los corchetes solo valen en el anfitrión de una URI.
        assert_eq!(
            the_error("/repository/releases/a%5Bb"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/a%5Db"),
            RawPathError::InvalidSegment
        );
        // Y las comillas no son un `pchar`.
        assert_eq!(
            the_error("/repository/releases/a%22b"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_characters_of_a_uri_segment_are_accepted() {
        // Los `pchar` de RFC 3986: los que un cliente escribe de verdad.
        let target = the_target("/repository/releases/informe+final_v2.1@2024.tar.gz")
            .expect("la ruta se lee");
        assert_eq!(target.asset_path, "informe+final_v2.1@2024.tar.gz");
    }

    #[test]
    fn an_encoded_character_is_the_character_it_encodes() {
        // `%41` es `A`: lo que se guarda es el nombre literal, y no hay
        // dos nombres para lo mismo. Un cliente que codifica lo que no
        // hace falta codificar no está pidiendo otra cosa.
        let target = the_target("/repository/releases/a%41b").expect("la ruta se lee");
        assert_eq!(target.asset_path, "aAb");
        // Y un doble codificado no es un nombre: `%2541` se decodifica a
        // `%41`, que todavía lleva la codificación dentro.
        assert_eq!(
            the_error("/repository/releases/a%2541b"),
            RawPathError::InvalidSegment
        );
    }
}
