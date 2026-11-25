//! La gramatica **conan** del protocolo de remote de Conan 2.
//!
//! Aqui vive lo **puro**: la tabla de rutas que el cliente Conan 2 habla
//! contra un remote con revisiones, y el mapa entre esas rutas y las rutas
//! de asset del catalogo. El dialecto lo fija el cliente
//! (`conan/internal/rest/rest_routes.py` de Conan 2.20.1) y lo verifico el
//! spike contra un remote de referencia: `conan/internal/rest/client_routes.py`
//! ensambla las URLs sobre `{remote}/v2/`, y el ping vive en `/v1/ping`.
//!
//! La identidad versionada es la del propio protocolo: la **recipe revision**
//! (`rrev`) y la **package revision** (`prev`) son segmentos de ruta opacos
//! que el servidor conserva tal cual. El `package_id` viaja en su propia
//! posicion y los binarios de un `package_id` viven bajo un prefijo que solo
//! es suyo: nada mezcla los bytes de dos `package_id` del mismo recipe.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};

/// El identificador versionado de un recipe: `name/version@user/channel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeRef {
    /// El nombre del paquete conan.
    pub name: String,
    /// La version.
    pub version: String,
    /// El usuario, `_` cuando el cliente no lleva.
    pub user: String,
    /// El canal, `_` cuando el cliente no lleva.
    pub channel: String,
}

impl RecipeRef {
    /// La referencia tal y como la imprime el cliente en los listados.
    ///
    /// `_` significa "sin usuario" y "sin canal": el cliente los vuelve a
    /// quitar al leer.
    pub fn render(&self) -> String {
        format!(
            "{}/{}@{}/{}",
            self.name, self.version, self.user, self.channel
        )
    }

    /// El prefijo de asset de un recipe (sin revision ni fichero).
    pub fn recipe_prefix(&self) -> String {
        format!(
            "recipes/{}/{}/{}/{}",
            self.name, self.version, self.user, self.channel
        )
    }

    /// El prefijo de asset de una revision del recipe.
    pub fn revision_prefix(&self, revision: &str) -> String {
        format!("{}/{revision}", self.recipe_prefix())
    }

    /// La ruta de asset de un fichero del recipe.
    pub fn recipe_file_path(&self, revision: &str, file: &str) -> String {
        format!("{}/{file}", self.revision_prefix(revision))
    }

    /// El prefijo de asset de los packages de una revision del recipe.
    ///
    /// Los binarios viven en su propio arbol, al lado del de las recipes:
    /// `packages/{n}/{v}/{u}/{c}/{rrev}/{pid}/{prev}/{file}`. Un
    /// `package_id` no comparte prefijo con otro, y ninguno comparte
    /// prefijo con los ficheros del recipe.
    pub fn packages_prefix(&self, revision: &str) -> String {
        format!(
            "packages/{}/{}/{}/{}/{}",
            self.name, self.version, self.user, self.channel, revision
        )
    }

    /// El prefijo de asset de un package_id de una revision.
    pub fn package_prefix(&self, revision: &str, package_id: &str) -> String {
        format!("{}/{package_id}", self.packages_prefix(revision))
    }

    /// El prefijo de asset de una revision de package.
    pub fn package_revision_prefix(
        &self,
        revision: &str,
        package_id: &str,
        package_revision: &str,
    ) -> String {
        format!(
            "{}/{package_revision}",
            self.package_prefix(revision, package_id)
        )
    }

    /// La ruta de asset de un fichero de un package.
    pub fn package_file_path(
        &self,
        revision: &str,
        package_id: &str,
        package_revision: &str,
        file: &str,
    ) -> String {
        format!(
            "{}/{file}",
            self.package_revision_prefix(revision, package_id, package_revision)
        )
    }
}

/// La marca de tiempo que devuelve el remote en los documentos de
/// revisiones, en el formato que el cliente lee (`datetime.fromisoformat`
/// acepta el `Z` final).
pub fn the_timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Lo que se puede pedir en la superficie conan.
///
/// La tabla es la de `rest_routes.py` del cliente: el oraculo del
/// dialecto. Un nombre de recipe se repite en cada ruta porque el
/// cliente no usa rutas cortas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /v1/ping`: el cliente lee las capacidades de la cabecera.
    Ping,
    /// `GET /v2/users/authenticate`: Basic de entrada, token de salida.
    Authenticate,
    /// `GET /v2/users/check_credentials`: comprueba el token.
    CheckCredentials,
    /// `GET /v2/conans/search`: la busqueda de recipes, con su patron `q`.
    Search,
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/latest`: la ultima recipe revision.
    RecipeLatest(RecipeRef),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions`: las recipe revisions.
    RecipeRevisions(RecipeRef),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/files`: el
    /// snapshot del recipe.
    RecipeSnapshot(RecipeRef, String),
    /// `GET|PUT /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/files/{path}`:
    /// un fichero del recipe, con su ruta relativa tal cual.
    RecipeFile(RecipeRef, String, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages`: los
    /// package ids de una revision del recipe.
    PackageIds(RecipeRef, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/search`:
    /// la busqueda de package ids (la del cliente).
    PackageSearch(RecipeRef, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/{pid}/latest`:
    /// la ultima package revision de ese package id.
    PackageLatest(RecipeRef, String, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/{pid}/revisions`:
    /// las package revisions de ese package id.
    PackageRevisions(RecipeRef, String, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/{pid}/revisions/{prev}/files`:
    /// el snapshot del package.
    PackageSnapshot(RecipeRef, String, String, String),
    /// `GET|PUT /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/{pid}/revisions/{prev}/files/{path}`:
    /// un fichero del package.
    PackageFile(RecipeRef, String, String, String, String),
}

/// Un nombre de componente del protocolo conan: recipe name, version,
/// user, channel, revision, package id.
///
/// Conan los escribe en el camino sin codificar, y un `..` o un segmento
/// vacio no es un nombre: es una ruta disfrazada. Las revisiones y los
/// package ids son hexadecimales en la practica, pero cualquier token sin
/// barra es lo que el servidor acepta: quien inventa el valor es el
/// cliente, no el servidor.
pub fn valid_component(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

/// Un fichero del recipe o del package.
///
/// Los ficheros pueden anidar (`metadata/sign`), pero cada segmento es un
/// nombre literal: sin barra, sin escape escondido, sin vacio.
pub fn valid_file_path(text: &str) -> bool {
    !text.is_empty() && text.len() <= 255 && text.split('/').all(valid_component)
}

fn the_ref(name: &str, version: &str, user: &str, channel: &str) -> Option<RecipeRef> {
    for part in [name, version, user, channel] {
        if !valid_component(part) {
            return None;
        }
    }
    Some(RecipeRef {
        name: name.to_string(),
        version: version.to_string(),
        user: user.to_string(),
        channel: channel.to_string(),
    })
}

/// La ruta pedida, ya interpretada.
///
/// La entrada es lo que queda de `/conan/{repositorio}/` en la URI, sin
/// decodificar: la gramatica del protocolo es la unica que puede mirar
/// un `%2f`. La query no viene aqui.
pub fn parse(rest: &str) -> Option<Route> {
    let segments: Vec<&str> = rest.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        ["v1", "ping"] => Some(Route::Ping),
        ["v2", "users", "authenticate"] => Some(Route::Authenticate),
        ["v2", "users", "check_credentials"] => Some(Route::CheckCredentials),
        ["v2", "conans", "search"] => Some(Route::Search),
        ["v2", "conans", name, version, user, channel, "latest"] => {
            Some(Route::RecipeLatest(the_ref(name, version, user, channel)?))
        }
        ["v2", "conans", name, version, user, channel, "revisions"] => Some(
            Route::RecipeRevisions(the_ref(name, version, user, channel)?),
        ),
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "files",
        ] if valid_component(revision) => Some(Route::RecipeSnapshot(
            the_ref(name, version, user, channel)?,
            revision.to_string(),
        )),
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "files",
            rest @ ..,
        ] if valid_component(revision) && !rest.is_empty() => {
            let file = rest.join("/");
            if !valid_file_path(&file) {
                return None;
            }
            Some(Route::RecipeFile(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                file,
            ))
        }
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
        ] if valid_component(revision) => Some(Route::PackageIds(
            the_ref(name, version, user, channel)?,
            revision.to_string(),
        )),
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            "search",
        ] if valid_component(revision) => Some(Route::PackageSearch(
            the_ref(name, version, user, channel)?,
            revision.to_string(),
        )),
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            package_id,
            "latest",
        ] if valid_component(revision) && valid_component(package_id) => {
            Some(Route::PackageLatest(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                package_id.to_string(),
            ))
        }
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            package_id,
            "revisions",
        ] if valid_component(revision) && valid_component(package_id) => {
            Some(Route::PackageRevisions(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                package_id.to_string(),
            ))
        }
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            package_id,
            "revisions",
            package_revision,
            "files",
        ] if valid_component(revision)
            && valid_component(package_id)
            && valid_component(package_revision) =>
        {
            Some(Route::PackageSnapshot(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                package_id.to_string(),
                package_revision.to_string(),
            ))
        }
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            package_id,
            "revisions",
            package_revision,
            "files",
            rest @ ..,
        ] if valid_component(revision)
            && valid_component(package_id)
            && valid_component(package_revision)
            && !rest.is_empty() =>
        {
            let file = rest.join("/");
            if !valid_file_path(&file) {
                return None;
            }
            Some(Route::PackageFile(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                package_id.to_string(),
                package_revision.to_string(),
                file,
            ))
        }
        _ => None,
    }
}

/// La respuesta del snapshot de un recipe o de un package.
///
/// El cliente lee `files` y descarta el resto; los `url` relativos son los
/// que él ensambla desde las rutas de la tabla, y el servidor no necesita
/// decirle nada que el dialecto ya fija.
pub fn snapshot_document(files: &[(String, u64)]) -> Value {
    let files: Value = files
        .iter()
        .map(|(name, size)| (name.clone(), json!({ "size": size })))
        .collect::<serde_json::Map<_, _>>()
        .into();
    json!({ "files": files, "download_urls": {} })
}

/// El documento de una lista de revisiones (recipe o package).
pub fn revisions_document(revisions: &[(String, DateTime<Utc>)]) -> Value {
    json!({
        "revisions": revisions
            .iter()
            .map(|(revision, time)| {
                json!({ "revision": revision, "time": the_timestamp(*time) })
            })
            .collect::<Vec<_>>(),
    })
}

/// El documento de la ultima revision: uno solo, con su tiempo.
pub fn revision_document(revision: &str, time: DateTime<Utc>) -> Value {
    json!({ "revision": revision, "time": the_timestamp(time) })
}

/// La respuesta de la busqueda de recipes.
pub fn search_document(results: &[RecipeRef]) -> Value {
    json!({ "results": results.iter().map(RecipeRef::render).collect::<Vec<_>>() })
}

/// El patron `q` de la query de busqueda.
///
/// La query llega en la forma del wire (`q=hello%2F%2A`); el valor se
/// decodifica **una** vez y sin interpretar nada mas: el patron es texto
/// de glob, no una expresion que el servidor invente.
pub fn the_query_of(query: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        if name != "q" {
            return None;
        }
        Some(percent_decode(value))
    })
}

/// Un valor de query decodificado, una vez.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (
                (bytes[index + 1] as char).to_digit(16),
                (bytes[index + 2] as char).to_digit(16),
            )
        {
            out.push((high * 16 + low) as u8);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// La busqueda de recipes del cliente, aplicada a las recetas conocidas.
///
/// El patron es el que el cliente manda en `q`: una referencia con
/// comodines (`hello/*`, `*/1.0.0@hodei/stable`). El match es el de glob
/// sobre la referencia impresa; sin patron, todo.
pub fn the_search_matches(pattern: &str, reference: &RecipeRef) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let rendered = reference.render();
    // El cliente pide la comparacion sin distinguir mayusculas (el remote
    // de referencia la aplica en su motor de busqueda); aqui la aplicamos
    // igual: un listado es una lectura, no una identidad.
    the_glob(
        &rendered.to_ascii_lowercase(),
        &pattern.to_ascii_lowercase(),
    )
}

fn the_glob(text: &str, pattern: &str) -> bool {
    match pattern.split_once('*') {
        None => text == pattern,
        Some((before, after)) => {
            let (Some(start), Some(end)) = (text.find(before), text.rfind(after)) else {
                return false;
            };
            before == &text[start..start + before.len()]
                && after == &text[end..]
                && start + before.len() <= end
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn the_ref() -> RecipeRef {
        RecipeRef {
            name: "hello".into(),
            version: "1.0.0".into(),
            user: "hodei".into(),
            channel: "stable".into(),
        }
    }

    #[test]
    fn every_route_of_the_client_dialect_parses() {
        for (path, expected) in [
            ("v1/ping", Route::Ping),
            ("v2/users/authenticate", Route::Authenticate),
            ("v2/users/check_credentials", Route::CheckCredentials),
            ("v2/conans/search", Route::Search),
            (
                "v2/conans/hello/1.0.0/hodei/stable/latest",
                Route::RecipeLatest(the_ref()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions",
                Route::RecipeRevisions(the_ref()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/files",
                Route::RecipeSnapshot(the_ref(), "abc123".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/files/conanfile.py",
                Route::RecipeFile(the_ref(), "abc123".into(), "conanfile.py".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages",
                Route::PackageIds(the_ref(), "abc123".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/search",
                Route::PackageSearch(the_ref(), "abc123".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/pid1/latest",
                Route::PackageLatest(the_ref(), "abc123".into(), "pid1".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/pid1/revisions",
                Route::PackageRevisions(the_ref(), "abc123".into(), "pid1".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/pid1/revisions/prev1/files",
                Route::PackageSnapshot(the_ref(), "abc123".into(), "pid1".into(), "prev1".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/pid1/revisions/prev1/files/conan_package.tgz",
                Route::PackageFile(
                    the_ref(),
                    "abc123".into(),
                    "pid1".into(),
                    "prev1".into(),
                    "conan_package.tgz".into(),
                ),
            ),
        ] {
            assert_eq!(parse(path), Some(expected), "{path} does not parse");
        }
    }

    #[test]
    fn a_nested_file_of_a_package_parses_whole() {
        let route = parse(
            "v2/conans/hello/1.0.0/hodei/stable/revisions/r1/packages/pid1/revisions/prev1/files/metadata/sign",
        )
        .expect("the nested file parses");
        assert_eq!(
            route,
            Route::PackageFile(
                the_ref(),
                "r1".into(),
                "pid1".into(),
                "prev1".into(),
                "metadata/sign".into()
            )
        );
    }

    #[test]
    fn a_path_that_hides_a_traversal_is_not_a_route() {
        assert_eq!(parse("v2/conans/../1.0.0/hodei/stable/latest"), None);
        assert_eq!(
            parse("v2/conans/hello/1.0.0/hodei/stable/revisions/r1/files/..%2fx"),
            None
        );
        assert_eq!(parse("v2/conans/hello//hodei/stable/latest"), None);
    }

    #[test]
    fn the_asset_paths_keep_every_package_id_apart() {
        let reference = the_ref();
        let left = reference.package_file_path("r1", "pid1", "prev1", "conan_package.tgz");
        let right = reference.package_file_path("r1", "pid2", "prev2", "conan_package.tgz");
        assert_ne!(left, right);
        assert!(left.starts_with("packages/hello/1.0.0/hodei/stable/r1/pid1/prev1/"));
        assert_eq!(
            reference.recipe_file_path("r1", "conanfile.py"),
            "recipes/hello/1.0.0/hodei/stable/r1/conanfile.py"
        );
    }

    #[test]
    fn the_query_of_the_wire_decodes_once() {
        assert_eq!(the_query_of("q=hello%2F%2A").as_deref(), Some("hello/*"));
        assert_eq!(the_query_of("q=%2A"), Some("*".to_string()));
        assert_eq!(the_query_of("ignorecase=False"), None);
        assert_eq!(the_query_of(""), None);
    }

    #[test]
    fn the_search_matches_the_patterns_the_client_sends() {
        assert!(the_search_matches("hello/*", &the_ref()));
        assert!(the_search_matches("hello/1.0.0@hodei/stable", &the_ref()));
        assert!(the_search_matches("*", &the_ref()));
        assert!(
            the_search_matches("HELLO/*", &the_ref()),
            "the match is not case sensitive"
        );
        assert!(!the_search_matches("goodbye/*", &the_ref()));
        assert!(!the_search_matches("hello/1.0.1", &the_ref()));
    }
}
