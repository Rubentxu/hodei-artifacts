//! The supported advisories entry point: the reactive CVE cycle (HSP-137).
//!
//! `POST /api/v1/advisories/accept` admits an OSV advisory against the
//! graph and **runs the full cycle in the same call**: the domain projection
//! writes the case, the engine emits the re-evaluation intents
//! and the promotions service decides against the active policy and the
//! ledger. When the advisory touches a granted artifact, the
//! re-evaluation withdraws the grant before this function responds:
//! the next pull of the digest is already a 404. There is no external consumer
//! that could have been down, and so there is no latency to promise: the
//! distribution blocks when the cycle blocks.
//!
//! The route is administrative on purpose: admitting an advisory is telling
//! the service "this is a fact", and whoever can say that can switch off
//! an artifact's trusted channel. The Cedar gate of `/api/v1/admin`
//! decides which principal may; no new decision is made here.

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::trust::reaction::AdvisoryAdmission;
use crate::trust::reaction::ReactorError;
use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hodei_authz::VerifiedContext;
use hodei_catalog::CatalogError;
use hodei_evidence::ContentDigest;
use serde::Deserialize;

/// The admission body.
#[derive(Debug, Deserialize)]
pub struct AcceptAdvisoryRequest {
    /// The repository whose channel the cycle protects.
    pub repository: String,
    /// The digest of the affected artifact, as `sha256:<hex>`: it is the
    /// subject the projection names, same as in the promotion.
    pub digest: String,
    /// The OSV advisory, as the standard publishes it.
    pub advisory: serde_json::Value,
    /// The CycloneDX inventory of the affected artifact, when the issuer
    /// has it: it is what writes the components and the `affected_by` in
    /// the graph. Without it, the advisory is admitted as a loose fact and the
    /// re-evaluation only touches whatever the already-projected inventory names.
    #[serde(default)]
    pub inventory: Option<serde_json::Value>,
}

/// Why the admission could not run.
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
            // The reason detail belongs to the client: a document that cannot
            // be read is a 400 with the reason.
            ReactorError::BadRequest(reason) => Self::new(StatusCode::BAD_REQUEST, reason),
            // The detail of an unresponsive dependency belongs to the log.
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

/// The body digest: `sha256:<hex>`, the SAME form the
/// promotion and the ledger use to name the subject.
fn parse_digest(text: &str) -> Option<ContentDigest> {
    let value = text.strip_prefix("sha256:")?;
    if value.chars().any(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let digest = ContentDigest::sha256(value);
    digest.is_well_formed().then_some(digest)
}

/// The document of a JSON body as text: the standard's reader asks for
/// text, and the already-parsed value is re-serialized canonically so
/// that the evidence identifier is the same for everyone who
/// states the same advisory.
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

    // The repository gives the graph tenant's region, the SAME region
    // with which the promotion wrote the subject. A repository that does not
    // exist or belongs to another account is the same old 404: the query
    // is scoped to the verified account.
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

    // The cycle's subject is THE ARTIFACT the body names
    // (repository + digest), not "everything the advisory touches": the
    // projection writes the components and the `affected_by` against that
    // subject, and the blast radius starts from them. The region is given by the
    // repository --the SAME region with which the promotion stamped the
    // subject's HRN, or the cycle would query another tenant's graph.
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
