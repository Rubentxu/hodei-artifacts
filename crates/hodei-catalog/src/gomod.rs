//! La gramatica pura del protocolo **GOPROXY** de Go Modules.
//!
//! Un repositorio `go` sirve cuatro cosas por modulo, todas bajo la ruta
//! `{modulo}/@v/`: la lista de versiones (`list`), el documento de version
//! (`.info`), su `go.mod` (`.mod`) y el zip canónico (`.zip`). Ademas
//! existe `{modulo}/@latest`, que senala la ultima version publicada.
//!
//! El protocolo no conoce la escritura: `go` solo lee. Publicar en un
//! hosted es dejar los tres artefactos por la misma ruta con `PUT`, y la
//! lista se ensambla con lo que hay en el catalogo, como en el indice
//! sparse de cargo: nada se reescribe, cada version es para siempre.
//!
//! Las rutas de modulo son sensibles a mayusculas y el protocolo las
//! escapa: `H` se escribe `!h`. Este modulo valida el nombre **ya
//! escapado**, que es la forma canonica con la que se almacena.

use serde_json::{Value, json};

/// Lo que el protocolo no puede aceptar.
#[derive(Debug, PartialEq, Eq)]
pub enum GoError {
    /// La ruta no pertenece a GOPROXY.
    InvalidPath(String),
    /// El nombre de modulo no es un camino de modulo escapado valido.
    InvalidModule(String),
    /// La version no es un semver con `v`.
    InvalidVersion(String),
    /// El documento publicado no cumple lo que su ruta anuncia.
    InvalidDocument(String),
}

impl GoError {
    /// Lo que se le dice al cliente.
    pub fn message(&self) -> String {
        match self {
            Self::InvalidPath(what) => {
                format!("the path is not part of the GOPROXY protocol: {what}")
            }
            Self::InvalidModule(what) => format!("not a valid module path: {what}"),
            Self::InvalidVersion(what) => format!("not a valid module version: {what}"),
            Self::InvalidDocument(what) => format!("the published document is not usable: {what}"),
        }
    }

    /// Una ruta mala es un 404: el cliente no puede distinguir "aqui no
    /// hay nada" de "aqui no se habla GOPROXY", y ningun mapa de rutas
    /// publica la estructura del servicio.
    pub fn is_path_error(&self) -> bool {
        matches!(self, Self::InvalidPath(_))
    }
}

/// Lo que el cliente puede pedir en `/go/{repositorio}/...`.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// Las versiones del modulo: `GET {modulo}/@v/list`.
    List {
        /// El camino del modulo, en su forma escapada.
        module: String,
    },
    /// El documento de una version: `GET {modulo}/@v/{version}.info`.
    Info {
        /// El camino del modulo, en su forma escapada.
        module: String,
        /// La version pedida.
        version: String,
    },
    /// El `go.mod` de una version: `GET {modulo}/@v/{version}.mod`.
    Mod {
        /// El camino del modulo, en su forma escapada.
        module: String,
        /// La version pedida.
        version: String,
    },
    /// El zip del modulo: `GET {modulo}/@v/{version}.zip`.
    Zip {
        /// El camino del modulo, en su forma escapada.
        module: String,
        /// La version pedida.
        version: String,
    },
    /// La ultima version: `GET {modulo}/@latest`.
    Latest {
        /// El camino del modulo, en su forma escapada.
        module: String,
    },
}

/// El tipo de contenido con el que se sirve cada artefacto.
pub fn content_type(extension: &str) -> &'static str {
    match extension {
        "info" => "application/json",
        "mod" => "text/plain; charset=utf-8",
        "zip" => "application/zip",
        _ => "text/plain; charset=utf-8",
    }
}

impl Route {
    /// La ruta del protocolo tras `{repositorio}/`, sin decodificar.
    pub fn parse(rest: &str) -> Result<Route, GoError> {
        let segments = split(rest)?;
        if segments.is_empty() {
            return Err(GoError::InvalidPath("an empty path".into()));
        }
        if segments.last().is_some_and(|last| *last == "@latest") {
            let module = join_module(&segments[..segments.len() - 1])?;
            return Ok(Route::Latest { module });
        }
        let at = segments
            .iter()
            .rposition(|segment| *segment == "@v")
            .ok_or_else(|| GoError::InvalidPath("the path has no @v segment".into()))?;
        if at + 1 != segments.len() - 1 {
            return Err(GoError::InvalidPath(
                "there is exactly one file after @v".into(),
            ));
        }
        let module = join_module(&segments[..at])?;
        let file = segments[at + 1];
        if file == "list" {
            return Ok(Route::List { module });
        }
        for extension in ["info", "mod", "zip"] {
            if let Some(version) = file.strip_suffix(&format!(".{extension}")) {
                let version = valid_version(version)?;
                return Ok(match extension {
                    "info" => Self::Info { module, version },
                    "mod" => Self::Mod { module, version },
                    _ => Self::Zip { module, version },
                });
            }
        }
        Err(GoError::InvalidPath(format!(
            "{file:?} is neither list nor a .info, .mod or .zip version file"
        )))
    }

    /// La ruta donde vive el artefacto publicado de esta version, o `None`
    /// si la ruta no habla de una version concreta.
    pub fn asset_path(&self) -> Option<String> {
        match self {
            Self::Info { module, version }
            | Self::Mod { module, version }
            | Self::Zip { module, version } => {
                let extension = match self {
                    Self::Info { .. } => "info",
                    Self::Mod { .. } => "mod",
                    _ => "zip",
                };
                Some(format!("{module}/@v/{version}.{extension}"))
            }
            Self::List { module } | Self::Latest { module } => Some(format!("{module}/@v/")),
        }
    }

    /// El modulo que la ruta busca, si es que busca uno.
    pub fn module(&self) -> &str {
        match self {
            Self::List { module }
            | Self::Info { module, .. }
            | Self::Mod { module, .. }
            | Self::Zip { module, .. }
            | Self::Latest { module } => module,
        }
    }
}

/// Un camino de modulo escapado: segmentos de minusculas ASCII con sus
/// puntos, guiones y guiones bajos, y las mayusculas de la forma `!x`.
///
/// Se permite el dos en el primer segmento porque los hosts con puerto
/// (`localhost:8080/folder`) son caminos de modulo validos.
pub fn valid_module(module: &str) -> Result<String, GoError> {
    if module.is_empty() || module.len() > 512 {
        return Err(GoError::InvalidModule(module.to_string()));
    }
    for segment in module.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment == "@v" {
            return Err(GoError::InvalidModule(module.to_string()));
        }
        let mut escaped = false;
        for byte in segment.bytes() {
            if escaped {
                if !byte.is_ascii_lowercase() {
                    return Err(GoError::InvalidModule(module.to_string()));
                }
                escaped = false;
            } else if byte == b'!' {
                escaped = true;
            } else if byte.is_ascii_uppercase() {
                // El `go` nunca pide una capital sin escapar: la escapa
                // como `!x`. Una ruta con la mayuscula cruda no es un
                // error de sintaxis que el cliente pueda corregir
                // reintentando: es un camino que no nombra nada, y se
                // responde como lo que es para el protocolo: un 404.
                // Aceptarla como nombre ademas crearia un alias del
                // modulo escapado, y un alias es otra forma de que la
                // misma version exista dos sitios distintos.
                return Err(GoError::InvalidPath(format!(
                    "the module path {module:?} carries a capital that is not escaped"
                )));
            } else if !(byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'-' | b'_' | b'~' | b':'))
            {
                return Err(GoError::InvalidModule(module.to_string()));
            }
        }
        if escaped || segment.starts_with('.') || segment.ends_with('.') {
            return Err(GoError::InvalidModule(module.to_string()));
        }
    }
    Ok(module.to_string())
}

/// Una version de modulo: la `v` y un semver estricto.
pub fn valid_version(version: &str) -> Result<String, GoError> {
    if parse_version(version).is_none() {
        return Err(GoError::InvalidVersion(version.to_string()));
    }
    Ok(version.to_string())
}

/// El documento `.info` canónico: la version y el momento en que se subio
/// por primera vez al registro, en RFC 3339.
pub fn info_document(version: &str, time: &str) -> Result<Value, GoError> {
    let version = valid_version(version)?;
    chrono::DateTime::parse_from_rfc3339(time)
        .map_err(|_| GoError::InvalidDocument(format!("the time {time:?} is not RFC 3339")))?;
    Ok(json!({ "Version": version, "Time": time }))
}

/// Comprueba un `.info` publicado contra la version que anuncia su ruta.
pub fn check_info(body: &[u8], version: &str) -> Result<(), GoError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| GoError::InvalidDocument("the info document is not JSON".into()))?;
    if value["Version"].as_str() != Some(version) {
        return Err(GoError::InvalidDocument(format!(
            "the info document does not declare version {version}"
        )));
    }
    let time = value["Time"].as_str().unwrap_or_default();
    chrono::DateTime::parse_from_rfc3339(time)
        .map_err(|_| GoError::InvalidDocument("the info time is not RFC 3339".into()))?;
    Ok(())
}

/// La forma sin escapar de un camino de modulo: `!x` vuelve a `X`. Es la
/// forma con la que el `go.mod` y el interior del zip nombran al modulo.
pub fn unescape(module: &str) -> String {
    let bytes = module.as_bytes();
    let mut out = String::with_capacity(module.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'!' && index + 1 < bytes.len() {
            out.push(bytes[index + 1].to_ascii_uppercase() as char);
            index += 2;
        } else {
            out.push(bytes[index] as char);
            index += 1;
        }
    }
    out
}

/// Comprueba un `.mod` publicado contra el modulo que anuncia su ruta: la
/// primera directiva `module` del fichero tiene que decir exactamente lo
/// mismo. Un `.mod` mudo o mentiroso no entra: el `go` lo usaria para
/// resolver dependencias con otra identidad.
pub fn check_mod(body: &[u8], module: &str) -> Result<(), GoError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| GoError::InvalidDocument("the go.mod is not utf-8".into()))?;
    let declared = declared_module(text)
        .ok_or_else(|| GoError::InvalidDocument("the go.mod declares no module".into()))?;
    if declared != unescape(module) {
        return Err(GoError::InvalidDocument(format!(
            "the go.mod declares {declared:?}, the route says {module:?}"
        )));
    }
    Ok(())
}

/// El modulo que declara un `go.mod`, sin escapados: la directiva puede
/// escribirse con mayusculas porque el `go.mod` vive dentro del zip y el
/// camino de la ruta vive fuera.
pub fn declared_module(go_mod: &str) -> Option<String> {
    for line in go_mod.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module") {
            let declared = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            return (!declared.is_empty()).then_some(declared);
        }
    }
    None
}

/// El numero de version sin la `v` para ordenar: el listado de un proxy
/// devuelve versiones ya validas; lo que no lo es se ignora en silencio,
/// como hace el propio `go`.
pub fn parse_version(version: &str) -> Option<semver::Version> {
    semver::Version::parse(version.strip_prefix('v')?).ok()
}

fn split(rest: &str) -> Result<Vec<&str>, GoError> {
    let mut segments = Vec::new();
    for segment in rest.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                return Err(GoError::InvalidPath(
                    "the path escapes the repository".into(),
                ));
            }
            other => segments.push(other),
        }
    }
    Ok(segments)
}

fn join_module(segments: &[&str]) -> Result<String, GoError> {
    let module = segments.join("/");
    valid_module(&module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_are_the_four_artifacts_and_latest() {
        assert_eq!(
            Route::parse("example.com/hello/@v/list").expect("list"),
            Route::List {
                module: "example.com/hello".into()
            }
        );
        assert_eq!(
            Route::parse("example.com/hello/@v/v1.0.0.info").expect("info"),
            Route::Info {
                module: "example.com/hello".into(),
                version: "v1.0.0".into()
            }
        );
        assert_eq!(
            Route::parse("example.com/hello/@v/v1.0.0.mod").expect("mod"),
            Route::Mod {
                module: "example.com/hello".into(),
                version: "v1.0.0".into()
            }
        );
        assert_eq!(
            Route::parse("github.com/!burnt!sushi/toml/@v/v1.3.2.zip").expect("zip"),
            Route::Zip {
                module: "github.com/!burnt!sushi/toml".into(),
                version: "v1.3.2".into()
            }
        );
        assert_eq!(
            Route::parse("example.com/hello/@latest").expect("latest"),
            Route::Latest {
                module: "example.com/hello".into()
            }
        );
    }

    #[test]
    fn pseudo_versions_and_incompatible_are_versions_too() {
        for version in [
            "v0.0.0-20191109021931-daa7c04131f0",
            "v2.0.0+incompatible",
            "v1.0.0-rc.1",
        ] {
            let route = Route::parse(&format!("example.com/m/@v/{version}.mod"))
                .unwrap_or_else(|error| panic!("{version} must parse: {error:?}"));
            assert_eq!(
                route.asset_path(),
                Some(format!("example.com/m/@v/{version}.mod"))
            );
        }
    }

    #[test]
    fn wild_paths_are_not_goproxy() {
        for hostile in [
            "",
            "example.com/hello",
            "example.com/hello/@v",
            "example.com/hello/@v/v1.0.0.0.0",
            "example.com/hello/@v/v1.0.0.rar",
            "example.com/hello/@v/1.0.0.info",
            "example.com/../@v/list",
            "example.com/@v/v1.0.0.info/extra",
        ] {
            assert!(
                Route::parse(hostile).is_err(),
                "{hostile:?} must be refused as a GOPROXY path"
            );
        }
    }

    #[test]
    fn modules_are_escaped_lowercase_paths() {
        assert_eq!(
            valid_module("golang.org/x/text").expect("plain"),
            "golang.org/x/text"
        );
        assert_eq!(
            valid_module("github.com/!burnt!sushi/toml").expect("escaped"),
            "github.com/!burnt!sushi/toml"
        );
        assert_eq!(
            valid_module("localhost:8080/folder/mod").expect("port"),
            "localhost:8080/folder/mod"
        );
        for hostile in [
            "",
            "Upper/Case",
            "example.com/un!escaped!",
            "example.com/!uPPER",
            "example.com//hello",
            "example.com/./hello",
            "example.com/@v",
        ] {
            assert!(
                valid_module(hostile).is_err(),
                "{hostile:?} must be refused"
            );
        }
    }

    #[test]
    fn the_info_document_is_version_and_time() {
        let document = info_document("v1.2.3", "2026-11-17T12:00:00Z").expect("document");
        assert_eq!(document["Version"], "v1.2.3");
        assert!(info_document("v1.2.3", "ayer").is_err());
        assert!(info_document("1.2.3", "2026-11-17T12:00:00Z").is_err());

        check_info(document.to_string().as_bytes(), "v1.2.3").expect("consistent");
        assert!(check_info(document.to_string().as_bytes(), "v9.9.9").is_err());
    }

    #[test]
    fn the_go_mod_must_declare_the_module_of_its_route() {
        check_mod(
            b"module example.com/hello\n\ngo 1.22\n",
            "example.com/hello",
        )
        .expect("mod");
        // El go.mod escribe el nombre real; la ruta, la forma escapada.
        check_mod(
            b"module github.com/BurntSushi/toml\n",
            "github.com/!burnt!sushi/toml",
        )
        .expect("the escape is folded before comparing");
        assert_eq!(
            unescape("github.com/!burnt!sushi/toml"),
            "github.com/BurntSushi/toml"
        );
        assert_eq!(unescape("example.com/plain"), "example.com/plain");
        assert!(check_mod(b"go 1.22\n", "example.com/hello").is_err());
        assert_eq!(
            declared_module("module \"quoted/x\"\ngo 1.22").as_deref(),
            Some("quoted/x")
        );
    }

    #[test]
    fn versions_order_like_the_go_tool() {
        let versions = ["v1.0.0", "v1.10.0", "v1.2.0"];
        let mut by_semver: Vec<_> = versions
            .iter()
            .filter_map(|version| parse_version(version).map(|parsed| (parsed, *version)))
            .collect();
        by_semver.sort();
        assert_eq!(
            by_semver
                .iter()
                .map(|(_, version)| *version)
                .collect::<Vec<_>>(),
            vec!["v1.0.0", "v1.2.0", "v1.10.0"],
            "the list is ordered by semver, not by bytes"
        );
    }

    #[test]
    fn content_types_answer_what_the_protocol_promises() {
        assert_eq!(content_type("info"), "application/json");
        assert_eq!(content_type("mod"), "text/plain; charset=utf-8");
        assert_eq!(content_type("zip"), "application/zip");
    }
}
