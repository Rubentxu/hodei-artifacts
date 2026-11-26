//! Trusted publishers (HSP-152): the exchange and its management.
//!
//! - `POST /api/v1/publishers/exchange`: the OIDC ID token travels as the
//!   `Bearer` credential, the body names `{account, repository}`. A refusal
//!   is generic (`401 exchange_denied`, or `503` when the registry or the
//!   issuer's keys cannot be obtained); the reason goes to the service trail
//!   and never has the token in it.
//! - `GET|PUT|DELETE /api/v1/trusted-publishers/{repository}[/{id}]`: the
//!   management of the registry. Authenticated as any user, the repository
//!   must exist in the caller's account, and Cedar decides with the
//!   dedicated action `ManageTrustedPublishers` on the repository (ABAC
//!   scope included).

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_catalog::RepositoryFormat;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{bearer_token, correlation_id, error_body, record_audit};
use crate::publishers::{ExchangeError, PublisherSpec, TrustedPublisher};
use crate::resolution::{ResolutionError, authorize_repository, the_context, the_repository_in};

/// The body of an exchange request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeRequest {
    pub account: String,
    pub repository: String,
}

#[derive(Debug, Serialize)]
struct ExchangeResponse {
    token_type: &'static str,
    credential: String,
    expires_in: u64,
    scope: ExchangeScope,
}

#[derive(Debug, Serialize)]
struct ExchangeScope {
    account: String,
    repository: String,
    actions: Vec<&'static str>,
}

/// `POST /api/v1/publishers/exchange`
pub async fn exchange(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ExchangeRequest>,
) -> Response {
    let Some(service) = state.publishers.as_ref() else {
        return ResolutionError::NotImplemented("trusted publishers are not configured")
            .into_response();
    };
    let correlation = correlation_id(&headers, &state.identity);
    let Some(token) = bearer_token(&headers) else {
        return denied();
    };
    let account = request.account.trim();
    let repository = request.repository.trim();
    let resource = format!("{account}/{repository}");
    match service.exchange(&token, account, repository).await {
        Ok(exchange) => {
            record_audit(
                &state,
                &exchange.workload,
                &exchange.account,
                "publishers.exchange",
                &resource,
                AuditResult::Allowed,
                &correlation,
            );
            (
                StatusCode::OK,
                Json(ExchangeResponse {
                    token_type: "Bearer",
                    credential: exchange.credential,
                    expires_in: exchange.expires_in,
                    scope: ExchangeScope {
                        account: exchange.account,
                        repository: exchange.repository,
                        actions: vec!["upload"],
                    },
                }),
            )
                .into_response()
        }
        Err(error) => {
            // The reason is a fixed label: it never carries the token.
            tracing::info!(correlation_id = %correlation, reason = error.reason(), "publisher exchange refused");
            let result = match error {
                ExchangeError::Unavailable(_) => AuditResult::Failed,
                _ => AuditResult::Denied,
            };
            record_audit(
                &state,
                "unknown-workload",
                account,
                "publishers.exchange",
                &resource,
                result,
                &correlation,
            );
            match error {
                ExchangeError::BadRequest => (
                    StatusCode::BAD_REQUEST,
                    error_body("bad_request", "invalid exchange request"),
                )
                    .into_response(),
                ExchangeError::Unavailable(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    error_body("exchange_unavailable", "the exchange cannot be decided now"),
                )
                    .into_response(),
                ExchangeError::Denied(_) => denied(),
            }
        }
    }
}

fn denied() -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        error_body("exchange_denied", "the exchange was denied"),
    )
        .into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        header::HeaderValue::from_static("Bearer"),
    );
    response
}

/// Authenticate, find the repository in the caller's account and ask Cedar
/// for `ManageTrustedPublishers` on it. A denial is audited.
async fn manage_gate(
    state: &AppState,
    headers: &HeaderMap,
    repository: &str,
) -> Result<(hodei_authz::VerifiedContext, String), ResolutionError> {
    let context = the_context(state, headers).await?;
    let descriptor = the_repository_in(
        state,
        &context,
        repository,
        &[RepositoryFormat::Raw, RepositoryFormat::Maven2],
    )
    .await?;
    let correlation = correlation_id(headers, &state.identity);
    let action = crate::actions::manage_trusted_publishers();
    if let Err(error) = authorize_repository(
        &state.authorize_resource,
        &context,
        action.clone(),
        &descriptor,
    ) {
        record_audit(
            state,
            context.principal().subject().as_str(),
            context.account().as_str(),
            &action.to_string(),
            repository,
            if matches!(error, ResolutionError::Forbidden) {
                AuditResult::Denied
            } else {
                AuditResult::Failed
            },
            &correlation,
        );
        return Err(error);
    }
    Ok((context, correlation))
}

fn service(state: &AppState) -> Result<&crate::publishers::PublishersService, ResolutionError> {
    state
        .publishers
        .as_deref()
        .ok_or(ResolutionError::NotImplemented(
            "trusted publishers are not configured",
        ))
}

/// `GET /api/v1/trusted-publishers/{repository}`
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(repository): Path<String>,
) -> Result<Response, ResolutionError> {
    let service = service(&state)?;
    let (context, _) = manage_gate(&state, &headers, &repository).await?;
    let records = service
        .registry()
        .list(context.account().as_str(), &repository)
        .await
        .map_err(|_| ResolutionError::Unavailable)?;
    Ok((StatusCode::OK, Json(records)).into_response())
}

/// `PUT /api/v1/trusted-publishers/{repository}/{id}`
pub async fn put_publisher(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repository, id)): Path<(String, String)>,
    Json(spec): Json<PublisherSpec>,
) -> Result<Response, ResolutionError> {
    let service = service(&state)?;
    let (context, correlation) = manage_gate(&state, &headers, &repository).await?;
    spec.validate(&id).map_err(ResolutionError::BadRequest)?;
    // The administrator's egress decides where keys may come from, at the
    // moment of registration and again at every fetch.
    let url =
        Url::parse(&spec.jwks_url).map_err(|_| ResolutionError::BadRequest("invalid jwks_url"))?;
    if service.fetcher().check_destination(&url).is_err() {
        return Err(ResolutionError::BadRequest(
            "the jwks_url is not in the egress allow-list of this service",
        ));
    }
    let record = TrustedPublisher {
        id: id.clone(),
        account: context.account().as_str().to_string(),
        repository: repository.clone(),
        issuer: spec.issuer,
        audience: spec.audience,
        jwks_url: spec.jwks_url,
        subject: spec.subject,
        claims: spec.claims,
        created_by: context.principal().subject().as_str().to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    service
        .registry()
        .put(&record)
        .await
        .map_err(|_| ResolutionError::Unavailable)?;
    record_audit(
        &state,
        &record.created_by,
        &record.account,
        "publishers.register",
        &format!("{repository}/{id}"),
        AuditResult::Allowed,
        &correlation,
    );
    Ok((StatusCode::OK, Json(record)).into_response())
}

/// `DELETE /api/v1/trusted-publishers/{repository}/{id}`
pub async fn delete_publisher(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repository, id)): Path<(String, String)>,
) -> Result<Response, ResolutionError> {
    let service = service(&state)?;
    let (context, correlation) = manage_gate(&state, &headers, &repository).await?;
    let existed = service
        .registry()
        .delete(context.account().as_str(), &repository, &id)
        .await
        .map_err(|_| ResolutionError::Unavailable)?;
    if !existed {
        return Err(ResolutionError::NotFound);
    }
    record_audit(
        &state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        "publishers.remove",
        &format!("{repository}/{id}"),
        AuditResult::Allowed,
        &correlation,
    );
    Ok(StatusCode::NO_CONTENT.into_response())
}
