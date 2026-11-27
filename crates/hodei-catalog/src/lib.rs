//! The Hodei artifact catalog.
//!
//! What is kept there is **that** something exists, not **where** the bytes are: the bytes
//! belong to the CAS and are called by their digest; the catalog says that an
//! asset exists, that it points to a version, and that the version has a digest.
//!
//! # The four things this catalog keeps separate
//!
//! - **The repository identity** (`RepositoryDescriptor`): account,
//!   key, format, mode and versioned configuration.
//! - **The content identity** (`ContentDigest`): the same digest is
//!   the same content, today or another day, in this account or another.
//! - **The package coordinates** (`PackageCoordinates`): name and
//!   version, immutable once published.
//! - **The position** (`AssetRecord`): the path inside the repository, which
//!   can be moved to another version without touching the bytes of either.
//!
//! # Why the separation matters
//!
//! A mutable tag that moves cannot rewrite the blob it
//! points to: if it did, `latest` would change the content of an already
//! published version, and the digest would stop being the identity of anything. That is why
//! the tag is a separate row, and moving it only writes that row.
//!
//! And that is why the digest has a **scope policy** (`DigestScope`):
//! the same content in two accounts can be one shared blob or two
//! blobs, depending on what the repository declares. The policy is stored
//! next to the repository, it is not deduced.
//!
//! # The errors that are not confused
//!
//! `Missing`, `Denied` and `Corrupt` are three different things and the catalog
//! keeps them different. A `Missing` is "there is nothing there"; a `Denied`
//! is "there is something there and this principal cannot see it", and it is decided by the
//! authorization port before looking at the database, so as not to leak the
//! existence of what belongs to others; a `Corrupt` is "there is something there and it is broken", and
//! it is detected by comparing the record with the version it references and with
//! the bytes the CAS says it has.
//!
//! What the probe checks is the **length** of the bytes, not their
//! digest: rehashing an artifact on every read is a cost a
//! registry cannot pay, and deep verification belongs to the scrub.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod apt;
pub mod cargo;
pub mod conan;
pub mod conformance;
pub mod error;
pub mod galaxy;
pub mod gc;
pub mod gomod;
pub mod group;
pub mod hf;
pub mod location;
pub mod maven;
pub mod model;
pub mod namespaces;
pub mod npm;
pub mod nuget;
pub mod ports;
pub mod proxy;
pub mod proxy_cache;
pub mod pypi;
pub mod search;
pub mod terraform;
pub mod terraform_modules;
pub mod trust;
pub mod yum;

pub use conformance::{
    Accreditation, AuditFailure, ConformanceCell, EvidenceMismatch, PROFILE_VERSION, accredits,
    audit, cell as conformance_cell, matrix as conformance_matrix, refuses, served_pairs,
};
pub use error::CatalogError;
pub use gc::{
    ContentHold, ContentLedger, ContentRoot, DeletionState, GcDeletion, GcLease, GcRun, HoldScope,
    Quarantined, RootKind, RootPage, RunCandidate, RunState,
};
pub use group::{
    GroupConfig, GroupConfigError, GroupError, Lookup, Node, Walk, walk as walk_group,
};
pub use location::{LocationEntry, LocationJournal, LocationLease, Phase};
pub use model::{
    AssetDeletion, AssetDisposition, AssetRecord, BlobLocation, BlobReference, DigestScope,
    MutableTag, PackageCoordinates, PackageVersion, Precondition, Publication, PublishOutcome,
    ReferrerRecord, RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
    TrustState, repository_hrn,
};
pub use namespaces::{Judgement, MAX_RESERVED, NamespacePolicy, NamespacePolicyError, OnConflict};
pub use ports::{
    AccessDecision, AccessRequest, AllowAllAuthorizer, AptCasReference, AptGroupSnapshot,
    AptSnapshotKey, AptSnapshotMember, AptSnapshotStore, BlobIntegrityProbe, BlobProbeError,
    CatalogAction, CatalogAuthorizer, CatalogReader, CatalogResource, CatalogSearch, CatalogWriter,
    DenyAllAuthorizer, MAX_ASSETS_UNDER,
};
pub use proxy::{
    ChecksumEvidence, CredentialRef, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
    UpstreamConfig, UpstreamConfigError, origin_of,
};
pub use search::{
    DEFAULT_LIMIT, MAX_COUNTED, MAX_LIMIT, MAX_OFFSET, RepositoryAttribute, SearchError, SearchHit,
    SearchPage, SearchQuery,
};
pub use trust::{GrantSweepRow, GrantSweepSource, ServeChannel, TrustGrant, TrustLedger};
