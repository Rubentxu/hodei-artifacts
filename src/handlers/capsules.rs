//! The trust capsule of a release.
//!
//! The endpoint answers the question the capsule exists for: what evidence
//! do we have about this artefact, what is missing, what is invalid, what is
//! stale, and where did each assertion come from -- without asking a
//! vendor API anything.
//!
//! It is a read: the capsule is a view of the evidence that is there, and
//! reading it does not change anything.

use axum::{
    Json,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hodei_authz::{Credentials, VerifiedContext, verify_principal};
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::domain::capsule::TrustCapsule;
use kernel::Hrn;

use crate::app_state::AppState;

/// The error of a request for a capsule.
#[derive(Debug)]
pub enum CapsuleApiError {
    /// The request could not be authenticated.
    Unauthorized,
    /// The request was authenticated and not allowed.
    Forbidden,
    /// There is no such capsule, or it is not the caller's.
    NotFound,
    /// The request was not coherent.
    BadRequest,
    /// The store was not available.
    Unavailable,
}

impl IntoResponse for CapsuleApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not found"),
            Self::BadRequest => (StatusCode::BAD_REQUEST, "bad request"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        };
        (status, body).into_response()
    }
}

/// The view of a capsule that the API returns.
///
/// It is the capsule and its profile: the profile says what each dimension
/// holds up, and every dimension links to the evidence that holds it up.
#[derive(Debug, serde::Serialize)]
pub struct CapsuleView {
    /// The subject the capsule is about.
    pub subject: String,
    /// The version of the assembly.
    pub version: String,
    /// The hash of the inputs the reading was made from.
    pub content_hash: String,
    /// The profile, dimension by dimension.
    pub profile: hodei_evidence::domain::capsule::TrustProfile,
}

impl From<TrustCapsule> for CapsuleView {
    fn from(capsule: TrustCapsule) -> Self {
        Self {
            subject: capsule.subject.to_string(),
            version: capsule.version.clone(),
            content_hash: capsule.content_hash.to_string(),
            profile: capsule.profile.clone(),
        }
    }
}

/// `GET /api/v1/capsules?hrn=...`
pub async fn get_capsule(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<CapsuleView>, CapsuleApiError> {
    // Authentication is the first gate.
    let token = bearer_token(&headers).ok_or(CapsuleApiError::Unauthorized)?;
    let principal =
        match verify_principal(state.authenticator.as_ref(), &Credentials::bearer(token)).await {
            Ok(principal) => principal,
            Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
                return Err(CapsuleApiError::Unavailable);
            }
            Err(_) => return Err(CapsuleApiError::Unauthorized),
        };
    let context = VerifiedContext::from_principal(principal);

    let query_hrn = parse_hrn_query(raw_query.as_deref()).ok_or(CapsuleApiError::BadRequest)?;
    let hrn = Hrn::from_string(&query_hrn).ok_or(CapsuleApiError::BadRequest)?;

    // The capsule is about an artefact. A well-formed HRN for another
    // service or type is not an artefact and is not evaluated.
    if hrn.partition() != "hodei"
        || hrn.service() != "artifacts"
        || hrn.resource_type() != "artifact"
    {
        return Err(CapsuleApiError::NotFound);
    }

    // The account comes from the verified principal. A foreign HRN is
    // hidden behind the same 404 as a missing one; it is not evaluated.
    if hrn.account_id() != context.account().as_str() {
        return Err(CapsuleApiError::NotFound);
    }

    let action = crate::actions::read_capsule();
    let outcome = state
        .authorize_resource
        .execute_canonical(context.clone(), action.clone(), &hrn)
        .map_err(|_| CapsuleApiError::BadRequest)?;
    if !outcome.allows_access() {
        return Err(CapsuleApiError::Forbidden);
    }

    let evidence = state
        .evidence_reader
        .evidence_of_subject(context.account().as_str(), &hrn)
        .await
        .map_err(|_| CapsuleApiError::Unavailable)?;

    let now = chrono::Utc::now();
    let capsule = CapsuleAssembler::new().assemble(&hrn, &evidence, now);

    Ok(Json(CapsuleView::from(capsule)))
}

/// The bearer token of a request.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?;
    let value = value.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() { None } else { Some(token) }
}

/// The `hrn` of a query string.
///
/// The query has to name exactly one HRN: a missing one and a repeated one
/// are both a request that does not say what it is about.
fn parse_hrn_query(raw_query: Option<&str>) -> Option<String> {
    let query = raw_query?;
    let mut found: Option<String> = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key != "hrn" {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(percent_decode(value));
    }
    found
}

/// The text of a percent-encoded value.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
