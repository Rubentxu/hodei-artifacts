//! El registro **Cargo** en su variante sparse: la gramatica de rutas, del
//! payload de publicacion y de las lineas de indice.
//!
//! Es puro: conoce el protocolo, no el almacen. El indexador solo necesita
//! `serde_json` y `sha2`, las mismas herramientas con las que el cliente
//! calcula el `cksum` que despues verificara al descargar.

use serde_json::{Map, Value, json};
use thiserror::Error;

/// Lo que el protocolo Cargo no admite sin decir por que.
#[derive(Debug, Error)]
pub enum CargoError {
    /// La ruta no es ninguna de las que habla el protocolo.
    #[error("the path is not part of the cargo protocol: {0}")]
    InvalidPath(&'static str),
    /// El nombre del crate no es admisible.
    #[error("the crate name is not admissible: {0}")]
    InvalidName(&'static str),
    /// El payload de publicacion no se entiende.
    #[error("the publish payload is not admissible: {0}")]
    InvalidPayload(&'static str),
}

impl CargoError {
    /// El motivo, tal y como se le dice al cliente.
    pub fn message(&self) -> String {
        match self {
            Self::InvalidPath(reason) => {
                format!("the path is not part of the cargo protocol: {reason}")
            }
            Self::InvalidName(reason) => format!("the crate name is not admissible: {reason}"),
            Self::InvalidPayload(reason) => {
                format!("the publish payload is not admissible: {reason}")
            }
        }
    }
}

/// Lo que el cliente puede pedir en `/cargo/{repositorio}/...`.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// La configuracion del indice sparse: `GET config.json`.
    Config,
    /// La linea de indice de un crate: el camino determinista de sparse.
    Index {
        /// El nombre ya normalizado del crate.
        name: String,
    },
    /// Los bytes de una version: `GET api/v1/crates/{name}/{version}/download`.
    Download {
        /// El nombre del crate tal y como lo pidio el cliente.
        name: String,
        /// La version pedida.
        version: String,
    },
    /// `PUT api/v1/crates/new`.
    Publish,
}

impl Route {
    /// La ruta del protocolo tras `{repositorio}/`, sin decodificar.
    pub fn parse(rest: &str) -> Result<Route, CargoError> {
        let Some(segments) = split(rest) else {
            return Err(CargoError::InvalidPath("the path escapes the repository"));
        };
        if segments.is_empty() {
            return Err(CargoError::InvalidPath("an empty path"));
        }
        if segments.len() == 1 && segments[0] == "config.json" {
            return Ok(Route::Config);
        }
        if segments.len() == 1 {
            let name = index_name(segments[0])?;
            return Ok(Route::Index { name });
        }
        if segments.len() == 2 && segments[0] == "1" && is_single(segments[1]) {
            return Ok(Route::Index {
                name: segments[1].to_string(),
            });
        }
        if segments.len() == 2 && segments[0] == "2" && is_double(segments[1]) {
            return Ok(Route::Index {
                name: segments[1].to_string(),
            });
        }
        if segments.len() == 3
            && segments[0].len() == 2
            && segments[1].len() == 2
            && is_long(segments[2])
            && let lower = segments[2].to_lowercase()
            && lower.starts_with(segments[0])
            && lower[2..].starts_with(segments[1])
        {
            // El camino de los nombres de cuatro o mas: `{ab}/{cd}/{nombre}`.
            return Ok(Route::Index {
                name: segments[2].to_string(),
            });
        }
        if segments.len() == 3
            && segments[0] == "3"
            && segments[1].len() == 1
            && is_triple(segments[2])
            && segments[2].starts_with(segments[1])
        {
            return Ok(Route::Index {
                name: segments[2].to_string(),
            });
        }
        if segments.len() == 6
            && segments[0] == "api"
            && segments[1] == "v1"
            && segments[2] == "crates"
            && segments[5] == "download"
            && is_name(segments[3])
        {
            return Ok(Route::Download {
                name: segments[3].to_string(),
                version: segments[4].to_string(),
            });
        }
        if segments.len() == 4
            && segments[0] == "api"
            && segments[1] == "v1"
            && segments[2] == "crates"
            && segments[3] == "new"
        {
            return Ok(Route::Publish);
        }
        Err(CargoError::InvalidPath(
            "no cargo operation lives at this path",
        ))
    }

    /// El camino sparse determinista del indice para un nombre, relativo al
    /// repositorio. Con esto el servidor y el cliente calculan lo mismo.
    pub fn index_path(name: &str) -> Result<String, CargoError> {
        let name = normalize(name)?;
        Ok(match name.len() {
            1 => format!("1/{name}"),
            2 => format!("2/{name}"),
            3 => format!("3/{}/{}", name[..1].to_lowercase(), name),
            _ => format!("{}/{}/{name}", first_two(&name), first_two(&name[2..])),
        })
    }

    /// El nombre del crate que la ruta busca, si es que busca uno.
    pub fn crate_name(&self) -> Option<&str> {
        match self {
            Route::Index { name } | Route::Download { name, .. } => Some(name),
            Route::Config | Route::Publish => None,
        }
    }

    /// La ruta del catalogo donde vive el .crate de esta version.
    pub fn crate_asset_path(name: &str, version: &str) -> String {
        format!("{name}/-/index/{version}.json")
    }

    /// El prefijo bajo el que viven las versiones publicadas de un crate.
    pub fn version_prefix(name: &str) -> String {
        format!("{name}/-/index/")
    }
}

fn split(rest: &str) -> Option<Vec<&str>> {
    let mut segments = Vec::new();
    for segment in rest.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            other => segments.push(other),
        }
    }
    Some(segments)
}

/// Un nombre de crate valido: alfanumerico ASCII, guiones, maximo 64.
/// Los guiones bajos solo valen en la forma `__`, que normaliza a guion.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Los unicos guiones bajos que el protocolo admite son los dos de `__`.
fn underscores_ok(name: &str) -> bool {
    match name.find('_') {
        None => true,
        Some(at) => name[at..].starts_with("__") && !name[at + 2..].contains('_'),
    }
}

fn is_single(name: &str) -> bool {
    is_name(name) && underscores_ok(name) && name.len() == 1
}

fn is_double(name: &str) -> bool {
    is_name(name) && underscores_ok(name) && name.len() == 2
}

fn is_triple(name: &str) -> bool {
    is_name(name) && underscores_ok(name) && name.len() == 3
}

fn is_long(name: &str) -> bool {
    is_name(name) && underscores_ok(name) && name.len() >= 4
}

/// El nombre normalizado con el que el indice y el almacen conocen al crate:
/// minusculas, y los guiones bajos dobles pasan a guiones simples.
pub fn normalize(name: &str) -> Result<String, CargoError> {
    if !is_name(name) || !underscores_ok(name) {
        return Err(CargoError::InvalidName(
            "a crate name is alphanumeric ASCII with single hyphens, or the well-formed __ form",
        ));
    }
    Ok(name.to_lowercase().replace("__", "-"))
}

/// El camino que pide el cliente resuelto a nombre: los caminos de indice
/// siempre traen su prefijo determinista, menos `config.json`.
fn index_name(_last: &str) -> Result<String, CargoError> {
    Err(CargoError::InvalidPath(
        "index paths arrive with their deterministic prefix",
    ))
}

fn first_two(name: &str) -> String {
    name.chars().take(2).collect()
}

/// Lo que `cargo publish` manda en el cuerpo de `PUT api/v1/crates/new`:
/// cabecera JSON y fichero .crate, cada uno preceded by su longitud u32 LE.
pub struct NewCrate<'a> {
    /// La cabecera tal y como la construyo el cliente desde su Cargo.toml.
    pub header: Map<String, Value>,
    /// Los bytes exactos del .crate: su sha256 es el cksum del indice.
    pub file: &'a [u8],
}

impl std::fmt::Debug for NewCrate<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NewCrate")
            .field("header", &self.header)
            .field("file_len", &self.file.len())
            .finish()
    }
}

/// El documento inmutable que deja una publicacion, version a version.
#[derive(Debug, Clone)]
pub struct VersionDocument {
    /// El nombre normalizado del crate.
    pub name: String,
    /// La version publicada.
    pub version: String,
    /// El JSON interno del indice, con la cabecera y el cksum.
    pub line: Value,
    /// Los bytes del .crate, para el download.
    pub file: Vec<u8>,
}

/// Parte el cuerpo de una publicacion: cabecera JSON y fichero .crate.
pub fn parse_publish(body: &[u8]) -> Result<NewCrate<'_>, CargoError> {
    let (header_bytes, rest) = take_u32_le(body)?;
    let header: Value = serde_json::from_slice(header_bytes)
        .map_err(|_| CargoError::InvalidPayload("the crate header is not valid JSON"))?;
    let header = header
        .as_object()
        .ok_or(CargoError::InvalidPayload(
            "the crate header is not a JSON object",
        ))?
        .clone();
    let (file, rest) = take_u32_le(rest)?;
    if !rest.is_empty() {
        return Err(CargoError::InvalidPayload(
            "the payload has bytes after the crate file",
        ));
    }
    Ok(NewCrate { header, file })
}

fn take_u32_le(bytes: &[u8]) -> Result<(&[u8], &[u8]), CargoError> {
    let len = u32::from_le_bytes(
        bytes
            .get(0..4)
            .ok_or(CargoError::InvalidPayload("the payload is truncated"))?
            .try_into()
            .expect("four bytes"),
    ) as usize;
    bytes
        .get(4..4 + len)
        .map(|head| (head, &bytes[4 + len..]))
        .ok_or(CargoError::InvalidPayload("the payload is truncated"))
}

/// El sha256 en hex minusculas: el mismo campo `cksum` que el cliente
/// calcula antes de publicar y verifica al descargar.
pub fn cksum(bytes: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(bytes).value
}

/// La linea de indice de una version: la cabecera que mando el cliente, mas
/// el cksum que el servicio calcula y el `yanked` que nadie puede cambiar
/// sin reescribir la version.
pub fn index_line(header: &Map<String, Value>, cksum: &str) -> Result<Value, CargoError> {
    let name = header
        .get("name")
        .and_then(Value::as_str)
        .ok_or(CargoError::InvalidPayload("the header has no crate name"))?;
    let version = header
        .get("vers")
        .and_then(Value::as_str)
        .ok_or(CargoError::InvalidPayload("the header has no version"))?;
    let mut line = Map::new();
    line.insert("name".into(), json!(normalize(name)?));
    line.insert("vers".into(), json!(version));
    line.insert(
        "deps".into(),
        header.get("deps").cloned().unwrap_or(json!([])),
    );
    line.insert("cksum".into(), json!(cksum));
    line.insert(
        "features".into(),
        header.get("features").cloned().unwrap_or(json!({})),
    );
    if let Some(features2) = header.get("features2") {
        line.insert("features2".into(), features2.clone());
    }
    line.insert("yanked".into(), json!(false));
    if let Some(links) = header.get("links") {
        line.insert("links".into(), links.clone());
    }
    if let Some(v) = header.get("v") {
        line.insert("v".into(), v.clone());
    }
    Ok(Value::Object(line))
}

/// El documento `config.json` del indice sparse: donde el cliente va a
/// buscar la descarga y la API.
pub fn config_json(base: &str) -> Value {
    json!({
        "dl": format!("{base}/api/v1/crates/{{crate}}/{{version}}/download"),
        "api": base,
        // Sin esto cargo no manda el token en las descargas y el registro
        // privado responde 401: el protocolo lo exige para registros con auth.
        "auth-required": true,
    })
}

/// Una version es la que cargo publica: semver estricto.
pub fn valid_version(version: &str) -> bool {
    semver::Version::parse(version).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_paths_are_the_deterministic_sparse_way() {
        for (name, path) in [
            ("a", "1/a"),
            ("ab", "2/ab"),
            ("abc", "3/a/abc"),
            ("serde", "se/rd/serde"),
            ("hodei-pkg", "ho/de/hodei-pkg"),
        ] {
            assert_eq!(Route::index_path(name).expect("valid"), path, "{name}");
        }
    }

    #[test]
    fn normalization_is_lowercase_and_double_underscore() {
        assert_eq!(normalize("Serde").expect("valid"), "serde");
        assert_eq!(normalize("a__b").expect("valid"), "a-b");
        for bad in [
            "",
            "-lead",
            "double--hyphen",
            "a_c",
            "x!".repeat(64).as_str(),
        ] {
            assert!(normalize(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn routes_are_recognized_and_wild_paths_are_refused() {
        assert_eq!(Route::parse("config.json").expect("cfg"), Route::Config);
        assert_eq!(
            Route::parse("se/rd/serde").expect("idx"),
            Route::Index {
                name: "serde".into()
            }
        );
        let bare = Route::parse("3/abc").expect_err("a three-letter name needs its 3/ prefix");
        assert!(
            bare.message().contains("not part of the cargo protocol"),
            "{bare:?}"
        );
        assert!(matches!(
            Route::parse("se/rd/hodei").unwrap_err(),
            CargoError::InvalidPath(_)
        ));
        assert_eq!(
            Route::parse("api/v1/crates/serde/1.0.0/download").expect("dl"),
            Route::Download {
                name: "serde".into(),
                version: "1.0.0".into()
            }
        );
        assert_eq!(
            Route::parse("api/v1/crates/new").expect("pub"),
            Route::Publish
        );
        for hostile in ["../se/serde", "se/../serde", "api/v1/../../etc"] {
            assert!(
                Route::parse(hostile).is_err(),
                "{hostile:?} must be refused"
            );
        }
    }

    #[test]
    fn a_publish_payload_is_length_prefixed_twice() {
        let header = json!({"name": "hodei-pkg", "vers": "0.1.0", "deps": []});
        let file = b"not really a crate, but bytes".to_vec();
        let body = encode(&header, &file);
        let new = parse_publish(&body).expect("valid payload");
        assert_eq!(new.header["name"], "hodei-pkg");
        assert_eq!(new.file, &file[..]);

        let mut truncated = encode(&header, &file);
        truncated.truncate(truncated.len() - 1);
        assert!(matches!(
            parse_publish(&truncated).unwrap_err(),
            CargoError::InvalidPayload(_)
        ));
    }

    fn encode(header: &Value, file: &[u8]) -> Vec<u8> {
        let json = serde_json::to_vec(header).expect("json");
        let mut body = Vec::new();
        body.extend_from_slice(&(json.len() as u32).to_le_bytes());
        body.extend_from_slice(&json);
        body.extend_from_slice(&(file.len() as u32).to_le_bytes());
        body.extend_from_slice(file);
        body
    }

    #[test]
    fn the_index_line_is_the_header_plus_what_the_service_knows() {
        let header = json!({
            "name": "Serde-Pkg",
            "vers": "1.0.0",
            "deps": [{"name": "libc", "optional": false}],
            "features": {"std": []},
        });
        let line = index_line(header.as_object().expect("object"), "ab12").expect("line");
        assert_eq!(
            line["name"], "serde-pkg",
            "the index knows normalized names"
        );
        assert_eq!(line["cksum"], "ab12");
        assert_eq!(line["yanked"], false);
        assert_eq!(line["deps"][0]["name"], "libc");

        let no_name = json!({"vers": "1.0.0"});
        assert!(matches!(
            index_line(no_name.as_object().expect("object"), "ab"),
            Err(CargoError::InvalidPayload(_))
        ));
    }

    #[test]
    fn config_json_points_at_this_repository() {
        let config = config_json("http://host/cargo/mirror");
        assert_eq!(
            config["dl"],
            "http://host/cargo/mirror/api/v1/crates/{crate}/{version}/download"
        );
        assert_eq!(config["api"], "http://host/cargo/mirror");
        assert_eq!(
            config["auth-required"], true,
            "cargo only sends the token on downloads if declared"
        );
    }

    #[test]
    fn versions_are_strict_semver() {
        assert!(valid_version("1.0.0"));
        assert!(valid_version("0.1.0-alpha.1"));
        assert!(!valid_version("1.0"));
        assert!(!valid_version("two"));
    }
}
