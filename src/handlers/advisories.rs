//! La entrada soportada de advisories: el ciclo reactivo CVE (HSP-137).
//!
//! `POST /api/v1/advisories/accept` admite un advisory OSV contra el
//! grafo y **corre el ciclo completo en la misma llamada**: la proyeccion
//! del dominio escribe el caso, el motor dice los intents de reevaluacion
//! y el servicio de promociones decide contra la politica activa y el
//! libro mayor. Cuando el advisory toca un artefacto concedido, la
//! reevaluacion retira la concesion antes de que esta funcion responda:
//! el pull siguiente del digest ya es 404. No hay un consumidor externo
//! que haya estado caido, y por eso no hay latencia que prometer: la
//! distribucion bloquea cuando el ciclo bloquea.
//!
//! La ruta es administrativa a proposito: admitir un advisory es decirle
//! al servicio "esto es un hecho", y quien puede decir eso puede apagar
//! el canal trusted de un artefacto. El gate Cedar de `/api/v1/admin`
//! decide que principal puede; aqui no se vuelve a decidir.

use crate::app_state::AppState;
#[cfg(feature = "legacy")]
use crate::audit::AuditResult;
#[cfg(feature = "legacy")]
use crate::handlers::{correlation_id, record_audit};
#[cfg(feature = "legacy")]
use crate::trust::reaction::AdvisoryAdmission;
use crate::trust::reaction::ReactorError;
use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hodei_authz::VerifiedContext;
#[cfg(feature = "legacy")]
use hodei_catalog::CatalogError;
// HSP-213: el digest solo lo necesita el tramo del ciclo, que no existe
// fuera de la feature `legacy`.
#[cfg(feature = "legacy")]
use hodei_evidence::ContentDigest;
use serde::Deserialize;

/// El cuerpo de la admision.
#[derive(Debug, Deserialize)]
pub struct AcceptAdvisoryRequest {
    /// El repositorio cuyo canal protege el ciclo.
    pub repository: String,
    /// El digest del artefacto afectado, como `sha256:<hex>`: es el
    /// sujeto que la proyeccion nombra, igual que en la promocion.
    pub digest: String,
    /// El advisory OSV, como el estandar lo publica.
    pub advisory: serde_json::Value,
    /// El inventario CycloneDX del artefacto afectado, cuando el emisor
    /// lo tiene: es lo que escribe los componentes y el `affected_by` en
    /// el grafo. Sin el, el advisory se admite como hecho suelto y la
    /// reevaluacion solo toca a quien el inventario ya proyectado nombre.
    #[serde(default)]
    pub inventory: Option<serde_json::Value>,
}

/// Por que la admision no pudo correr.
pub struct AdvisoryApiError {
    status: StatusCode,
    message: String,
}

impl AdvisoryApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl From<ReactorError> for AdvisoryApiError {
    fn from(error: ReactorError) -> Self {
        match error {
            // El detalle del motivo es del cliente: un documento que no
            // se lee es un 400 con la razon.
            ReactorError::BadRequest(reason) => Self::new(StatusCode::BAD_REQUEST, reason),
            // El detalle de una dependencia que no responde es del log.
            ReactorError::Unavailable(reason) => {
                tracing::warn!(%reason, "the reactive cycle could not run");
                Self::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "the reactive cycle is not available",
                )
            }
        }
    }
}

impl IntoResponse for AdvisoryApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({
            "error": self.message,
            "status": self.status.as_u16(),
        }));
        (self.status, body).into_response()
    }
}

/// El digest del cuerpo: `sha256:<hex>`, la MISMA forma que la
/// promocion y el libro mayor usan para nombrar el sujeto.
#[cfg(feature = "legacy")]
fn parse_digest(text: &str) -> Option<ContentDigest> {
    let value = text.strip_prefix("sha256:")?;
    if value.chars().any(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let digest = ContentDigest::sha256(value);
    digest.is_well_formed().then_some(digest)
}

/// El documento de un cuerpo JSON a texto: el lector del estandar pide
/// texto, y el valor ya parseado se re-serializa de forma canonica para
/// que el identificador de la evidencia sea el mismo para todos los que
/// digan el mismo advisory.
#[cfg(feature = "legacy")]
fn document_of(value: &serde_json::Value) -> String {
    value.to_string()
}

/// `POST /api/v1/advisories/accept`
pub async fn accept(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    headers: HeaderMap,
    Json(request): Json<AcceptAdvisoryRequest>,
) -> Result<Response, AdvisoryApiError> {
    // HSP-213: fuera de la feature `legacy` no hay ciclo reactivo que
    // correr --el ciclo vive sobre los almacenes embebidos--, y la
    // entrada responde 501 con el mismo mensaje de siempre.
    #[cfg(not(feature = "legacy"))]
    {
        let _ = (&state, &context, &headers, &request);
        Err(AdvisoryApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "the reactive cycle is not wired in this deployment",
        ))
    }
    #[cfg(feature = "legacy")]
    {
        let correlation = correlation_id(&headers, &state.identity);
        let account = context.account().as_str().to_string();
        let actor = context.principal().subject().as_str().to_string();

        let reactor = state.reactor.as_ref().ok_or_else(|| {
            AdvisoryApiError::new(
                StatusCode::NOT_IMPLEMENTED,
                "the reactive cycle is not wired in this deployment",
            )
        })?;
        let reader = state.catalog_reader.as_ref().ok_or_else(|| {
            AdvisoryApiError::new(StatusCode::NOT_IMPLEMENTED, "the catalog is not available")
        })?;

        // El repositorio da la region del tenant del grafo, la MISMA_region
        // con la que la promocion escribio el sujeto. Un repositorio que no
        // existe o es de otra cuenta es el mismo 404 de siempre: la consulta
        // va acotada a la cuenta verificada.
        let repository = reader
            .repository(&account, &request.repository)
            .await
            .map_err(|error| match error {
                CatalogError::Missing { .. } => {
                    AdvisoryApiError::new(StatusCode::NOT_FOUND, "the repository does not exist")
                }
                _ => AdvisoryApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "the catalog could not answer",
                ),
            })?;
        let region = repository.hrn.region().to_string();

        let advisory_text = document_of(&request.advisory);
        let inventory_text = request.inventory.as_ref().map(document_of);
        let digest = parse_digest(&request.digest).ok_or_else(|| {
            AdvisoryApiError::new(
                StatusCode::BAD_REQUEST,
                "a digest is `sha256:` followed by 64 lowercase hexadecimal digits",
            )
        })?;

        // El sujeto del ciclo es EL ARTEFACTO que el cuerpo nombra
        // (repositorio + digest), no "todo lo que el advisory toca": la
        // proyeccion escribe los componentes y el `affected_by` contra ese
        // sujeto, y el radio de impacto parte de ellos. La region la da el
        // repositorio --la MISMA region con la que la promocion estampo el
        // HRN del sujeto, o el ciclo preguntaria el grafo de otro tenant.
        let outcome = reactor
            .admit_advisory(
                &account,
                &region,
                &request.repository,
                &digest,
                &advisory_text,
                inventory_text.as_deref(),
            )
            .await;

        let admission: AdvisoryAdmission = match outcome {
            Ok(admission) => admission,
            Err(error) => {
                record_audit(
                    &state,
                    &actor,
                    &account,
                    "AcceptAdvisory",
                    &request.repository,
                    AuditResult::Failed,
                    &correlation,
                );
                return Err(error.into());
            }
        };

        record_audit(
            &state,
            &actor,
            &account,
            "AcceptAdvisory",
            &admission.advisory,
            AuditResult::Allowed,
            &correlation,
        );
        Ok((StatusCode::OK, Json(admission)).into_response())
    }
}
