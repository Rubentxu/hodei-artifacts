//! La vista de detalle de un paquete.
//!
//! La vista es lo que el navegador de paquetes muestra de un asset: donde
//! estan los bytes, que se publico, de quien cuelga, que evidencia hay
//! y que decision se ha tomado sobre el.
//!
//! # Lo que la vista no dice
//!
//! La confianza sale de la **concesion**, no de la ausencia de
//! hallazgos. Un contenido del que nadie ha escrito nada no es de fiar:
//! es desconocido, y lo que se dice de el es que esta en cuarentena.
//! Confundir "nadie ha dicho que sea malo" con "nadie ha dicho que sea
//! bueno" es como un registro acaba sirviendo software sin mirar.
//!
//! Y la decision que se muestra es la **vigente**: si la concesion se ha
//! revocado, la vista dice que el contenido esta en cuarentena, porque
//! eso es lo que es ahora.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use hodei_catalog::{CatalogError, DigestScope, TrustGrant, TrustState};
use hodei_evidence::ContentDigest;
use hodei_evidence::application::capsule::CapsuleAssembler;

use crate::app_state::AppState;
use crate::handlers::capsules::CapsuleView;
use crate::handlers::{Authentication, the_context};

/// Lo que puede fallar al pedir el detalle de un paquete.
#[derive(Debug)]
pub enum PackageApiError {
    /// La peticion no se pudo autenticar.
    Unauthorized,
    /// El principal no puede leer el repositorio.
    Forbidden,
    /// No hay nada ahi, o no es de esta cuenta.
    NotFound,
    /// La peticion no es coherente.
    BadRequest,
    /// El catalogo o el libro de concesiones no pudieron responder.
    Unavailable,
    /// Algo fallo por dentro.
    Internal,
}

impl IntoResponse for PackageApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not found"),
            Self::BadRequest => (StatusCode::BAD_REQUEST, "bad request"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
        };
        let mut response = (status, body).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                header::HeaderValue::from_static("Bearer"),
            );
        }
        response
    }
}

/// El detalle de un paquete.
#[derive(Debug, serde::Serialize)]
pub struct PackageView {
    /// El repositorio.
    pub repository: String,
    /// La ruta del asset dentro del repositorio.
    pub asset_path: String,
    /// El nombre del paquete.
    pub package_name: String,
    /// La version del paquete.
    pub package_version: String,
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// Los bytes que tiene el contenido.
    pub size_bytes: u64,
    /// Cuando se movio por ultima vez.
    pub updated_at: DateTime<Utc>,
    /// Donde estan los bytes.
    pub blob: BlobView,
    /// Lo que se publico, tal y como se publico.
    pub published: PublishedView,
    /// De quien cuelga y quien cuelga de el.
    pub lineage: LineageView,
    /// Lo que dice la evidencia.
    pub capsule: CapsuleView,
    /// Lo que se ha decidido sobre el contenido.
    pub trust: TrustView,
}

/// Donde estan los bytes.
#[derive(Debug, serde::Serialize)]
pub struct BlobView {
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// El ambito en el que vive.
    pub scope: DigestScope,
    /// Los bytes que tiene.
    pub size_bytes: u64,
    /// Cuando se escribio la referencia.
    pub updated_at: DateTime<Utc>,
}

/// Lo que se publico.
///
/// Es la version tal y como se publico: las coordenadas, el digest y el
/// momento. El asset puede haberse movido despues a otra version, y
/// esta es la version a la que apunta ahora.
#[derive(Debug, serde::Serialize)]
pub struct PublishedView {
    /// El nombre del paquete.
    pub package_name: String,
    /// La version del paquete.
    pub package_version: String,
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// Los bytes que tiene el contenido.
    pub size_bytes: u64,
    /// Cuando se publico.
    pub published_at: DateTime<Utc>,
}

/// La linea de los manifiestos.
#[derive(Debug, serde::Serialize)]
pub struct LineageView {
    /// El manifiesto al que cuelga este asset, si cuelga de alguno.
    pub subject: Option<ContentDigest>,
    /// Los manifiestos que cuelgan de este.
    pub referrers: Vec<ReferrerView>,
}

/// Un manifiesto que cuelga de otro.
#[derive(Debug, serde::Serialize)]
pub struct ReferrerView {
    /// La ruta del referrer dentro del repositorio.
    pub asset_path: String,
    /// El digest del manifiesto.
    pub digest: ContentDigest,
    /// Los bytes que tiene el manifiesto.
    pub size_bytes: u64,
    /// El digest al que apunta.
    pub subject: ContentDigest,
}

/// Lo que se ha decidido sobre el contenido.
#[derive(Debug, serde::Serialize)]
pub struct TrustView {
    /// El estado de confianza en este repositorio.
    pub state: TrustState,
    /// La decision que lo concede, si hay alguna vigente.
    pub decision: Option<DecisionView>,
}

/// La decision de confianza que concede un contenido.
#[derive(Debug, serde::Serialize)]
pub struct DecisionView {
    /// La decision que concedio el contenido.
    pub decision_id: String,
    /// Quien concedio.
    pub actor: String,
    /// Cuando se concedio.
    pub granted_at: DateTime<Utc>,
}

/// `GET /api/v1/packages/{repository}/{path}`
///
/// La ruta del asset llega con las barras de dentro, porque la ruta es lo
/// que nombra el contenido dentro del repositorio.
pub async fn get_package(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repository, path)): Path<(String, String)>,
) -> Result<Json<PackageView>, PackageApiError> {
    let context = the_context(&state, &headers)
        .await
        .map_err(|failure| match failure {
            Authentication::Unauthorized => PackageApiError::Unauthorized,
            Authentication::Unavailable => PackageApiError::Unavailable,
        })?;

    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or(PackageApiError::Unavailable)?;
    let account = context.account().as_str();

    // El repositorio primero: si no es de esta cuenta, no se evalua. Un
    // repositorio ajeno y uno que no existen se esconden detras del
    // mismo 404, porque responder distinto seria decir que existe.
    let descriptor = reader
        .repository(account, &repository)
        .await
        .map_err(the_catalog_error)?;

    let action = crate::actions::read_repository();
    let outcome = state
        .authorize_resource
        .execute_canonical(context.clone(), action.clone(), &descriptor.hrn)
        .map_err(|_| PackageApiError::BadRequest)?;
    if !outcome.allows_access() {
        return Err(PackageApiError::Forbidden);
    }

    let asset = reader
        .asset(account, &repository, &path)
        .await
        .map_err(the_catalog_error)?;
    let reference = reader
        .reference(account, &repository, &path)
        .await
        .map_err(the_catalog_error)?;
    let published = reader
        .version(account, &repository, &asset.coordinates)
        .await
        .map_err(the_catalog_error)?;
    let referrers = reader
        .referrers(account, &repository, &asset.digest)
        .await
        .map_err(the_catalog_error)?;

    // La decision: la concesion vigente del libro. Lo que no esta
    // concedido esta en cuarentena, y no se deduce de la evidencia.
    let decision = the_decision(&state, account, &repository, &asset.digest).await?;
    let state_of_trust = if decision.is_some() {
        TrustState::Trusted
    } else {
        TrustState::Quarantined
    };

    // La capsula: lo que dice la evidencia que hay. Una capsula sin
    // evidencia no dice que el contenido sea malo: dice que no se sabe
    // nada de el.
    let subject = crate::trust::hrn::artifact_hrn(
        descriptor.hrn.region(),
        account,
        &repository,
        &asset.digest,
    );
    let evidence = state
        .evidence_reader
        .evidence_of_subject(account, &subject)
        .await
        .map_err(|_| PackageApiError::Unavailable)?;
    let capsule = CapsuleAssembler::new().assemble(&subject, &evidence, state.identity.now());

    Ok(Json(PackageView {
        repository: repository.clone(),
        asset_path: asset.asset_path.clone(),
        package_name: asset.coordinates.package_name.clone(),
        package_version: asset.coordinates.package_version.clone(),
        digest: asset.digest.clone(),
        size_bytes: asset.size_bytes,
        updated_at: asset.updated_at,
        blob: BlobView {
            digest: reference.digest,
            scope: reference.scope,
            size_bytes: reference.size_bytes,
            updated_at: reference.updated_at,
        },
        published: PublishedView {
            package_name: published.coordinates.package_name,
            package_version: published.coordinates.package_version,
            digest: published.digest,
            size_bytes: published.size_bytes,
            published_at: published.published_at,
        },
        lineage: LineageView {
            subject: asset.subject.clone(),
            referrers: referrers
                .into_iter()
                .map(|referrer| ReferrerView {
                    asset_path: referrer.asset_path,
                    digest: referrer.digest,
                    size_bytes: referrer.size_bytes,
                    subject: referrer.subject,
                })
                .collect(),
        },
        capsule: CapsuleView::from(capsule),
        trust: TrustView {
            state: state_of_trust,
            decision: decision.map(DecisionView::of),
        },
    }))
}

/// La concesion vigente de un contenido, si la hay.
///
/// Sin libro de concesiones no hay decision que leer: lo que no se puede
/// leer no se concede, y lo que no se concede esta en cuarentena.
async fn the_decision(
    state: &AppState,
    account: &str,
    repository: &str,
    digest: &ContentDigest,
) -> Result<Option<TrustGrant>, PackageApiError> {
    let Some(ledger) = state.trust_ledger.as_ref() else {
        return Ok(None);
    };
    ledger
        .active_grant(account, repository, digest)
        .await
        .map_err(|_| PackageApiError::Unavailable)
}

/// La vista de una decision.
impl DecisionView {
    fn of(grant: TrustGrant) -> Self {
        Self {
            decision_id: grant.decision_id,
            actor: grant.actor,
            granted_at: grant.granted_at,
        }
    }
}

/// El error del catalogo, traducido a lo que ve el cliente.
fn the_catalog_error(error: CatalogError) -> PackageApiError {
    match error {
        CatalogError::Missing { .. } => PackageApiError::NotFound,
        CatalogError::Denied { .. } => PackageApiError::Forbidden,
        CatalogError::InvalidRequest { .. } => PackageApiError::BadRequest,
        CatalogError::Unavailable { .. } => PackageApiError::Unavailable,
        _ => PackageApiError::Internal,
    }
}
