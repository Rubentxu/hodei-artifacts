//! La gramatica **hf** del dialecto de lectura del Hugging Face Hub.
//!
//! Aqui vive lo **puro**: la tabla de rutas que el CLI `hf`
//! (`huggingface_hub` 1.x, que habla httpx) construye contra un hub, los
//! documentos de revision JSON y las cabeceras de metadata del resolve.
//! El dialecto lo fija el cliente y lo verifico el spike de HSP-169
//! contra huggingface.co y contra un fixture local (FMT-07 de
//! `uat/UAT-FORMATS.md`):
//!
//! 1. `GET /api/models/{repo_id}/revision/{rev}`: el documento de
//!    revision, con `id`, `sha` y `siblings[]` (`rfilename`). El `sha`
//!    es el commit que despues el cliente usa en los resolve.
//! 2. `HEAD /{repo_id}/resolve/{rev}/{rfilename}`: una `302` cuya
//!    metadata viaja en cabeceras: `X-Repo-Commit` (el commit),
//!    `X-Linked-Etag` (el sha256 del fichero, entre comillas),
//!    `X-Linked-Size` (su longitud) y `Location`, **relativa** (una ruta
//!    sin scheme ni host, la forma del hub clasico) hacia el blob.
//! 3. `HEAD`/`GET {Location}`: la metadata se lee de la respuesta final
//!    del redirect y los bytes van en streaming; el `ETag` de la
//!    respuesta es el sha256 del contenido.
//!
//! El cliente **no** emite `X-Xet-*` cuando el servidor no lo anuncia
//! (decision del servidor): el dialecto cae a la ruta HTTP clasica. Y en
//! esa ruta el cliente SOLO verifica el tamaño —el sha256 declarado no
//! lo comprueba—: la integridad la garantiza el CAS del servidor
//! (conexion con HSP-105), que se niega a servir bytes que no son su
//! digest.
//!
//! Un modelo en el catalogo es el conjunto de assets
//! `artifacts/{owner}/{name}/{rfilename}`; el commit de la revision es
//! el digest del listado del snapshot (`rfilename sha256` ordenado): es
//! determinista y solo cambia cuando cambia el contenido que declara.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// La identidad de un modelo: `owner/name`, el `repo_id` del cliente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// El propietario del modelo.
    pub owner: String,
    /// El nombre del modelo.
    pub name: String,
}

impl ModelRef {
    /// El `repo_id` tal y como el cliente lo nombra.
    pub fn repo_id(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// El prefijo del catalogo de todos los ficheros del modelo:
    /// `artifacts/{owner}/{name}/`.
    pub fn asset_prefix(&self) -> String {
        format!("artifacts/{}/{}", self.owner, self.name)
    }

    /// El asset del catalogo de un fichero del modelo.
    pub fn asset_path(&self, filename: &str) -> String {
        format!("{}/{filename}", self.asset_prefix())
    }
}

/// Una parte del `repo_id` (`owner` o `name`).
///
/// La gramatica del hub: letras, digitos, guion, punto y subrayado, y
/// empieza por letra o digito. Un punto inicial es un repo oculto del
/// hub real, y `.` y `..` esconden rutas: nada de eso es un repo.
pub fn valid_repo_part(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 96
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && text
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
}

/// Un `rfilename` de un sibling: la ruta del fichero dentro del modelo.
///
/// Puede llevar subdirectorios (el hub guarda rutas, no nombres), así que
/// cada segmento tiene que ser un segmento legitimo: ni vacio, ni `..`,
/// ni separador de Windows, ni delimitadores de referencia URI. La lista
/// es la que hace que la ruta sea una referencia sin ambigüedad, la
/// misma que la del contenido crudo.
pub fn valid_filename(text: &str) -> bool {
    if text.is_empty() || text.len() > 1024 {
        return false;
    }
    let segments: Vec<&str> = text.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return false;
    }
    segments.iter().all(|segment| valid_segment(segment))
}

/// Un segmento de un `rfilename`: los `pchar` de RFC 3986, la misma lista
/// que la del contenido crudo del hub.
fn valid_segment(segment: &str) -> bool {
    if segment.is_empty() || segment == "." || segment == ".." {
        return false;
    }
    segment.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b':'
                    | b'@'
            )
    })
}

/// Un sha256, como lo llevan el `X-Linked-Etag`, el `ETag` y la ruta del
/// blob.
pub fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Una revision que el dialecto de lectura nombra: la rama `main` o un
/// commit de 40 hex. Otra rama no existe en la hosted de este release, y
/// un commit que no sea el sha256 del snapshot es una revision que el
/// hub no llego a servir.
pub fn valid_revision(text: &str) -> bool {
    text == "main" || valid_sha256(text)
}

/// Lo que puede pedir el CLI sobre la superficie de un repositorio hf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /api/models/{owner}/{name}/revision/{rev}`: el documento de
    /// revision del modelo.
    ApiRevision(ModelRef, String),
    /// `HEAD`/`GET /{owner}/{name}/resolve/{rev}/{rfilename}`: la
    /// metadata del fichero (la `302` del resolve) y, en `GET`, los
    /// bytes sin redirect.
    Resolve(ModelRef, String, String),
    /// `HEAD`/`GET /cdn-lfs/{owner}/{name}/{sha256}/{rfilename}`: la
    /// metadata final del blob y sus bytes en streaming. El digest de la
    /// ruta nombra el contenido: la identidad del blob es su sha256, CAS
    /// exacto.
    Blob(ModelRef, String, String),
}

/// La ruta pedida, ya interpretada.
///
/// La entrada es lo que queda de `/hf/{repository}/` en el URI, **sin**
/// decodificar: la gramatica del protocolo es la unica que puede mirar
/// un `%2f`. Una barra final no es gramatica del cliente; la consulta
/// tampoco llega aqui.
pub fn parse(rest: &str) -> Option<Route> {
    let rest = rest.trim_start_matches('/').trim_end_matches('/');
    if rest.is_empty() {
        return None;
    }
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        ["api", "models", owner, name, "revision", rev] if valid_revision(rev) => {
            Some(Route::ApiRevision(the_model(owner, name)?, rev.to_string()))
        }
        [owner, name, "resolve", rev, rest @ ..] if valid_revision(rev) && !rest.is_empty() => {
            let filename = rest.join("/");
            if !valid_filename(&filename) {
                return None;
            }
            Some(Route::Resolve(
                the_model(owner, name)?,
                rev.to_string(),
                filename,
            ))
        }
        ["cdn-lfs", owner, name, digest, rest @ ..] if valid_sha256(digest) && !rest.is_empty() => {
            let filename = rest.join("/");
            if !valid_filename(&filename) {
                return None;
            }
            Some(Route::Blob(
                the_model(owner, name)?,
                digest.to_string(),
                filename,
            ))
        }
        _ => None,
    }
}

fn the_model(owner: &str, name: &str) -> Option<ModelRef> {
    if !valid_repo_part(owner) || !valid_repo_part(name) {
        return None;
    }
    Some(ModelRef {
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

/// El commit del snapshot de un modelo: el digest del listado que lo
/// forma, `rfilename sha256` en orden. Dos snapshots con el mismo
/// contenido son el mismo commit, y el commit solo cambia cuando cambia
/// el contenido que declara: la revision es content-addressed como el
/// CAS que la sirve.
pub fn snapshot_commit(entries: &[(&str, &str)]) -> String {
    let mut listing: Vec<String> = entries
        .iter()
        .map(|(filename, digest)| format!("{filename} {digest}"))
        .collect();
    listing.sort();
    let mut hasher = Sha256::new();
    for line in listing {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

/// La marca de tiempo que llevan los documentos, en el formato que el
/// cliente lee sin mirar (`ISO 8601` con segundos).
pub fn the_timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// El documento de revision del modelo. El cliente lee `sha` (el commit
/// que usara en los resolve) y `siblings[]` (`rfilename`): lo demas es
/// cortesia del documento real.
pub fn revision_document(
    model: &ModelRef,
    commit: &str,
    last_modified: DateTime<Utc>,
    siblings: &[String],
) -> Value {
    json!({
        "id": model.repo_id(),
        "sha": commit,
        "lastModified": the_timestamp(last_modified),
        "siblings": siblings
            .iter()
            .map(|filename| json!({ "rfilename": filename }))
            .collect::<Vec<Value>>(),
    })
}

/// La metadata que llevan las cabeceras del resolve y del blob: el
/// commit, el sha256 del fichero y su longitud. La identidad del blob es
/// su sha256: es el etag que el cliente usa para nombrar su cache y el
/// digest que el CAS garantiza.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobMeta {
    /// El commit del snapshot que declara el fichero.
    pub commit: String,
    /// El sha256 del contenido, sin comillas.
    pub sha256: String,
    /// La longitud del contenido, en bytes.
    pub size: u64,
}

impl BlobMeta {
    /// Las cabeceras del dialecto, en el orden del wire: `X-Repo-Commit`,
    /// `X-Linked-Etag` (el sha256 entre comillas, como lo escribe el hub
    /// real) y `X-Linked-Size`.
    pub fn the_linked_headers(&self) -> [(&'static str, String); 3] {
        [
            ("x-repo-commit", self.commit.clone()),
            ("x-linked-etag", format!("\"{}\"", self.sha256)),
            ("x-linked-size", self.size.to_string()),
        ]
    }
}

/// La `Location` del resolve: una **ruta** desde la raiz del servidor,
/// sin scheme ni host, la forma que el hub clasico entrega y la que el
/// cliente resuelve contra el anfitrion de la URL que pidio. Lleva el
/// modelo, el digest y el fichero porque la URL tiene que nombrar lo que
/// sirve.
pub fn the_download_location(
    base: &str,
    model: &ModelRef,
    meta: &BlobMeta,
    filename: &str,
) -> String {
    format!(
        "{base}/cdn-lfs/{}/{}/{}/{}",
        model.owner, model.name, meta.sha256, filename
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn the_model() -> ModelRef {
        ModelRef {
            owner: "hodei".into(),
            name: "demo-model".into(),
        }
    }

    #[test]
    fn every_route_of_the_client_dialect_parses() {
        for (path, expected) in [
            (
                "api/models/hodei/demo-model/revision/main",
                Route::ApiRevision(the_model(), "main".into()),
            ),
            (
                "api/models/hodei/demo-model/revision/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9",
                Route::ApiRevision(
                    the_model(),
                    "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
                ),
            ),
            (
                "hodei/demo-model/resolve/main/model.safetensors",
                Route::Resolve(the_model(), "main".into(), "model.safetensors".into()),
            ),
            (
                "hodei/demo-model/resolve/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/config.json",
                Route::Resolve(
                    the_model(),
                    "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
                    "config.json".into(),
                ),
            ),
            (
                "hodei/demo-model/resolve/main/subdir/tokenizer.json",
                Route::Resolve(the_model(), "main".into(), "subdir/tokenizer.json".into()),
            ),
            (
                "cdn-lfs/hodei/demo-model/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/model.safetensors",
                Route::Blob(
                    the_model(),
                    "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
                    "model.safetensors".into(),
                ),
            ),
        ] {
            assert_eq!(parse(path), Some(expected), "{path} does not parse");
        }
    }

    #[test]
    fn a_path_that_hides_a_traversal_is_not_a_route() {
        assert_eq!(parse("api/models/../demo-model/revision/main"), None);
        assert_eq!(parse("hodei/demo-model/resolve/main/../config.json"), None);
        assert_eq!(parse("hodei/demo-model/resolve/main/sub//file"), None);
        assert_eq!(
            parse("cdn-lfs/hodei/demo-model/short/model.safetensors"),
            None
        );
        assert_eq!(parse("hodei/demo-model/resolve/dev/config.json"), None);
        assert_eq!(parse("hodei/demo-model/resolve/main/"), None);
        assert_eq!(
            parse(
                "cdn-lfs/hodei/demo-model/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/"
            ),
            None
        );
    }

    #[test]
    fn a_filename_the_wire_cannot_carry_is_not_a_route() {
        // El backslash es un separador en algun sistema, y un espacio el
        // mismo nombre escrito de dos maneras: la gramatica del hub no
        // los lleva.
        assert_eq!(parse("hodei/demo-model/resolve/main/a\\b"), None);
        assert_eq!(parse("hodei/demo-model/resolve/main/a b"), None);
        // Un `?` o un `#` le cambia el sentido a la referencia.
        assert_eq!(parse("hodei/demo-model/resolve/main/a%3fb"), None);
        // Y un segmento codificado dos veces todavia lleva codificacion.
        assert_eq!(parse("hodei/demo-model/resolve/main/a%252fb"), None);
    }

    #[test]
    fn the_blob_route_carries_the_model_its_digest_and_the_file() {
        // La ruta del blob lleva el modelo, el digest y el fichero: quien
        // la sirve busca el asset, comprueba que su contenido es el que
        // la ruta nombra y no sirve nada que no sea su digest.
        let route = parse(
            "cdn-lfs/hodei/demo-model/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/subdir/model.safetensors",
        );
        assert_eq!(
            route,
            Some(Route::Blob(
                the_model(),
                "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
                "subdir/model.safetensors".into()
            ))
        );
    }

    #[test]
    fn the_repo_parts_of_the_hub_grammar() {
        assert!(valid_repo_part("hodei"));
        assert!(valid_repo_part("demo-model_v2.1"));
        assert!(!valid_repo_part(""));
        assert!(!valid_repo_part("."));
        assert!(!valid_repo_part(".."));
        assert!(!valid_repo_part(".hidden"));
        assert!(!valid_repo_part("a/b"));
        assert!(!valid_repo_part("a b"));
    }

    #[test]
    fn the_document_of_a_revision_carries_the_fields_the_client_reads() {
        let commit = snapshot_commit(&[
            ("config.json", "e4d31dec"),
            ("model.safetensors", "9f215d17"),
        ]);
        let document = revision_document(
            &the_model(),
            &commit,
            DateTime::parse_from_rfc3339("2026-11-20T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            &["config.json".into(), "model.safetensors".into()],
        );
        assert_eq!(document["id"], "hodei/demo-model");
        assert_eq!(document["sha"], commit);
        assert_eq!(document["lastModified"], "2026-11-20T10:00:00Z");
        let siblings = document["siblings"].as_array().unwrap();
        assert_eq!(siblings.len(), 2);
        assert_eq!(siblings[0]["rfilename"], "config.json");
        assert_eq!(siblings[1]["rfilename"], "model.safetensors");
    }

    #[test]
    fn the_commit_of_a_snapshot_is_its_sorted_listing() {
        let one = snapshot_commit(&[
            ("config.json", "e4d31dec"),
            ("model.safetensors", "9f215d17"),
        ]);
        let two = snapshot_commit(&[
            ("model.safetensors", "9f215d17"),
            ("config.json", "e4d31dec"),
        ]);
        assert_eq!(one, two, "el orden del listado no cambia el commit");
        let other = snapshot_commit(&[("config.json", "e4d31dec")]);
        assert_ne!(one, other, "otro contenido es otro commit");
        assert_eq!(one.len(), 64, "el commit es un sha256");
    }

    #[test]
    fn the_linked_headers_are_the_ones_the_wire_carries() {
        let meta = BlobMeta {
            commit: "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91".into(),
            sha256: "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
            size: 268435456,
        };
        let headers = meta.the_linked_headers();
        assert_eq!(headers[0].0, "x-repo-commit");
        assert_eq!(headers[0].1, "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91");
        assert_eq!(headers[1].0, "x-linked-etag");
        assert_eq!(
            headers[1].1, "\"9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9\"",
            "el etag va entre comillas, como lo escribe el hub real"
        );
        assert_eq!(headers[2].0, "x-linked-size");
        assert_eq!(headers[2].1, "268435456");
    }

    #[test]
    fn the_download_location_is_a_relative_path_with_the_digest() {
        let meta = BlobMeta {
            commit: "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91".into(),
            sha256: "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
            size: 268435456,
        };
        let location =
            the_download_location("/hf/hf-hosted", &the_model(), &meta, "model.safetensors");
        assert_eq!(
            location,
            "/hf/hf-hosted/cdn-lfs/hodei/demo-model/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/model.safetensors"
        );
        assert!(
            location.starts_with('/'),
            "la Location es una ruta, no una URL"
        );
        assert!(
            !location.contains("://"),
            "la Location no anuncia un anfitrion"
        );
    }

    #[test]
    fn the_asset_path_of_a_file_sits_under_its_model_prefix() {
        let model = the_model();
        assert_eq!(model.asset_prefix(), "artifacts/hodei/demo-model");
        assert_eq!(
            model.asset_path("config.json"),
            "artifacts/hodei/demo-model/config.json"
        );
        assert_eq!(
            model.asset_path("subdir/tokenizer.json"),
            "artifacts/hodei/demo-model/subdir/tokenizer.json"
        );
    }
}
