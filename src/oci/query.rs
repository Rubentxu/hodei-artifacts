//! Las dos preguntas de la superficie OCI que no son contenido: **que
//! etiquetas hay** y **que cuelga de este manifiesto**.
//!
//! Las dos son lecturas del catalogo, y las dos hablan el dialecto que el
//! protocolo dice: la lista de etiquetas con su paginacion, y los
//! referrers como un indice de imagenes.
//!
//! Y las dos tienen una regla que parece un detalle y no lo es: **no
//! dicen 404**. Un `subject` que no tiene referrers es una lista vacia,
//! y un repositorio sin etiquetas es una lista vacia. Un 404 en la API de
//! referrers haria que un cliente que busca la firma de un manifiesto
//! tuviera que distinguir entre "no hay firma" y "no hay manifiesto", y el
//! protocolo dice que esa distincion no existe.
//!
//! Lo que si se dice con 404 es lo que no existe: el **repositorio**. Un
//! repositorio de otra cuenta no existe para esta, y se responde con el
//! `NAME_UNKNOWN` de la distribucion.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hodei_evidence::ContentDigest;
use hodei_hub::oci::{DOCKER_MANIFEST_LIST_MEDIA_TYPE, INDEX_MEDIA_TYPE};

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;
use crate::oci::target::the_digest_of_reference;
use crate::oci::{the_catalog, the_content_store, the_decision, the_repository};

/// La cabecera que dice que se ha aplicado un filtro.
///
/// El protocolo la exige cuando se pide un filtro: sin ella, un cliente
/// no sabe si la lista esta filtrada o si el registro no ha entendido lo
/// que se le pedia.
const FILTERS_APPLIED_HEADER: &str = "OCI-Filters-Applied";

/// Los parametros de la lista de etiquetas.
///
/// `n` es cuantas etiquetas se piden, y `last` es la etiqueta por la que
/// se empieza: el cursor del cliente, que no se incluye en el resultado.
#[derive(Debug, Default)]
struct TheTagsQuery {
    /// Cuantas etiquetas se piden. `None` es "todas".
    n: Option<usize>,
    /// La etiqueta por la que se empieza, sin incluirla.
    last: Option<String>,
}

/// Los parametros de la consulta de referrers.
#[derive(Debug, Default)]
struct TheReferrersQuery {
    /// El tipo de artefacto por el que se filtra.
    artifact_type: Option<String>,
}

/// La lista de etiquetas del repositorio.
///
/// `GET /v2/<name>/tags/list`
///
/// Las etiquetas van en orden lexicografico, que el protocolo define como
/// orden alfanumerico **sin distinguir mayusculas**, y la pagina se
/// recorta con `n` y `last`.
pub async fn the_tags(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    query: &str,
) -> Result<Response, OciFailure> {
    let repository = the_repository(state, context, name).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    let parsed = the_tags_query(query)?;
    let (reader, _) = the_catalog(state)?;
    let tags = reader
        .tags(context.account().as_str(), name)
        .await
        .map_err(|error| crate::oci::the_catalog_error(&error))?;

    let mut names: Vec<String> = tags.into_iter().map(|tag| tag.tag_name).collect();
    names.sort_by(|left, right| the_order(left).cmp(&the_order(right)));

    // El cursor: la lista empieza **despues** de `last`. El cursor se
    // compara con el mismo orden que la lista, para que un cliente que
    // pagina con una etiqueta que ya no existe no se quede a medias.
    let start = match &parsed.last {
        None => 0,
        Some(last) => names.partition_point(|name| the_order(name) <= the_order(last)),
    };

    let page: Vec<String> = match parsed.n {
        Some(n) => names[start.min(names.len())..]
            .iter()
            .take(n)
            .cloned()
            .collect(),
        None => names[start.min(names.len())..].to_vec(),
    };

    let body = serde_json::json!({
        "name": name,
        "tags": page,
    });

    let mut response = (StatusCode::OK, body.to_string()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    // El `Link` dice si hay mas: el cliente que pagina lo sigue. Sin
    // `n` no hay pagina, y con `n = 0` el protocolo dice que no hay
    // `Link`.
    if let Some(n) = parsed.n
        && n > 0
        && let Some(last) = page.last()
        && start + page.len() < names.len()
    {
        let link = format!("</v2/{name}/tags/list?n={n}&last={last}>; rel=\"next\"");
        if let Ok(value) = HeaderValue::from_str(&link) {
            response.headers_mut().insert(header::LINK, value);
        }
    }
    Ok(response)
}

/// Los referrers de un manifiesto.
///
/// `GET /v2/<name>/referrers/<digest>`
///
/// La respuesta es un indice de imagenes con un descriptor por referrer.
/// El descriptor lleva el `artifactType` del manifiesto --o, si no lo
/// tiene, el tipo de medio de su configuracion-- y las anotaciones del
/// manifiesto.
pub async fn the_referrers(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    digest: &str,
    query: &str,
) -> Result<Response, OciFailure> {
    let repository = the_repository(state, context, name).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    let parsed = the_referrers_query(query)?;
    // El digest ya lo ha comprobado el reparto del path: aqui solo se
    // convierte al digest del catalogo.
    let subject = ContentDigest::sha256(the_digest_of_reference(digest)?);

    let (reader, _) = the_catalog(state)?;
    let referrers = reader
        .referrers(context.account().as_str(), name, &subject)
        .await
        .map_err(|error| crate::oci::the_catalog_error(&error))?;

    let store = the_content_store(state)?;
    let mut manifests = Vec::with_capacity(referrers.len());
    for referrer in referrers {
        let bytes = store.get(&referrer.digest).await.map_err(|_| {
            OciFailure::unavailable("the catalog and the store disagree about a referrer")
        })?;
        let manifest: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| OciFailure::unavailable("the manifest of a referrer is not readable"))?;
        let descriptor = the_descriptor(&manifest, &referrer.digest.value, bytes.len());
        // El filtro es sobre el `artifactType` del descriptor, que es el
        // que el protocolo dice: el del manifiesto, o el de su
        // configuracion cuando el manifiesto no lo trae.
        if let Some(filter) = &parsed.artifact_type
            && descriptor.artifact_type.as_deref() != Some(filter.as_str())
        {
            continue;
        }
        manifests.push(descriptor.json);
    }

    let body = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": INDEX_MEDIA_TYPE,
        "manifests": manifests,
    });

    let mut response = (StatusCode::OK, body.to_string()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(INDEX_MEDIA_TYPE),
    );
    // El filtro se dice: sin esta cabecera, un cliente no sabe si la
    // lista esta filtrada o si el registro no ha entendido el filtro.
    if parsed.artifact_type.is_some() {
        response.headers_mut().insert(
            FILTERS_APPLIED_HEADER,
            HeaderValue::from_static("artifactType"),
        );
    }
    Ok(response)
}

/// El descriptor de un referrer.
///
/// El protocolo dice que el descriptor lleva el `artifactType` del
/// manifiesto --o el tipo de medio de su configuracion, cuando el
/// manifiesto es de imagen y no lo trae-- y las anotaciones del
/// manifiesto. Un indice sin `artifactType` lo omite, porque un indice
/// no tiene configuracion.
fn the_descriptor(manifest: &serde_json::Value, digest: &str, size: usize) -> TheDescriptor {
    let media_type = manifest
        .get("mediaType")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let is_index = media_type == INDEX_MEDIA_TYPE || media_type == DOCKER_MANIFEST_LIST_MEDIA_TYPE;
    let declared = manifest
        .get("artifactType")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty());
    let artifact_type = match declared {
        Some(declared) => Some(declared.to_string()),
        // Un indice sin `artifactType` lo omite: el protocolo lo dice.
        None if is_index => None,
        // Sin `artifactType` declarado, el tipo es el de la
        // configuracion. Y si no hay configuracion tampoco, el tipo es
        // el del propio manifiesto: el descriptor tiene que **decir**
        // que tipo de artefacto es, porque un descriptor sin tipo no lo
        // puede filtrar un cliente.
        None => manifest
            .get("config")
            .and_then(|config| config.get("mediaType"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| (!media_type.is_empty()).then(|| media_type.to_string())),
    };

    let mut json = serde_json::json!({
        "mediaType": media_type,
        "size": size,
        "digest": format!("sha256:{digest}"),
    });
    if let Some(artifact_type) = &artifact_type {
        json["artifactType"] = serde_json::Value::String(artifact_type.clone());
    }
    if let Some(annotations) = manifest.get("annotations")
        && annotations.is_object()
    {
        json["annotations"] = annotations.clone();
    }
    TheDescriptor {
        json,
        artifact_type,
    }
}

/// El descriptor de un referrer, y el `artifactType` por el que se
/// filtra.
///
/// El filtro va sobre el `artifactType` del descriptor, y no sobre el del
/// manifiesto: son el mismo valor, pero el del descriptor es el que el
/// protocolo dice, y el que el cliente ve.
struct TheDescriptor {
    /// El descriptor, tal y como va en la respuesta.
    json: serde_json::Value,
    /// El `artifactType` del descriptor.
    artifact_type: Option<String>,
}

/// El orden de las etiquetas.
///
/// El protocolo lo define como orden alfanumerico **sin distinguir
/// mayusculas**, y el desempate es el orden exacto de los bytes: dos
/// etiquetas que solo se distinguen por la mayuscula tienen que tener un
/// orden, y un orden estable es el que permite paginar sin perder ni
/// repetir.
fn the_order(tag: &str) -> (String, &str) {
    (tag.to_lowercase(), tag)
}

/// Los parametros de la lista de etiquetas.
///
/// Un `n` que no es un numero es un error del cliente, y se dice con un
/// 400: el parametro es un entero, y un entero que no lo es es una
/// peticion mal escrita, no una peticion a una operacion que el registro
/// no sepa atender.
fn the_tags_query(query: &str) -> Result<TheTagsQuery, OciFailure> {
    let mut parsed = TheTagsQuery::default();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "n" => {
                // Un `n` vacio no es un `n`: el registro de referencia lo
                // ignora y devuelve la lista entera, y un cliente que lo
                // manda no esta pidiendo una pagina.
                if value.is_empty() {
                    continue;
                }
                let n = value.parse::<i64>().map_err(|_| {
                    OciFailure::pagination_number_invalid("the n parameter is not a number")
                })?;
                if n < 0 {
                    return Err(OciFailure::pagination_number_invalid(
                        "the n parameter is negative",
                    ));
                }
                parsed.n = Some(n as usize);
            }
            "last" => parsed.last = Some(value.into_owned()),
            _ => {}
        }
    }
    Ok(parsed)
}

/// Los parametros de la consulta de referrers.
fn the_referrers_query(query: &str) -> Result<TheReferrersQuery, OciFailure> {
    let mut parsed = TheReferrersQuery::default();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if key == "artifactType" {
            parsed.artifact_type = Some(value.into_owned());
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El orden de las etiquetas es el que dice el protocolo: sin
    /// distinguir mayusculas, y con un desempate estable.
    #[test]
    fn the_order_of_the_tags_is_the_one_the_protocol_says() {
        let mut tags = vec!["beta".to_string(), "Alpha".to_string(), "alfa".to_string()];
        tags.sort_by(|left, right| the_order(left).cmp(&the_order(right)));
        assert_eq!(tags, vec!["alfa", "Alpha", "beta"]);
    }

    /// Un `n` que no es un numero no es una pagina, y es un error del
    /// cliente: un 400, no un 405.
    #[test]
    fn an_n_that_is_not_a_number_is_not_a_page() {
        let error = the_tags_query("n=many").expect_err("the n is not a number");
        assert_eq!(error.the_code(), "PAGINATION_NUMBER_INVALID");
        assert_eq!(error.the_status(), axum::http::StatusCode::BAD_REQUEST);
        let error = the_tags_query("n=-1").expect_err("the n is negative");
        assert_eq!(error.the_code(), "PAGINATION_NUMBER_INVALID");
        let parsed = the_tags_query("n=0").expect("zero is a number");
        assert_eq!(parsed.n, Some(0));
    }

    /// El filtro de referrers se lee tal cual.
    #[test]
    fn the_filter_of_the_referrers_is_read() {
        let parsed = the_referrers_query("artifactType=application/vnd.example.sbom.v1")
            .expect("the filter");
        assert_eq!(
            parsed.artifact_type.as_deref(),
            Some("application/vnd.example.sbom.v1")
        );
        let parsed = the_referrers_query("").expect("no filter");
        assert!(parsed.artifact_type.is_none());
    }

    /// El descriptor lleva el `artifactType` del manifiesto, y las
    /// anotaciones.
    #[test]
    fn the_descriptor_carries_the_artifact_type_and_the_annotations() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "artifactType": "application/vnd.example.sbom.v1",
            "annotations": { "org.example.format": "json" },
        });
        let descriptor = the_descriptor(&manifest, "abc", 42);
        assert_eq!(
            descriptor.json["artifactType"].as_str(),
            Some("application/vnd.example.sbom.v1")
        );
        assert_eq!(descriptor.json["annotations"]["org.example.format"], "json");
        assert_eq!(descriptor.json["size"], 42);
        assert_eq!(descriptor.json["digest"], "sha256:abc");
        assert_eq!(
            descriptor.artifact_type.as_deref(),
            Some("application/vnd.example.sbom.v1")
        );
    }

    /// Un manifiesto de imagen sin `artifactType` lo toma de su
    /// configuracion, y un indice sin `artifactType` lo omite.
    #[test]
    fn the_artifact_type_falls_back_to_the_config_of_the_image() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "mediaType": "application/vnd.oci.image.config.v1+json" },
        });
        let descriptor = the_descriptor(&manifest, "abc", 42);
        assert_eq!(
            descriptor.json["artifactType"].as_str(),
            Some("application/vnd.oci.image.config.v1+json")
        );

        // Y un manifiesto que no dice ni que es ni que tiene: el
        // descriptor dice el tipo del propio manifiesto, porque un
        // descriptor sin tipo no lo puede filtrar un cliente.
        let artifact = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
        });
        let descriptor = the_descriptor(&artifact, "abc", 42);
        assert_eq!(
            descriptor.json["artifactType"].as_str(),
            Some("application/vnd.oci.artifact.manifest.v1+json")
        );

        let index = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [],
        });
        let descriptor = the_descriptor(&index, "abc", 42);
        assert!(
            descriptor.json.get("artifactType").is_none(),
            "un indice sin artifactType lo omite: {:?}",
            descriptor.json
        );
    }
}
