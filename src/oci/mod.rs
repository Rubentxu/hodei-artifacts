//! The OCI distribution surface: pull and push.
//!
//! Distribution is an **inbound adapter**: it translates the OCI
//! protocol into the use cases that already exist, the catalog and the
//! content store. There is no separate image storage, and no
//! second truth about what is published:
//!
//! - A **blob** is an asset of the repository, at `blobs/<digest>`.
//! - A **manifest** is an asset of the repository, at `manifests/<digest>`.
//! - A **tag** is the mutable reference the catalog resolves to
//!   the asset's coordinates.
//!
//! And the rules are those of the other surfaces, because they are those
//! of the product: the account comes from the **verified** principal, the
//! decision is made by Cedar on the repository, and a resource of another
//! account does not exist for this one.
//!
//! # The two slices
//!
//! - **Pull** (`src/handlers/oci.rs`): `GET` and `HEAD` of blobs and
//!   manifests, by digest and by tag, with type negotiation.
//! - **Push** (`src/oci/push.rs`): `POST`, `PATCH`, `GET`, `PUT` and
//!   `DELETE` of upload sessions, and the `PUT` of the manifest.
//!
//! What the two slices share --the error dialect, the repository name,
//! the digest, the path split-- lives here, so that the
//! OCI error is stated **the same way** on both sides. A `DIGEST_INVALID` that
//! is a `400` on pull and a `415` on push is a registry that
//! speaks two languages.

pub mod failure;
pub mod group;
pub mod proxy;
pub mod push;
pub mod query;
pub mod session_journal;
pub mod target;
pub mod token;
pub mod uploads;

use std::sync::Arc;

use axum::http::HeaderMap;
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    CatalogReader, CatalogWriter, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
};

use crate::app_state::AppState;
use failure::OciFailure;

/// The verified principal of the request.
///
/// Authentication is the first door, just as on the raw surface: a
/// request without a credential does not get to look at the route.
pub async fn the_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, OciFailure> {
    let token = crate::handlers::bearer_token(headers).ok_or_else(|| {
        // Without a credential there is nothing to verify. The challenge --which says
        // **how** to get one-- is set by the edge, which is the one that knows
        // where the deployment lives: here it is not yet known.
        OciFailure::unauthorized()
    })?;
    let principal = match hodei_authz::verify_principal(
        state.authenticator.as_ref(),
        &hodei_authz::Credentials::bearer(token),
    )
    .await
    {
        Ok(principal) => principal,
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            return Err(OciFailure::unavailable(
                "the authentication provider is not available",
            ));
        }
        Err(_) => {
            return Err(OciFailure::unauthorized());
        }
    };
    Ok(VerifiedContext::from_principal(principal))
}

/// The catalog, with what it needs in order to read.
pub type TheCatalog<'a> = (&'a Arc<dyn CatalogReader>, &'a Arc<dyn CatalogWriter>);

/// The catalog.
pub fn the_catalog(state: &AppState) -> Result<TheCatalog<'_>, OciFailure> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let writer = state
        .catalog_writer
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    Ok((reader, writer))
}

/// The content store.
pub fn the_content_store(
    state: &AppState,
) -> Result<&Arc<dyn hodei_hub::blob::BlobStore>, OciFailure> {
    state
        .content_store
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the content store is not wired"))
}

/// The repository, in the verified account.
///
/// A repository of another account does not exist for this one, and is answered with
/// the distribution's `NAME_UNKNOWN`: it does not say whether it exists elsewhere.
pub async fn the_repository(
    state: &AppState,
    context: &VerifiedContext,
    name: &str,
) -> Result<RepositoryDescriptor, OciFailure> {
    let (reader, _) = the_catalog(state)?;
    match reader.repository(context.account().as_str(), name).await {
        // A repository of a format this release does not serve, or that
        // is not `hosted`, is not a repository of this surface: it is
        // answered with the same NAME_UNKNOWN as one that does not exist, so
        // as not to say that a repository exists that cannot be used.
        //
        // The accepted format is the one that holds this surface's
        // content --raw, image and chart--: a Helm chart
        // over OCI is an OCI artifact, and `helm push` and `helm pull`
        // speak the standard distribution, with no surface of their own. The
        // others (maven2, npm, pypi…) mean nothing here: a path of
        // another format is not a distribution resource.
        Ok(descriptor)
            if !matches!(
                descriptor.format,
                RepositoryFormat::Raw | RepositoryFormat::Oci | RepositoryFormat::Helm
            ) || !descriptor.serves_the_distribution() =>
        {
            Err(OciFailure::name_unknown(format!(
                "the repository {name:?} is not known to this account"
            )))
        }
        Ok(descriptor) => Ok(descriptor),
        Err(hodei_catalog::CatalogError::Missing { .. })
        | Err(hodei_catalog::CatalogError::Denied { .. }) => Err(OciFailure::name_unknown(
            format!("the repository {name:?} is not known to this account"),
        )),
        Err(_other) => Err(OciFailure::unavailable("the catalog is not available")),
    }
}

/// Whether the repository is served by this surface.
///
/// A **hosted** repository of a distribution format is always
/// served. A **proxy** repository only if its format is image or
/// chart: a raw proxy is served by the raw surface, and a proxy of another
/// format means nothing in the distribution.
///
/// An image **group** repository is also served: an OCI group is an
/// ordered list of OCI repositories and the distribution reads it with the same
/// precedence as any other group. A chart group is not: Helm charts
/// have no surface of their own and their format serves no
/// modality.
trait ServesTheDistribution {
    fn serves_the_distribution(&self) -> bool;
}

impl ServesTheDistribution for RepositoryDescriptor {
    fn serves_the_distribution(&self) -> bool {
        match self.mode {
            RepositoryMode::Hosted => true,
            RepositoryMode::Proxy => {
                matches!(self.format, RepositoryFormat::Oci | RepositoryFormat::Helm)
            }
            RepositoryMode::Group => matches!(self.format, RepositoryFormat::Oci),
        }
    }
}

/// That the repository accepts writes.
///
/// A group stores nothing: it is a list of repositories that are queried.
/// Writing against a group has no destination, and it is stated with the
/// dialect error that the distribution reserves for what cannot be done on a
/// read-only repository.
pub fn the_writable(repository: &RepositoryDescriptor) -> Result<(), OciFailure> {
    if repository.mode == RepositoryMode::Group {
        return Err(OciFailure::unsupported(
            "a group repository does not accept writes",
        ));
    }
    Ok(())
}

/// The Cedar decision on the repository.
///
/// The resource is the repository, just as on the raw surface: an
/// action on a resource that does not exist cannot be evaluated, and the
/// decision is made by the gate with the resource in view.
pub fn the_decision(
    state: &AppState,
    context: &VerifiedContext,
    action: hodei_authz::model::ActionId,
    repository: &RepositoryDescriptor,
) -> Result<(), OciFailure> {
    let outcome = state
        .authorize_resource
        .execute_repository(context.clone(), action, repository)
        .map_err(|_| OciFailure::unavailable("the authorization engine is not available"))?;
    if !outcome.allows_access() {
        return Err(OciFailure::denied(
            "the principal is not allowed to act on this repository",
        ));
    }
    Ok(())
}

/// A catalog error, translated to the distribution dialect.
pub fn the_catalog_error(error: &hodei_catalog::CatalogError) -> OciFailure {
    use hodei_catalog::CatalogError;
    match error {
        // A conflict is a conflict: the path already points at something else.
        //
        // The reason travels: a client that receives "conflict" without knowing
        // **what** collides cannot decide whether to republish with other
        // coordinates or whether its content is the one that is surplus.
        CatalogError::Conflict { kind, key, detail } => OciFailure::blob_upload_invalid(format!(
            "the {kind} {key} already holds another content: {detail}"
        )),
        // The precondition the client demanded is not met: it is not a
        // conflict, it is a precondition.
        CatalogError::PreconditionFailed { .. } => {
            OciFailure::blob_upload_invalid("the precondition is not met")
        }
        CatalogError::InvalidRequest { .. } => {
            OciFailure::blob_upload_invalid("the publication is not well formed")
        }
        CatalogError::Missing { .. } => OciFailure::name_unknown("the repository is not there"),
        CatalogError::Denied { .. } => OciFailure::denied("the principal is not allowed"),
        CatalogError::ModeNotImplemented { .. } => {
            OciFailure::name_unknown("the repository is not served by this release")
        }
        CatalogError::SchemaMismatch { .. } | CatalogError::Corrupt { .. } => {
            OciFailure::unavailable("the catalog is not available")
        }
        CatalogError::Unavailable { .. } => OciFailure::unavailable("the catalog is not available"),
    }
}
