//! The catalog ports.
//!
//! The catalog talks to the world through four ports: read, write,
//! decide who may do each thing, and ask about the integrity of
//! the bytes that the catalog references. None of them names a database:
//! that knowledge lives in the adapters crate.

use async_trait::async_trait;
use hodei_evidence::ContentDigest;
use thiserror::Error;

use crate::error::CatalogError;
use crate::model::{
    AssetDeletion, AssetRecord, BlobLocation, BlobReference, MutableTag, PackageCoordinates,
    PackageVersion, Publication, PublishOutcome, ReferrerRecord, RepositoryConfig,
    RepositoryDescriptor,
};
use crate::search::{SearchPage, SearchQuery};

/// Durable identity of an APT group snapshot namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AptSnapshotKey {
    /// Owning account, used as the tenant boundary.
    pub account: String,
    /// Group repository key.
    pub group: String,
    /// Distribution within the group.
    pub distribution: String,
    /// Stable Cedar principal subject, not a display name.
    pub principal_subject: String,
}

/// Typed CAS object identity and its verified byte length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AptCasReference {
    /// CAS location, including tenant, scope, and digest.
    pub location: BlobLocation,
    /// Expected object length.
    pub size_bytes: u64,
}

/// Identity and content contributed by a member repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AptSnapshotMember {
    /// Member repository key.
    pub repository: String,
    /// Package name contributed by the member.
    pub package: String,
    /// Package version contributed by the member.
    pub version: String,
    /// Immutable package content reference.
    pub content: AptCasReference,
}

/// Immutable, fully materialized APT view, including all referenced CAS objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AptGroupSnapshot {
    /// Monotonic revision within the key namespace.
    pub revision: u64,
    /// Stable content digest identifying this revision.
    pub digest: ContentDigest,
    /// Release metadata reference.
    pub release: AptCasReference,
    /// Detached signature reference for the signed Release metadata.
    pub signature: AptCasReference,
    /// Packages indexes keyed by architecture.
    pub packages_by_arch: std::collections::BTreeMap<String, AptCasReference>,
    /// Package archives keyed by their canonical pool path.
    pub pool_by_path: std::collections::BTreeMap<String, AptCasReference>,
    /// Member identities and content used by the consumer to revalidate current membership.
    pub members: Vec<AptSnapshotMember>,
}

/// Durable storage contract for immutable APT group generations.
///
/// This port only persists and retrieves data. Its consumer must re-evaluate current group
/// membership and Cedar authorization on every request, including historical and by-hash reads;
/// a stored snapshot is not an authorization grant and must not bypass revocation.
#[async_trait]
pub trait AptSnapshotStore: Send + Sync {
    /// Read the current committed generation, if one exists.
    async fn current_apt_snapshot(
        &self,
        key: &AptSnapshotKey,
    ) -> Result<Option<AptGroupSnapshot>, CatalogError>;
    /// Read an immutable generation by its monotonic revision.
    async fn apt_snapshot_by_revision(
        &self,
        key: &AptSnapshotKey,
        revision: u64,
    ) -> Result<Option<AptGroupSnapshot>, CatalogError>;
    /// Resolve a registered Packages generation for the given architecture and digest.
    ///
    /// The returned snapshot carries its member identities so the consumer can revalidate
    /// them against the current Cedar-authorized group leaves before serving any bytes.
    async fn apt_snapshot_by_packages_hash(
        &self,
        key: &AptSnapshotKey,
        architecture: &str,
        digest: &ContentDigest,
    ) -> Result<Option<AptGroupSnapshot>, CatalogError>;
    /// Atomically persist a generation and move its pointer iff the expected revision matches.
    ///
    /// A failed comparison returns `CatalogError::PreconditionFailed`; metadata and pointer
    /// movement are one durable transaction. Referenced CAS bytes may have been written earlier.
    async fn publish_apt_snapshot(
        &self,
        key: &AptSnapshotKey,
        expected_revision: Option<u64>,
        snapshot: AptGroupSnapshot,
    ) -> Result<(), CatalogError>;
}

/// What a principal wants to do with the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogAction {
    /// Read an asset, a version or a tag.
    Read,
    /// Publish content.
    Publish,
    /// Move a tag.
    MoveTag,
    /// Move an asset to another version.
    MoveAsset,
    /// Create or reconfigure a repository.
    Configure,
    /// Delete an asset and the history of its path.
    Delete,
}

impl CatalogAction {
    /// The verb, for error messages.
    pub fn as_str(&self) -> &'static str {
        match self {
            CatalogAction::Read => "read",
            CatalogAction::Publish => "publish",
            CatalogAction::MoveTag => "move the tag",
            CatalogAction::MoveAsset => "move the asset",
            CatalogAction::Configure => "configure",
            CatalogAction::Delete => "delete",
        }
    }
}

/// The resource class on which permission is requested.
///
/// The decider not only needs to know whether the principal may read: it needs to
/// know **what** it reads. Allowing the read of an asset and that of a tag
/// are two different decisions, and a decider that only sees "read" cannot
/// tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogResource {
    /// The repository.
    Repository,
    /// A published version.
    Version,
    /// An asset, identified by its path.
    Asset,
    /// A mutable tag.
    Tag,
}

impl CatalogResource {
    /// The name of the class, which is also the `kind` of the `Denied` error.
    pub fn as_str(&self) -> &'static str {
        match self {
            CatalogResource::Repository => "repository",
            CatalogResource::Version => "version",
            CatalogResource::Asset => "asset",
            CatalogResource::Tag => "tag",
        }
    }
}

/// The request being decided.
///
/// It carries the principal and the coordinates of what is requested, and nothing of
/// what exists. The decision is made on what is requested, not on what
/// is there: if the decision depended on existence, answering `Denied` to
/// an object of another account would already leak its existence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRequest {
    /// The account of the verified principal.
    pub principal_account: String,
    /// What it wants to do.
    pub action: CatalogAction,
    /// The resource class on which it is decided.
    pub resource: CatalogResource,
    /// The repository.
    pub repository: String,
    /// The key of the resource within the repository: the repository
    /// key, the asset path, the package coordinates or the
    /// tag name, depending on the resource class.
    pub subject: String,
}

impl AccessRequest {
    /// The request on a repository.
    pub fn repository(principal_account: &str, action: CatalogAction, repo_key: &str) -> Self {
        Self {
            principal_account: principal_account.to_string(),
            action,
            resource: CatalogResource::Repository,
            repository: repo_key.to_string(),
            subject: repo_key.to_string(),
        }
    }

    /// The request on an asset, by its path.
    pub fn asset(
        principal_account: &str,
        action: CatalogAction,
        repository: &str,
        asset_path: &str,
    ) -> Self {
        Self {
            principal_account: principal_account.to_string(),
            action,
            resource: CatalogResource::Asset,
            repository: repository.to_string(),
            subject: asset_path.to_string(),
        }
    }

    /// The request on a version, by its coordinates.
    pub fn version(
        principal_account: &str,
        action: CatalogAction,
        repository: &str,
        coordinates: &PackageCoordinates,
    ) -> Self {
        Self {
            principal_account: principal_account.to_string(),
            action,
            resource: CatalogResource::Version,
            repository: repository.to_string(),
            subject: format!(
                "{}@{}",
                coordinates.package_name, coordinates.package_version
            ),
        }
    }

    /// The request on a tag, by its name.
    pub fn tag(
        principal_account: &str,
        action: CatalogAction,
        repository: &str,
        tag_name: &str,
    ) -> Self {
        Self {
            principal_account: principal_account.to_string(),
            action,
            resource: CatalogResource::Tag,
            repository: repository.to_string(),
            subject: tag_name.to_string(),
        }
    }

    /// The publication request.
    pub fn publish(principal_account: &str, repository: &str, asset_path: &str) -> Self {
        Self::asset(
            principal_account,
            CatalogAction::Publish,
            repository,
            asset_path,
        )
    }
}

/// What is decided on a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecision {
    /// It is allowed.
    Allow,
    /// It is forbidden, with the reason.
    Deny(String),
}

/// Who decides whether a request is served.
///
/// The catalog knows nothing of Cedar, roles or tokens: it only knows how to
/// ask. Whoever decides is injected, and the default value is
/// to deny: a catalog that has not been given a decider publishes
/// nothing.
#[async_trait]
pub trait CatalogAuthorizer: Send + Sync {
    /// Decides on the request.
    async fn authorize(&self, request: &AccessRequest) -> Result<AccessDecision, CatalogError>;
}

/// The decider that allows nothing.
///
/// It is the default value of a catalog without a decider, and for that reason it is
/// also the one used to check that the path without permissions is
/// really closed.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyAllAuthorizer;

#[async_trait]
impl CatalogAuthorizer for DenyAllAuthorizer {
    async fn authorize(&self, request: &AccessRequest) -> Result<AccessDecision, CatalogError> {
        Ok(AccessDecision::Deny(format!(
            "the catalog has no authorizer configured, so {} is refused",
            request.action.as_str()
        )))
    }
}

/// The decider that allows everything.
///
/// Only for the internal service, which already authorized the request before
/// getting here. It is not a shortcut for tests: it is the catalog's
/// boundary, which does not decide again what the boundary already decided.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllAuthorizer;

#[async_trait]
impl CatalogAuthorizer for AllowAllAuthorizer {
    async fn authorize(&self, _request: &AccessRequest) -> Result<AccessDecision, CatalogError> {
        Ok(AccessDecision::Allow)
    }
}

/// What can fail when asking about a blob.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlobProbeError {
    /// The blob is not in the store.
    #[error("the blob is not in the store")]
    Missing,
    /// The store could not answer.
    #[error("the blob store is unavailable: {detail}")]
    Unavailable {
        /// What the store said.
        detail: String,
    },
}

/// The real length of a blob in the CAS.
///
/// The catalog stores the length that the client declared. This is the
/// length that the content really has, and comparing them is what
/// separates a healthy record from a broken one.
///
/// The question carries the blob's **location** and not just its digest, because
/// the key with which the CAS stores the bytes depends on the scope: the
/// same digest lives in a global slot or in one partitioned by
/// account. Asking for the bare digest could only find the
/// global content, and would deem a partitioned blob that is
/// healthy to be broken.
///
/// What the probe does NOT do is recompute the digest: it checks the
/// length, which is cheap, and leaves deep verification to the scrub.
#[async_trait]
pub trait BlobIntegrityProbe: Send + Sync {
    /// The length of the blob, in bytes.
    async fn length_of(&self, location: &BlobLocation) -> Result<u64, BlobProbeError>;
}

/// The maximum number of assets that `assets_under` returns.
pub const MAX_ASSETS_UNDER: usize = 20_000;

/// The catalog read.
///
/// All reads carry the account of the verified principal. The account
/// is not a filter that can be forgotten: it is part of the query, and for
/// that reason two accounts with the same path do not see each other.
#[async_trait]
pub trait CatalogReader: Send + Sync {
    /// The repository of the account.
    async fn repository(
        &self,
        principal_account: &str,
        repo_key: &str,
    ) -> Result<RepositoryDescriptor, CatalogError>;

    /// The assets whose path starts with the prefix, ordered by path.
    ///
    /// It is the read needed by the formats that **derive** views from what was
    /// published (for example, Maven metadata). It does not check that each
    /// asset points to a version that exists: that is done by `asset`, which is what
    /// serves content. If there are more than `MAX_ASSETS_UNDER`, it fails instead of
    /// returning a list that looks complete and is not.
    async fn assets_under(
        &self,
        principal_account: &str,
        repository: &str,
        prefix: &str,
    ) -> Result<Vec<AssetRecord>, CatalogError>;

    /// The asset at a path of the repository.
    async fn asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<AssetRecord, CatalogError>;

    /// The published version.
    async fn version(
        &self,
        principal_account: &str,
        repository: &str,
        coordinates: &PackageCoordinates,
    ) -> Result<PackageVersion, CatalogError>;

    /// The tag and the version it points to.
    async fn tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
    ) -> Result<MutableTag, CatalogError>;

    /// The live reference to the blob.
    async fn reference(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<BlobReference, CatalogError>;

    /// The tags of the repository.
    ///
    /// The complete list, without a page: whoever paginates is the one who knows what
    /// the client has already seen, and the cut is made by the surface that
    /// speaks the protocol.
    async fn tags(
        &self,
        principal_account: &str,
        repository: &str,
    ) -> Result<Vec<MutableTag>, CatalogError>;

    /// The referrers of a manifest: the manifests of the account and the
    /// repository whose `subject` is the given digest.
    ///
    /// An empty list is a valid answer: a `subject` that has no
    /// referrers is not an error, and the protocol says so.
    async fn referrers(
        &self,
        principal_account: &str,
        repository: &str,
        subject: &ContentDigest,
    ) -> Result<Vec<ReferrerRecord>, CatalogError>;
}

/// The catalog search.
///
/// It is a port apart from the read one, and not an extension of it: "what
/// is there?" and "what is there at this path?" are different questions,
/// with different limits and different pages.
///
/// # The permission comes first
///
/// The search receives the repositories the principal may read, and only
/// looks in them. The account is not a filter that can be forgotten: it
/// is part of the query, and a total that counted what cannot be read
/// would say there is something where there is nothing to see.
#[async_trait]
pub trait CatalogSearch: Send + Sync {
    /// The repositories of the account.
    ///
    /// The ones the principal may read are decided outside: here only the
    /// ones of the account are listed, which is what the permission can
    /// narrow.
    async fn repositories(
        &self,
        principal_account: &str,
    ) -> Result<Vec<RepositoryDescriptor>, CatalogError>;

    /// The page of results of a search.
    ///
    /// The repositories are the ones the principal may read. A principal
    /// that cannot read any of them sees nothing, and its total is zero.
    async fn search(
        &self,
        principal_account: &str,
        repositories: &[RepositoryDescriptor],
        query: &SearchQuery,
    ) -> Result<SearchPage, CatalogError>;
}

/// The catalog write.
#[async_trait]
pub trait CatalogWriter: Send + Sync {
    /// Creates a repository.
    async fn create_repository(
        &self,
        principal_account: &str,
        descriptor: RepositoryDescriptor,
    ) -> Result<RepositoryDescriptor, CatalogError>;

    /// Changes the repository configuration and bumps its version.
    async fn update_config(
        &self,
        principal_account: &str,
        repo_key: &str,
        config: RepositoryConfig,
    ) -> Result<RepositoryDescriptor, CatalogError>;

    /// Publishes content: version, asset, reference and tags, in a
    /// single operation.
    async fn publish(
        &self,
        principal_account: &str,
        publication: Publication,
    ) -> Result<PublishOutcome, CatalogError>;

    /// Moves a tag to another version.
    ///
    /// Moving the tag does not touch the blob of either version.
    async fn move_tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
        coordinates: PackageCoordinates,
    ) -> Result<MutableTag, CatalogError>;

    /// Moves an asset to another version.
    ///
    /// The path of a repository is an index: it can be moved, but moving it
    /// is an explicit operation and not a side effect of publishing. For
    /// that reason publishing to an occupied path is a conflict, and this is the
    /// operation that resolves that conflict when the client asks for it.
    ///
    /// Moving the asset also moves the reference to the blob: the reference
    /// has to point to the content that the asset points to, or the
    /// download would serve one blob and the collection would keep another.
    async fn move_asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
        coordinates: PackageCoordinates,
    ) -> Result<AssetRecord, CatalogError>;

    /// Deletes an asset: its row, the reference to its blob and the versions
    /// of its package, in one atomic operation.
    ///
    /// The deletion is logical and never touches bytes: once the rows are
    /// gone nothing in the catalog keeps the digest alive, and the garbage
    /// collector (which honors holds) reclaims it later. Deleting an asset
    /// that is not there is not an error (`removed: false`), so a retry
    /// after a lost answer is harmless.
    ///
    /// The audit event of the deletion (`actor`, `correlation_id`) is written
    /// in the same transaction as the removal, like the one of a publication:
    /// there is no removal without its event and no event without its removal.
    /// At most a bounded number of versions go in one call (`partial`).
    async fn delete_asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
        actor: &str,
        correlation_id: &str,
    ) -> Result<AssetDeletion, CatalogError>;
}
