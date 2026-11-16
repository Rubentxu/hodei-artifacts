//! La primera resolucion de un contenido contra su upstream.
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

use chrono::Utc;
use hodei_catalog::{CatalogError, ProvenanceLedger, ProvenanceReceipt, UpstreamConfig};
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::blob::{BlobError, BlobStore, CommitExpectation};

use super::fetch::{FetchError, ProxyFetcher};

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
/// Si devuelve `Ok`, el contenido esta publicado en el almacen **y** su
/// recibo en el libro. Si devuelve `Err`, no hay recibo y nada se sirve: un
/// fallo antes del commit no deja contenido; uno del libro, despues del
/// commit, deja un blob sin catalogar que recoge el GC (un blob ya presente
/// con ese mismo digest no se toca: es el mismo contenido).
pub async fn resolve(
    fetcher: &ProxyFetcher,
    store: &dyn BlobStore,
    ledger: &dyn ProvenanceLedger,
    request: &Resolution<'_>,
) -> Result<Resolved, ResolveError> {
    let mut opened = fetcher.open(request.upstream, request.asset_path).await?;
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

    let receipt = ProvenanceReceipt {
        id: format!("prv-{}", uuid::Uuid::new_v4()),
        account: request.account.to_string(),
        repository: request.repository.to_string(),
        asset_path: request.asset_path.to_string(),
        upstream_id: request.upstream.id.clone(),
        source_url: opened.source_url(),
        config_version: request.config_version,
        digest: digest.clone(),
        size_bytes: verified.size_bytes,
        checksum: verified.evidence,
        status: 200,
        redirects: opened.redirects(),
        etag: opened.etag().map(str::to_string),
        actor: request.actor.to_string(),
        correlation_id: request.correlation_id.to_string(),
        fetched_at: Utc::now(),
    };
    // Sin recibo no hay procedencia: si el libro falla, el contenido ya
    // publicado queda como un blob mas sin catalogar (se recoge por GC) y
    // la peticion falla; nunca se sirve sin su recibo.
    ledger.record(&receipt).await?;

    Ok(Resolved {
        digest,
        size_bytes: verified.size_bytes,
        receipt,
    })
}
