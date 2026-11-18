//! Promocion al canal trusted y su revocacion.
//!
//! Dos rutas de inquilino (no de administracion de plataforma):
//!
//! - `POST /api/v1/promotions`: pide promover un digest de un repositorio.
//!   Cedar decide **quien** (`PromoteAsset` sobre el repositorio) y la
//!   politica de confianza decide si el **software** lo merece; solo si
//!   las dos dicen que si se concede el digest, y se concede con el
//!   identificador de la decision que lo avalo.
//! - `POST /api/v1/promotions/revoke`: retira una concesion por digest o
//!   por decision (`RevokePromotion`). Tiene efecto en la peticion
//!   siguiente: no hay cache.
//!
//! La cuenta sale siempre del principal verificado. Ningun campo del
//! cuerpo ni cabecera confiere cuenta ni permiso.

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

/// El error de una peticion de promocion o revocacion.
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
    /// La politica rechazo la promocion; lleva las razones y la version
    /// del riesgo contra la que se midio.
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

/// El digest `sha256:<hex>` de una peticion, o nada.
///
/// Solo se acepta la forma canonica: minusculas y la longitud que dice el
/// algoritmo. Un digest que no es el que el catalogo guarda no es una
/// forma alternativa de nombrarlo, es otra cosa.
fn parse_digest(text: &str) -> Option<ContentDigest> {
    let value = text.strip_prefix("sha256:")?;
    if value.chars().any(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let digest = ContentDigest::sha256(value);
    digest.is_well_formed().then_some(digest)
}

/// Un nombre de repositorio o una ruta que no desbordan lo razonable.
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
    // El cuerpo se lee despues de autenticar: sin credencial es siempre 401.
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

    // Un repositorio de otra cuenta o que no existe es el mismo 404: la
    // consulta ya va acotada a la cuenta verificada.
    let repository = reader
        .repository(&account, &body.repository)
        .await
        .map_err(|error| catalog_error(&error))?;
    let action = crate::actions::promote_asset();
    let authorization = match state.authorize_resource.execute_canonical(
        context.clone(),
        action.clone(),
        &repository.hrn,
    ) {
        Ok(AuthorizationOutcome::Permit) => AuthorizationResult::Permit,
        Ok(AuthorizationOutcome::Deny(_)) => AuthorizationResult::Deny,
        // Un motor que no responde no es un permiso.
        Ok(AuthorizationOutcome::Indeterminate(_)) | Err(_) => AuthorizationResult::Indeterminate,
    };
    let permitted = authorization == AuthorizationResult::Permit;

    // Quien no puede promover no averigua si la ruta existe: sin permiso,
    // un asset ausente o distinto responde igual que uno presente (403).
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
    // La promocion es de un digest, no de un nombre. Si la ruta apunta ya
    // a otro contenido (una etiqueta que se movio), no se promueve lo que
    // el cliente no vio.
    if asset.digest != digest {
        return Err(if permitted {
            PromotionApiError::Conflict
        } else {
            PromotionApiError::Forbidden
        });
    }

    // El HRN del sujeto: el que la evidencia y la capsula usan.
    let subject = artifact_hrn(repository.hrn.region(), &account, &body.repository, &digest);
    let evidence = state
        .evidence_reader
        .evidence_of_subject(&account, &subject)
        .await
        .map_err(|_| PromotionApiError::Unavailable)?;
    let now = Utc::now();
    let capsule = CapsuleAssembler::new().assemble(&subject, &evidence, now);
    // Los hechos de riesgo salen del grafo de impacto: que alcanza a este
    // sujeto y hasta que version del registro. Si el grafo no puede
    // establecerlos --sin cobertura, vista atrasada, severidad desconocida
    // o fallo de lectura-- la respuesta es UNKNOWN, y una politica que
    // exige el riesgo no concede sobre UNKNOWN.
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
    // El servicio se llama **siempre**, tambien con un Deny de Cedar: la
    // decision, la que sea, queda escrita.
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

    // Lo que abarca: el digest y lo que un manifiesto nombra.
    let covered = covered_by(
        reader.as_ref(),
        content.as_ref(),
        &account,
        &body.repository,
        &digest,
        &body.path,
    )
    .await;

    // AC1 y AC3: la concesion se emite desde la decision autorizada y
    // persistida, y solo se publica si su instantanea sigue vigente. Si
    // entre la decision y aqui se activo otra version de la politica, la
    // decision esta obsoleta y ninguna concesion se publica. Y al
    // emitirse desde ella, lo que el grant promete como sujeto,
    // politica, evidencia y riesgo es lo que de verdad se decidio, no lo
    // que trajo el cliente.
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
    if plane.active_policy_version().as_deref() != Some(decision.trust.policy_version.as_str()) {
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
    // Exactamente uno: por digest o por decision.
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
    match state.authorize_resource.execute_canonical(
        context.clone(),
        action.clone(),
        &repository.hrn,
    ) {
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
            // Revocar un manifiesto retira lo que su promocion concedio
            // con el: su configuracion, sus capas y los hijos de un
            // indice. Si no, retirar la imagen dejaria sus capas
            // sirviendose. Es la misma cuenta que al promover.
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

/// Los digests que abarca un digest del repositorio: el mismo y, si es un
/// manifiesto, lo que nombra. Sirve igual para conceder que para retirar.
///
/// Un digest que el repositorio no tiene como manifiesto se abarca a si
/// mismo y nada mas.
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
