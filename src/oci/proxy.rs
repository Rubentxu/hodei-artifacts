//! El pull de un repositorio OCI **proxy**.
//!
//! Un repositorio OCI en modo proxy no guarda lo que se publico en el:
//! resuelve contra un **upstream** de distribucion y conserva lo que baja.
//! Y lo hace por la **superficie de la distribucion**, no por la cruda: un
//! manifiesto se pide en `/v2/<nombre>/manifests/<referencia>` y un blob en
//! `/v2/<nombre>/blobs/<digest>`, con el reto de autenticacion y el
//! `Docker-Content-Digest` que el protocolo dice.
//!
//! # Lo que garantiza
//!
//! - **La identidad es el digest.** Un manifiesto pedido por `sha256:...`
//!   que llega con otros bytes no es el que se pidio: se retiene y no se
//!   sirve. Un blob igual. Lo que se sirve es lo que se calculo, no lo que
//!   el upstream dijo de si mismo.
//! - **La etiqueta se mueve; el digest no.** Una etiqueta es una referencia
//!   mutable: cuando el upstream la mueve, el contenido nuevo se publica y
//!   la etiqueta se mueve con ella, y lo que habia detras sigue publicado
//!   con su digest. Un digest no se mueve nunca.
//! - **El reto no decide a donde va la credencial.** El realm que nombra el
//!   reto tiene que ser el que declara la configuracion: un upstream que
//!   invente un realm no recibe ninguna credencial, ni la peticion.
//! - **Sin red, solo lo integro y con recibo.** Un contenido cacheado sin
//!   procedencia no se sirve, y el que no esta integro tampoco.
//!
//! # Lo que se reusa
//!
//! La salida a la red es la de las demas superficies --la politica de
//! ejection, los limites, la cola--, y la verificacion del contenido es la
//! de la superficie cruda: [`crate::proxy::resolution`]. Aqui solo se anade
//! lo que es propio de la distribucion: el camino del upstream, el reto,
//! el digest que es la identidad, y la etiqueta que se mueve.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    PackageCoordinates, Publication, RepositoryDescriptor, UpstreamConfig, origin_of,
};
use hodei_evidence::ContentDigest;
use url::Url;

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;
use crate::oci::target::{is_a_digest, the_digest_of_reference, the_tag_name};
use crate::oci::{the_catalog, the_content_store};
use crate::proxy::fetch::FetchError;
use crate::proxy::resolution::{self, Outcome};

/// El prefijo de la ruta del asset de un manifiesto.
const MANIFESTS: &str = "manifests";

/// El prefijo de la ruta del asset de un blob.
const BLOBS: &str = "blobs";

/// El prefijo de la ruta del puntero de una etiqueta.
///
/// Una etiqueta es una referencia **mutable**: el mismo nombre puede
/// apuntar a otro contenido. El catálogo no mueve las etiquetas de un
/// repositorio proxy --un proxy no acepta escrituras de un cliente--, asi
/// que el puntero se guarda como lo que es: un asset mas, en
/// `manifests/tag/<etiqueta>`, que apunta al digest.
///
/// Y el **recibo** va siempre a la ruta del digest (`manifests/<digest>`):
/// la procedencia del contenido es una sola, se haya pedido por etiqueta o
/// por digest.
const TAGS: &str = "manifests/tag";

/// Los recibos que se leen para saber si un contenido tiene procedencia.
const RECEIPTS_READ_ON_THE_HOT_PATH: usize = 4;

/// Que se pide al upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheKind {
    /// Un manifiesto, por etiqueta o por digest.
    Manifest,
    /// Un blob, siempre por digest.
    Blob,
}

impl TheKind {
    /// El prefijo de la ruta del asset en el catalogo.
    fn the_prefix(self) -> &'static str {
        match self {
            TheKind::Manifest => MANIFESTS,
            TheKind::Blob => BLOBS,
        }
    }

    /// La palabra del camino del upstream.
    fn the_upstream_word(self) -> &'static str {
        match self {
            TheKind::Manifest => "manifests",
            TheKind::Blob => "blobs",
        }
    }

    /// El error que se responde cuando el upstream no lo tiene.
    fn the_unknown(self, reference: &str) -> OciFailure {
        match self {
            TheKind::Manifest => {
                OciFailure::manifest_unknown(format!("the manifest {reference:?} is unknown"))
            }
            TheKind::Blob => OciFailure::blob_unknown(format!("the blob {reference:?} is unknown")),
        }
    }
}

/// Un manifiesto del upstream, por etiqueta o por digest.
///
/// `GET /v2/<nombre>/manifests/<referencia>`
pub async fn the_manifest(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    reference: &str,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    the_content(
        state,
        context,
        repository,
        name,
        reference,
        TheKind::Manifest,
        headers,
        head,
    )
    .await
}

/// Un blob del upstream, por digest.
///
/// `GET /v2/<nombre>/blobs/<digest>`
pub async fn the_blob(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    digest: &str,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    the_content(
        state,
        context,
        repository,
        name,
        digest,
        TheKind::Blob,
        headers,
        head,
    )
    .await
}

/// El contenido que el upstream resuelve, con su procedencia.
///
/// Es la resolucion **sin** la respuesta HTTP: el camino de un grupo OCI
/// necesita los bytes y el digest que gano un miembro proxy, no una
/// `Response` que ya sabe servir. La procedencia es la del recibo que deja el
/// resolvedor, y el contenido se publica en el namespace del **miembro**,
/// no en el de quien pregunta.
pub struct Resolved {
    /// La fila del asset, en el namespace del repositorio resuelto.
    pub asset: hodei_catalog::AssetRecord,
    /// Los bytes del contenido.
    pub bytes: Vec<u8>,
}

/// El manifiesto que el upstream resuelve, con su procedencia.
///
/// Es el camino de [`the_manifest`] sin la respuesta: lo que un grupo OCI
/// consulta en un miembro proxy.
pub async fn the_resolved_manifest(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    reference: &str,
    headers: &HeaderMap,
) -> Result<Resolved, OciFailure> {
    the_resolved(
        state,
        context,
        repository,
        name,
        reference,
        TheKind::Manifest,
        headers,
    )
    .await
}

/// El blob que el upstream resuelve, con su procedencia.
///
/// Es el camino de [`the_blob`] sin la respuesta: lo que un grupo OCI
/// consulta en un miembro proxy.
pub async fn the_resolved_blob(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    digest: &str,
    headers: &HeaderMap,
) -> Result<Resolved, OciFailure> {
    the_resolved(
        state,
        context,
        repository,
        name,
        digest,
        TheKind::Blob,
        headers,
    )
    .await
}

/// El contenido del upstream, resuelto y verificado.
///
/// El camino es el mismo que el de un repositorio **hosted**: la cuenta
/// viene del principal verificado, Cedar decide sobre el repositorio, y
/// la compuerta de confianza decide sobre el **digest** que se va a servir.
/// Lo que cambia es de donde salen los bytes: de un upstream, y con las
/// reglas de la distribucion por delante.
///
/// La resolucion va aparte de la respuesta: quien necesita el contenido
/// --un grupo OCI, que consulta a un miembro proxy-- no quiere una
/// `Response`, sino los bytes con su digest. La respuesta se arma despues,
/// con lo que la resolucion devuelve.
#[allow(clippy::too_many_arguments)]
async fn the_content(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    reference: &str,
    kind: TheKind,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    let resolved = the_resolved(state, context, repository, name, reference, kind, headers).await?;
    the_served(
        state,
        context,
        repository,
        &resolved.asset,
        resolved.bytes,
        kind,
        headers,
        head,
    )
    .await
}

/// La resolucion del contenido del upstream, con su procedencia.
///
/// Es el cuerpo del camino: el mismo que el de un repositorio **hosted**,
/// con la cuenta del principal verificado y la compuerta de confianza
/// aplicada al digest que se va a servir. Lo que cambia es de donde salen
/// los bytes: de un upstream, y con las reglas de la distribucion por
/// delante.
///
/// El contenido se publica en el namespace del **repositorio** al que se
/// pregunta --el miembro, cuando quien pregunta es un grupo-- y el recibo
/// que lo respalda va con el.
async fn the_resolved(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    reference: &str,
    kind: TheKind,
    headers: &HeaderMap,
) -> Result<Resolved, OciFailure> {
    let plane = state
        .proxy_plane
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("this service has no egress for remote proxies"))?;
    let correlation = crate::handlers::correlation_id(headers, &state.identity);

    // La configuracion del upstream. Una configuracion que no se entiende no
    // se sustituye por un valor por defecto: un ajuste de salida a la red
    // ilegible no puede degradar a "permitido".
    let upstream = match UpstreamConfig::of(&repository.config) {
        Ok(Some(upstream)) => upstream,
        Ok(None) => return Err(OciFailure::unavailable("the proxy is not configured")),
        Err(_) => {
            return Err(OciFailure::unavailable(
                "the proxy configuration is not valid",
            ));
        }
    };

    // La referencia: un digest (inmutable) o una etiqueta (mutable).
    //
    // El camino del upstream lleva el digest **con su algoritmo** --la
    // distribucion lo pide asi-- y la ruta del catalogo lleva el
    // hexadecimal pelado.
    let (requested, upstream_reference) = match kind {
        TheKind::Blob => {
            // La referencia de un blob llega ya validada y en hexadecimal:
            // el camino del upstream lo pide con su algoritmo.
            let digest = reference.to_string();
            (Some(digest.clone()), format!("sha256:{digest}"))
        }
        TheKind::Manifest if is_a_digest(reference) => {
            let digest = the_digest_of_reference(reference)?;
            (Some(digest.clone()), reference.to_string())
        }
        TheKind::Manifest => (None, reference.to_string()),
    };
    // Una etiqueta tiene reglas: no es cualquier palabra.
    if kind == TheKind::Manifest && requested.is_none() {
        the_tag_name(reference)?;
    }

    // El camino del upstream: `v2/<nombre>/manifests/<referencia>`.
    let upstream_path = format!(
        "v2/{name}/{}/{upstream_reference}",
        kind.the_upstream_word()
    );
    // Lo que no cae en un scope no se pide al upstream.
    if !upstream.in_scope(&upstream_path) {
        return Err(kind.the_unknown(reference));
    }
    let Some(url) = upstream.target(&upstream_path) else {
        return Err(kind.the_unknown(reference));
    };

    // El digest que se sirve: el que se pide, o el al que apunta la
    // etiqueta. Una etiqueta que no se conoce todavia no tiene digest: hay
    // que ir a buscarlo.
    let (reader, writer) = the_catalog(state)?;
    let account = context.account().as_str();
    let resolved = match requested.clone() {
        Some(digest) => Some(digest),
        // La etiqueta se resuelve por su puntero: el asset que dice a que
        // digest apunta.
        None => reader
            .asset(account, name, &format!("{TAGS}/{reference}"))
            .await
            .ok()
            .map(|asset| asset.digest.value),
    };
    let asset_path = resolved
        .as_ref()
        .map(|digest| format!("{}/{digest}", kind.the_prefix()));

    // La clase de la ruta: un digest no se mueve, una etiqueta si.
    let cache_kind = match kind {
        TheKind::Blob => hodei_catalog::proxy_cache::CacheKind::Content,
        TheKind::Manifest => match requested {
            Some(_) => hodei_catalog::proxy_cache::CacheKind::Content,
            None => hodei_catalog::proxy_cache::CacheKind::Metadata,
        },
    };

    // Lo que hay hoy del contenido, con la procedencia que lo respalda.
    let current = match asset_path.as_deref() {
        Some(path) => reader.asset(account, name, path).await.ok(),
        None => None,
    };
    let receipts = match current {
        Some(_) => plane
            .ledger
            .latest_receipts(
                account,
                &repository.repo_key,
                asset_path.as_deref().unwrap_or(""),
                RECEIPTS_READ_ON_THE_HOT_PATH,
            )
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let entry = current.as_ref().map(|asset| {
        hodei_catalog::proxy_cache::CacheEntry::of(&asset.digest, asset.size_bytes, &receipts)
    });
    let config_version = repository.config.version;
    let decision_context = hodei_catalog::proxy_cache::Context {
        format: repository.format,
        policy: &upstream.cache,
        asset_path: asset_path.as_deref().unwrap_or(""),
        upstream_id: &upstream.id,
        in_scope: upstream.in_scope(&upstream_path),
        config_version,
        offline: upstream.cache.offline || plane.is_offline(),
        now: chrono::Utc::now(),
        kind: Some(cache_kind),
    };
    let decision = hodei_catalog::proxy_cache::decide(&decision_context, entry.as_ref());

    use hodei_catalog::proxy_cache::CacheDecision;
    let known = match decision {
        CacheDecision::OutOfScope => return Err(kind.the_unknown(reference)),
        CacheDecision::Fresh => {
            plane.metrics.record_hit();
            let asset = current.ok_or_else(|| {
                OciFailure::unavailable("the catalog and the cache disagree about the content")
            })?;
            let bytes = the_bytes(state, &asset).await?;
            return Ok(Resolved { asset, bytes });
        }
        CacheDecision::OfflineServe => {
            let asset = current.ok_or_else(|| {
                OciFailure::unavailable("the catalog and the cache disagree about the content")
            })?;
            // Sin red no hay quien corrija un blob dañado: se comprueba
            // antes de servirlo.
            let store = the_content_store(state)?;
            if store.verify(&asset.digest).await.is_err() {
                plane.metrics.record_offline_miss();
                return Err(OciFailure::unavailable(
                    "the cached content failed its integrity check",
                ));
            }
            plane.metrics.record_offline_hit();
            let bytes = the_bytes(state, &asset).await?;
            return Ok(Resolved { asset, bytes });
        }
        CacheDecision::OfflineMiss => {
            plane.metrics.record_offline_miss();
            return Err(OciFailure::unavailable(
                "the proxy is offline and does not hold a verified copy of that content",
            ));
        }
        CacheDecision::Quarantined => {
            plane.metrics.record_quarantined();
            return Err(OciFailure::unavailable(
                "the upstream content is withheld pending review",
            ));
        }
        CacheDecision::Fetch => None,
        CacheDecision::Revalidate(known) => Some(known),
    };

    // Lo que se trae del upstream, con la identidad que se espera.
    let fetched = the_fetched(
        state,
        plane,
        repository,
        &upstream,
        &url,
        kind,
        requested.as_deref(),
        known.as_ref(),
        asset_path.as_deref(),
        context,
        &correlation,
    )
    .await?;

    // El digest que se sirve: el que se calculo, no el que dijo el upstream.
    let digest = match &fetched {
        Outcome::Stored(resolved) => resolved.digest.clone(),
        Outcome::Unchanged(receipt) => ContentDigest::sha256(receipt.digest.value.clone()),
        Outcome::Quarantined(_) => {
            return Err(OciFailure::unavailable(
                "the upstream content is withheld pending review",
            ));
        }
    };

    // El contenido se publica en el catalogo: un manifiesto es un asset en
    // `manifests/<digest>` y un blob en `blobs/<digest>`, igual que en un
    // repositorio hosted. Sin publicar, el digest no es contenido de este
    // repositorio y no se puede servir.
    if let Outcome::Stored(resolved) = &fetched {
        let fetched_size = resolved.size_bytes;
        let publication = Publication {
            repository: repository.repo_key.clone(),
            asset_path: format!("{}/{}", kind.the_prefix(), digest.value),
            coordinates: PackageCoordinates {
                package_name: format!("{}/{}", kind.the_prefix(), digest.value),
                package_version: digest.value.clone(),
            },
            digest: digest.clone(),
            size_bytes: fetched_size,
            tags: Vec::new(),
            actor: context.principal().subject().as_str().to_string(),
            correlation_id: correlation.to_string(),
            // El camino lleva el digest: republicar el mismo contenido es lo
            // mismo, y no hay nada que sustituir.
            overwrite: true,
            precondition: None,
            subject: None,
        };
        writer
            .publish(account, publication)
            .await
            .map_err(|error| {
                tracing::warn!(
                    target: "hodei_artifacts::oci",
                    repository = %repository.repo_key,
                    error = %error,
                    "the content of the upstream could not be published"
                );
                crate::oci::the_catalog_error(&error)
            })?;

        // La etiqueta se **mueve**: el protocolo dice que una etiqueta es
        // mutable, y lo que hay detras de la anterior sigue publicado con
        // su digest. El puntero es un asset mas, porque el catalogo no
        // mueve las etiquetas de un proxy: eso es de un cliente, y un
        // proxy no acepta escrituras de un cliente.
        if kind == TheKind::Manifest && requested.is_none() {
            let pointer = Publication {
                repository: repository.repo_key.clone(),
                asset_path: format!("{TAGS}/{reference}"),
                coordinates: PackageCoordinates {
                    package_name: format!("{TAGS}/{reference}"),
                    package_version: digest.value.clone(),
                },
                digest: digest.clone(),
                size_bytes: fetched_size,
                tags: Vec::new(),
                actor: context.principal().subject().as_str().to_string(),
                correlation_id: correlation.to_string(),
                // El puntero se mueve: es una referencia mutable, y lo que
                // hay detras sigue publicado con su digest.
                overwrite: true,
                precondition: None,
                subject: None,
            };
            writer.publish(account, pointer).await.map_err(|error| {
                tracing::warn!(
                    target: "hodei_artifacts::oci",
                    repository = %repository.repo_key,
                    error = %error,
                    "the pointer of the tag could not be published"
                );
                crate::oci::the_catalog_error(&error)
            })?;
        }
    }

    // Y se sirve lo que hay en el catalogo, por el camino de siempre.
    let (reader, _) = the_catalog(state)?;
    let asset = reader
        .asset(
            account,
            name,
            &format!("{}/{}", kind.the_prefix(), digest.value),
        )
        .await
        .map_err(|_| kind.the_unknown(reference))?;
    let bytes = the_bytes(state, &asset).await?;
    Ok(Resolved { asset, bytes })
}

/// Los bytes que la fila del asset referencia.
///
/// Los bytes son los de la fila, no los que dice la peticion: la fila es la
/// referencia de lo que el repositorio tiene.
async fn the_bytes(
    state: &AppState,
    asset: &hodei_catalog::AssetRecord,
) -> Result<Vec<u8>, OciFailure> {
    let store = the_content_store(state)?;
    store
        .get(&asset.digest)
        .await
        .map_err(|_| OciFailure::unavailable("the content is not in the store"))
}

/// El contenido que se trae del upstream, verificado y con recibo.
///
/// El camino es el de la superficie cruda --traer, comprobar, guardar y
/// dejar constancia-- con dos reglas de la distribucion por delante: el
/// **digest esperado** (la identidad del contenido) y el **token** que el
/// upstream puede pedir en su reto.
#[allow(clippy::too_many_arguments)]
async fn the_fetched(
    state: &AppState,
    plane: &crate::proxy::ProxyPlane,
    repository: &RepositoryDescriptor,
    upstream: &UpstreamConfig,
    url: &Url,
    kind: TheKind,
    requested: Option<&str>,
    known: Option<&hodei_catalog::proxy_cache::Known>,
    asset_path: Option<&str>,
    context: &VerifiedContext,
    correlation: &str,
) -> Result<Outcome, OciFailure> {
    let store = the_content_store(state)?;
    let token_key = format!("{}|{}", repository.repo_key, upstream.origin());
    let token = plane.tokens.get(&token_key);

    let outcome = match resolution::resolve_url(
        &plane.fetcher,
        store.as_ref(),
        plane.ledger.as_ref(),
        &the_request(
            context,
            repository,
            upstream,
            kind,
            asset_path,
            requested,
            correlation,
            token.as_deref(),
        ),
        known,
        url.clone(),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            // El upstream pide una credencial: se resuelve el reto y se
            // reintenta **una** vez. Un reto que no se puede satisfacer es
            // un upstream que no sirve, no un bucle.
            let Some(challenge) = the_challenge(&error) else {
                return Err(the_failure(error));
            };
            let Some(credential) = plane.fetcher.the_credential(upstream) else {
                return Err(OciFailure::unavailable(
                    "the upstream asks for a credential and the repository has none",
                ));
            };
            let token = the_token(plane, upstream, &challenge, &credential).await?;
            plane.tokens.put(
                &token_key,
                token.clone(),
                std::time::Duration::from_secs(300),
            );
            resolution::resolve_url(
                &plane.fetcher,
                store.as_ref(),
                plane.ledger.as_ref(),
                &the_request(
                    context,
                    repository,
                    upstream,
                    kind,
                    asset_path,
                    requested,
                    correlation,
                    Some(&token),
                ),
                known,
                url.clone(),
            )
            .await
            .map_err(the_failure)?
        }
    };

    // Un token que el upstream rechazo no se reusa.
    if matches!(outcome, Outcome::Quarantined(_)) {
        plane.tokens.forget(&token_key);
    }
    Ok(outcome)
}

/// La peticion de resolucion, con las reglas de la distribucion.
///
/// Lo que se pide por digest tiene que ser lo que llega, y el recibo va a
/// la ruta del digest: un recibo en la ruta de una etiqueta no respalda el
/// contenido que hay.
#[allow(clippy::too_many_arguments)]
fn the_request<'a>(
    context: &'a VerifiedContext,
    repository: &'a RepositoryDescriptor,
    upstream: &'a UpstreamConfig,
    kind: TheKind,
    asset_path: Option<&'a str>,
    requested: Option<&'a str>,
    correlation: &'a str,
    authorization: Option<&'a str>,
) -> resolution::Resolution<'a> {
    resolution::Resolution {
        account: context.account().as_str(),
        repository: &repository.repo_key,
        asset_path: asset_path.unwrap_or(""),
        upstream,
        config_version: repository.config.version,
        actor: context.principal().subject().as_str(),
        correlation_id: correlation,
        accept: None,
        authorization,
        expect_digest: requested,
        digest_path_prefix: Some(kind.the_prefix()),
    }
}

/// El reto de un upstream que pide un token.
///
/// El texto del reto es el que el upstream mando: `WWW-Authenticate`. De
/// el se lee el realm, el servicio y el alcance.
fn the_challenge(error: &resolution::ResolveError) -> Option<String> {
    match error {
        resolution::ResolveError::Fetch(FetchError::Unauthorized(challenge)) => {
            Some(challenge.clone())
        }
        _ => None,
    }
}

/// El token del realm, si el realm es el que la configuracion declara.
///
/// El realm del reto lo dice el upstream, y un upstream puede decir
/// cualquier cosa: la credencial solo se pide si el realm del reto **es**
/// el realm declarado, con el mismo origen y el mismo camino. Un realm
/// distinto no recibe ni la peticion.
async fn the_token(
    plane: &crate::proxy::ProxyPlane,
    upstream: &UpstreamConfig,
    challenge: &str,
    credential: &crate::proxy::fetch::Secret,
) -> Result<String, OciFailure> {
    let declared = upstream.auth.as_ref().ok_or_else(|| {
        OciFailure::unavailable("the upstream asks for a credential and no realm is declared")
    })?;
    let challenge = the_bearer_challenge(challenge).ok_or_else(|| {
        OciFailure::unavailable(
            "the upstream asks for a credential in a way that is not understood",
        )
    })?;
    if origin_of(&challenge.realm) != origin_of(&declared.realm)
        || challenge.realm.path() != declared.realm.path()
    {
        tracing::warn!(
            target: "hodei_artifacts::proxy",
            upstream = %upstream.id,
            "the upstream asked for a token in a realm that is not the declared one"
        );
        return Err(OciFailure::unavailable(
            "the upstream asks for a credential in a realm that is not the declared one",
        ));
    }

    // La peticion del token: al realm declarado, con la credencial del
    // upstream y solo a el.
    let mut url = declared.realm.clone();
    {
        let mut query = url.query_pairs_mut();
        if let Some(service) = declared.service.as_deref().or(challenge.service.as_deref()) {
            query.append_pair("service", service);
        }
        if let Some(scope) = challenge.scope.as_deref() {
            query.append_pair("scope", scope);
        }
    }
    let user = declared.the_username(&upstream.id).to_string();
    let mut opened = plane
        .fetcher
        .open_with_basic(
            upstream,
            url,
            &origin_of(&declared.realm),
            &user,
            credential,
        )
        .await
        .map_err(|error| {
            tracing::warn!(
                target: "hodei_artifacts::proxy",
                upstream = %upstream.id,
                error = %error,
                "the token could not be asked for"
            );
            OciFailure::unavailable("the credential of the upstream could not be used")
        })?;
    let mut body = Vec::new();
    while let Some(chunk) = opened.chunk().await.map_err(|_| {
        OciFailure::unavailable("the answer of the token endpoint could not be read")
    })? {
        body.extend_from_slice(&chunk);
    }
    let answer: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| OciFailure::unavailable("the answer of the token endpoint is not readable"))?;
    answer
        .get("token")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| OciFailure::unavailable("the answer of the token endpoint has no token"))
}

/// El reto `Bearer` de una cabecera `WWW-Authenticate`.
///
/// El formato es `Bearer realm="...",service="...",scope="..."`. Solo se
/// entiende el reto `Bearer`: es el que dice el protocolo de la
/// distribucion, y un reto en otro dialecto no se adivina.
fn the_bearer_challenge(header: &str) -> Option<TheChallenge> {
    let rest = header.trim();
    let rest = rest
        .strip_prefix("Bearer ")
        .or_else(|| rest.strip_prefix("bearer "))?;
    let mut realm = None;
    let mut service = None;
    let mut scope = None;
    for part in rest.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "realm" => realm = Some(value.to_string()),
            "service" => service = Some(value.to_string()),
            "scope" => scope = Some(value.to_string()),
            _ => {}
        }
    }
    let realm = Url::parse(&realm?).ok()?;
    Some(TheChallenge {
        realm,
        service,
        scope,
    })
}

/// El reto de autenticacion de un upstream.
#[derive(Debug, Clone)]
struct TheChallenge {
    /// Donde se pide el token.
    realm: Url,
    /// El servicio que se pide, si el reto lo nombra.
    service: Option<String>,
    /// El alcance que se pide, si el reto lo nombra.
    scope: Option<String>,
}

/// El fallo de la resolucion, en el dialecto de la distribucion.
///
/// Lo que el cliente ve no dice ni la URL del upstream ni el motivo exacto:
/// el upstream es la red interna del servicio.
fn the_failure(error: resolution::ResolveError) -> OciFailure {
    match error {
        resolution::ResolveError::Fetch(FetchError::NotFound) => {
            OciFailure::manifest_unknown("the upstream does not have that content")
        }
        resolution::ResolveError::Fetch(FetchError::Unauthorized(_)) => {
            OciFailure::unavailable("the upstream asks for a credential that could not be used")
        }
        resolution::ResolveError::Fetch(FetchError::Offline) => {
            OciFailure::unavailable("the proxy is offline and does not reach any upstream")
        }
        resolution::ResolveError::Fetch(_) => {
            OciFailure::unavailable("the upstream did not deliver a usable content")
        }
        resolution::ResolveError::Blob(_) | resolution::ResolveError::Ledger(_) => {
            OciFailure::unavailable("the content could not be stored")
        }
    }
}

/// El contenido que se sirve, con las cabeceras de la distribucion.
///
/// La compuerta de confianza va **antes** de las condiciones de lectura: un
/// `304` o un `412` dirian que el contenido existe.
#[allow(clippy::too_many_arguments)]
async fn the_served(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset: &hodei_catalog::AssetRecord,
    bytes: Vec<u8>,
    kind: TheKind,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    let digest = asset.digest.value.clone();
    crate::trust::admit(state, context.account().as_str(), repository, &asset.digest)
        .await
        .map_err(|admission| match admission {
            crate::trust::Admission::Hidden => kind.the_unknown(&digest),
            crate::trust::Admission::Unavailable => {
                OciFailure::unavailable("the trust ledger is not available")
            }
        })?;

    let etag = crate::raw::conditional::the_etag(&asset.digest);
    let outcome = crate::raw::conditional::the_read_condition(
        crate::raw::conditional::the_if_match(headers).as_deref(),
        crate::raw::conditional::the_if_none_match(headers).as_deref(),
        Some(&etag),
    );
    match outcome {
        crate::raw::conditional::ReadOutcome::Failed => {
            return Err(OciFailure::manifest_unknown(
                "the content is not the one the client asked for",
            ));
        }
        crate::raw::conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            let headers = response.headers_mut();
            headers.insert(
                header::ETAG,
                HeaderValue::from_str(&etag).expect("the etag is a header value"),
            );
            headers.insert(
                "Docker-Content-Digest",
                HeaderValue::from_str(&format!("sha256:{digest}"))
                    .expect("the digest is a header value"),
            );
            return Ok(response);
        }
        crate::raw::conditional::ReadOutcome::Serve => {}
    }

    // El tipo de medio lo dice el manifiesto, no el cliente: el manifiesto
    // es la verdad de que es. Y son los mismos tipos que el registro
    // sirve en un repositorio hosted: un proxy no sirve lo que el
    // registro no sirve.
    let media_type = match kind {
        TheKind::Blob => "application/octet-stream".to_string(),
        TheKind::Manifest => crate::handlers::oci::the_declared_media_type(&bytes)?,
    };

    // Y la negociacion es la misma: un cliente que solo nombra tipos que
    // este registro no sirve recibe el error de manifiesto desconocido, no
    // una respuesta que finge ser lo que pidio.
    if kind == TheKind::Manifest {
        crate::handlers::oci::the_negotiation(headers, &media_type)?;
    }

    let length = bytes.len().to_string();
    let mut response = if head {
        StatusCode::OK.into_response()
    } else {
        (StatusCode::OK, bytes).into_response()
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&media_type).expect("the media type is a header value"),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length).expect("the length is a header value"),
    );
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("the etag is a header value"),
    );
    headers.insert(
        "Docker-Content-Digest",
        HeaderValue::from_str(&format!("sha256:{digest}")).expect("the digest is a header value"),
    );
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bearer_challenge_is_read() {
        let challenge = the_bearer_challenge(
            "Bearer realm=\"https://auth.example.com/token\",service=\"registry\",scope=\"repository:foo:pull\"",
        )
        .expect("the challenge");
        assert_eq!(challenge.realm.as_str(), "https://auth.example.com/token");
        assert_eq!(challenge.service.as_deref(), Some("registry"));
        assert_eq!(challenge.scope.as_deref(), Some("repository:foo:pull"));
    }

    #[test]
    fn a_challenge_that_is_not_bearer_is_not_understood() {
        assert!(the_bearer_challenge("Basic realm=\"registry\"").is_none());
        assert!(the_bearer_challenge("Bearer").is_none());
        assert!(the_bearer_challenge("").is_none());
        // Un realm que no es una URL no es un realm.
        assert!(the_bearer_challenge("Bearer realm=\"not a url\"").is_none());
    }

    #[test]
    fn the_kind_of_a_route_says_what_moves_and_what_does_not() {
        // Un digest no se mueve; una etiqueta si.
        assert_eq!(
            TheKind::Manifest.the_prefix(),
            MANIFESTS,
            "un manifiesto vive en manifests/<digest>"
        );
        assert_eq!(TheKind::Blob.the_prefix(), BLOBS);
    }
}
