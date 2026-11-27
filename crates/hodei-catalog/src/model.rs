//! The catalog model.
//!
//! Here the four things that an artifact repository easily
//! confuses are separated, and this catalog keeps them separate:
//!
//! 1. **The repository identity**: account, key, format, mode and
//!    versioned configuration.
//! 2. **The content identity**: the digest, which does not depend on where it
//!    lives or who published it.
//! 3. **The package coordinates**: the name and the version, which are
//!    immutable once published.
//! 4. **The asset path**: the position within the repository, which is what
//!    the client requests and what can point to another version without touching
//!    the bytes.
//!
//! The separation is not aesthetic. It is what allows a mutable tag
//! to move without altering a blob, and two accounts to use the same
//! path without stepping on each other.

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The format of the content that a repository stores.
///
/// The catalog knows all of them because it must be able to describe a
/// repository before the adapter that serves it exists. A format
/// that is not served yet is not an error: it is a repository that is declared and
/// waits for its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryFormat {
    /// Opaque content, served as is. It is the Raw 1.0 format.
    Raw,
    /// OCI images, with manifests and tags.
    Oci,
    /// Maven2, with `maven-metadata.xml` and group layouts.
    Maven2,
    /// npm, with packuments, scopes and dist-tags.
    Npm,
    /// PyPI, with the Simple API.
    PyPi,
    /// Cargo, with the sparse index.
    Cargo,
    /// NuGet v3, with service index and flat container.
    NuGet,
    /// Go Modules, with the GOPROXY protocol.
    Go,
    /// Helm, with charts and index repository.
    Helm,
    /// APT, with indexes and signatures.
    Apt,
    /// RPM, with repository creation.
    Rpm,
    /// Terraform/OpenTofu, as a provider **network mirror**.
    ///
    /// The served modality is the mirror one: it serves the protocol documents
    /// (`index.json`, the provider index, the version
    /// document and the zips) brought from the upstream and stored as is. It is not
    /// an origin registry or a module registry: nothing is published to it.
    Terraform,
    /// Conan 2, with recipes and binaries identified by recipe revision,
    /// `package_id` and package revision.
    ///
    /// The served modality is the hosted one: a Conan 2 client publishes with
    /// `conan upload` and resolves with `conan install` against the Hodei
    /// remote. The proxy needs the upstream dialect validated with a
    /// real client before it can be declared.
    Conan,
    /// Ansible Galaxy, collections via API v3.
    ///
    /// The served modality is the hosted one: an `ansible-galaxy` client
    /// publishes with `collection publish` and resolves with `collection
    /// install`/`verify` against Hodei's v3 surface. The proxy is not
    /// declared: the real upstream delivers through signed redirects and there is no
    /// client verified against it.
    Galaxy,
    /// Hugging Face, models via the hub's read dialect.
    ///
    /// It is a **read surface for seeded content**, not a
    /// served modality: an `hf` client (`huggingface_hub`) resolves with
    /// `hf download` against Hodei's hub surface (JSON revision and
    /// resolve with relative redirects to the CAS), but the release has no
    /// API ingestion --the handler only admits GET/HEAD and the
    /// content enters through the operator's seeding--. For that reason `serves_modes`
    /// is empty, it is not offered when creating repositories and the conformance
    /// matrix marks it as unsupported in all three modalities.
    Hf,
}

impl RepositoryFormat {
    /// The canonical name of the format, the one that goes to the wire and to the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            RepositoryFormat::Raw => "raw",
            RepositoryFormat::Oci => "oci",
            RepositoryFormat::Maven2 => "maven2",
            RepositoryFormat::Npm => "npm",
            RepositoryFormat::PyPi => "pypi",
            RepositoryFormat::Cargo => "cargo",
            RepositoryFormat::NuGet => "nuget",
            RepositoryFormat::Go => "go",
            RepositoryFormat::Helm => "helm",
            RepositoryFormat::Apt => "apt",
            RepositoryFormat::Rpm => "rpm",
            RepositoryFormat::Terraform => "terraform",
            RepositoryFormat::Conan => "conan",
            RepositoryFormat::Galaxy => "galaxy",
            RepositoryFormat::Hf => "hf",
        }
    }

    /// The format that says its canonical name.
    ///
    /// The name is the one that goes to the wire and to the database, and
    /// it is the same one [`RepositoryFormat::as_str`] returns: what is
    /// written is read, and what is read is written. A name that does not
    /// exist is not a format: it is a text nobody can interpret.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "raw" => Some(Self::Raw),
            "oci" => Some(Self::Oci),
            "maven2" => Some(Self::Maven2),
            "npm" => Some(Self::Npm),
            "pypi" => Some(Self::PyPi),
            "cargo" => Some(Self::Cargo),
            "nuget" => Some(Self::NuGet),
            "go" => Some(Self::Go),
            "helm" => Some(Self::Helm),
            "apt" => Some(Self::Apt),
            "rpm" => Some(Self::Rpm),
            "terraform" => Some(Self::Terraform),
            "conan" => Some(Self::Conan),
            "galaxy" => Some(Self::Galaxy),
            "hf" => Some(Self::Hf),
            _ => None,
        }
    }

    /// The modalities that this format serves in this release.
    ///
    /// A format is a way of storing content; whether the release knows to
    /// serve it in a given modality is a fact of the release, and it is
    /// written here, in one place, so that nobody has to answer the
    /// question twice and answer it differently.
    ///
    /// The raw surface, and the npm and PyPI ones that ride on it, serve
    /// the three modalities: what a client publishes, what a proxy brings
    /// from its upstream, and what a group composes of its members. The
    /// OCI surface serves the hosted one, the proxy one and the group one:
    /// an OCI group resolves the reference among its members with the
    /// precedence declared by its configuration. The cargo one serves the hosted
    /// one and the proxy one —its sparse index upstream was verified with
    /// the real client— but not the group: composing an index by package
    /// across members has not been proven with a client. The go surface serves the
    /// hosted one, the proxy one and the group one: a go group assembles
    /// the union of the versions of its members and resolves a version
    /// from a single member, so the `go.sum` of the client always
    /// describes one and the same version. The NuGet surface serves the
    /// hosted and proxy modalities, plus groups that combine version indexes
    /// across members while resolving each package version from one owner.
    ///
    /// The formats that are declared but not served yet —helm— have no
    /// modality at all: a repository of one of them can be declared, and
    /// it waits for its turn. Apt serves hosted, proxy and group (signed distro
    /// mirror snapshots); rpm serves hosted.
    pub fn serves_modes(&self) -> &'static [RepositoryMode] {
        match self {
            RepositoryFormat::Raw
            | RepositoryFormat::Maven2
            | RepositoryFormat::Npm
            | RepositoryFormat::PyPi => &[
                RepositoryMode::Hosted,
                RepositoryMode::Proxy,
                RepositoryMode::Group,
            ],
            RepositoryFormat::Oci => &[
                RepositoryMode::Hosted,
                RepositoryMode::Proxy,
                RepositoryMode::Group,
            ],
            RepositoryFormat::Cargo => &[RepositoryMode::Hosted, RepositoryMode::Proxy],
            RepositoryFormat::Apt => &[
                RepositoryMode::Hosted,
                RepositoryMode::Proxy,
                RepositoryMode::Group,
            ],
            RepositoryFormat::Rpm => &[
                RepositoryMode::Hosted,
                RepositoryMode::Proxy,
                RepositoryMode::Group,
            ],
            RepositoryFormat::NuGet => &[
                RepositoryMode::Hosted,
                RepositoryMode::Proxy,
                RepositoryMode::Group,
            ],
            RepositoryFormat::Go => &[
                RepositoryMode::Hosted,
                RepositoryMode::Proxy,
                RepositoryMode::Group,
            ],
            // Terraform speaks two dialects depending on the modality: the provider
            // mirror is proxy (it serves what the upstream has,
            // cached; it does not publish and does not assemble groups) and the module
            // registry is hosted (the hub stores the tarballs that are
            // published to it, versioned and immutable).
            RepositoryFormat::Terraform => &[RepositoryMode::Proxy, RepositoryMode::Hosted],
            // Conan serves the hosted one: the client publishes what it has created and
            // resolves what the remote stores. The proxy needs the upstream
            // dialect proven with a real client, and the group needs a
            // composed resolution proven, before they can be promised.
            RepositoryFormat::Conan => &[RepositoryMode::Hosted],
            // Galaxy serves the hosted one: the client publishes with `collection
            // publish` and resolves with `collection install`/`verify`. The
            // proxy is not declared: the real upstream delivers through signed
            // redirects, a dialect that has not been proven with a client.
            RepositoryFormat::Galaxy => &[RepositoryMode::Hosted],
            // Hf serves no modality: without API ingestion (the
            // handler only reads and the content is seeded by the operator) there
            // is no hosted to promise, and without writes there is no proxy or
            // group either. Reading the seeded content exists and is
            // proven with a real client, but it is not a modality.
            RepositoryFormat::Hf => &[],
            RepositoryFormat::Helm => &[],
        }
    }

    /// Whether this format can be declared in that modality.
    ///
    /// The answer is the one of [`RepositoryFormat::serves_modes`], and it
    /// is asked in one place: a format that does not serve a modality
    /// cannot be declared in it.
    pub fn supports(&self, mode: &RepositoryMode) -> bool {
        self.serves_modes().contains(mode)
    }

    /// The formats that this release serves, in the order they are named.
    ///
    /// The formats that this release does not serve yet are not here, and
    /// they cannot be declared either: a repository of one of them would
    /// be a repository that nothing could serve. This list is what a
    /// client asks to know what it can create.
    pub fn served() -> &'static [RepositoryFormat] {
        &[
            RepositoryFormat::Raw,
            RepositoryFormat::Oci,
            RepositoryFormat::Maven2,
            RepositoryFormat::Npm,
            RepositoryFormat::PyPi,
            RepositoryFormat::Cargo,
            RepositoryFormat::Go,
            RepositoryFormat::NuGet,
            RepositoryFormat::Apt,
            RepositoryFormat::Rpm,
            RepositoryFormat::Terraform,
            RepositoryFormat::Conan,
            RepositoryFormat::Galaxy,
        ]
    }

    /// The formats this release declares, whether it serves them or not.
    ///
    /// A format can be part of the vocabulary of the registry —it parses,
    /// it has a name on the wire— without a surface that serves it: helm
    /// is declared, and no helm repository can be created yet. A client
    /// that builds a matrix of what exists and what is refused walks this
    /// list, and the refusal of a declared but unserved combination is an
    /// answer, not a failure: the registry knows the name and says no.
    pub fn declared() -> &'static [RepositoryFormat] {
        &[
            RepositoryFormat::Raw,
            RepositoryFormat::Oci,
            RepositoryFormat::Maven2,
            RepositoryFormat::Npm,
            RepositoryFormat::PyPi,
            RepositoryFormat::Cargo,
            RepositoryFormat::NuGet,
            RepositoryFormat::Go,
            RepositoryFormat::Helm,
            RepositoryFormat::Apt,
            RepositoryFormat::Rpm,
            RepositoryFormat::Terraform,
            RepositoryFormat::Conan,
            RepositoryFormat::Galaxy,
            RepositoryFormat::Hf,
        ]
    }
}

/// How a repository behaves with respect to the outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryMode {
    /// The repository stores what is published to it.
    Hosted,
    /// The repository resolves against an upstream and caches what it downloads.
    Proxy,
    /// The repository groups other repositories and resolves in them.
    Group,
}

impl std::fmt::Display for RepositoryMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl RepositoryMode {
    /// The canonical name of the mode, the one that goes to the wire and to the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            RepositoryMode::Hosted => "hosted",
            RepositoryMode::Proxy => "proxy",
            RepositoryMode::Group => "group",
        }
    }

    /// The mode that says its canonical name.
    ///
    /// The name is the one that goes to the wire and to the database, and
    /// it is the same one [`RepositoryMode::as_str`] returns. A name that
    /// does not exist is not a mode: it is a text nobody can interpret.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "hosted" => Some(Self::Hosted),
            "proxy" => Some(Self::Proxy),
            "group" => Some(Self::Group),
            _ => None,
        }
    }

    /// Whether this release knows how to behave this way.
    ///
    /// The catalog knows how to DESCRIBE the three modes, because a repository is
    /// declared before it exists; the release only SERVES them if it knows how to
    /// resolve them. The `match` is exhaustive on purpose: a new mode
    /// forces a decision here on whether it is already served, and promising it without delivering would be
    /// a false promise that is paid for when someone publishes where there is
    /// nothing.
    ///
    /// A proxy resolves against the upstream declared by its configuration and
    /// what it downloads is stored like any other content. A group resolves
    /// in its members, in order, and stores nothing of its own.
    pub fn is_served_by_this_release(&self) -> bool {
        match self {
            RepositoryMode::Hosted | RepositoryMode::Proxy | RepositoryMode::Group => true,
        }
    }

    /// Whether the repository stores its own content (assets, versions, blobs).
    ///
    /// A group only contains a list of members: what it reads comes from
    /// them, never from rows of its own. For that reason no content write
    /// -- neither a client's nor a proxy's cache -- can reach it.
    pub fn stores_content(&self) -> bool {
        matches!(self, RepositoryMode::Hosted | RepositoryMode::Proxy)
    }

    /// Whether clients can publish, tag and move aliases here.
    ///
    /// Only a `Hosted` repository stores what is published to it. A
    /// proxy writes its own content when resolving, but no request
    /// from a client writes or moves it: what a proxy says comes from the
    /// upstream, not from whoever asks.
    pub fn accepts_client_writes(&self) -> bool {
        matches!(self, RepositoryMode::Hosted)
    }
}

/// Where the blob of a digest lives.
///
/// The digest identifies the content, but does not say where its
/// bytes are stored. That is a decision of the account, and for that reason it is policy and not
/// an implementation detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestScope {
    /// A single blob per digest, shared by all accounts.
    ///
    /// Two accounts that publish the same content share the bytes.
    /// It is what makes a registry efficient, and also what makes
    /// deleting a blob have to take everyone into account.
    Global,
    /// One blob per digest and account: the same content in two accounts is
    /// two sets of bytes.
    ///
    /// It is what an account needs when it does not want its content to
    /// be deduplicated against that of another account.
    AccountPartitioned,
}

impl DigestScope {
    /// The key with which the CAS stores the blob.
    ///
    /// The CAS stores SHA-256 digests by their hexadecimal value, so
    /// the global key is that value. The partitioned form prefixes the
    /// account, and for that reason the two policies cannot slip the same blob
    /// into the same slot.
    pub fn blob_key(&self, account: &str, digest: &ContentDigest) -> String {
        match self {
            DigestScope::Global => digest.value.clone(),
            DigestScope::AccountPartitioned => format!("{account}/{}", digest.value),
        }
    }
}

/// Where a specific blob lives.
///
/// The digest says **what** the content is; the location says **where**
/// its bytes are. Asking the byte store for the content
/// means asking for both things: the same digest can be in
/// the global slot or in an account's partitioned one, and whoever looks for only
/// one of the two finds a blob that is not theirs, or finds nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobLocation {
    /// The account the content belongs to.
    pub account: String,
    /// The scope in which it lives.
    pub scope: DigestScope,
    /// The digest of the content.
    pub digest: ContentDigest,
}

impl BlobLocation {
    /// The location of a content in a scope.
    pub fn new(account: &str, scope: DigestScope, digest: ContentDigest) -> Self {
        Self {
            account: account.to_string(),
            scope,
            digest,
        }
    }

    /// The key with which the CAS stores that blob.
    pub fn blob_key(&self) -> String {
        self.scope.blob_key(&self.account, &self.digest)
    }
}

/// The configuration of a repository, with the version that identifies it.
///
/// The version is not decorative: it is what makes it possible to know whether what the
/// client has in front of it is what the repository has stored. A change in
/// configuration increments the version, and two configurations with the same
/// version are the same configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryConfig {
    /// The version of this configuration.
    pub version: u64,
    /// Where the blobs of this repository live.
    pub digest_scope: DigestScope,
    /// The format's own settings.
    ///
    /// An empty object is a valid object: a newly created repository
    /// has no settings to apply.
    #[serde(default)]
    pub config_settings: Value,
}

impl RepositoryConfig {
    /// The default configuration: version 1, shared digests and no
    /// settings.
    pub fn new(digest_scope: DigestScope) -> Self {
        Self {
            version: 1,
            digest_scope,
            config_settings: Value::Object(serde_json::Map::new()),
        }
    }

    /// Whether the settings are an object and not something else.
    ///
    /// A `settings` that is not an object has no possible interpretation, and
    /// accepting it would be accepting a configuration that nobody can read.
    pub fn has_readable_settings(&self) -> bool {
        self.config_settings.is_object()
    }
}

/// The identity of a repository.
///
/// The HRN is the canonical name of the resource and is not deduced from anything: it is
/// declared by whoever creates the repository, and the catalog keeps it as is
/// so that the same HRN keeps meaning the same thing after a
/// restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryDescriptor {
    /// The canonical name of the resource (HRN v2).
    pub hrn: Hrn,
    /// The owning account.
    pub account: String,
    /// The short key of the repository within the account.
    pub repo_key: String,
    /// The format of the content.
    pub format: RepositoryFormat,
    /// How it behaves.
    pub mode: RepositoryMode,
    /// The versioned configuration.
    pub config: RepositoryConfig,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it was last touched.
    pub updated_at: DateTime<Utc>,
}

/// The canonical HRN of a repository.
///
/// It is built with the same shape as any other HRN v2 resource, with
/// the type `repository` and the repository key as the identifier.
pub fn repository_hrn(
    partition: &str,
    service: &str,
    region: &str,
    account: &str,
    repo_key: &str,
) -> Hrn {
    Hrn::with_region(
        partition.to_string(),
        service.to_string(),
        region.to_string(),
        account.to_string(),
        "repository".to_string(),
        repo_key.to_string(),
    )
}

/// The coordinates of a version within a repository.
///
/// They are immutable: a published version does not change its name or its
/// content. If the content changes, it is another version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageCoordinates {
    /// The name of the package.
    pub package_name: String,
    /// The version of the package.
    pub package_version: String,
}

/// A published version: the content with its name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageVersion {
    /// The owning account.
    pub account: String,
    /// The repository.
    pub repository: String,
    /// The name and the version.
    pub coordinates: PackageCoordinates,
    /// The digest of the content.
    pub digest: ContentDigest,
    /// The bytes the content has.
    pub size_bytes: u64,
    /// When it was published.
    pub published_at: DateTime<Utc>,
}

/// An asset: a path of a repository.
///
/// The asset is the position, not the content: it points to a version and to its
/// digest. Moving the asset to another version is an index operation, and
/// it does not touch the bytes of either one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetRecord {
    /// The owning account.
    pub account: String,
    /// The repository.
    pub repository: String,
    /// The path within the repository.
    pub asset_path: String,
    /// The version it points to.
    pub coordinates: PackageCoordinates,
    /// The digest of the content it points to.
    pub digest: ContentDigest,
    /// The bytes that content has.
    pub size_bytes: u64,
    /// The `subject` of the manifest, if it has one.
    ///
    /// It is the digest of the manifest this one points to: what makes a
    /// manifest a **referrer** of another. An asset without `subject` is
    /// a perfectly normal manifest, and one with `subject` hangs from another.
    pub subject: Option<ContentDigest>,
    /// When it was last moved.
    pub updated_at: DateTime<Utc>,
}

/// A referrer: a manifest that points to another with `subject`.
///
/// It is the relationship that the protocol's referrers API exposes: the
/// manifests of an account and a repository whose `subject` is a
/// digest. It says nothing about the **validity** of what the referrer
/// claims: a referrer is a manifest that says it hangs from another, and
/// whoever judges what it says is the trust engine, not the catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferrerRecord {
    /// The owning account.
    pub account: String,
    /// The repository.
    pub repository: String,
    /// The path of the referrer within the repository.
    pub asset_path: String,
    /// The digest of the referrer's manifest.
    pub digest: ContentDigest,
    /// The bytes the referrer's manifest has.
    pub size_bytes: u64,
    /// The digest of the `subject` it points to.
    pub subject: ContentDigest,
}

/// A live reference to a blob of the CAS.
///
/// The reference says which blob serves **a path**: it is the row that
/// moves with the asset, so that the download and the collection do not
/// point to different contents.
///
/// The blob of a published version is protected by its version row, which is
/// immutable and keeps the digest and the length. The reference protects the
/// content that the path serves **while it serves it**: moving the asset
/// rewrites the reference to the new digest, and the version that is left
/// behind remains declared, with its digest, in its own row. Whoever collects
/// blobs has to take both things into account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlobReference {
    /// The owning account.
    pub account: String,
    /// The repository.
    pub repository: String,
    /// The path that references the blob.
    pub asset_path: String,
    /// The digest of the blob.
    pub digest: ContentDigest,
    /// Where the blob lives.
    pub scope: DigestScope,
    /// The bytes the blob has.
    pub size_bytes: u64,
    /// In which channel the content lives in this repository.
    ///
    /// It is written by **admission**: a newly admitted artifact is in
    /// quarantine, and leaving it requires a trust decision. And it
    /// does not belong to the blob: the blob is immutable and what decides is the relationship
    /// between the repository and what it stores.
    pub trust_state: TrustState,
    /// When the reference was written.
    pub updated_at: DateTime<Utc>,
}

/// In which channel an artifact lives within a repository.
///
/// The state is **not** the blob's: the blob is immutable and its digest is its
/// identity, and the same content can be in quarantine in one
/// account and promoted in another. The state belongs to the **relationship** between the
/// repository and the content, which is what the catalog reference
/// writes.
///
/// And the default state of a newly admitted artifact is
/// **quarantine**: without a trust decision there is no promotion, and an
/// artifact without a decision has not said anything yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    /// The artifact is admitted, but not promoted: it is in
    /// quarantine.
    Quarantined,
    /// The artifact has been promoted with a trust decision
    /// in force.
    Trusted,
}

impl TrustState {
    /// The canonical name, as it goes in the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Quarantined => "quarantined",
            Self::Trusted => "trusted",
        }
    }

    /// The state that the name says.
    ///
    /// What is not known is quarantine: a state that cannot be
    /// read is not a permission, and an artifact whose channel is not known cannot
    /// be in the trust channel.
    pub fn of(name: &str) -> Self {
        match name {
            "trusted" => Self::Trusted,
            _ => Self::Quarantined,
        }
    }
}

/// A mutable tag: a name that points to a version.
///
/// Moving the tag changes this row and nothing else. The version that is left
/// behind keeps its digest and its bytes, and remains reachable by
/// its coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutableTag {
    /// The owning account.
    pub account: String,
    /// The repository.
    pub repository: String,
    /// The name of the tag.
    pub tag_name: String,
    /// The version it now points to.
    pub coordinates: PackageCoordinates,
    /// When it was moved.
    pub updated_at: DateTime<Utc>,
}

/// What is to be published, in a single operation.
///
/// Publishing is four things at once: the version, the asset, the blob
/// reference and the tags. They go together because an asset without a version does not
/// resolve, and a version without an asset cannot be downloaded.
///
/// The publication carries **who** publishes and with what correlation, because
/// publishing is someone's operation: without that, the trail of what was
/// published does not say who published it. What it does **not** carry is the account: the
/// tenant of the audit event is the **verified** account of the
/// principal, and an account declared by the caller is not a verified
/// account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Publication {
    /// The repository where it is published.
    pub repository: String,
    /// The path within the repository.
    pub asset_path: String,
    /// The name and the version.
    pub coordinates: PackageCoordinates,
    /// The digest of the content.
    pub digest: ContentDigest,
    /// The bytes the content has.
    pub size_bytes: u64,
    /// The tags that are born with this publication.
    pub tags: Vec<String>,
    /// The actor that publishes.
    ///
    /// The catalog verifies the **account** against the authorizer, not the
    /// subject: for that reason the actor is declared by whoever publishes, and for that reason it is not
    /// called "verified". In the HTTP layer the actor comes from the verified
    /// principal, so there it is indeed.
    pub actor: String,
    /// The correlation of the request.
    pub correlation_id: String,
    /// Whether the path may point to ANOTHER version when it already points to one.
    ///
    /// The catalog is, by default, **immutable**: a path that already points
    /// to a version does not move on its own, and publishing other content at the
    /// same path is a conflict. That is correct for a package
    /// catalog, where a published version is a fact that is not
    /// rewritten.
    ///
    /// A **raw** content server is something else: its path is the
    /// address of the content, and resending a file to the same
    /// address is restoring what is there. For that reason replacement is an
    /// **explicit decision** and not a side effect: whoever publishes
    /// says whether it can, and whoever does not say so, cannot.
    ///
    /// What is replaced is the **pointer**: the previous version is not
    /// deleted or rewritten, it is still there, and the path now points to
    /// the new one.
    #[serde(default)]
    pub overwrite: bool,
    /// The condition that the client demands on what is there now.
    ///
    /// It is not a decoration of the caller: it is what makes the write
    /// **conditional**, and the catalog checks it **inside the same
    /// transaction** that writes the rows. Checking it earlier, at the
    /// boundary, leaves a window between the check and the write in
    /// which another publication slips in.
    ///
    /// Without a condition, the publication is what it is: a write.
    #[serde(default)]
    pub precondition: Option<Precondition>,
    /// The `subject` of the manifest, if it has one.
    ///
    /// It is the digest of the manifest this one hangs from: what makes a
    /// manifest a **referrer** of another. It is written in the **same**
    /// transaction as the publication, and not in a second operation: a
    /// half-visible referrer would be a referrer that exists when it does not
    /// exist, and the referrers list would stop being the truth.
    ///
    /// The `subject` is **not** checked against the catalog: the protocol
    /// requires accepting a manifest whose `subject` is not there yet, because
    /// a client may upload the referrer before the subject.
    #[serde(default)]
    pub subject: Option<ContentDigest>,
}

/// What the client demands be at the path before writing.
///
/// It is not the same as a conflict: the conflict says there is something
/// different from what is published, and the precondition says that **what
/// is there** is not what the client expected. The client treats them differently,
/// and for that reason the catalog distinguishes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Precondition {
    /// What is at the path must be one of these contents.
    ///
    /// It is the `If-Match` of RFC 9110: an entity tag, or a list of
    /// entity tags of which one must match.
    Present(Vec<ContentDigest>),
    /// There must be nothing at the path.
    ///
    /// It is the `If-None-Match: *` of RFC 9110.
    Absent,
    /// There must be something at the path.
    ///
    /// It is the `If-Match: *` of RFC 9110.
    Exists,
    /// What is at the path must be **other** content.
    ///
    /// It is the `If-None-Match` with entity tags on a write: "write,
    /// but only if what is there is none of these".
    Other(Vec<ContentDigest>),
}

/// What the publication left.
///
/// `created` says whether the rows were created or were already there with the same
/// content. Republishing the same thing is neither an error nor a write: it is the
/// same thing, said twice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishOutcome {
    /// The version.
    pub version: PackageVersion,
    /// The asset.
    pub asset: AssetRecord,
    /// The reference to the blob.
    pub reference: BlobReference,
    /// Whether the rows were created.
    pub created: bool,
    /// What happened to the asset row.
    ///
    /// `created` says whether something was written, and does not say **what**: publishing
    /// for the first time and restoring what is there both write rows, and
    /// for the client it is not the same. The disposition says it.
    pub disposition: AssetDisposition,
}

/// What happened to the asset row of a publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetDisposition {
    /// The row did not exist, and has been created.
    Created,
    /// The row existed, and has been moved to other content.
    ///
    /// The previous version is not deleted or rewritten: what moves is
    /// the pointer.
    Replaced,
    /// The row was already there with this content, and has not been touched.
    Unchanged,
}

/// What a deletion of an asset left behind (HSP-113).
///
/// The deletion is logical: the catalog stops naming the content, and the
/// bytes are the garbage collector's to reclaim (holds and other references
/// keep them alive). Deleting what is not there is not an error: the result
/// says `removed: false` and nothing was written.
///
/// # Semantics under concurrency
///
/// One guarded pass removes what the pass read. A publication of the same path
/// that commits **after** it is not deleted: the verification read that follows
/// reports it as `settled: false`, and the caller says so instead of pretending
/// the path is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetDeletion {
    /// Whether any row of the catalog was removed by this call.
    pub removed: bool,
    /// The contents the catalog no longer names because of this call
    /// (the asset's own and the ones of the versions of its package).
    pub released: Vec<ContentDigest>,
    /// How many version rows were removed.
    pub versions_removed: usize,
    /// Whether the verification read found nothing of the path left. `false`
    /// means a version row is still there (see `partial`) or the path was
    /// published again while the deletion ran; a failed verification read
    /// also leaves it `false`: what was not read back is not reported settled.
    pub settled: bool,
    /// Whether more versions than one deletion may remove were found: the
    /// rest is removed by repeating the request.
    pub partial: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_is_read_from_its_own_name() {
        let formats = [
            RepositoryFormat::Raw,
            RepositoryFormat::Oci,
            RepositoryFormat::Maven2,
            RepositoryFormat::Npm,
            RepositoryFormat::PyPi,
            RepositoryFormat::Cargo,
            RepositoryFormat::NuGet,
            RepositoryFormat::Go,
            RepositoryFormat::Helm,
            RepositoryFormat::Apt,
            RepositoryFormat::Rpm,
            RepositoryFormat::Terraform,
            RepositoryFormat::Conan,
            RepositoryFormat::Galaxy,
            RepositoryFormat::Hf,
        ];
        // The round trip covers every declared format: a format added to the
        // registry without a line here is a format whose name nobody reads back.
        for declared in RepositoryFormat::declared() {
            assert!(
                formats.contains(declared),
                "{} is declared but not in the round trip",
                declared.as_str()
            );
        }
        for format in formats {
            assert_eq!(
                RepositoryFormat::parse(format.as_str()),
                Some(format),
                "{} does not read back",
                format.as_str()
            );
        }
        assert_eq!(RepositoryFormat::parse("zip"), None);
        assert_eq!(RepositoryFormat::parse(""), None);
    }

    #[test]
    fn every_mode_is_read_from_its_own_name() {
        let modes = [
            RepositoryMode::Hosted,
            RepositoryMode::Proxy,
            RepositoryMode::Group,
        ];
        for mode in modes {
            assert_eq!(
                RepositoryMode::parse(mode.as_str()),
                Some(mode),
                "{} does not read back",
                mode.as_str()
            );
        }
        assert_eq!(RepositoryMode::parse("mirror"), None);
        assert_eq!(RepositoryMode::parse("Hosted"), None);
    }
}
