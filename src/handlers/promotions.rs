//! Promotion to the trusted channel and its revocation.
//!
//! Two tenant routes (not platform administration):
//!
//! - `POST /api/v1/promotions`: requests promoting a digest of a repository.
//!   Cedar decides **who** (`PromoteAsset` on the repository) and the
//!   trust policy decides whether the **software** deserves it; only if
//!   both say yes is the digest granted, and it is granted with the
//!   identifier of the decision that backed it.
//! - `POST /api/v1/promotions/revoke`: withdraws a grant by digest or
//!   by decision (`RevokePromotion`). It takes effect on the next
//!   request: there is no cache.
//!
//! The account always comes from the verified principal. No body field
//! or header confers an account or a permission.

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use hodei_authz::{AuthorizationOutcome, Credentials, VerifiedContext, verify_principal};
use hodei_catalog::{CatalogError, TrustGrant};
use hodei_evidence::ContentDigest;
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_trust_policy::{
    AuthorizationResult, DecisionId, PromotionFailure, PromotionOutcome, PromotionRequest,
    TrustContext,
};
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{bearer_token, correlation_id, error_body, record_audit};
use crate::trust::hrn::artifact_hrn;
use crate::trust::promotion::{TRUSTED_DISTRIBUTION, digests_covered};

/// The error of a promotion or revocation request.
#[derive(Debug)]
pub enum PromotionApiError {
    Unauthorized,
    Forbidden,
    NotFound,
    BadRequest,
    Conflict,
    NotImplemented,
    Unavailable,
    Corrupt,
    /// The policy rejected the promotion; it carries the reasons and the risk
    /// version against which it was measured.
    Refused(Vec<String>, u64),
}

impl IntoResponse for PromotionApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                error_body("unauthenticated", "unauthorized"),
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, error_body("permission", "forbidden")),
            Self::NotFound => (StatusCode::NOT_FOUND, error_body("absence", "not found")),
            Self::BadRequest => (
                StatusCode::BAD_REQUEST,
                error_body("invalid", "the request is not valid"),
            ),
            Self::Conflict => (StatusCode::CONFLICT, error_body("conflict", "conflict")),
            Self::NotImplemented => (
                StatusCode::NOT_IMPLEMENTED,
                error_body("unavailable", "this deployment does not serve promotions"),
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                error_body("unavailable", "the promotions are not available"),
            ),
            Self::Corrupt => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_body("corruption", "what is stored cannot be read"),
            ),
            Self::Refused(reasons, risk_version) => {
                let body = serde_json::json!({
                    "promoted": false,
                    "reasons": reasons,
                    "risk_version": risk_version,
                });
                return (StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response();
            }
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromoteRequestBody {
    pub repository: String,
    pub path: String,
    pub digest: String,
}

#[derive(Debug, Serialize)]
pub struct PromoteResponse {
    pub promoted: bool,
    pub decision_id: String,
    pub digest: String,
    pub granted: Vec<String>,
    /// The version of the risk feed the decision was measured against.
    pub risk_version: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeRequestBody {
    pub repository: String,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub decision_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct RevokeResponse {
    pub revoked: u64,
}

/// The `sha256:<hex>` digest of a request, or nothing.
///
/// Only the canonical form is accepted: lowercase and the length the
/// algorithm dictates. A digest that is not the one the catalog stores is not an
/// alternative way of naming it, it is something else.
fn parse_digest(text: &str) -> Option<ContentDigest> {
    let value = text.strip_prefix("sha256:")?;
    if value.chars().any(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let digest = ContentDigest::sha256(value);
    digest.is_well_formed().then_some(digest)
}

/// A repository name or a path that does not exceed what is reasonable.
fn sane(text: &str, max: usize) -> bool {
    !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}

async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, PromotionApiError> {
    let token = bearer_token(headers).ok_or(PromotionApiError::Unauthorized)?;
    match verify_principal(state.authenticator.as_ref(), &Credentials::bearer(token)).await {
        Ok(principal) => Ok(VerifiedContext::from_principal(principal)),
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            Err(PromotionApiError::Unavailable)
        }
        Err(_) => Err(PromotionApiError::Unauthorized),
    }
}

async fn body_of<T: serde::de::DeserializeOwned>(
    request: Request<Body>,
) -> Result<T, PromotionApiError> {
    let body = to_bytes(request.into_body(), 64 * 1024)
        .await
        .map_err(|_| PromotionApiError::BadRequest)?;
    serde_json::from_slice(&body).map_err(|_| PromotionApiError::BadRequest)
}

fn catalog_error(error: &CatalogError) -> PromotionApiError {
    match error {
        CatalogError::Missing { .. } => PromotionApiError::NotFound,
        CatalogError::Corrupt { .. } => PromotionApiError::Corrupt,
        _ => PromotionApiError::Unavailable,
    }
}

/// `POST /api/v1/promotions`
pub async fn promote(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Json<PromoteResponse>, PromotionApiError> {
    let context = authenticate(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    // The body is read after authenticating: without a credential it is always 401.
    let body: PromoteRequestBody = body_of(request).await?;
    if !sane(&body.repository, 128) || !sane(&body.path, 1024) {
        return Err(PromotionApiError::BadRequest);
    }
    let digest = parse_digest(&body.digest).ok_or(PromotionApiError::BadRequest)?;

    let plane = state
        .trust_plane
        .as_ref()
        .ok_or(PromotionApiError::NotImplemented)?;
    let (reader, _) = match (&state.catalog_reader, &state.catalog_writer) {
        (Some(reader), Some(writer)) => (reader, writer),
        _ => return Err(PromotionApiError::NotImplemented),
    };
    let ledger = state
        .trust_ledger
        .as_ref()
        .ok_or(PromotionApiError::NotImplemented)?;
    let content = state
        .content_store
        .as_ref()
        .ok_or(PromotionApiError::NotImplemented)?;

    let account = context.account().as_str().to_string();
    let actor = context.principal().subject().as_str().to_string();

    // A repository of another account or one that does not exist is the same 404: the
    // query is already scoped to the verified account.
    let repository = reader
        .repository(&account, &body.repository)
        .await
        .map_err(|error| catalog_error(&error))?;
    let action = crate::actions::promote_asset();
    let authorization = match state.authorize_resource.execute_repository(
        context.clone(),
        action.clone(),
        &repository,
    ) {
        Ok(AuthorizationOutcome::Permit) => AuthorizationResult::Permit,
        Ok(AuthorizationOutcome::Deny(_)) => AuthorizationResult::Deny,
        // An engine that does not respond is not a permission.
        Ok(AuthorizationOutcome::Indeterminate(_)) | Err(_) => AuthorizationResult::Indeterminate,
    };
    let permitted = authorization == AuthorizationResult::Permit;

    // Whoever cannot promote does not find out whether the path exists: without permission,
    // an absent or different asset answers the same as a present one (403).
    let asset = match reader.asset(&account, &body.repository, &body.path).await {
        Ok(asset) => asset,
        Err(_) if !permitted => {
            record_audit(
                &state,
                &actor,
                &account,
                &action.to_string(),
                &repository.hrn.to_string(),
                AuditResult::Denied,
                &correlation,
            );
            return Err(PromotionApiError::Forbidden);
        }
        Err(error) => return Err(catalog_error(&error)),
    };
    // The promotion is for a digest, not a name. If the path already points
    // to other content (a tag that moved), what the client did not see
    // is not promoted.
    if asset.digest != digest {
        return Err(if permitted {
            PromotionApiError::Conflict
        } else {
            PromotionApiError::Forbidden
        });
    }

    // The subject's HRN: the one that the evidence and the capsule use.
    let subject = artifact_hrn(repository.hrn.region(), &account, &body.repository, &digest);
    let evidence = state
        .evidence_reader
        .evidence_of_subject(&account, &subject)
        .await
        .map_err(|_| PromotionApiError::Unavailable)?;
    let now = Utc::now();
    let capsule = CapsuleAssembler::new().assemble(&subject, &evidence, now);
    // The risk facts come from the impact graph: what reaches this
    // subject and up to which registry version. If the graph cannot
    // establish them --no coverage, stale view, unknown severity
    // or read failure-- the answer is UNKNOWN, and a policy that
    // requires the risk does not grant on UNKNOWN.
    let risk_facts = match state.investigation_limits.the_graph_tenant(&subject) {
        Some(tenant) => match state.risk_facts.risk_facts(&tenant, &subject).await {
            Ok(facts) => facts,
            Err(error) => {
                tracing::warn!(%error, "the risk facts could not be read: the decision will say UNKNOWN");
                hodei_risk_storage::subject_risk::SubjectFacts::unknown()
            }
        },
        None => hodei_risk_storage::subject_risk::SubjectFacts::unknown(),
    };
    let trust_context = TrustContext::builder(&capsule)
        .with_digest(digest.to_string())
        .with_risk(crate::risk_facts::to_trust_facts(&risk_facts))
        .assembled_at(now)
        .build();

    let decision_id = DecisionId::new(state.identity.new_id());
    let promotion = PromotionRequest::new(
        correlation.clone(),
        actor.clone(),
        account.clone(),
        subject.to_string(),
        TRUSTED_DISTRIBUTION,
        now,
    );
    // The service is called **always**, also with a Cedar Deny: the
    // decision, whatever it is, is written down.
    let outcome = plane
        .service()
        .request_promotion(
            decision_id.clone(),
            promotion,
            authorization,
            trust_context,
            plane.policy_id(),
        )
        .await;

    let audit = |result: AuditResult| {
        record_audit(
            &state,
            &actor,
            &account,
            &action.to_string(),
            &subject.to_string(),
            result,
            &correlation,
        );
    };

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(_) => {
            audit(AuditResult::Failed);
            return Err(PromotionApiError::Unavailable);
        }
    };

    if !permitted {
        audit(AuditResult::Denied);
        return Err(PromotionApiError::Forbidden);
    }
    match outcome {
        PromotionOutcome::Promoted => {}
        PromotionOutcome::Failed(PromotionFailure::NoTarget) => {
            audit(AuditResult::Failed);
            return Err(PromotionApiError::NotImplemented);
        }
        PromotionOutcome::Failed(PromotionFailure::Refused) => {
            audit(AuditResult::Denied);
            let reasons = plane
                .service()
                .decision(&decision_id)
                .await
                .ok()
                .flatten()
                .map(|decision| {
                    decision
                        .reasons
                        .iter()
                        .map(|reason| format!("{reason:?}"))
                        .collect()
                })
                .unwrap_or_default();
            return Err(PromotionApiError::Refused(reasons, risk_facts.version));
        }
    }

    // What it covers: the digest and what a manifest names.
    let covered = covered_by(
        reader.as_ref(),
        content.as_ref(),
        &account,
        &body.repository,
        &digest,
        &body.path,
    )
    .await;

    // AC1 and AC3: the grant is issued from the authorized and
    // persisted decision, and is only published if its snapshot is still current. If
    // another policy version was activated between the decision and here, the
    // decision is stale and no grant is published. And as it is
    // issued from it, what the grant promises as subject,
    // policy, evidence and risk is what was actually decided, not
    // what the client brought.
    let decision = plane
        .service()
        .decision(&decision_id)
        .await
        .map_err(|_| {
            audit(AuditResult::Failed);
            PromotionApiError::Unavailable
        })?
        .ok_or_else(|| {
            audit(AuditResult::Failed);
            PromotionApiError::Unavailable
        })?;
    if !plane.decision_is_current(&account, &decision).await {
        audit(AuditResult::Failed);
        return Err(PromotionApiError::Conflict);
    }
    let valid_until = plane.validity_of(&decision, now);

    let grants: Vec<TrustGrant> = covered
        .iter()
        .map(|covered| TrustGrant {
            account: account.clone(),
            repository: body.repository.clone(),
            digest: covered.clone(),
            decision_id: decision.id.as_str().to_string(),
            actor: actor.clone(),
            granted_at: now,
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: Some(decision.request.subject.clone()),
            policy_version: Some(decision.trust.policy_version.clone()),
            capsule_hash: decision.trust.capsule_hash.clone(),
            risk_version: Some(decision.trust.risk_version),
            valid_until: Some(valid_until),
        })
        .collect();
    if ledger.grant(&grants).await.is_err() {
        audit(AuditResult::Failed);
        return Err(PromotionApiError::Unavailable);
    }

    audit(AuditResult::Allowed);
    Ok(Json(PromoteResponse {
        promoted: true,
        decision_id: decision_id.as_str().to_string(),
        digest: body.digest,
        granted: covered
            .iter()
            .map(|digest| format!("{}:{}", digest.algorithm, digest.value))
            .collect(),
        risk_version: risk_facts.version,
    }))
}

/// `POST /api/v1/promotions/revoke`
pub async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Json<RevokeResponse>, PromotionApiError> {
    let context = authenticate(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let body: RevokeRequestBody = body_of(request).await?;
    if !sane(&body.repository, 128) || !sane(&body.reason, 1024) {
        return Err(PromotionApiError::BadRequest);
    }
    // Exactly one: by digest or by decision.
    let target = match (&body.digest, &body.decision_id) {
        (Some(digest), None) => {
            Target::Digest(parse_digest(digest).ok_or(PromotionApiError::BadRequest)?)
        }
        (None, Some(decision)) if sane(decision, 128) => Target::Decision(decision.clone()),
        _ => return Err(PromotionApiError::BadRequest),
    };

    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or(PromotionApiError::NotImplemented)?;
    let ledger = state
        .trust_ledger
        .as_ref()
        .ok_or(PromotionApiError::NotImplemented)?;
    if state.trust_plane.is_none() {
        return Err(PromotionApiError::NotImplemented);
    }

    let account = context.account().as_str().to_string();
    let actor = context.principal().subject().as_str().to_string();
    let repository = reader
        .repository(&account, &body.repository)
        .await
        .map_err(|error| catalog_error(&error))?;

    let action = crate::actions::revoke_promotion();
    let resource = repository.hrn.to_string();
    let audit = |result: AuditResult| {
        record_audit(
            &state,
            &actor,
            &account,
            &action.to_string(),
            &resource,
            result,
            &correlation,
        );
    };
    match state
        .authorize_resource
        .execute_repository(context.clone(), action.clone(), &repository)
    {
        Ok(AuthorizationOutcome::Permit) => {}
        Ok(AuthorizationOutcome::Deny(_)) => {
            audit(AuditResult::Denied);
            return Err(PromotionApiError::Forbidden);
        }
        Ok(AuthorizationOutcome::Indeterminate(_)) | Err(_) => {
            audit(AuditResult::Failed);
            return Err(PromotionApiError::Forbidden);
        }
    }

    let now = Utc::now();
    let revoked = match &target {
        Target::Digest(digest) => {
            // Revoking a manifest withdraws what its promotion granted
            // with it: its config, its layers and the children of an
            // index. Otherwise, withdrawing the image would leave its layers
            // being served. It is the same accounting as when promoting.
            let covered = match state.content_store.as_ref() {
                Some(content) => {
                    covered_by(
                        reader.as_ref(),
                        content.as_ref(),
                        &account,
                        &body.repository,
                        digest,
                        &format!("manifests/{}", digest.value),
                    )
                    .await
                }
                None => vec![digest.clone()],
            };
            let mut total = 0u64;
            let mut failed = None;
            for covered in &covered {
                match ledger
                    .revoke_digest(
                        &account,
                        &body.repository,
                        covered,
                        &actor,
                        &body.reason,
                        now,
                    )
                    .await
                {
                    Ok(count) => total += count,
                    Err(error) => {
                        failed = Some(error);
                        break;
                    }
                }
            }
            match failed {
                Some(error) => Err(error),
                None => Ok(total),
            }
        }
        Target::Decision(decision) => {
            ledger
                .revoke_decision(
                    &account,
                    &body.repository,
                    decision,
                    &actor,
                    &body.reason,
                    now,
                )
                .await
        }
    };
    match revoked {
        Ok(revoked) => {
            audit(AuditResult::Allowed);
            Ok(Json(RevokeResponse { revoked }))
        }
        Err(_) => {
            audit(AuditResult::Failed);
            Err(PromotionApiError::Unavailable)
        }
    }
}

/// The digests covered by a digest of the repository: itself and, if it is a
/// manifest, what it names. It works equally for granting and for withdrawing.
///
/// A digest the repository does not have as a manifest covers itself
/// and nothing else.
async fn covered_by(
    reader: &dyn hodei_catalog::CatalogReader,
    content: &dyn hodei_hub::BlobStore,
    account: &str,
    repository: &str,
    digest: &ContentDigest,
    path: &str,
) -> Vec<ContentDigest> {
    digests_covered(content, digest, path, |candidate| async move {
        reader
            .asset(
                account,
                repository,
                &format!("manifests/{}", candidate.value),
            )
            .await
            .map(|asset| asset.digest == candidate)
            .unwrap_or(false)
    })
    .await
}

enum Target {
    Digest(ContentDigest),
    Decision(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_canonical_digest_form_is_accepted() {
        let hex = "a".repeat(64);
        assert!(parse_digest(&format!("sha256:{hex}")).is_some());
        for bad in [
            hex.clone(),
            format!("sha256:{}", hex.to_uppercase()),
            format!("sha256:{}", &hex[..63]),
            format!("sha512:{hex}"),
            "sha256:".to_string(),
            format!("sha256:{}g", &hex[..63]),
        ] {
            assert!(parse_digest(&bad).is_none(), "{bad}");
        }
    }
}
