//! The detail view of a package.
//!
//! The view is what the package browser shows of an asset: where the
//! bytes are, what was published, what it hangs from, what evidence there
//! is and what decision has been taken about it.
//!
//! # What the view does not say
//!
//! The trust comes from the **grant**, not from the absence of findings.
//! Content nobody has ever written about is not trusted: it is unknown,
//! and what is said about it is that it is quarantined. Confusing
//! "nobody said it is bad" with "nobody said it is good" is how a
//! registry ends up serving software without looking at it.
//!
//! And the decision that is shown is the **live** one: if the grant has
//! been revoked, the view says the content is quarantined, because that
//! is what it is now.

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

/// What can fail when the detail of a package is asked for.
#[derive(Debug)]
pub enum PackageApiError {
    /// The request could not be authenticated.
    Unauthorized,
    /// The principal cannot read the repository.
    Forbidden,
    /// There is nothing there, or it is not of this account.
    NotFound,
    /// The request is not coherent.
    BadRequest,
    /// The catalog or the grant ledger could not answer.
    Unavailable,
    /// Something failed inside.
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

/// The detail of a package.
#[derive(Debug, serde::Serialize)]
pub struct PackageView {
    /// The repository.
    pub repository: String,
    /// The path of the asset inside the repository.
    pub asset_path: String,
    /// The package name.
    pub package_name: String,
    /// The package version.
    pub package_version: String,
    /// The digest of the content.
    pub digest: ContentDigest,
    /// The bytes the content has.
    pub size_bytes: u64,
    /// When it moved last.
    pub updated_at: DateTime<Utc>,
    /// Where the bytes are.
    pub blob: BlobView,
    /// What was published, as it was published.
    pub published: PublishedView,
    /// What it hangs from, and what hangs from it.
    pub lineage: LineageView,
    /// What the evidence says.
    pub capsule: CapsuleView,
    /// What has been decided about the content.
    pub trust: TrustView,
}

/// Where the bytes are.
#[derive(Debug, serde::Serialize)]
pub struct BlobView {
    /// The digest of the content.
    pub digest: ContentDigest,
    /// The scope it lives in.
    pub scope: DigestScope,
    /// The bytes it has.
    pub size_bytes: u64,
    /// When the reference was written.
    pub updated_at: DateTime<Utc>,
}

/// What was published.
///
/// It is the version as it was published: the coordinates, the digest and
/// the moment. The asset may have moved afterwards to another version,
/// and this is the version it points at now.
#[derive(Debug, serde::Serialize)]
pub struct PublishedView {
    /// The package name.
    pub package_name: String,
    /// The package version.
    pub package_version: String,
    /// The digest of the content.
    pub digest: ContentDigest,
    /// The bytes the content has.
    pub size_bytes: u64,
    /// When it was published.
    pub published_at: DateTime<Utc>,
}

/// The line of the manifests.
#[derive(Debug, serde::Serialize)]
pub struct LineageView {
    /// The manifest this asset hangs from, if it hangs from one.
    pub subject: Option<ContentDigest>,
    /// The manifests that hang from this one.
    pub referrers: Vec<ReferrerView>,
}

/// A manifest that hangs from another one.
#[derive(Debug, serde::Serialize)]
pub struct ReferrerView {
    /// The path of the referrer inside the repository.
    pub asset_path: String,
    /// The digest of the manifest.
    pub digest: ContentDigest,
    /// The bytes the manifest has.
    pub size_bytes: u64,
    /// The digest it points at.
    pub subject: ContentDigest,
}

/// What has been decided about the content.
#[derive(Debug, serde::Serialize)]
pub struct TrustView {
    /// The trust state in this repository.
    pub state: TrustState,
    /// The decision that grants it, if there is a live one.
    pub decision: Option<DecisionView>,
}

/// The trust decision that grants a content.
#[derive(Debug, serde::Serialize)]
pub struct DecisionView {
    /// The decision that granted the content.
    pub decision_id: String,
    /// Who granted it.
    pub actor: String,
    /// When it was granted.
    pub granted_at: DateTime<Utc>,
}

/// `GET /api/v1/packages/{repository}/{*path}`
///
/// The path of the asset arrives with the slashes inside it, because the
/// path is what names the content inside the repository.
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

    // The repository first: if it is not of this account, it is not
    // evaluated. A foreign repository and one that does not exist hide behind
    // the same 404, because answering differently would say it exists.
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

    // The decision: the live grant of the ledger. What is not
    // granted is quarantined, and it is not deduced from the evidence.
    let decision = the_decision(&state, account, &repository, &asset.digest).await?;
    let state_of_trust = if decision.is_some() {
        TrustState::Trusted
    } else {
        TrustState::Quarantined
    };

    // The capsule: what the evidence says is there. A capsule with no
    // evidence does not say the content is bad: it says nobody
    // knows anything about it.
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

/// The live grant of a content, if there is one.
///
/// Without a grant ledger there is no decision to read: what cannot be
/// read is not granted, and what is not granted is quarantined.
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

/// The view of a decision.
impl DecisionView {
    fn of(grant: TrustGrant) -> Self {
        Self {
            decision_id: grant.decision_id,
            actor: grant.actor,
            granted_at: grant.granted_at,
        }
    }
}

/// The error of the catalog, translated into what the client sees.
fn the_catalog_error(error: CatalogError) -> PackageApiError {
    match error {
        CatalogError::Missing { .. } => PackageApiError::NotFound,
        CatalogError::Denied { .. } => PackageApiError::Forbidden,
        CatalogError::InvalidRequest { .. } => PackageApiError::BadRequest,
        CatalogError::Unavailable { .. } => PackageApiError::Unavailable,
        _ => PackageApiError::Internal,
    }
}
