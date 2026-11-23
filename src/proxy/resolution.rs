//! La resolucion de un contenido contra su upstream: la primera vez y cada
//! revalidacion.
//!
//! Une las tres piezas del proxy: el cliente de salida ([`ProxyFetcher`]),
//! el almacen de contenido y el libro de procedencia. El orden importa y es
//! lo que da la garantia:
//!
//! 1. Los bytes entran a una sesion de escritura que **no es visible**.
//! 2. Cuando el cuerpo termina se comprueba: entero, dentro del presupuesto
//!    y con el checksum que el upstream declaro.
//! 3. Solo entonces se publica el blob (`commit`) y se escribe el recibo.
//!
//! Un fetch que falla en cualquier punto aborta la sesion y no deja ni
//! contenido ni recibo: nada de lo que quede en el sistema puede presentarse
//! como contenido verificado de un upstream.
//!
//! Si ya se tiene un contenido para la ruta ([`Known`]) la peticion es
//! condicional y hay tres desenlaces ademas de traerlo:
//!
//! - el upstream confirma (`304`) o entrega los mismos bytes: se escribe un
//!   recibo `Revalidated` y no se mueve nada;
//! - entrega **otros** bytes para un contenido inmutable: no se publican, y
//!   el recibo `Quarantined` guarda lo que se ofrecio;
//! - entrega otros bytes para metadata mutable: es una actualizacion, y se
//!   publica.

use chrono::Utc;
use hodei_catalog::proxy_cache::Known;
use hodei_catalog::{
    CatalogError, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
    UpstreamConfig,
};
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::blob::{BlobError, BlobStore, CommitExpectation};
use url::Url;

use super::fetch::{FetchError, Fetched, ProxyFetcher, Validators};

/// Lo que hace falta saber para resolver una ruta.
#[derive(Debug, Clone)]
pub struct Resolution<'a> {
    /// La cuenta propietaria del repositorio.
    pub account: &'a str,
    /// La clave del repositorio proxy.
    pub repository: &'a str,
    /// La ruta que se pide.
    pub asset_path: &'a str,
    /// El upstream configurado.
    pub upstream: &'a UpstreamConfig,
    /// La version de la configuracion del repositorio.
    pub config_version: u64,
    /// Quien pide el contenido.
    pub actor: &'a str,
    /// La correlacion de la peticion.
    pub correlation_id: &'a str,
    /// El `Accept` que se pide al upstream, si es distinto del de siempre.
    pub accept: Option<&'a str>,
    /// Un token que el upstream ya pidio y que viaja como `Bearer`.
    ///
    /// Solo lo usa la superficie OCI, cuyo upstream puede pedir un token en
    /// su reto. El token viaja **solo** al origen del upstream.
    pub authorization: Option<&'a str>,
    /// El digest que el contenido **tiene** que tener.
    ///
    /// Lo usa la superficie OCI, donde la identidad del contenido es el
    /// digest: un manifiesto pedido por `sha256:...` que llega con otros
    /// bytes no es el que se pidio, y no se publica.
    pub expect_digest: Option<&'a str>,
    /// El prefijo de la ruta del asset, cuando la ruta **es** el digest.
    ///
    /// La superficie OCI guarda el manifiesto en `manifests/<digest>` y el
    /// blob en `blobs/<digest>`, y el recibo tiene que ir ahi: un recibo en
    /// la ruta por la que se pidio --una etiqueta-- no respalda el contenido
    /// que hay, y el contenido sin respaldo no se sirve sin red.
    pub digest_path_prefix: Option<&'a str>,
}

/// Un contenido resuelto y verificado.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// El digest del contenido, ya publicado en el almacen.
    pub digest: ContentDigest,
    /// Sus bytes.
    pub size_bytes: u64,
    /// El recibo que quedo escrito.
    pub receipt: ProvenanceReceipt,
}

/// Como termino una resolucion que no fallo.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Hay bytes nuevos, publicados en el almacen.
    Stored(Resolved),
    /// El upstream confirma el contenido que ya se tenia.
    Unchanged(ProvenanceReceipt),
    /// El upstream entrego otros bytes para un contenido inmutable: no se
    /// publicaron.
    Quarantined(ProvenanceReceipt),
}

/// Por que no se pudo resolver.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// El upstream no entrego un contenido utilizable.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// El almacen no pudo guardar el contenido.
    #[error("the content store failed: {0}")]
    Blob(BlobError),
    /// El libro de procedencia no pudo guardar el recibo.
    #[error("the provenance ledger failed: {0}")]
    Ledger(#[from] CatalogError),
}

impl From<BlobError> for ResolveError {
    fn from(error: BlobError) -> Self {
        Self::Blob(error)
    }
}

/// Trae, verifica, guarda y deja constancia.
///
/// Si devuelve `Ok`, el desenlace esta en el libro: el contenido nuevo esta
/// publicado en el almacen **y** su recibo escrito, o el recibo dice que se
/// confirmo o se retuvo. Si devuelve `Err`, no hay recibo y nada se sirve
/// por esta via: un fallo antes del commit no deja contenido; uno del libro,
/// despues del commit, deja un blob sin catalogar que recoge el GC (un blob
/// ya presente con ese mismo digest no se toca: es el mismo contenido).
pub async fn resolve(
    fetcher: &ProxyFetcher,
    store: &dyn BlobStore,
    ledger: &dyn ProvenanceLedger,
    request: &Resolution<'_>,
    known: Option<&Known>,
) -> Result<Outcome, ResolveError> {
    let Some(url) = request.upstream.target(request.asset_path) else {
        return Err(FetchError::OutOfScope.into());
    };
    resolve_url(fetcher, store, ledger, request, known, url).await
}

/// Como [`resolve`], pero con la URL exacta de donde se trae el contenido.
///
/// Lo usa la superficie PyPI: la pagina de un proyecto y cada distribucion
/// se piden por la URL que declara el upstream, que no es la ruta del
/// asset. La URL se juzga con la misma politica de salida que cualquier
/// otra.
pub async fn resolve_url(
    fetcher: &ProxyFetcher,
    store: &dyn BlobStore,
    ledger: &dyn ProvenanceLedger,
    request: &Resolution<'_>,
    known: Option<&Known>,
    url: Url,
) -> Result<Outcome, ResolveError> {
    let validators = known.and_then(|known| {
        let validators = Validators {
            etag: known.etag.clone(),
            last_modified: known.last_modified.clone(),
        };
        (!validators.is_empty()).then_some(validators)
    });
    let fetched = fetcher
        .open_conditional_with_token(
            request.upstream,
            url,
            request.accept,
            validators.as_ref(),
            request.authorization,
        )
        .await?;
    let mut opened = match fetched {
        Fetched::Content(opened) => *opened,
        // El upstream pide una credencial y no hay ninguna todavia: quien
        // llama tiene que resolver el reto antes de volver a intentarlo. El
        // reto viaja en el error, que es donde se lee lo que dice el
        // upstream sobre como autenticarse.
        Fetched::Unauthorized { challenge } => {
            return Err(FetchError::Unauthorized(challenge.unwrap_or_default()).into());
        }
        Fetched::NotModified(confirmation) => {
            // Un `304` solo se acepta si se enviaron validadores: sin
            // `known` no hay a que referirlo.
            let Some(known) = known else {
                return Err(FetchError::Status(304).into());
            };
            let receipt = ProvenanceReceipt {
                id: format!("prv-{}", uuid::Uuid::new_v4()),
                account: request.account.to_string(),
                repository: request.repository.to_string(),
                asset_path: the_asset_path(request, &known.digest.value),
                upstream_id: request.upstream.id.clone(),
                source_url: confirmation.source_url,
                config_version: request.config_version,
                digest: known.digest.clone(),
                size_bytes: known.size_bytes,
                // Un `304` no trae cuerpo que comprobar: hereda la
                // evidencia del recibo que respaldaba esos bytes.
                checksum: known
                    .checksum
                    .clone()
                    .unwrap_or(ChecksumEvidence::ComputedOnly),
                status: 304,
                redirects: confirmation.redirects,
                etag: confirmation.etag.or_else(|| known.etag.clone()),
                last_modified: confirmation
                    .last_modified
                    .or_else(|| known.last_modified.clone()),
                outcome: ReceiptOutcome::Revalidated,
                actor: request.actor.to_string(),
                correlation_id: request.correlation_id.to_string(),
                fetched_at: Utc::now(),
            };
            ledger.record(&receipt).await?;
            return Ok(Outcome::Unchanged(receipt));
        }
    };
    let mut guard = opened.guard(request.upstream.require_checksum);
    let mut session = store.begin().await?;

    // Cualquier fallo a partir de aqui descarta la escritura en curso.
    loop {
        match opened.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(error) = guard.push(chunk.len()) {
                    let _ = session.abort().await;
                    return Err(error.into());
                }
                if let Err(error) = session.append(&chunk).await {
                    let _ = session.abort().await;
                    return Err(error.into());
                }
            }
            Ok(None) => break,
            Err(error) => {
                let _ = session.abort().await;
                return Err(error.into());
            }
        }
    }

    // El digest lo calcula la sesion mientras pasan los trozos: es el que
    // el commit va a comprobar y el que se compara con lo declarado.
    let computed = session.the_digest();
    let verified = match guard.finish(&computed.value) {
        Ok(verified) => verified,
        Err(error) => {
            let _ = session.abort().await;
            return Err(error.into());
        }
    };

    // El digest esperado: el contenido tiene que **ser** el que se pidio.
    //
    // La superficie OCI lo necesita: el manifiesto pedido por `sha256:...`
    // que llega con otros bytes no es el que se pidio, y un contenido que
    // no es el pedido no se publica --ni se sirve, ni queda con recibo que
    // lo respalde. Se retiene, y el recibo lo dice.
    if let Some(expected) = request.expect_digest
        && expected != computed.value
    {
        let _ = session.abort().await;
        let receipt = ProvenanceReceipt {
            id: format!("prv-{}", uuid::Uuid::new_v4()),
            account: request.account.to_string(),
            repository: request.repository.to_string(),
            // El recibo va a la ruta del contenido **pedido**: es el que
            // tiene que saber que lo que se ofrecio no era lo suyo.
            asset_path: the_asset_path(request, expected),
            upstream_id: request.upstream.id.clone(),
            source_url: opened.source_url(),
            config_version: request.config_version,
            digest: computed.clone(),
            size_bytes: verified.size_bytes,
            checksum: verified.evidence.clone(),
            status: 200,
            redirects: opened.redirects(),
            etag: opened.etag().map(str::to_string),
            last_modified: opened.last_modified().map(str::to_string),
            outcome: ReceiptOutcome::Quarantined,
            actor: request.actor.to_string(),
            correlation_id: request.correlation_id.to_string(),
            fetched_at: Utc::now(),
        };
        ledger.record(&receipt).await?;
        return Ok(Outcome::Quarantined(receipt));
    }

    let mut receipt = ProvenanceReceipt {
        id: format!("prv-{}", uuid::Uuid::new_v4()),
        account: request.account.to_string(),
        repository: request.repository.to_string(),
        asset_path: the_asset_path(request, &computed.value),
        upstream_id: request.upstream.id.clone(),
        source_url: opened.source_url(),
        config_version: request.config_version,
        digest: computed.clone(),
        size_bytes: verified.size_bytes,
        checksum: verified.evidence.clone(),
        status: 200,
        redirects: opened.redirects(),
        etag: opened.etag().map(str::to_string),
        last_modified: opened.last_modified().map(str::to_string),
        outcome: ReceiptOutcome::Fetched,
        actor: request.actor.to_string(),
        correlation_id: request.correlation_id.to_string(),
        fetched_at: Utc::now(),
    };

    // Ya se tenia un contenido: la pregunta es si estos bytes son los mismos.
    if let Some(known) = known {
        let same = known.digest == computed;
        if same || known.immutable {
            // Ni los mismos bytes ni los ofrecidos como cambio de un
            // contenido inmutable se publican: no hay nada que escribir.
            let _ = session.abort().await;
            receipt.outcome = if same {
                ReceiptOutcome::Revalidated
            } else {
                ReceiptOutcome::Quarantined
            };
            ledger.record(&receipt).await?;
            return Ok(if same {
                Outcome::Unchanged(receipt)
            } else {
                Outcome::Quarantined(receipt)
            });
        }
    }

    let digest = match session
        .commit(CommitExpectation {
            digest: computed,
            length: verified.size_bytes,
        })
        .await
    {
        Ok(digest) => digest,
        Err(error) => {
            let _ = session.abort().await;
            return Err(error.into());
        }
    };
    // Sin recibo no hay procedencia: si el libro falla, el contenido ya
    // publicado queda como un blob mas sin catalogar (se recoge por GC) y
    // la peticion falla; nunca se sirve sin su recibo.
    ledger.record(&receipt).await?;

    Ok(Outcome::Stored(Resolved {
        digest,
        size_bytes: verified.size_bytes,
        receipt,
    }))
}

/// La ruta del asset que respalda el recibo.
///
/// Cuando la ruta **es** el digest del contenido --como en la superficie
/// OCI, donde el manifiesto vive en `manifests/<digest>`-- el recibo va ahi
/// y no a la ruta por la que se pidio: un recibo en la ruta de una etiqueta
/// no respalda el contenido que hay, y el contenido sin respaldo no se
/// sirve sin red.
fn the_asset_path(request: &Resolution<'_>, digest: &str) -> String {
    match request.digest_path_prefix {
        Some(prefix) => format!("{prefix}/{digest}"),
        None => request.asset_path.to_string(),
    }
}
