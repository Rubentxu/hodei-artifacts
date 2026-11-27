//! The catalog's Dgraph port (HSP-187, Raw slice).
//!
//! It ports the durable catalog of HAR-02/HAR-03 onto the native graph
//! engine (`hodei-graph-client`): Repository, Asset, Version,
//! BlobReference and the audit outbox, keeping the CAS **external**
//! -- the bytes never live in the graph; the graph points by digest.
//!
//! The commit boundary is the one HAR-03 marks: **the catalog commit
//! and the audit event are ONE transaction**. The transaction is the
//! atomic upsert of the projection boundary (HSP-199): the scope guard
//! (`hodei.guard.*`) advances, the operation marker
//! (`hodei.operation.*`) is written only once, and the catalog rows
//! plus the event are applied in conditioned blocks of the same commit.
//! The answer is decided by **the state after the commit**, never by the
//! hope that the transaction committed.
//!
//! The per-key guarantees: each row carries its composite key
//! (`hodei.catalog.*.key`, exact index and `@upsert`), so the server
//! --not a pre-read-- arbitrates two writers of the same key. The
//! idempotent case is that of HAR-02: republishing the same thing is neither an error nor a new
//! write, and an event that says "published" about what was already
//! there adds no fact.
//!
//! The mutable-tags slice (the one for OCI registries) rides on the
//! same boundary: the guard per tag key is what makes two
//! writers moving the same tag serialize -- the commit that
//! the graph arbitrates last wins, and neither loses its fact. The
//! redelivery of the same move (same tag, same version) re-reads the
//! committed result: idempotency by fact, not by luck.
//!
//! What the slice does NOT serve yet (repository reconfiguration,
//! derived views) it says with a typed error instead of pretending
//! to serve it.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use hodei_audit::{
    AuditContext, AuditEntry, AuditError, AuditEvent, AuditOutbox, AuditResult, EntryState,
};
use hodei_catalog::search::{
    RepositoryAttribute, SearchHit, SearchPage, SearchQuery, parse_search_cursor,
};
use hodei_catalog::{
    AccessDecision, AccessRequest, AptCasReference, AptGroupSnapshot, AptSnapshotKey,
    AptSnapshotMember, AptSnapshotStore, AssetDeletion, AssetDisposition, AssetRecord,
    BlobIntegrityProbe, BlobLocation, BlobProbeError, BlobReference, CatalogAction,
    CatalogAuthorizer, CatalogError, CatalogReader, CatalogResource, CatalogSearch, CatalogWriter,
    DigestScope, MutableTag, PackageCoordinates, PackageVersion, Precondition, Publication,
    PublishOutcome, ReferrerRecord, RepositoryConfig, RepositoryDescriptor, RepositoryFormat,
    RepositoryMode, ServeChannel, TrustLedger, TrustState,
};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_graph_client::guard::{GuardKind, GuardScope, GuardedEffect};
use hodei_graph_client::{
    GraphClient, GraphClientError, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
    GuardStore, OperationId, RevisionDemand, Tenant, dql_string,
};
use kernel::application::ports::clock::Identity;
use sha2::{Digest as _, Sha256};

mod gc_ledger;

/// How many times the publication re-reads and retries before giving up.
const WRITE_ATTEMPTS: usize = 6;

/// The graph type of a repository row.
pub const REPOSITORY_TYPE: &str = "CatalogRepository";
/// The graph type of an asset.
pub const ASSET_TYPE: &str = "CatalogAsset";
/// The graph type of a published version.
pub const VERSION_TYPE: &str = "CatalogVersion";
/// The graph type of a live reference to a CAS blob.
pub const REFERENCE_TYPE: &str = "CatalogReference";
/// The graph type of an audit outbox row.
pub const OUTBOX_TYPE: &str = "CatalogAuditOutbox";
/// The graph type of a mutable tag.
pub const TAG_TYPE: &str = "CatalogTag";

/// Version of the catalog's **own** schema over Dgraph. It is separate from the
/// graph's versioned schema (`hodei-graph-client`, v7): this slice
/// declares its predicates when the store is opened, and does not touch the startup
/// contract that the rest of the stack validates.
pub const CATALOG_DGRAPH_SCHEMA_VERSION: i64 = 1;

/// The additive DDL of the catalog over Dgraph. Each key is an exact index and
/// `@upsert`: the server aborts whoever writes a second row with the
/// same key, within the same transaction or across transactions.
pub fn catalog_schema_dql() -> String {
    let mut ddl = String::new();
    for (predicate, kind, index) in [
        (
            "hodei.catalog.repository.key",
            "string",
            "@index(exact) @upsert",
        ),
        (
            "hodei.catalog.repository.account",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.repository.repo_key",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.repository.hrn", "string", ""),
        ("hodei.catalog.repository.format", "string", "@index(exact)"),
        ("hodei.catalog.repository.mode", "string", "@index(exact)"),
        (
            "hodei.catalog.repository.config_version",
            "int",
            "@index(int)",
        ),
        ("hodei.catalog.repository.config_scope", "string", ""),
        ("hodei.catalog.repository.config_settings", "string", ""),
        ("hodei.catalog.repository.created_at", "datetime", ""),
        ("hodei.catalog.repository.updated_at", "datetime", ""),
        ("hodei.catalog.asset.key", "string", "@index(exact) @upsert"),
        ("hodei.catalog.asset.account", "string", "@index(exact)"),
        ("hodei.catalog.asset.repository", "string", "@index(exact)"),
        ("hodei.catalog.asset.asset_path", "string", "@index(exact)"),
        (
            "hodei.catalog.asset.package_name",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.asset.package_version",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.asset.digest_algorithm",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.asset.digest", "string", "@index(exact)"),
        ("hodei.catalog.asset.size_bytes", "int", ""),
        ("hodei.catalog.asset.subject", "string", "@index(exact)"),
        ("hodei.catalog.asset.updated_at", "datetime", ""),
        (
            "hodei.catalog.version.key",
            "string",
            "@index(exact) @upsert",
        ),
        ("hodei.catalog.version.account", "string", "@index(exact)"),
        (
            "hodei.catalog.version.repository",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.version.package_name",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.version.package_version",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.version.digest_algorithm",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.version.digest", "string", "@index(exact)"),
        ("hodei.catalog.version.size_bytes", "int", ""),
        ("hodei.catalog.version.published_at", "datetime", ""),
        (
            "hodei.catalog.reference.key",
            "string",
            "@index(exact) @upsert",
        ),
        ("hodei.catalog.reference.account", "string", "@index(exact)"),
        (
            "hodei.catalog.reference.repository",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.reference.asset_path",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.reference.digest_algorithm",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.reference.digest", "string", "@index(exact)"),
        ("hodei.catalog.reference.scope", "string", "@index(exact)"),
        ("hodei.catalog.reference.size_bytes", "int", ""),
        ("hodei.catalog.reference.trust_state", "string", ""),
        ("hodei.catalog.reference.updated_at", "datetime", ""),
        // Immutable APT generations and their independently addressable index rows.
        (
            "hodei.catalog.apt_snapshot.key",
            "string",
            "@index(exact) @upsert",
        ),
        (
            "hodei.catalog.apt_snapshot.namespace",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.apt_snapshot.revision", "int", "@index(int)"),
        (
            "hodei.catalog.apt_snapshot.digest",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.apt_snapshot.payload", "string", ""),
        (
            "hodei.catalog.apt_snapshot_root.key",
            "string",
            "@index(exact) @upsert",
        ),
        (
            "hodei.catalog.apt_snapshot_root.account",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.apt_snapshot_root.repository",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.apt_snapshot_root.subject", "string", ""),
        (
            "hodei.catalog.apt_snapshot_root.digest_algorithm",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.apt_snapshot_root.digest",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.apt_snapshot_root.size_bytes", "int", ""),
        (
            "hodei.catalog.apt_pointer.key",
            "string",
            "@index(exact) @upsert",
        ),
        ("hodei.catalog.apt_pointer.revision", "int", ""),
        (
            "hodei.catalog.apt_packages_hash.key",
            "string",
            "@index(exact) @upsert",
        ),
        (
            "hodei.catalog.apt_packages_hash.snapshot_key",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.apt_packages_hash.namespace",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.apt_packages_hash.architecture",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.apt_packages_hash.digest",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.apt_packages_hash.revision",
            "int",
            "@index(int)",
        ),
        (
            "hodei.catalog.outbox.key",
            "string",
            "@index(exact) @upsert",
        ),
        ("hodei.catalog.outbox.tenant", "string", "@index(exact)"),
        ("hodei.catalog.outbox.actor", "string", ""),
        ("hodei.catalog.outbox.correlation_id", "string", ""),
        ("hodei.catalog.outbox.action", "string", ""),
        ("hodei.catalog.outbox.resource", "string", ""),
        ("hodei.catalog.outbox.digest", "string", ""),
        ("hodei.catalog.outbox.result", "string", ""),
        ("hodei.catalog.outbox.occurred_at", "datetime", ""),
        ("hodei.catalog.outbox.state", "string", "@index(exact)"),
        ("hodei.catalog.outbox.attempts", "int", ""),
        (
            "hodei.catalog.outbox.lease_owner",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.outbox.lease_until", "datetime", ""),
        ("hodei.catalog.outbox.last_error", "string", ""),
        ("hodei.catalog.outbox.delivered_at", "datetime", ""),
        // The mutable-tags slice (OCI): the key carries the upsert
        // so that two writers moving the same tag fight over
        // the index and not over the row.
        ("hodei.catalog.tag.key", "string", "@index(exact) @upsert"),
        ("hodei.catalog.tag.account", "string", "@index(exact)"),
        ("hodei.catalog.tag.repository", "string", "@index(exact)"),
        ("hodei.catalog.tag.tag_name", "string", "@index(exact)"),
        ("hodei.catalog.tag.package_name", "string", "@index(exact)"),
        (
            "hodei.catalog.tag.package_version",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.tag.updated_at", "datetime", ""),
    ] {
        ddl.push_str(predicate);
        ddl.push_str(": ");
        ddl.push_str(kind);
        if !index.is_empty() {
            ddl.push(' ');
            ddl.push_str(index);
        }
        ddl.push_str(" .\n");
    }
    for (name, family, fields) in [
        (
            REPOSITORY_TYPE,
            "repository",
            "key account repo_key hrn format mode config_version config_scope \
             config_settings created_at updated_at",
        ),
        (
            ASSET_TYPE,
            "asset",
            "key account repository asset_path package_name package_version \
             digest_algorithm digest size_bytes subject updated_at",
        ),
        (
            VERSION_TYPE,
            "version",
            "key account repository package_name package_version digest_algorithm \
             digest size_bytes published_at",
        ),
        (
            REFERENCE_TYPE,
            "reference",
            "key account repository asset_path digest_algorithm digest scope \
             size_bytes trust_state updated_at",
        ),
        (
            OUTBOX_TYPE,
            "outbox",
            "key tenant actor correlation_id action resource digest result \
             occurred_at state attempts lease_owner lease_until last_error delivered_at",
        ),
        (
            TAG_TYPE,
            "tag",
            "key account repository tag_name package_name package_version updated_at",
        ),
        (
            "CatalogAptSnapshot",
            "apt_snapshot",
            "key namespace revision digest payload",
        ),
        (
            "CatalogAptSnapshotRoot",
            "apt_snapshot_root",
            "key account repository subject digest_algorithm digest size_bytes",
        ),
        ("CatalogAptPointer", "apt_pointer", "key revision"),
        (
            "CatalogAptPackagesHash",
            "apt_packages_hash",
            "key snapshot_key namespace architecture digest revision",
        ),
    ] {
        ddl.push_str(&format!("type {name} {{\n"));
        for field in fields.split_whitespace() {
            ddl.push_str(&format!("hodei.catalog.{family}.{field}\n"));
        }
        ddl.push_str("}\n");
    }
    ddl
}

/// Escapes a part of a composite key: the `|` separator and the `@` of
/// the coordinates cannot appear as is inside a part, or two
/// different parts would produce the same key.
fn key_part(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('|', "%7C")
        .replace('@', "%40")
}

/// The composite key of a repository.
pub fn repository_key(account: &str, repo_key: &str) -> String {
    format!("{}|{}", key_part(account), key_part(repo_key))
}

/// The composite key of an asset (and of its reference: the reference is the
/// row that moves with the asset).
pub fn asset_key(account: &str, repository: &str, asset_path: &str) -> String {
    format!(
        "{}|{}|{}",
        key_part(account),
        key_part(repository),
        key_part(asset_path)
    )
}

/// The composite key of a published version.
pub fn version_key(
    account: &str,
    repository: &str,
    package_name: &str,
    package_version: &str,
) -> String {
    format!(
        "{}|{}|v|{}@{}",
        key_part(account),
        key_part(repository),
        key_part(package_name),
        key_part(package_version)
    )
}

/// The composite key of a mutable tag.
pub fn tag_key(account: &str, repository: &str, tag_name: &str) -> String {
    format!(
        "{}|{}|t|{}",
        key_part(account),
        key_part(repository),
        key_part(tag_name)
    )
}

/// The subset of a composite key that a guard subject may carry.
///
/// `GuardScope` accepts a narrow charset (ASCII alphanumerics and
/// `- _ . / : @`). Real asset paths carry more: Debian pool paths have `+`
/// and `~` (`1.2+dfsg`, `1.0.0~rc1`) and Go module paths carry the canonical
/// case escape of the GOPROXY protocol (`github.com/!azure/...`, where `!`
/// stands for an uppercase letter). Each of them was refused on Dgraph while
/// Surreal accepted it (the publication answered 400 "not well formed").
///
/// This escapes every byte outside the guard charset with `@` as the
/// sentinel (`@` doubles, any other byte becomes `@XX` with its uppercase
/// hex), so the output always sits inside the guard charset. The map is
/// injective: a verbatim `@` only ever appears as `@@`, so no plain string
/// can be confused with an escape and two different paths never inherit the
/// same guard. Stored composite keys are unchanged -- the escape lives only
/// on the guard subject -- and the guards of the paths that were already
/// accepted keep their subject (`+ ~ % |` map to the same escapes as before).
fn guard_part(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'@' => out.push_str("@@"),
            b if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/' | b':') => {
                out.push(char::from(b));
            }
            other => out.push_str(&format!("@{other:02X}")),
        }
    }
    out
}

/// The subject of the guard of a publication: the asset's path within
/// the account. Two writers of the same path --wherever the
/// code comes from-- derive the same subject and therefore fight over the same
/// guard; two different paths never collide.
fn asset_guard_scope(
    account: &str,
    repository: &str,
    asset_path: &str,
) -> Result<GuardScope, CatalogError> {
    scope_of(
        GuardKind::CatalogAsset,
        account,
        &format!(
            "asset/{}/{}",
            guard_part(repository),
            guard_part(asset_path)
        ),
    )
}

/// The subject of the guard of a tag: the tag's key within
/// the account. Two writers moving the same tag --to the same
/// version or to different versions-- fight over THIS guard, and the commit
/// that the graph arbitrates last is the one that wins.
fn tag_guard_scope(
    account: &str,
    repository: &str,
    tag_name: &str,
) -> Result<GuardScope, CatalogError> {
    scope_of(
        GuardKind::Tag,
        account,
        &format!("tag/{repository}/{tag_name}"),
    )
}

/// The npm settings of a repository, as the Surreal
/// reference validates them (HSP-111): only an npm repository declares them, and
/// the ones it declares have to be understood. The Dgraph slice shares the
/// same creation contract, so that the same descriptor is valid in both
/// engines (HSP-193).
fn the_npm_of(format: RepositoryFormat, config: &RepositoryConfig) -> Result<(), CatalogError> {
    if format != RepositoryFormat::Npm {
        if config.config_settings.get("npm").is_some() {
            return Err(CatalogError::invalid(
                "only an npm repository can declare config_settings.npm",
            ));
        }
        return Ok(());
    }
    hodei_catalog::npm::NpmConfig::of_settings(&config.config_settings)
        .map(|_| ())
        .map_err(CatalogError::invalid)
}

/// The configuration of a repository, seen from its mode, as the
/// Surreal reference validates it (HSP-112): a proxy without a valid upstream is not
/// created, and a `proxy` in another mode is an error, not a dormant setting.
/// The Dgraph slice shares the same creation contract so that a PyPI
/// descriptor --the one that exercises the upstream in the hosted/proxy
/// of HSP-194-- is valid in both engines (HSP-178).
fn the_upstream_of(mode: RepositoryMode, config: &RepositoryConfig) -> Result<(), CatalogError> {
    if mode != RepositoryMode::Proxy {
        if config.config_settings.get("proxy").is_some() {
            return Err(CatalogError::invalid(
                "only a proxy repository can declare config_settings.proxy",
            ));
        }
        return Ok(());
    }
    match hodei_catalog::UpstreamConfig::of(config) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(CatalogError::invalid(
            "a proxy repository has to declare its upstream in config_settings.proxy.upstream",
        )),
        Err(error) => Err(CatalogError::invalid(error.to_string())),
    }
}

/// The subject of the guard of a repository creation.
fn repository_guard_scope(account: &str, repo_key: &str) -> Result<GuardScope, CatalogError> {
    scope_of(
        GuardKind::CatalogRepository,
        account,
        &format!("repository/{repo_key}"),
    )
}

/// Validates the subject of a guard: a key that the guard cannot
/// protect (the separator, spaces, control characters) is not a
/// key this slice publishes.
fn scope_of(kind: GuardKind, tenant: &str, subject: &str) -> Result<GuardScope, CatalogError> {
    let tenant = Tenant::new(tenant).map_err(|error| {
        CatalogError::invalid(format!("the account is not a guard tenant: {error}"))
    })?;
    GuardScope::new(kind, &tenant, subject)
        .map_err(|error| CatalogError::invalid(format!("the key is not publishable here: {error}")))
}

/// The deterministic operation identifier of a publication: the same
/// fact said twice carries the same identifier, so the
/// retry after the commit re-reads the committed result instead of
/// writing a second time.
fn publication_operation_id(
    account: &str,
    publication: &Publication,
    kind: &WriteKind,
) -> OperationId {
    let mut hasher = Sha256::new();
    // The account enters the fact: the same path in two accounts is two
    // different facts, and a once-only marker is per graph.
    hasher.update(account.as_bytes());
    hasher.update([0]);
    hasher.update(publication.repository.as_bytes());
    hasher.update([0]);
    hasher.update(publication.asset_path.as_bytes());
    hasher.update([0]);
    hasher.update(publication.coordinates.package_name.as_bytes());
    hasher.update([0]);
    hasher.update(publication.coordinates.package_version.as_bytes());
    hasher.update([0]);
    hasher.update(publication.digest.algorithm.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(publication.digest.value.as_bytes());
    hasher.update([0]);
    hasher.update(publication.size_bytes.to_le_bytes());
    hasher.update([0]);
    hasher.update(if publication.overwrite { b"1" } else { b"0" });
    hasher.update([0]);
    hasher.update(publication.correlation_id.as_bytes());
    // A replace is a compare-and-set over the pointer it read: the same
    // content moved over ANOTHER previous pointer is another fact (the
    // marker of an attempt that skipped because the pointer had moved must
    // not replay for the attempt that re-read it).
    if let WriteKind::Replace { previous } = kind {
        hasher.update([0]);
        hasher.update(b"replace");
        hasher.update([0]);
        hasher.update(previous.as_bytes());
    }
    let id = format!("pub-{:x}", hasher.finalize());
    OperationId::new(&id).expect("a sha256-derived operation id is always valid")
}

fn repository_config_operation_id(
    account: &str,
    repo_key: &str,
    previous_version: u64,
    config: &RepositoryConfig,
) -> OperationId {
    let mut hasher = Sha256::new();
    hasher.update(account.as_bytes());
    hasher.update([0]);
    hasher.update(repo_key.as_bytes());
    hasher.update(previous_version.to_le_bytes());
    hasher.update([0]);
    hasher.update(scope_name(config.digest_scope).as_bytes());
    hasher.update([0]);
    hasher.update(config.config_settings.to_string().as_bytes());
    OperationId::new(&format!("cfg-{:x}", hasher.finalize()))
        .expect("a sha256-derived operation id is always valid")
}

/// The deterministic operation identifier of a repository creation.
fn repository_operation_id(descriptor: &RepositoryDescriptor) -> OperationId {
    let mut hasher = Sha256::new();
    hasher.update(descriptor.account.as_bytes());
    hasher.update([0]);
    hasher.update(descriptor.repo_key.as_bytes());
    hasher.update([0]);
    hasher.update(descriptor.format.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(descriptor.mode.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(
        match descriptor.config.digest_scope {
            DigestScope::Global => "global",
            DigestScope::AccountPartitioned => "account_partitioned",
        }
        .as_bytes(),
    );
    hasher.update([0]);
    hasher.update(descriptor.config.config_settings.to_string().as_bytes());
    let id = format!("repo-{:x}", hasher.finalize());
    OperationId::new(&id).expect("a sha256-derived operation id is always valid")
}

/// The deterministic operation identifier of a tag
/// move: the same tag moved to the SAME version is the same fact
/// (the retry after the timeout re-reads the committed result), and the
/// same tag moved to ANOTHER version is another fact (the last commit
/// wins, and both are applied in the order in which the graph arbitrated them).
fn tag_operation_id(
    account: &str,
    repository: &str,
    tag_name: &str,
    coordinates: &PackageCoordinates,
) -> OperationId {
    let mut hasher = Sha256::new();
    hasher.update(account.as_bytes());
    hasher.update([0]);
    hasher.update(repository.as_bytes());
    hasher.update([0]);
    hasher.update(tag_name.as_bytes());
    hasher.update([0]);
    hasher.update(coordinates.package_name.as_bytes());
    hasher.update([0]);
    hasher.update(coordinates.package_version.as_bytes());
    let id = format!("tag-{:x}", hasher.finalize());
    OperationId::new(&id).expect("a sha256-derived operation id is always valid")
}

// ---------------------------------------------------------------------------
// Reading rows.
// ---------------------------------------------------------------------------

/// The fields read by a repository row.
const REPOSITORY_FIELDS: &str = "hodei.catalog.repository.key hodei.catalog.repository.account \
     hodei.catalog.repository.repo_key hodei.catalog.repository.hrn hodei.catalog.repository.format \
     hodei.catalog.repository.mode hodei.catalog.repository.config_version \
     hodei.catalog.repository.config_scope hodei.catalog.repository.config_settings \
     hodei.catalog.repository.created_at hodei.catalog.repository.updated_at";

/// The fields read by an asset row.
const ASSET_FIELDS: &str = "hodei.catalog.asset.key hodei.catalog.asset.account \
     hodei.catalog.asset.repository hodei.catalog.asset.asset_path \
     hodei.catalog.asset.package_name hodei.catalog.asset.package_version \
     hodei.catalog.asset.digest_algorithm hodei.catalog.asset.digest \
     hodei.catalog.asset.size_bytes hodei.catalog.asset.subject hodei.catalog.asset.updated_at";

/// The fields read by a version row.
const VERSION_FIELDS: &str = "hodei.catalog.version.key hodei.catalog.version.account \
     hodei.catalog.version.repository hodei.catalog.version.package_name \
     hodei.catalog.version.package_version hodei.catalog.version.digest_algorithm \
     hodei.catalog.version.digest hodei.catalog.version.size_bytes \
     hodei.catalog.version.published_at";

/// The fields read by a reference row.
const REFERENCE_FIELDS: &str = "hodei.catalog.reference.key hodei.catalog.reference.account \
     hodei.catalog.reference.repository hodei.catalog.reference.asset_path \
     hodei.catalog.reference.digest_algorithm hodei.catalog.reference.digest \
     hodei.catalog.reference.scope hodei.catalog.reference.size_bytes \
     hodei.catalog.reference.trust_state hodei.catalog.reference.updated_at";

/// The fields read by an outbox row.
const OUTBOX_FIELDS: &str = "hodei.catalog.outbox.key hodei.catalog.outbox.tenant \
     hodei.catalog.outbox.actor hodei.catalog.outbox.correlation_id hodei.catalog.outbox.action \
     hodei.catalog.outbox.resource hodei.catalog.outbox.digest hodei.catalog.outbox.result \
     hodei.catalog.outbox.occurred_at hodei.catalog.outbox.state hodei.catalog.outbox.attempts \
     hodei.catalog.outbox.lease_owner hodei.catalog.outbox.lease_until \
     hodei.catalog.outbox.last_error hodei.catalog.outbox.delivered_at";

/// Reads a text from a node, or reports it as corruption.
fn node_text(node: &serde_json::Value, field: &str) -> Result<String, CatalogError> {
    node.get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CatalogError::corrupt("row", field, format!("the node has no '{field}'")))
}

fn node_u64(node: &serde_json::Value, field: &str) -> Result<u64, CatalogError> {
    node.get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| CatalogError::corrupt("row", field, format!("the node has no '{field}'")))
}

fn node_time(node: &serde_json::Value, field: &str) -> Result<DateTime<Utc>, CatalogError> {
    let text = node_text(node, field)?;
    DateTime::parse_from_rfc3339(&text)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| CatalogError::corrupt("row", field, format!("'{text}' is not a timestamp")))
}

fn node_digest(
    node: &serde_json::Value,
    algorithm_field: &str,
    value_field: &str,
) -> Result<ContentDigest, CatalogError> {
    let algorithm = match node_text(node, algorithm_field)?.as_str() {
        "sha256" => DigestAlgorithm::Sha256,
        "sha512" => DigestAlgorithm::Sha512,
        other => {
            return Err(CatalogError::corrupt(
                "row",
                algorithm_field,
                format!("'{other}' is not a digest algorithm"),
            ));
        }
    };
    Ok(ContentDigest {
        algorithm,
        value: node_text(node, value_field)?,
    })
}

/// The row of a repository as it is in the graph.
#[derive(Clone)]
struct RepositoryRow {
    account: String,
    repo_key: String,
    hrn: String,
    format: String,
    mode: String,
    config_version: u64,
    config_scope: String,
    config_settings: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl RepositoryRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        Ok(Self {
            account: node_text(node, "hodei.catalog.repository.account")?,
            repo_key: node_text(node, "hodei.catalog.repository.repo_key")?,
            hrn: node_text(node, "hodei.catalog.repository.hrn")?,
            format: node_text(node, "hodei.catalog.repository.format")?,
            mode: node_text(node, "hodei.catalog.repository.mode")?,
            config_version: node_u64(node, "hodei.catalog.repository.config_version")?,
            config_scope: node_text(node, "hodei.catalog.repository.config_scope")?,
            config_settings: node_text(node, "hodei.catalog.repository.config_settings")?,
            created_at: node_time(node, "hodei.catalog.repository.created_at")?,
            updated_at: node_time(node, "hodei.catalog.repository.updated_at")?,
        })
    }

    /// The row's descriptor, with the identity checked against what
    /// the key says they are.
    fn into_descriptor(
        self,
        account: &str,
        repo_key: &str,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        if self.account != account || self.repo_key != repo_key {
            return Err(CatalogError::corrupt(
                "repository",
                format!("{account}/{repo_key}"),
                "the row does not carry the identity its key names",
            ));
        }
        let format = RepositoryFormat::parse(&self.format).ok_or_else(|| {
            CatalogError::corrupt(
                "repository",
                format!("{account}/{repo_key}"),
                format!("'{}' is not a format", self.format),
            )
        })?;
        let mode = RepositoryMode::parse(&self.mode).ok_or_else(|| {
            CatalogError::corrupt(
                "repository",
                format!("{account}/{repo_key}"),
                format!("'{}' is not a mode", self.mode),
            )
        })?;
        let scope = match self.config_scope.as_str() {
            "global" => DigestScope::Global,
            "account_partitioned" => DigestScope::AccountPartitioned,
            other => {
                return Err(CatalogError::corrupt(
                    "repository",
                    format!("{account}/{repo_key}"),
                    format!("'{other}' is not a digest scope"),
                ));
            }
        };
        let settings: serde_json::Value = serde_json::from_str(&self.config_settings)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        let hrn = kernel::Hrn::from_string(&self.hrn).ok_or_else(|| {
            CatalogError::corrupt(
                "repository",
                format!("{account}/{repo_key}"),
                format!("'{}' is not an HRN", self.hrn),
            )
        })?;
        Ok(RepositoryDescriptor {
            hrn,
            account: self.account,
            repo_key: self.repo_key,
            format,
            mode,
            config: RepositoryConfig {
                version: self.config_version,
                digest_scope: scope,
                config_settings: settings,
            },
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// The row of a published version.
#[derive(Clone)]
struct VersionRow {
    account: String,
    repository: String,
    package_name: String,
    package_version: String,
    digest: ContentDigest,
    size_bytes: u64,
    published_at: DateTime<Utc>,
}

impl VersionRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        Ok(Self {
            account: node_text(node, "hodei.catalog.version.account")?,
            repository: node_text(node, "hodei.catalog.version.repository")?,
            package_name: node_text(node, "hodei.catalog.version.package_name")?,
            package_version: node_text(node, "hodei.catalog.version.package_version")?,
            digest: node_digest(
                node,
                "hodei.catalog.version.digest_algorithm",
                "hodei.catalog.version.digest",
            )?,
            size_bytes: node_u64(node, "hodei.catalog.version.size_bytes")?,
            published_at: node_time(node, "hodei.catalog.version.published_at")?,
        })
    }

    fn into_package_version(self) -> PackageVersion {
        PackageVersion {
            account: self.account,
            repository: self.repository,
            coordinates: PackageCoordinates {
                package_name: self.package_name,
                package_version: self.package_version,
            },
            digest: self.digest,
            size_bytes: self.size_bytes,
            published_at: self.published_at,
        }
    }
}

/// The row of an asset.
#[derive(Clone)]
struct AssetRow {
    account: String,
    repository: String,
    asset_path: String,
    package_name: String,
    package_version: String,
    digest: ContentDigest,
    size_bytes: u64,
    subject: Option<String>,
    updated_at: DateTime<Utc>,
}

impl AssetRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        let subject = node
            .get("hodei.catalog.asset.subject")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        Ok(Self {
            account: node_text(node, "hodei.catalog.asset.account")?,
            repository: node_text(node, "hodei.catalog.asset.repository")?,
            asset_path: node_text(node, "hodei.catalog.asset.asset_path")?,
            package_name: node_text(node, "hodei.catalog.asset.package_name")?,
            package_version: node_text(node, "hodei.catalog.asset.package_version")?,
            digest: node_digest(
                node,
                "hodei.catalog.asset.digest_algorithm",
                "hodei.catalog.asset.digest",
            )?,
            size_bytes: node_u64(node, "hodei.catalog.asset.size_bytes")?,
            subject,
            updated_at: node_time(node, "hodei.catalog.asset.updated_at")?,
        })
    }

    /// The asset's record, with the identity checked against what
    /// the key says: a row that is not named like its key is broken.
    fn into_record(
        self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<AssetRecord, CatalogError> {
        if self.account != account || self.repository != repository || self.asset_path != asset_path
        {
            return Err(CatalogError::corrupt(
                "asset",
                format!("{repository}/{asset_path}"),
                "the row does not carry the identity its key names",
            ));
        }
        let subject = self.subject.map(|value| ContentDigest {
            algorithm: DigestAlgorithm::Sha256,
            value,
        });
        Ok(AssetRecord {
            account: self.account,
            repository: self.repository,
            asset_path: self.asset_path,
            coordinates: PackageCoordinates {
                package_name: self.package_name,
                package_version: self.package_version,
            },
            digest: self.digest,
            size_bytes: self.size_bytes,
            subject,
            updated_at: self.updated_at,
        })
    }
}

/// The row of a live reference to a blob.
#[derive(Clone)]
struct ReferenceRow {
    digest: ContentDigest,
    scope: DigestScope,
    size_bytes: u64,
    trust_state: TrustState,
    updated_at: DateTime<Utc>,
}

impl ReferenceRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        let scope = match node_text(node, "hodei.catalog.reference.scope")?.as_str() {
            "global" => DigestScope::Global,
            "account_partitioned" => DigestScope::AccountPartitioned,
            other => {
                return Err(CatalogError::corrupt(
                    "reference",
                    "scope",
                    format!("'{other}' is not a digest scope"),
                ));
            }
        };
        Ok(Self {
            digest: node_digest(
                node,
                "hodei.catalog.reference.digest_algorithm",
                "hodei.catalog.reference.digest",
            )?,
            scope,
            size_bytes: node_u64(node, "hodei.catalog.reference.size_bytes")?,
            trust_state: TrustState::of(&node_text(node, "hodei.catalog.reference.trust_state")?),
            updated_at: node_time(node, "hodei.catalog.reference.updated_at")?,
        })
    }

    fn into_reference(
        self,
        account: String,
        repository: String,
        asset_path: String,
    ) -> BlobReference {
        BlobReference {
            account,
            repository,
            asset_path,
            digest: self.digest,
            scope: self.scope,
            size_bytes: self.size_bytes,
            trust_state: self.trust_state,
            updated_at: self.updated_at,
        }
    }
}

/// The row of a mutable tag as it is in the graph.
struct TagRow {
    account: String,
    repository: String,
    tag_name: String,
    package_name: String,
    package_version: String,
    updated_at: DateTime<Utc>,
}

impl TagRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        Ok(Self {
            account: node_text(node, "hodei.catalog.tag.account")?,
            repository: node_text(node, "hodei.catalog.tag.repository")?,
            tag_name: node_text(node, "hodei.catalog.tag.tag_name")?,
            package_name: node_text(node, "hodei.catalog.tag.package_name")?,
            package_version: node_text(node, "hodei.catalog.tag.package_version")?,
            updated_at: node_time(node, "hodei.catalog.tag.updated_at")?,
        })
    }

    /// The tag, with the identity checked against what the key
    /// says they are: a row that does not carry the identity its key names
    /// is not an answer, it is corruption.
    fn into_tag(
        self,
        account: &str,
        repository: &str,
        tag_name: &str,
    ) -> Result<MutableTag, CatalogError> {
        if self.account != account || self.repository != repository || self.tag_name != tag_name {
            return Err(CatalogError::corrupt(
                "tag",
                format!("{account}/{repository}/{tag_name}"),
                "the row does not carry the identity its key names",
            ));
        }
        Ok(MutableTag {
            account: self.account,
            repository: self.repository,
            tag_name: self.tag_name,
            coordinates: PackageCoordinates {
                package_name: self.package_name,
                package_version: self.package_version,
            },
            updated_at: self.updated_at,
        })
    }
}

// ---------------------------------------------------------------------------
// The state of a publication, pre-read in one question.
// ---------------------------------------------------------------------------

/// What is in the graph for a publication.
struct PublicationState {
    repository: Option<RepositoryRow>,
    version: Option<VersionRow>,
    asset: Option<AssetRow>,
    reference: Option<ReferenceRow>,
    event: Option<OutboxRow>,
}

/// Reads the state of a publication in a single question: repository,
/// version, asset, reference and the row of the event the publication is going
/// to say. The event is read here so that a row already said with other
/// content is discovered **before** writing anything.
async fn read_publication_state(
    client: &Arc<GraphClient>,
    account: &str,
    publication: &Publication,
    event_id: &str,
) -> Result<PublicationState, CatalogError> {
    let repo = repository_key(account, &publication.repository);
    let version = version_key(
        account,
        &publication.repository,
        &publication.coordinates.package_name,
        &publication.coordinates.package_version,
    );
    let asset = asset_key(account, &publication.repository, &publication.asset_path);
    let query = format!(
        "{{\n \
         r(func: eq(hodei.catalog.repository.key, {})) {{ {REPOSITORY_FIELDS} }}\n \
         v(func: eq(hodei.catalog.version.key, {})) {{ {VERSION_FIELDS} }}\n \
         a(func: eq(hodei.catalog.asset.key, {})) {{ {ASSET_FIELDS} }}\n \
         rf(func: eq(hodei.catalog.reference.key, {})) {{ {REFERENCE_FIELDS} }}\n \
         ev(func: eq(hodei.catalog.outbox.key, {})) {{ {OUTBOX_FIELDS} }}\n \
         }}",
        dql_string(&repo),
        dql_string(&version),
        dql_string(&asset),
        dql_string(&asset),
        dql_string(event_id),
    );
    let response = client
        .execute_dql(&query)
        .await
        .map_err(|error| CatalogError::unavailable(error.to_string()))?;
    let node_of = |root: &str| -> Result<Option<serde_json::Value>, CatalogError> {
        let nodes = response
            .json
            .get(root)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            0 => Ok(None),
            1 => Ok(Some(nodes[0].clone())),
            n => Err(CatalogError::corrupt(
                "row",
                root,
                format!("{n} rows carry the same key"),
            )),
        }
    };
    let repository = node_of("r")?
        .map(|node| RepositoryRow::from_node(&node))
        .transpose()?;
    let version = node_of("v")?
        .map(|node| VersionRow::from_node(&node))
        .transpose()?;
    let asset = node_of("a")?
        .map(|node| AssetRow::from_node(&node))
        .transpose()?;
    let reference = node_of("rf")?
        .map(|node| ReferenceRow::from_node(&node))
        .transpose()?;
    let event = node_of("ev")?
        .map(|node| OutboxRow::from_node(&node))
        .transpose()?;
    Ok(PublicationState {
        repository,
        version,
        asset,
        reference,
        event,
    })
}

/// What the publication has to do, decided from the state: the same
/// function before the transaction (to decide) and after the commit
/// (to answer), because the answer comes from the state and not from the
/// hope that the transaction committed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WriteKind {
    /// The path is empty: create asset, version (if missing), reference and
    /// event.
    Create,
    /// The path points to other content and the publication replaces it: the
    /// pointer moves, the previous version is not touched.
    Replace {
        /// The digest the path points to now.
        previous: String,
    },
    /// The path already points to this content: repair what is missing
    /// (reference, version) without saying a new fact.
    Repair,
}

/// The publication judged over the state AFTER the commit. The
/// client's precondition was already arbitrated by the transaction: the pre-read
/// decided it and the graph's guard enforced it at commit, so the
/// later state satisfies it BY CONSTRUCTION -- an `Absent` that was born
/// of an empty path, or a `Present` of the digest that the commit itself
/// moved. Judging it again would say that the write that just fulfilled
/// the condition violates it, and the publication that settled would come out as a
/// precondition failure (HSP-193, the npm consumer of the packument
/// pointer was the first to stumble on it).
fn the_settled_publication(publication: &Publication) -> Publication {
    Publication {
        precondition: None,
        ..publication.clone()
    }
}

/// The plan decided over a publication state.
struct PublishPlan {
    /// What has to be written, if anything has to be written.
    write: Option<WriteKind>,
    /// Everything the publication declares is already there: nothing is written nor
    /// is a new fact said.
    complete: bool,
    /// There is already something there with other content.
    conflict: Option<CatalogError>,
    /// The condition the client demanded is not met.
    precondition_failure: Option<CatalogError>,
}

impl PublishPlan {
    /// Decides the plan over the state read. The logic is that of the Surreal
    /// oracle: the version conflict decides first (a published
    /// version does not change), then the path one (a path that points to
    /// something does not move on its own), then the precondition, and only then
    /// is the write planned. The missing repository is checked by whoever
    /// calls, before getting here.
    fn decide(state: &PublicationState, publication: &Publication) -> Self {
        // The version is immutable: if it is there with other content, nothing is
        // written and the conflict is reported.
        if let Some(version) = state.version.as_ref() {
            let same = version.digest == publication.digest
                && version.size_bytes == publication.size_bytes;
            if !same {
                return Self {
                    conflict: Some(CatalogError::Conflict {
                        kind: "version",
                        key: format!(
                            "{}/{}@{}",
                            publication.repository,
                            publication.coordinates.package_name,
                            publication.coordinates.package_version
                        ),
                        detail: "a published version does not change: publish another version"
                            .to_owned(),
                    }),
                    ..Self::empty()
                };
            }
        }

        match state.asset.as_ref() {
            None => {
                let precondition_failure = the_precondition_failure(publication, None);
                if let Some(failure) = precondition_failure {
                    return Self {
                        precondition_failure: Some(failure),
                        ..Self::empty()
                    };
                }
                Self {
                    write: Some(WriteKind::Create),
                    ..Self::empty()
                }
            }
            Some(asset) => {
                let same_digest = asset.digest == publication.digest
                    && asset.size_bytes == publication.size_bytes;
                if same_digest {
                    // The path already points to this content: the publication
                    // is the same thing said twice. The precondition is
                    // checked against what is there.
                    if let Some(failure) =
                        the_precondition_failure(publication, Some(&asset.digest))
                    {
                        return Self {
                            precondition_failure: Some(failure),
                            ..Self::empty()
                        };
                    }
                    if state.version.is_some() && state.reference.is_some() {
                        Self {
                            complete: true,
                            ..Self::empty()
                        }
                    } else {
                        Self {
                            write: Some(WriteKind::Repair),
                            ..Self::empty()
                        }
                    }
                } else if !publication.overwrite {
                    // The path already points to something else and nobody asked to
                    // replace it: the path already says something.
                    Self {
                        conflict: Some(CatalogError::Conflict {
                            kind: "asset",
                            key: format!("{}/{}", publication.repository, publication.asset_path),
                            detail: "the path already points at other content".to_owned(),
                        }),
                        ..Self::empty()
                    }
                } else if let Some(failure) =
                    the_precondition_failure(publication, Some(&asset.digest))
                {
                    Self {
                        precondition_failure: Some(failure),
                        ..Self::empty()
                    }
                } else {
                    Self {
                        write: Some(WriteKind::Replace {
                            previous: asset.digest.value.clone(),
                        }),
                        ..Self::empty()
                    }
                }
            }
        }
    }

    fn empty() -> Self {
        Self {
            write: None,
            complete: false,
            conflict: None,
            precondition_failure: None,
        }
    }
}

/// The condition the client demanded, decided against the digest that is
/// at the path (or nothing, if the path is empty).
fn the_precondition_failure(
    publication: &Publication,
    current: Option<&ContentDigest>,
) -> Option<CatalogError> {
    let key = format!("{}/{}", publication.repository, publication.asset_path);
    let failure = |detail: String| {
        Some(CatalogError::PreconditionFailed {
            kind: "asset",
            key: key.clone(),
            detail,
        })
    };
    match &publication.precondition {
        None => None,
        Some(Precondition::Absent) => {
            if current.is_none() {
                None
            } else {
                failure("the path must be empty, and it already points somewhere".to_owned())
            }
        }
        Some(Precondition::Exists) => {
            if current.is_some() {
                None
            } else {
                failure("the path must point somewhere, and it is empty".to_owned())
            }
        }
        Some(Precondition::Present(digests)) => {
            let Some(current) = current else {
                return failure(
                    "the path must point at one of the requested contents, and it is empty"
                        .to_owned(),
                );
            };
            if digests.iter().any(|digest| digest == current) {
                None
            } else {
                failure(
                    "the path points at content that is not one of the requested digests"
                        .to_owned(),
                )
            }
        }
        Some(Precondition::Other(digests)) => {
            let current = current?;
            if digests.iter().any(|digest| digest == current) {
                failure("the path points at one of the contents the write forbids".to_owned())
            } else {
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The N-Quads of each row.
// ---------------------------------------------------------------------------

/// The name of the scope, as it goes to the database.
fn scope_name(scope: DigestScope) -> &'static str {
    match scope {
        DigestScope::Global => "global",
        DigestScope::AccountPartitioned => "account_partitioned",
    }
}

/// The result the operation records in its marker: what a replay
/// returns as an already-committed answer.
fn the_recorded_result(kind: &WriteKind) -> String {
    match kind {
        WriteKind::Create => "created".to_owned(),
        WriteKind::Replace { .. } => "replaced".to_owned(),
        WriteKind::Repair => "repaired".to_owned(),
    }
}

/// The audit event as an outbox row, in the SAME transaction
/// as the catalog rows.
fn outbox_event_nquads(event: &AuditEvent) -> String {
    format!(
        "_:evn <dgraph.type> \"{OUTBOX_TYPE}\" .\n\
         _:evn <hodei.catalog.outbox.key> {key} .\n\
         _:evn <hodei.catalog.outbox.tenant> {tenant} .\n\
         _:evn <hodei.catalog.outbox.actor> {actor} .\n\
         _:evn <hodei.catalog.outbox.correlation_id> {correlation} .\n\
         _:evn <hodei.catalog.outbox.action> {action} .\n\
         _:evn <hodei.catalog.outbox.resource> {resource} .\n\
         _:evn <hodei.catalog.outbox.digest> {digest} .\n\
         _:evn <hodei.catalog.outbox.result> \"{result}\" .\n\
         _:evn <hodei.catalog.outbox.occurred_at> \"{occurred}\" .\n\
         _:evn <hodei.catalog.outbox.state> \"pending\" .\n\
         _:evn <hodei.catalog.outbox.attempts> \"0\" .\n",
        key = dql_string(&event.event_id),
        tenant = dql_string(event.tenant()),
        actor = dql_string(event.actor()),
        correlation = dql_string(event.correlation_id()),
        action = dql_string(&event.action),
        resource = dql_string(&event.resource),
        digest = dql_string(event.digest.as_deref().unwrap_or_default()),
        result = event.result,
        occurred = event.timestamp.to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}

/// The asset row that is born with the publication.
fn asset_create_nquads(account: &str, publication: &Publication, now: &str) -> String {
    let subject = publication
        .subject
        .as_ref()
        .map(|digest| {
            format!(
                "_:an <hodei.catalog.asset.subject> {} .\n",
                dql_string(&digest.value)
            )
        })
        .unwrap_or_default();
    format!(
        "_:an <dgraph.type> \"{ASSET_TYPE}\" .\n\
         _:an <hodei.catalog.asset.key> {key} .\n\
         _:an <hodei.catalog.asset.account> {account} .\n\
         _:an <hodei.catalog.asset.repository> {repository} .\n\
         _:an <hodei.catalog.asset.asset_path> {path} .\n\
         _:an <hodei.catalog.asset.package_name> {name} .\n\
         _:an <hodei.catalog.asset.package_version> {version} .\n\
         _:an <hodei.catalog.asset.digest_algorithm> \"{algorithm}\" .\n\
         _:an <hodei.catalog.asset.digest> {digest} .\n\
         _:an <hodei.catalog.asset.size_bytes> \"{size}\" .\n\
         _:an <hodei.catalog.asset.updated_at> \"{now}\" .\n\
         {subject}",
        key = dql_string(&asset_key(
            account,
            &publication.repository,
            &publication.asset_path
        )),
        account = dql_string(account),
        repository = dql_string(&publication.repository),
        path = dql_string(&publication.asset_path),
        name = dql_string(&publication.coordinates.package_name),
        version = dql_string(&publication.coordinates.package_version),
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        size = publication.size_bytes,
    )
}

/// The version row that is born with the publication.
fn version_create_nquads(account: &str, publication: &Publication, now: &str) -> String {
    format!(
        "_:vn <dgraph.type> \"{VERSION_TYPE}\" .\n\
         _:vn <hodei.catalog.version.key> {key} .\n\
         _:vn <hodei.catalog.version.account> {account} .\n\
         _:vn <hodei.catalog.version.repository> {repository} .\n\
         _:vn <hodei.catalog.version.package_name> {name} .\n\
         _:vn <hodei.catalog.version.package_version> {version} .\n\
         _:vn <hodei.catalog.version.digest_algorithm> \"{algorithm}\" .\n\
         _:vn <hodei.catalog.version.digest> {digest} .\n\
         _:vn <hodei.catalog.version.size_bytes> \"{size}\" .\n\
         _:vn <hodei.catalog.version.published_at> \"{now}\" .\n",
        key = dql_string(&version_key(
            account,
            &publication.repository,
            &publication.coordinates.package_name,
            &publication.coordinates.package_version,
        )),
        account = dql_string(account),
        repository = dql_string(&publication.repository),
        name = dql_string(&publication.coordinates.package_name),
        version = dql_string(&publication.coordinates.package_version),
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        size = publication.size_bytes,
        now = now,
    )
}

/// The row of the blob reference that is born with the publication. The bytes
/// live in the CAS; the reference only says where they are.
fn reference_create_nquads(
    account: &str,
    publication: &Publication,
    scope: DigestScope,
    now: &str,
) -> String {
    format!(
        "_:rn <dgraph.type> \"{REFERENCE_TYPE}\" .\n\
         _:rn <hodei.catalog.reference.key> {key} .\n\
         _:rn <hodei.catalog.reference.account> {account} .\n\
         _:rn <hodei.catalog.reference.repository> {repository} .\n\
         _:rn <hodei.catalog.reference.asset_path> {path} .\n\
         _:rn <hodei.catalog.reference.digest_algorithm> \"{algorithm}\" .\n\
         _:rn <hodei.catalog.reference.digest> {digest} .\n\
         _:rn <hodei.catalog.reference.scope> \"{scope}\" .\n\
         _:rn <hodei.catalog.reference.size_bytes> \"{size}\" .\n\
         _:rn <hodei.catalog.reference.trust_state> \"quarantined\" .\n\
         _:rn <hodei.catalog.reference.updated_at> \"{now}\" .\n",
        key = dql_string(&asset_key(
            account,
            &publication.repository,
            &publication.asset_path
        )),
        account = dql_string(account),
        repository = dql_string(&publication.repository),
        path = dql_string(&publication.asset_path),
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        scope = scope_name(scope),
        size = publication.size_bytes,
    )
}

/// The row of a tag that is born: the version it points to already exists
/// (the caller checks it), and the key's @upsert index ensures that a
/// repeated creation does not duplicate the row.
fn tag_create_nquads(
    account: &str,
    repository: &str,
    tag_name: &str,
    coordinates: &PackageCoordinates,
    now: &str,
) -> String {
    format!(
        "_:tn <dgraph.type> \"{TAG_TYPE}\" .\n\
         _:tn <hodei.catalog.tag.key> {key} .\n\
         _:tn <hodei.catalog.tag.account> {account} .\n\
         _:tn <hodei.catalog.tag.repository> {repository} .\n\
         _:tn <hodei.catalog.tag.tag_name> {name} .\n\
         _:tn <hodei.catalog.tag.package_name> {package} .\n\
         _:tn <hodei.catalog.tag.package_version> {version} .\n\
         _:tn <hodei.catalog.tag.updated_at> \"{now}\" .\n",
        key = dql_string(&tag_key(account, repository, tag_name)),
        account = dql_string(account),
        repository = dql_string(repository),
        name = dql_string(tag_name),
        package = dql_string(&coordinates.package_name),
        version = dql_string(&coordinates.package_version),
        now = now,
    )
}

/// The move of the tag's row: only the version it points to and the moment
/// change. Neither the version nor the asset nor the referrer are
/// touched: the tag is a reference, not the content.
fn tag_move_nquads(coordinates: &PackageCoordinates, now: &str) -> String {
    format!(
        "uid(tn) <hodei.catalog.tag.package_name> {package} .\n\
         uid(tn) <hodei.catalog.tag.package_version> {version} .\n\
         uid(tn) <hodei.catalog.tag.updated_at> \"{now}\" .\n",
        package = dql_string(&coordinates.package_name),
        version = dql_string(&coordinates.package_version),
        now = now,
    )
}

/// The row of a repository that is born.
fn repository_create_nquads(descriptor: &RepositoryDescriptor, now: &str) -> String {
    format!(
        "_:rn <dgraph.type> \"{REPOSITORY_TYPE}\" .\n\
         _:rn <hodei.catalog.repository.key> {key} .\n\
         _:rn <hodei.catalog.repository.account> {account} .\n\
         _:rn <hodei.catalog.repository.repo_key> {repo_key} .\n\
         _:rn <hodei.catalog.repository.hrn> {hrn} .\n\
         _:rn <hodei.catalog.repository.format> \"{format}\" .\n\
         _:rn <hodei.catalog.repository.mode> \"{mode}\" .\n\
         _:rn <hodei.catalog.repository.config_version> \"{version}\" .\n\
         _:rn <hodei.catalog.repository.config_scope> \"{scope}\" .\n\
         _:rn <hodei.catalog.repository.config_settings> {settings} .\n\
         _:rn <hodei.catalog.repository.created_at> \"{now}\" .\n\
         _:rn <hodei.catalog.repository.updated_at> \"{now}\" .\n",
        key = dql_string(&repository_key(&descriptor.account, &descriptor.repo_key)),
        account = dql_string(&descriptor.account),
        repo_key = dql_string(&descriptor.repo_key),
        hrn = dql_string(&descriptor.hrn.to_string()),
        format = descriptor.format.as_str(),
        mode = descriptor.mode.as_str(),
        version = descriptor.config.version,
        scope = scope_name(descriptor.config.digest_scope),
        settings = dql_string(&descriptor.config.config_settings.to_string()),
        now = now,
    )
}

/// The parent's gate of each block: the same one the guard uses, so
/// that a write whose pre-read became stale writes nothing.
fn parent_gate(current: u64) -> &'static str {
    if current == 0 {
        "eq(len(op), 0) AND eq(len(g), 0)"
    } else {
        "eq(len(op), 0) AND eq(len(gv), 1)"
    }
}

/// The query lines of a publication: they bind exactly the
/// variables the blocks use, because DQL rejects variables that
/// nobody names.
fn the_effect_query_lines(
    kind: &WriteKind,
    event: &AuditEvent,
    event_said: bool,
    account: &str,
    publication: &Publication,
) -> Vec<String> {
    let version = version_key(
        account,
        &publication.repository,
        &publication.coordinates.package_name,
        &publication.coordinates.package_version,
    );
    let asset = asset_key(account, &publication.repository, &publication.asset_path);
    // DQL rejects defined and unused variables: each case binds
    // exactly what its blocks name.
    let mut lines = vec![format!(
        "v as var(func: eq(hodei.catalog.version.key, {}))",
        dql_string(&version)
    )];
    match kind {
        WriteKind::Create => {
            lines.push(format!(
                "a as var(func: eq(hodei.catalog.asset.key, {}))",
                dql_string(&asset)
            ));
            lines.push(format!(
                "rf as var(func: eq(hodei.catalog.reference.key, {}))",
                dql_string(&asset)
            ));
            if !event_said {
                lines.push(format!(
                    "ev as var(func: eq(hodei.catalog.outbox.key, {}))",
                    dql_string(&event.event_id)
                ));
            }
        }
        WriteKind::Replace { previous } => {
            // The row that moves is bound by key AND by the digest the plan
            // read (`previous`): the pointer is a compare-and-set INSIDE the
            // committing transaction. The guard only proves that the scope
            // revision did not move between ITS pre-read and the commit; the
            // plan read the pointer BEFORE that pre-read, so a writer that
            // committed in between would otherwise be overwritten silently
            // (a lost update: the second npm packument dropped the version
            // the first one had just published). When the pointer is no
            // longer at `previous` the bind is empty, every block skips, and
            // the settle check after the commit says the write did not land:
            // the next attempt re-reads and re-decides (the client
            // precondition then fails, or the replace is planned again over
            // the new pointer). The positive filter matters: the filter
            // `NOT eq(digest, previous)` excluded exactly the row the plan
            // decided to move (HSP-193).
            lines.push(format!(
                "am as var(func: eq(hodei.catalog.asset.key, {})) @filter(eq(hodei.catalog.asset.digest, {}))",
                dql_string(&asset),
                dql_string(previous),
            ));
            lines.push(format!(
                "rf as var(func: eq(hodei.catalog.reference.key, {}))",
                dql_string(&asset)
            ));
            if !event_said {
                lines.push(format!(
                    "ev as var(func: eq(hodei.catalog.outbox.key, {}))",
                    dql_string(&event.event_id)
                ));
            }
        }
        WriteKind::Repair => {
            lines.push(format!(
                "rf as var(func: eq(hodei.catalog.reference.key, {}))",
                dql_string(&asset)
            ));
        }
    }
    lines
}

/// The move of the asset's row: the pointer now points to the
/// new content, through the uid that the query's filter bound.
fn asset_move_nquads(publication: &Publication, now: &str) -> String {
    let subject = publication
        .subject
        .as_ref()
        .map(|digest| {
            format!(
                "uid(am) <hodei.catalog.asset.subject> {} .\n",
                dql_string(&digest.value)
            )
        })
        .unwrap_or_default();
    format!(
        "uid(am) <hodei.catalog.asset.digest_algorithm> \"{algorithm}\" .\n\
         uid(am) <hodei.catalog.asset.digest> {digest} .\n\
         uid(am) <hodei.catalog.asset.size_bytes> \"{size}\" .\n\
         uid(am) <hodei.catalog.asset.package_name> {name} .\n\
         uid(am) <hodei.catalog.asset.package_version> {version} .\n\
         uid(am) <hodei.catalog.asset.updated_at> \"{now}\" .\n\
         {subject}",
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        size = publication.size_bytes,
        name = dql_string(&publication.coordinates.package_name),
        version = dql_string(&publication.coordinates.package_version),
    )
}

/// The move of the reference's row, which moves with the asset.
fn reference_move_nquads(publication: &Publication, scope: DigestScope, now: &str) -> String {
    format!(
        "uid(rf) <hodei.catalog.reference.digest_algorithm> \"{algorithm}\" .\n\
         uid(rf) <hodei.catalog.reference.digest> {digest} .\n\
         uid(rf) <hodei.catalog.reference.scope> \"{scope}\" .\n\
         uid(rf) <hodei.catalog.reference.size_bytes> \"{size}\" .\n\
         uid(rf) <hodei.catalog.reference.updated_at> \"{now}\" .\n",
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        scope = scope_name(scope),
        size = publication.size_bytes,
    )
}

/// The conditioned blocks of a publication, with the parent's gate
/// of the call. The boundary's contract: the event is written only
/// if the content rows are written, and the content rows are only
/// written if the event can be written -- an asset without its event is
/// a fact without a trace, and the gate prevents it before writing anything.
#[allow(clippy::too_many_arguments)]
fn the_blocks(
    parent: &str,
    kind: &WriteKind,
    event: &AuditEvent,
    event_said: bool,
    account: &str,
    publication: &Publication,
    scope: DigestScope,
    now: &str,
) -> Vec<GraphMutationBlock> {
    // The event gate: while the fact is not said, the content
    // rows carry the condition that the event's row be free.
    let ev_gate = if event_said {
        String::new()
    } else {
        " AND eq(len(ev), 0)".to_owned()
    };
    let mut blocks = Vec::new();
    match kind {
        WriteKind::Create => {
            if !event_said {
                blocks.push(GraphMutationBlock::set_if(
                    format!("{parent} AND eq(len(a), 0){ev_gate}"),
                    outbox_event_nquads(event),
                ));
            }
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(a), 0){ev_gate}"),
                asset_create_nquads(account, publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(a), 0) AND eq(len(v), 0){ev_gate}"),
                version_create_nquads(account, publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(a), 0) AND eq(len(rf), 0){ev_gate}"),
                reference_create_nquads(account, publication, scope, now),
            ));
        }
        WriteKind::Replace { .. } => {
            if !event_said {
                blocks.push(GraphMutationBlock::set_if(
                    format!("{parent} AND eq(len(am), 1){ev_gate}"),
                    outbox_event_nquads(event),
                ));
            }
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(am), 1){ev_gate}"),
                asset_move_nquads(publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(am), 1) AND eq(len(v), 0){ev_gate}"),
                version_create_nquads(account, publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(am), 1) AND eq(len(rf), 1){ev_gate}"),
                reference_move_nquads(publication, scope, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(am), 1) AND eq(len(rf), 0){ev_gate}"),
                reference_create_nquads(account, publication, scope, now),
            ));
        }
        WriteKind::Repair => {
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(v), 0)"),
                version_create_nquads(account, publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(rf), 0)"),
                reference_create_nquads(account, publication, scope, now),
            ));
        }
    }
    blocks
}

/// The deterministic-looking but unique operation identifier of a deletion.
///
/// A deletion is NOT the same fact when it is repeated after a republication
/// (delete, publish the same bytes, delete again), so the marker carries a
/// fresh nonce: the once-only replay of the guard must never swallow a later
/// deletion. Deleting twice is harmless by construction (the second finds
/// nothing), so a retry needs no replay.
fn deletion_operation_id(
    account: &str,
    repository: &str,
    asset_path: &str,
    nonce: &str,
) -> OperationId {
    let mut hasher = Sha256::new();
    hasher.update(account.as_bytes());
    hasher.update([0]);
    hasher.update(repository.as_bytes());
    hasher.update([0]);
    hasher.update(asset_path.as_bytes());
    hasher.update([0]);
    hasher.update(nonce.as_bytes());
    OperationId::new(&format!("del-{:x}", hasher.finalize()))
        .expect("a sha256-derived operation id is always valid")
}

/// The query lines of a deletion: the asset, its reference, the versions of
/// its package and the OTHER assets that still name that package (a package
/// whose name another asset carries keeps its versions).
fn deletion_query_lines(
    account: &str,
    repository: &str,
    asset_path: &str,
    package: &str,
) -> Vec<String> {
    let key = dql_string(&asset_key(account, repository, asset_path));
    vec![
        format!("a as var(func: eq(hodei.catalog.asset.key, {key}))"),
        format!("rf as var(func: eq(hodei.catalog.reference.key, {key}))"),
        format!(
            "vs as var(func: eq(hodei.catalog.version.repository, {repository})) \
             @filter(eq(hodei.catalog.version.account, {account}) AND \
             eq(hodei.catalog.version.package_name, {package}))",
            repository = dql_string(repository),
            account = dql_string(account),
            package = dql_string(package),
        ),
        format!(
            "ot as var(func: eq(hodei.catalog.asset.repository, {repository})) \
             @filter(eq(hodei.catalog.asset.account, {account}) AND \
             eq(hodei.catalog.asset.package_name, {package}) AND \
             NOT eq(hodei.catalog.asset.key, {key}))",
            repository = dql_string(repository),
            account = dql_string(account),
            package = dql_string(package),
        ),
    ]
}

/// The blocks of a deletion, each under the parent's gate of the call.
fn deletion_blocks(parent: &str) -> Vec<GraphMutationBlock> {
    let delete_if = |cond: String, nquads: &str| GraphMutationBlock {
        cond: Some(cond),
        set_nquads: String::new(),
        delete_nquads: nquads.to_owned(),
    };
    vec![
        delete_if(format!("{parent} AND eq(len(a), 1)"), "uid(a) * * .\n"),
        delete_if(format!("{parent} AND eq(len(rf), 1)"), "uid(rf) * * .\n"),
        delete_if(
            format!("{parent} AND gt(len(vs), 0) AND eq(len(ot), 0)"),
            "uid(vs) * * .\n",
        ),
    ]
}

// ---------------------------------------------------------------------------
// The store.
// ---------------------------------------------------------------------------

/// The catalog over Dgraph. It is opened with the validated guard (the
/// graph's versioned startup) and declares its own additive schema on top.
pub struct DgraphCatalogStore {
    client: Arc<GraphClient>,
    guards: GuardStore,
    authorizer: Arc<dyn CatalogAuthorizer>,
    probe: Option<Arc<dyn BlobIntegrityProbe>>,
    identity: Identity,
    trust: Option<Arc<dyn TrustLedger>>,
}

impl std::fmt::Debug for DgraphCatalogStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DgraphCatalogStore").finish_non_exhaustive()
    }
}

/// The decision of a typed guard error of the client, said as a
/// catalog error. Nothing is silenced: each class is said as it is.
fn the_guard_error(error: hodei_graph_client::GuardError) -> CatalogError {
    match error {
        hodei_graph_client::GuardError::InvalidScope { scope, reason } => {
            CatalogError::invalid(format!("the scope '{scope}' is not valid: {reason}"))
        }
        hodei_graph_client::GuardError::Operation(_) => {
            CatalogError::unavailable(error.to_string())
        }
        hodei_graph_client::GuardError::CasRejected { .. } => CatalogError::Conflict {
            kind: "asset",
            key: "guard".to_owned(),
            detail: error.to_string(),
        },
        hodei_graph_client::GuardError::CorruptState { .. } => {
            CatalogError::corrupt("row", "guard", error.to_string())
        }
        _ => CatalogError::unavailable(error.to_string()),
    }
}

impl DgraphCatalogStore {
    /// Opens the store over an already connected and authenticated client: the
    /// guard validates the graph's versioned schema (v7) and then this
    /// slice declares its own additive DDL, which is idempotent.
    pub async fn open(
        client: Arc<GraphClient>,
        authorizer: Arc<dyn CatalogAuthorizer>,
    ) -> Result<Self, CatalogError> {
        let guards = GuardStore::open(client.clone())
            .await
            .map_err(the_guard_error)?;
        client
            .alter_schema(&catalog_schema_dql())
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        // The additive DDL of the coordinated collection ledger
        // (HSP-192): own types and keys, idempotent, without touching the
        // graph's versioned schema.
        client
            .alter_schema(&gc_ledger::gc_schema_dql())
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(Self {
            client,
            guards,
            authorizer,
            probe: None,
            identity: Identity::system(),
            trust: None,
        })
    }

    /// The store with the ledger of the trust grants the search reads.
    ///
    /// The search serves the trusted channel and says the trust state of
    /// every result from the grants, so it asks the same ledger the
    /// download path asks: a listing that read another source could say
    /// that content exists where the download answers that it does not.
    /// Without a ledger the trusted channel cannot be searched (it fails
    /// closed) and the results of the open channel carry no grant, which
    /// is quarantine.
    pub fn with_trust_ledger(mut self, trust: Arc<dyn TrustLedger>) -> Self {
        self.trust = Some(trust);
        self
    }

    /// The store with the decider that the boundary no longer repeats.
    pub fn with_authorizer(mut self, authorizer: Arc<dyn CatalogAuthorizer>) -> Self {
        self.authorizer = authorizer;
        self
    }

    /// The store with the CAS integrity probe.
    pub fn with_probe(mut self, probe: Arc<dyn BlobIntegrityProbe>) -> Self {
        self.probe = Some(probe);
        self
    }

    /// The store with the identity (clock and identifiers) that the trace
    /// uses.
    pub fn with_identity(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    /// Decides the request against the injected authorizer.
    async fn decide(&self, request: &AccessRequest) -> Result<(), CatalogError> {
        match self.authorizer.authorize(request).await? {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny(reason) => Err(CatalogError::Denied {
                action: request.action.as_str(),
                kind: CatalogResource::as_str(&request.resource),
                key: request.subject.clone(),
                reason,
            }),
        }
    }

    /// The row of a repository by its key, without authorizing.
    async fn the_repository_row(
        &self,
        account: &str,
        repo_key: &str,
    ) -> Result<Option<RepositoryRow>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.repository.key, {})) {{ {REPOSITORY_FIELDS} }} }}",
            dql_string(&repository_key(account, repo_key)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        single_row(
            response.json.get("q"),
            "repository",
            format!("{account}/{repo_key}"),
        )
        .map(|node| node.map(|node| RepositoryRow::from_node(&node)).transpose())
        .and_then(|row| row)
    }

    /// The row of a version by its coordinates, without authorizing.
    async fn the_version_row(
        &self,
        account: &str,
        repository: &str,
        coordinates: &PackageCoordinates,
    ) -> Result<Option<VersionRow>, CatalogError> {
        let key = version_key(
            account,
            repository,
            &coordinates.package_name,
            &coordinates.package_version,
        );
        let query = format!(
            "{{ q(func: eq(hodei.catalog.version.key, {})) {{ {VERSION_FIELDS} }} }}",
            dql_string(&key),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let kind = format!(
            "{repository}/{}@{}",
            coordinates.package_name, coordinates.package_version
        );
        single_row(response.json.get("q"), "version", kind)
            .map(|node| node.map(|node| VersionRow::from_node(&node)).transpose())
            .and_then(|row| row)
    }

    /// The row of a reference by its key, without authorizing.
    async fn the_reference_row(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<Option<ReferenceRow>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.reference.key, {})) {{ {REFERENCE_FIELDS} }} }}",
            dql_string(&asset_key(account, repository, asset_path)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        single_row(
            response.json.get("q"),
            "reference",
            format!("{repository}/{asset_path}"),
        )
        .map(|node| node.map(|node| ReferenceRow::from_node(&node)).transpose())
        .and_then(|row| row)
    }

    /// The asset row and its reference row read in ONE query, hence from ONE
    /// snapshot of the graph. A publication moves both in a single
    /// transaction; two separate reads can straddle that commit and see the
    /// new pointer with the old reference (or the other way round), which
    /// the read model would then report as corruption of a healthy asset
    /// (the concurrent npm publishers answered 503 because of it).
    async fn the_asset_and_reference_rows(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<(Option<AssetRow>, Option<ReferenceRow>), CatalogError> {
        let key = dql_string(&asset_key(account, repository, asset_path));
        let query = format!(
            "{{ a(func: eq(hodei.catalog.asset.key, {key})) {{ {ASSET_FIELDS} }} \
             r(func: eq(hodei.catalog.reference.key, {key})) {{ {REFERENCE_FIELDS} }} }}"
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let name = format!("{repository}/{asset_path}");
        let asset = single_row(response.json.get("a"), "asset", name.clone())?
            .map(|node| AssetRow::from_node(&node))
            .transpose()?;
        let reference = single_row(response.json.get("r"), "reference", name)?
            .map(|node| ReferenceRow::from_node(&node))
            .transpose()?;
        Ok((asset, reference))
    }

    /// The row of a tag by its key, without authorizing: the identity
    /// (account, repository, name) is checked against the row, so that
    /// a key that points to another owner does not cross accounts.
    async fn the_tag_row(
        &self,
        account: &str,
        repository: &str,
        tag_name: &str,
    ) -> Result<Option<TagRow>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.tag.key, {})) {{ \
             hodei.catalog.tag.account \
             hodei.catalog.tag.repository \
             hodei.catalog.tag.tag_name \
             hodei.catalog.tag.package_name \
             hodei.catalog.tag.package_version \
             hodei.catalog.tag.updated_at }} }}",
            dql_string(&tag_key(account, repository, tag_name)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        single_row(
            response.json.get("q"),
            "tag",
            format!("{account}/{repository}/{tag_name}"),
        )
        .map(|node| node.map(|node| TagRow::from_node(&node)).transpose())
        .and_then(|row| row)
    }

    /// The audit event of a publication. The **tenant** is the
    /// verified account of the principal, not the one the publication says.
    fn the_publication_event(
        &self,
        event_id: &str,
        occurred_at: DateTime<Utc>,
        principal_account: &str,
        publication: &Publication,
        result: AuditResult,
    ) -> AuditEvent {
        AuditEvent::new(
            event_id.to_string(),
            AuditContext::new(
                publication.actor.clone(),
                principal_account.to_string(),
                publication.correlation_id.clone(),
            ),
            occurred_at,
            "publish",
            format!("{}/{}", publication.repository, publication.asset_path),
            Some(format!(
                "{}:{}",
                publication.digest.algorithm, publication.digest.value
            )),
            result,
        )
    }

    /// Says the event of a publication that did not get published, outside
    /// the catalog's transaction: a rejection or a failure writes no
    /// catalog rows and there is no mutation to tie to. If the write
    /// fails, it warns and does not pretend the trace is there.
    async fn audit_the_publication(
        &self,
        event_id: &str,
        occurred_at: DateTime<Utc>,
        principal_account: &str,
        publication: &Publication,
        result: AuditResult,
    ) {
        let event = self.the_publication_event(
            event_id,
            occurred_at,
            principal_account,
            publication,
            result,
        );
        let outbox = DgraphAuditOutbox::new(self.client.clone());
        if let Err(error) = outbox.put_pending(&event).await {
            tracing::warn!(
                event_id = %event.event_id,
                error = %error,
                "the audit event could not be written to the outbox"
            );
        }
    }

    /// What can fail before writing, by content: the CAS
    /// has to have the bytes the publication declares. Without a probe it
    /// cannot be checked, and the catalog keeps what it is declared.
    async fn the_probe_failure(
        &self,
        principal_account: &str,
        scope: DigestScope,
        publication: &Publication,
    ) -> Option<CatalogError> {
        let probe = self.probe.as_ref()?;
        let location = BlobLocation::new(principal_account, scope, publication.digest.clone());
        match probe.length_of(&location).await {
            Ok(size) if size == publication.size_bytes => None,
            Ok(size) => Some(CatalogError::invalid(format!(
                "the content in the store has {size} bytes and the publication declares {}",
                publication.size_bytes
            ))),
            Err(BlobProbeError::Missing) => Some(CatalogError::invalid(
                "the content is not in the blob store",
            )),
            Err(BlobProbeError::Unavailable { detail }) => Some(CatalogError::unavailable(detail)),
        }
    }

    /// Writes a publication: the path's guard advances and, in the SAME
    /// transaction, the catalog rows and the audit event are
    /// applied or none is. Returns what the operation's marker
    /// answered.
    #[allow(clippy::too_many_arguments)]
    async fn write_the_publication(
        &self,
        guard: &GuardScope,
        kind: &WriteKind,
        event: &AuditEvent,
        event_said: bool,
        principal_account: &str,
        publication: &Publication,
        scope: DigestScope,
    ) -> Result<hodei_graph_client::GuardedOutcome, CatalogError> {
        let operation = publication_operation_id(principal_account, publication, kind);
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let effect = GuardedEffect {
            query_lines: the_effect_query_lines(
                kind,
                event,
                event_said,
                principal_account,
                publication,
            ),
            set_nquads: String::new(),
            recorded_result: the_recorded_result(kind),
        };
        self.guards
            .execute_guarded_composite(
                guard,
                &operation,
                RevisionDemand::Advance,
                effect,
                &|current| {
                    the_blocks(
                        parent_gate(current),
                        kind,
                        event,
                        event_said,
                        principal_account,
                        publication,
                        scope,
                        &now,
                    )
                },
            )
            .await
            .map_err(the_guard_error)
    }
}

/// The single row a read by key can return: zero, one, or
/// corruption (two rows with the same key are not two answers).
fn single_row(
    nodes: Option<&serde_json::Value>,
    kind: &'static str,
    key: String,
) -> Result<Option<serde_json::Value>, CatalogError> {
    let nodes = nodes
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    match nodes.len() {
        0 => Ok(None),
        1 => Ok(Some(nodes[0].clone())),
        n => Err(CatalogError::corrupt(
            kind,
            key,
            format!("{n} rows carry the same key"),
        )),
    }
}

/// What the publication left, built from the state the graph says
/// now. The answer comes from the state, never from hope.
fn the_outcome_from_state(
    state: &PublicationState,
    principal_account: &str,
    publication: &Publication,
    created: bool,
    disposition: AssetDisposition,
) -> Result<PublishOutcome, CatalogError> {
    let key = format!("{}/{}", publication.repository, publication.asset_path);
    let asset_row = state.asset.as_ref().ok_or_else(|| {
        CatalogError::unavailable(format!("the asset {key} is not there after the commit"))
    })?;
    let asset = asset_row.clone().into_record(
        principal_account,
        &publication.repository,
        &publication.asset_path,
    )?;
    let version = state
        .version
        .as_ref()
        .map(|row| row.clone().into_package_version());
    let reference_row = state.reference.as_ref().ok_or_else(|| {
        CatalogError::unavailable(format!("the reference {key} is not there after the commit"))
    })?;
    let reference = reference_row.clone().into_reference(
        principal_account.to_string(),
        publication.repository.clone(),
        publication.asset_path.clone(),
    );
    Ok(PublishOutcome {
        version: version.unwrap_or(PackageVersion {
            account: principal_account.to_string(),
            repository: publication.repository.clone(),
            coordinates: publication.coordinates.clone(),
            digest: publication.digest.clone(),
            size_bytes: publication.size_bytes,
            published_at: Utc::now(),
        }),
        asset,
        reference,
        created,
        disposition,
    })
}

#[async_trait]
impl CatalogReader for DgraphCatalogStore {
    async fn repository(
        &self,
        principal_account: &str,
        repo_key: &str,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Read,
            repo_key,
        ))
        .await?;
        self.the_repository_row(principal_account, repo_key)
            .await?
            .ok_or_else(|| CatalogError::missing("repository", repo_key.to_string()))?
            .into_descriptor(principal_account, repo_key)
    }

    async fn assets_under(
        &self,
        principal_account: &str,
        repository: &str,
        prefix: &str,
    ) -> Result<Vec<AssetRecord>, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Read,
            repository,
        ))
        .await?;
        // The prefix range over the path's exact index: everything
        // that starts with the prefix is greater than or equal to the prefix and
        // less than the prefix with the last character incremented.
        let upper = format!("{prefix}\u{10FFFF}");
        let query = format!(
            "{{ q(func: eq(hodei.catalog.asset.repository, {}), orderasc: hodei.catalog.asset.asset_path, first: {}) \
             @filter(eq(hodei.catalog.asset.account, {}) AND ge(hodei.catalog.asset.asset_path, {}) AND le(hodei.catalog.asset.asset_path, {})) \
             {{ {ASSET_FIELDS} }} }}",
            dql_string(repository),
            hodei_catalog::MAX_ASSETS_UNDER + 1,
            dql_string(principal_account),
            dql_string(prefix),
            dql_string(&upper),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if nodes.len() > hodei_catalog::MAX_ASSETS_UNDER {
            return Err(CatalogError::invalid(
                "the prefix holds more assets than the catalog is willing to list",
            ));
        }
        nodes
            .iter()
            .map(|node| {
                let row = AssetRow::from_node(node)?;
                let path = row.asset_path.clone();
                row.into_record(principal_account, repository, &path)
            })
            .collect()
    }

    async fn asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<AssetRecord, CatalogError> {
        self.decide(&AccessRequest::asset(
            principal_account,
            CatalogAction::Read,
            repository,
            asset_path,
        ))
        .await?;
        let key = format!("{repository}/{asset_path}");
        let (asset, reference) = self
            .the_asset_and_reference_rows(principal_account, repository, asset_path)
            .await?;
        let asset = asset.ok_or_else(|| CatalogError::missing("asset", key.clone()))?;

        // The version it points to has to say the same content.
        let coordinates = PackageCoordinates {
            package_name: asset.package_name.clone(),
            package_version: asset.package_version.clone(),
        };
        let version = self
            .the_version_row(principal_account, repository, &coordinates)
            .await?
            .ok_or_else(|| {
                CatalogError::corrupt(
                    "asset",
                    key.clone(),
                    "the asset points at a version that is not there",
                )
            })?;
        if version.digest != asset.digest {
            return Err(CatalogError::corrupt(
                "asset",
                key.clone(),
                "the asset and its version point at different content",
            ));
        }
        if version.size_bytes != asset.size_bytes {
            return Err(CatalogError::corrupt(
                "asset",
                key.clone(),
                "the asset and its version disagree on the size",
            ));
        }

        // And the reference has to point to the same content as the
        // asset: otherwise, the download would serve one blob and the collection
        // would keep another.
        let reference = reference.ok_or_else(|| {
            CatalogError::corrupt("asset", key.clone(), "the asset has no reference to a blob")
        })?;
        if reference.digest != asset.digest {
            return Err(CatalogError::corrupt(
                "asset",
                key.clone(),
                "the asset and its blob reference point at different content",
            ));
        }

        // And the CAS blob has to be of the size the catalog says.
        if let Some(probe) = &self.probe {
            let location =
                BlobLocation::new(principal_account, reference.scope, asset.digest.clone());
            match probe.length_of(&location).await {
                Ok(size) if size == asset.size_bytes => {}
                Ok(size) => {
                    return Err(CatalogError::corrupt(
                        "asset",
                        key,
                        format!(
                            "the blob has {size} bytes and the catalog says {}",
                            asset.size_bytes
                        ),
                    ));
                }
                Err(BlobProbeError::Missing) => {
                    return Err(CatalogError::corrupt(
                        "asset",
                        key,
                        "the blob is not in the store",
                    ));
                }
                Err(BlobProbeError::Unavailable { detail }) => {
                    return Err(CatalogError::unavailable(detail));
                }
            }
        }
        asset.into_record(principal_account, repository, asset_path)
    }

    async fn version(
        &self,
        principal_account: &str,
        repository: &str,
        coordinates: &PackageCoordinates,
    ) -> Result<PackageVersion, CatalogError> {
        self.decide(&AccessRequest::version(
            principal_account,
            CatalogAction::Read,
            repository,
            coordinates,
        ))
        .await?;
        self.the_version_row(principal_account, repository, coordinates)
            .await?
            .ok_or_else(|| {
                CatalogError::missing(
                    "version",
                    format!(
                        "{repository}/{}@{}",
                        coordinates.package_name, coordinates.package_version
                    ),
                )
            })
            .map(|row| row.into_package_version())
    }

    async fn tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
    ) -> Result<MutableTag, CatalogError> {
        self.decide(&AccessRequest::tag(
            principal_account,
            CatalogAction::Read,
            repository,
            tag_name,
        ))
        .await?;
        self.the_tag_row(principal_account, repository, tag_name)
            .await?
            .ok_or_else(|| CatalogError::missing("tag", tag_name.to_string()))?
            .into_tag(principal_account, repository, tag_name)
    }

    async fn reference(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<BlobReference, CatalogError> {
        self.decide(&AccessRequest::asset(
            principal_account,
            CatalogAction::Read,
            repository,
            asset_path,
        ))
        .await?;
        self.the_reference_row(principal_account, repository, asset_path)
            .await?
            .ok_or_else(|| CatalogError::missing("reference", format!("{repository}/{asset_path}")))
            .map(|row| {
                row.into_reference(
                    principal_account.to_string(),
                    repository.to_string(),
                    asset_path.to_string(),
                )
            })
    }

    async fn tags(
        &self,
        principal_account: &str,
        repository: &str,
    ) -> Result<Vec<MutableTag>, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Read,
            repository,
        ))
        .await?;
        // The list is a read of the repository, and the order is by
        // name: whoever lists tags lists them in an order that does not
        // change from one call to the next.
        let query = format!(
            "{{ q(func: eq(hodei.catalog.tag.account, {}), orderasc: hodei.catalog.tag.tag_name) \
             @filter(eq(hodei.catalog.tag.repository, {})) \
             {{ \
             hodei.catalog.tag.account \
             hodei.catalog.tag.repository \
             hodei.catalog.tag.tag_name \
             hodei.catalog.tag.package_name \
             hodei.catalog.tag.package_version \
             hodei.catalog.tag.updated_at }} }}",
            dql_string(principal_account),
            dql_string(repository),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut tags = Vec::with_capacity(nodes.len());
        for node in &nodes {
            let row = TagRow::from_node(node)?;
            let tag_name = row.tag_name.clone();
            tags.push(row.into_tag(principal_account, repository, &tag_name)?);
        }
        Ok(tags)
    }

    async fn referrers(
        &self,
        principal_account: &str,
        repository: &str,
        subject: &ContentDigest,
    ) -> Result<Vec<ReferrerRecord>, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Read,
            repository,
        ))
        .await?;
        let query = format!(
            "{{ q(func: eq(hodei.catalog.asset.subject, {})) \
             @filter(eq(hodei.catalog.asset.account, {}) AND eq(hodei.catalog.asset.repository, {})) \
             {{ {ASSET_FIELDS} }} }}",
            dql_string(&subject.value),
            dql_string(principal_account),
            dql_string(repository),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut referrers = Vec::new();
        for node in &nodes {
            let row = AssetRow::from_node(node)?;
            referrers.push(ReferrerRecord {
                account: row.account,
                repository: row.repository,
                asset_path: row.asset_path,
                digest: row.digest,
                size_bytes: row.size_bytes,
                subject: ContentDigest {
                    algorithm: subject.algorithm.clone(),
                    value: node_text(node, "hodei.catalog.asset.subject")?,
                },
            });
        }
        Ok(referrers)
    }
}

#[async_trait]
impl CatalogWriter for DgraphCatalogStore {
    async fn create_repository(
        &self,
        principal_account: &str,
        descriptor: RepositoryDescriptor,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Configure,
            &descriptor.repo_key,
        ))
        .await?;
        if descriptor.account != principal_account {
            return Err(CatalogError::invalid(
                "the repository belongs to another account",
            ));
        }
        the_npm_of(descriptor.format, &descriptor.config)?;
        the_upstream_of(descriptor.mode, &descriptor.config)?;
        let guard = repository_guard_scope(principal_account, &descriptor.repo_key)?;
        let operation = repository_operation_id(&descriptor);
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let key = repository_key(principal_account, &descriptor.repo_key);

        for _attempt in 0..WRITE_ATTEMPTS {
            // What is there decides: the same creation said twice is the
            // same repository; the key occupied with other content is a
            // conflict that the index arbitrates.
            if let Some(row) = self
                .the_repository_row(principal_account, &descriptor.repo_key)
                .await?
            {
                let existing = row.into_descriptor(principal_account, &descriptor.repo_key)?;
                let same = existing.format == descriptor.format
                    && existing.mode == descriptor.mode
                    && existing.config.digest_scope == descriptor.config.digest_scope
                    && existing.config.config_settings == descriptor.config.config_settings;
                if same {
                    return Ok(existing);
                }
                return Err(CatalogError::Conflict {
                    kind: "repository",
                    key: descriptor.repo_key.clone(),
                    detail: "the key is taken with different content".to_owned(),
                });
            }
            let effect = GuardedEffect {
                query_lines: vec![format!(
                    "r as var(func: eq(hodei.catalog.repository.key, {}))",
                    dql_string(&key)
                )],
                set_nquads: String::new(),
                recorded_result: "created".to_owned(),
            };
            self.guards
                .execute_guarded_composite(
                    &guard,
                    &operation,
                    RevisionDemand::Advance,
                    effect,
                    &|current| {
                        vec![GraphMutationBlock::set_if(
                            format!("{} AND eq(len(r), 0)", parent_gate(current)),
                            repository_create_nquads(&descriptor, &now),
                        )]
                    },
                )
                .await
                .map_err(the_guard_error)?;
            if let Some(row) = self
                .the_repository_row(principal_account, &descriptor.repo_key)
                .await?
            {
                return row.into_descriptor(principal_account, &descriptor.repo_key);
            }
            // The row did not land: another writer won the window. The
            // next attempt re-reads and decides with what is there.
        }
        Err(CatalogError::unavailable(
            "the repository could not be created after the attempts",
        ))
    }

    async fn update_config(
        &self,
        principal_account: &str,
        repo_key: &str,
        mut config: RepositoryConfig,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Configure,
            repo_key,
        ))
        .await?;
        if !config.has_readable_settings() {
            return Err(CatalogError::invalid(
                "repository config_settings must be an object",
            ));
        }
        let guard = repository_guard_scope(principal_account, repo_key)?;
        let key = repository_key(principal_account, repo_key);

        for _attempt in 0..WRITE_ATTEMPTS {
            let row = self
                .the_repository_row(principal_account, repo_key)
                .await?
                .ok_or_else(|| {
                    CatalogError::missing("repository", format!("{principal_account}/{repo_key}"))
                })?;
            let descriptor = row.clone().into_descriptor(principal_account, repo_key)?;
            if !descriptor.format.supports(&descriptor.mode) {
                return Err(CatalogError::invalid(
                    "the repository format does not serve its declared mode",
                ));
            }
            if config.digest_scope != descriptor.config.digest_scope {
                return Err(CatalogError::invalid(
                    "a repository digest scope cannot change after creation",
                ));
            }
            the_npm_of(descriptor.format, &config)?;
            the_upstream_of(descriptor.mode, &config)?;
            if descriptor.mode == RepositoryMode::Group {
                hodei_catalog::group::GroupConfig::of(&config)
                    .map_err(|error| CatalogError::invalid(error.to_string()))?;
            } else if config.config_settings.get("group").is_some() {
                return Err(CatalogError::invalid(
                    "only a group repository can declare config_settings.group",
                ));
            }
            let previous_version = row.config_version;
            config.version = previous_version
                .checked_add(1)
                .ok_or_else(|| CatalogError::invalid("repository config version is exhausted"))?;
            let operation = repository_config_operation_id(
                principal_account,
                repo_key,
                previous_version,
                &config,
            );
            let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let effect = GuardedEffect {
                query_lines: vec![format!(
                    "r as var(func: eq(hodei.catalog.repository.key, {})) @filter(eq(hodei.catalog.repository.config_version, \"{}\"))",
                    dql_string(&key),
                    previous_version
                )],
                set_nquads: String::new(),
                recorded_result: "configured".to_owned(),
            };
            self.guards
                .execute_guarded_composite(
                    &guard,
                    &operation,
                    RevisionDemand::Advance,
                    effect,
                    &|current| {
                        vec![GraphMutationBlock::set_if(
                            format!("{} AND eq(len(r), 1)", parent_gate(current)),
                            format!(
                                "uid(r) <hodei.catalog.repository.config_version> \"{}\" .\n\
                                 uid(r) <hodei.catalog.repository.config_scope> \"{}\" .\n\
                                 uid(r) <hodei.catalog.repository.config_settings> {} .\n\
                                 uid(r) <hodei.catalog.repository.updated_at> \"{}\" .\n",
                                config.version,
                                scope_name(config.digest_scope),
                                dql_string(&config.config_settings.to_string()),
                                now,
                            ),
                        )]
                    },
                )
                .await
                .map_err(the_guard_error)?;
            let updated = self
                .the_repository_row(principal_account, repo_key)
                .await?
                .ok_or_else(|| {
                    CatalogError::unavailable("the repository disappeared after configuration")
                })?;
            let updated_descriptor = updated.into_descriptor(principal_account, repo_key)?;
            if updated_descriptor.config.version == config.version
                && updated_descriptor.config.digest_scope == config.digest_scope
                && updated_descriptor.config.config_settings == config.config_settings
            {
                return Ok(updated_descriptor);
            }
        }
        Err(CatalogError::unavailable(
            "the repository configuration could not be updated after the attempts",
        ))
    }

    async fn publish(
        &self,
        principal_account: &str,
        publication: Publication,
    ) -> Result<PublishOutcome, CatalogError> {
        // The audit event is identified once per operation: the
        // same fact is not said twice with two names, nor is it said with
        // two different results.
        let event_id = self.identity.new_id();
        let occurred_at = self.identity.now();

        // It is decided **before** validating: a principal without permission does not
        // get to know whether what it asks for is well formed.
        if let Err(error) = self
            .decide(&AccessRequest::publish(
                principal_account,
                &publication.repository,
                &publication.asset_path,
            ))
            .await
        {
            let result = if matches!(error, CatalogError::Denied { .. }) {
                AuditResult::Denied
            } else {
                AuditResult::Failed
            };
            self.audit_the_publication(
                &event_id,
                occurred_at,
                principal_account,
                &publication,
                result,
            )
            .await;
            return Err(error);
        }

        // A **malformed** request is rejected before the effect boundary
        // and leaves no trace: there was no operation to audit.
        if publication.repository.trim().is_empty() {
            return Err(CatalogError::invalid("the repository is empty"));
        }
        if publication.asset_path.trim().is_empty() {
            return Err(CatalogError::invalid("the path of the asset is empty"));
        }
        if publication.coordinates.package_name.trim().is_empty() {
            return Err(CatalogError::invalid("the name of the package is empty"));
        }
        if publication.coordinates.package_version.trim().is_empty() {
            return Err(CatalogError::invalid("the version of the package is empty"));
        }
        if !publication.digest.is_well_formed() {
            return Err(CatalogError::invalid("the digest is not well formed"));
        }
        let guard = asset_guard_scope(
            principal_account,
            &publication.repository,
            &publication.asset_path,
        )?;

        let mut last_failure = String::new();
        for _attempt in 0..WRITE_ATTEMPTS {
            let state =
                read_publication_state(&self.client, principal_account, &publication, &event_id)
                    .await?;

            // The event identifier is already said with ANOTHER fact: the
            // publication cannot go out, and the contradiction is reported,
            // not silenced.
            if let Some(row) = state.event.as_ref() {
                let planned = self.the_publication_event(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Allowed,
                );
                if !row.the_event().same_fact(&planned) {
                    return Err(CatalogError::unavailable(format!(
                        "the audit event {event_id} is already said with another content"
                    )));
                }
            }

            let Some(repository_row) = state.repository.as_ref() else {
                // The repository does not exist. The request happened and the
                // system rejected it, and that is said.
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(CatalogError::missing(
                    "repository",
                    publication.repository.clone(),
                ));
            };
            if repository_row.account != principal_account
                || repository_row.repo_key != publication.repository
            {
                return Err(CatalogError::corrupt(
                    "repository",
                    publication.repository.clone(),
                    "the row does not carry the identity its key names",
                ));
            }
            let mode = RepositoryMode::parse(&repository_row.mode).ok_or_else(|| {
                CatalogError::corrupt(
                    "repository",
                    publication.repository.clone(),
                    "the mode is not a mode",
                )
            })?;
            if !mode.stores_content() {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(CatalogError::ModeNotImplemented { mode });
            }
            let scope = match repository_row.config_scope.as_str() {
                "global" => DigestScope::Global,
                "account_partitioned" => DigestScope::AccountPartitioned,
                other => {
                    return Err(CatalogError::corrupt(
                        "repository",
                        publication.repository.clone(),
                        format!("'{other}' is not a digest scope"),
                    ));
                }
            };
            if let Some(failure) = self
                .the_probe_failure(principal_account, scope, &publication)
                .await
            {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(failure);
            }

            let plan = PublishPlan::decide(&state, &publication);
            if let Some(conflict) = plan.conflict {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Conflict,
                )
                .await;
                return Err(conflict);
            }
            if let Some(failure) = plan.precondition_failure {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(failure);
            }
            if plan.complete {
                // Nothing is written: there is no mutation, and an event that
                // says "published" about a publication that was already there
                // adds no new fact.
                return the_outcome_from_state(
                    &state,
                    principal_account,
                    &publication,
                    false,
                    AssetDisposition::Unchanged,
                );
            }

            let kind = plan.write.clone().expect("the plan demands a write");
            let event = self.the_publication_event(
                &event_id,
                occurred_at,
                principal_account,
                &publication,
                AuditResult::Allowed,
            );
            let event_said = state.event.is_some();
            let outcome = self
                .write_the_publication(
                    &guard,
                    &kind,
                    &event,
                    event_said,
                    principal_account,
                    &publication,
                    scope,
                )
                .await?;
            if outcome.replayed {
                // The same fact is already committed: the retry after the
                // commit re-reads the committed result and writes nothing.
                let state = read_publication_state(
                    &self.client,
                    principal_account,
                    &publication,
                    &event_id,
                )
                .await?;
                let plan = PublishPlan::decide(&state, &the_settled_publication(&publication));
                if plan.complete {
                    return the_outcome_from_state(
                        &state,
                        principal_account,
                        &publication,
                        false,
                        AssetDisposition::Unchanged,
                    );
                }
                last_failure =
                    "the operation replayed but the state does not say the fact".to_owned();
                continue;
            }
            // The commit answered; the STATE decides what this call
            // left: a creation that settled is a creation, and a
            // replacement that settled is a replacement. The client's
            // precondition was already arbitrated by the transaction.
            let state =
                read_publication_state(&self.client, principal_account, &publication, &event_id)
                    .await?;
            let plan = PublishPlan::decide(&state, &the_settled_publication(&publication));
            if let Some(conflict) = plan.conflict {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Conflict,
                )
                .await;
                return Err(conflict);
            }
            if let Some(failure) = plan.precondition_failure {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(failure);
            }
            if plan.complete {
                let created = !matches!(kind, WriteKind::Repair);
                let disposition = match &kind {
                    WriteKind::Create if created => AssetDisposition::Created,
                    WriteKind::Replace { .. } if created => AssetDisposition::Replaced,
                    _ => AssetDisposition::Unchanged,
                };
                return the_outcome_from_state(
                    &state,
                    principal_account,
                    &publication,
                    created,
                    disposition,
                );
            }
            last_failure = "the write did not settle after the commit".to_owned();
        }

        // The last attempt is also re-read: losing the race on the
        // last attempt is a conflict, not a storage failure.
        // The client's precondition was already arbitrated by the pre-read and the
        // guard, as in the rest of the post-commit reads.
        let state =
            read_publication_state(&self.client, principal_account, &publication, &event_id)
                .await?;
        let plan = PublishPlan::decide(&state, &the_settled_publication(&publication));
        if let Some(conflict) = plan.conflict {
            self.audit_the_publication(
                &event_id,
                occurred_at,
                principal_account,
                &publication,
                AuditResult::Conflict,
            )
            .await;
            return Err(conflict);
        }
        if let Some(failure) = plan.precondition_failure {
            self.audit_the_publication(
                &event_id,
                occurred_at,
                principal_account,
                &publication,
                AuditResult::Failed,
            )
            .await;
            return Err(failure);
        }
        if plan.complete {
            return the_outcome_from_state(
                &state,
                principal_account,
                &publication,
                false,
                AssetDisposition::Unchanged,
            );
        }
        self.audit_the_publication(
            &event_id,
            occurred_at,
            principal_account,
            &publication,
            AuditResult::Failed,
        )
        .await;
        Err(CatalogError::unavailable(last_failure))
    }

    async fn move_tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
        coordinates: PackageCoordinates,
    ) -> Result<MutableTag, CatalogError> {
        self.decide(&AccessRequest::tag(
            principal_account,
            CatalogAction::MoveTag,
            repository,
            tag_name,
        ))
        .await?;

        if tag_name.trim().is_empty() {
            return Err(CatalogError::invalid("the name of the tag is empty"));
        }

        // The repository has to exist and accept client writes:
        // a tag is a client write even if the content it points
        // to is already there.
        let row = self
            .the_repository_row(principal_account, repository)
            .await?
            .ok_or_else(|| CatalogError::missing("repository", repository.to_string()))?;
        if row.account != principal_account || row.repo_key != repository {
            return Err(CatalogError::corrupt(
                "repository",
                repository.to_string(),
                "the row does not carry the identity its key names",
            ));
        }
        let mode = RepositoryMode::parse(&row.mode).ok_or_else(|| {
            CatalogError::corrupt(
                "repository",
                repository.to_string(),
                "the mode is not a mode",
            )
        })?;
        if !mode.accepts_client_writes() {
            return Err(CatalogError::invalid(
                "this repository does not accept tags from a client",
            ));
        }

        // The tag points to a version that exists. Versions are not
        // deleted, so this check does not go stale.
        self.the_version_row(principal_account, repository, &coordinates)
            .await?
            .ok_or_else(|| {
                CatalogError::missing(
                    "version",
                    format!(
                        "{repository}/{}@{}",
                        coordinates.package_name, coordinates.package_version
                    ),
                )
            })?;

        let guard = tag_guard_scope(principal_account, repository, tag_name)?;
        let operation = tag_operation_id(principal_account, repository, tag_name, &coordinates);
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let effect = GuardedEffect {
            query_lines: vec![format!(
                "tn as var(func: eq(hodei.catalog.tag.key, {}))",
                dql_string(&tag_key(principal_account, repository, tag_name)),
            )],
            set_nquads: String::new(),
            recorded_result: "moved".to_owned(),
        };
        // The same block serves to create and to move: if the row is
        // already there, the index finds it and the uid(tn) rewrites it; if it is not
        // there, the node is born with its key, which the @upsert keeps unique.
        let _outcome = self
            .guards
            .execute_guarded_composite(
                &guard,
                &operation,
                RevisionDemand::Advance,
                effect,
                &|current| {
                    let gate = parent_gate(current);
                    vec![
                        GraphMutationBlock::set_if(
                            format!("{gate} AND eq(len(tn), 0)"),
                            tag_create_nquads(
                                principal_account,
                                repository,
                                tag_name,
                                &coordinates,
                                &now,
                            ),
                        ),
                        GraphMutationBlock::set_if(
                            format!("{gate} AND eq(len(tn), 1)"),
                            tag_move_nquads(&coordinates, &now),
                        ),
                    ]
                },
            )
            .await
            .map_err(the_guard_error)?;

        // The tag is read from the inside: the write was already authorized, and
        // re-reading through the public path would require a read permission that
        // whoever moves the tag need not have.
        //
        // A marker in replay says nothing about the CURRENT state: a
        // redelivery that arrives after another commit moved the tag
        // is an already-said fact that another fact won over -- that is last
        // commit wins, not corruption. What is returned is what the
        // graph says now.
        let written = self
            .the_tag_row(principal_account, repository, tag_name)
            .await?
            .ok_or_else(|| {
                CatalogError::unavailable("the tag was written and then it was not there")
            })?;
        written.into_tag(principal_account, repository, tag_name)
    }

    async fn move_asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
        _coordinates: PackageCoordinates,
    ) -> Result<AssetRecord, CatalogError> {
        self.decide(&AccessRequest::asset(
            principal_account,
            CatalogAction::MoveAsset,
            repository,
            asset_path,
        ))
        .await?;
        Err(CatalogError::invalid(
            "the dgraph raw slice does not serve asset moves",
        ))
    }

    async fn delete_asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<AssetDeletion, CatalogError> {
        self.decide(&AccessRequest::asset(
            principal_account,
            CatalogAction::Delete,
            repository,
            asset_path,
        ))
        .await?;
        if asset_path.trim().is_empty() {
            return Err(CatalogError::invalid("the path of the asset is empty"));
        }

        // The repository has to exist and accept client writes, like any
        // other write: a deletion is a client write.
        let row = self
            .the_repository_row(principal_account, repository)
            .await?
            .ok_or_else(|| CatalogError::missing("repository", repository.to_string()))?;
        if row.account != principal_account || row.repo_key != repository {
            return Err(CatalogError::corrupt(
                "repository",
                repository.to_string(),
                "the row does not carry the identity its key names",
            ));
        }
        let mode = RepositoryMode::parse(&row.mode).ok_or_else(|| {
            CatalogError::corrupt(
                "repository",
                repository.to_string(),
                "the mode is not a mode",
            )
        })?;
        if !mode.accepts_client_writes() {
            return Err(CatalogError::invalid(
                "this repository does not accept deletions from a client",
            ));
        }

        let guard = asset_guard_scope(principal_account, repository, asset_path)?;
        let mut removed = false;
        let mut released: Vec<ContentDigest> = Vec::new();
        let mut versions_removed = 0usize;
        for _attempt in 0..WRITE_ATTEMPTS {
            // What is there NOW decides: the loop converges on "nothing of
            // this path is left", and a state that is already clean writes
            // nothing (idempotent, and no trace of a no-op).
            let (asset, reference) = self
                .the_asset_and_reference_rows(principal_account, repository, asset_path)
                .await?;
            // A raw path is its package; an asset that is already gone still
            // names its history by the path.
            let package = asset
                .as_ref()
                .map(|asset| asset.package_name.clone())
                .unwrap_or_else(|| asset_path.to_string());
            let versions = self
                .the_package_versions(principal_account, repository, &package)
                .await?;
            let others = self
                .the_other_assets_of_package(principal_account, repository, asset_path, &package)
                .await?;
            let versions: Vec<VersionRow> = if others { Vec::new() } else { versions };
            if asset.is_none() && reference.is_none() && versions.is_empty() {
                return Ok(AssetDeletion {
                    removed,
                    released,
                    versions_removed,
                });
            }
            let mut digests: Vec<ContentDigest> = Vec::new();
            digests.extend(asset.iter().map(|asset| asset.digest.clone()));
            digests.extend(reference.iter().map(|reference| reference.digest.clone()));
            digests.extend(versions.iter().map(|version| version.digest.clone()));
            for digest in digests {
                if !released.contains(&digest) {
                    released.push(digest);
                }
            }
            versions_removed += versions.len();

            let operation = deletion_operation_id(
                principal_account,
                repository,
                asset_path,
                &self.identity.new_id(),
            );
            let effect = GuardedEffect {
                query_lines: deletion_query_lines(
                    principal_account,
                    repository,
                    asset_path,
                    &package,
                ),
                set_nquads: String::new(),
                recorded_result: "deleted".to_owned(),
            };
            self.guards
                .execute_guarded_composite(
                    &guard,
                    &operation,
                    RevisionDemand::Advance,
                    effect,
                    &|current| deletion_blocks(parent_gate(current)),
                )
                .await
                .map_err(the_guard_error)?;
            removed = true;
        }
        Err(CatalogError::unavailable(
            "the deletion did not settle: the path kept changing under it",
        ))
    }
}

impl DgraphCatalogStore {
    /// The version rows of a package, without authorizing.
    async fn the_package_versions(
        &self,
        account: &str,
        repository: &str,
        package: &str,
    ) -> Result<Vec<VersionRow>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.version.repository, {repository})) \
             @filter(eq(hodei.catalog.version.account, {account}) AND \
             eq(hodei.catalog.version.package_name, {package})) {{ {VERSION_FIELDS} }} }}",
            repository = dql_string(repository),
            account = dql_string(account),
            package = dql_string(package),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .map(|nodes| nodes.iter().map(VersionRow::from_node).collect())
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    /// Whether another asset of the repository still names the package.
    async fn the_other_assets_of_package(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
        package: &str,
    ) -> Result<bool, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.asset.repository, {repository}), first: 1) \
             @filter(eq(hodei.catalog.asset.account, {account}) AND \
             eq(hodei.catalog.asset.package_name, {package}) AND \
             NOT eq(hodei.catalog.asset.key, {key})) {{ uid }} }}",
            repository = dql_string(repository),
            account = dql_string(account),
            package = dql_string(package),
            key = dql_string(&asset_key(account, repository, asset_path)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|rows| !rows.is_empty()))
    }
}

// ---------------------------------------------------------------------------
// The indexed search (HSP-191).
//
// Each query is bounded by account and by repository over exact
// indexes. The trusted channel is walked through the digests with an active
// grant given by the grant ledger; the trust state of each
// result comes from the same ledger, never from the reference. The text is a
// case-insensitive substring evaluated as a filter inside the
// already bounded repository (no trigram index: no scan leaves
// the repository, and the write cost of each asset does not change).
// ---------------------------------------------------------------------------

/// How many repositories can share a tag that the search
/// resolves, the same boundary as the Surreal reference.
const MAX_TAG_TARGETS: usize = 64;

/// The coordinate a tag points to, read from the graph.
struct TagTarget {
    repository: String,
    package_name: String,
    package_version: String,
}

impl DgraphCatalogStore {
    /// The account's repository, by the account's exact index, in
    /// the order of its key: the same order as the Surreal reference.
    async fn the_repository_listing(
        &self,
        principal_account: &str,
    ) -> Result<Vec<RepositoryDescriptor>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.repository.account, {}), \
             orderasc: hodei.catalog.repository.repo_key) {{ {REPOSITORY_FIELDS} }} }}",
            dql_string(principal_account),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut repositories = Vec::with_capacity(nodes.len());
        for node in &nodes {
            let row = RepositoryRow::from_node(node)?;
            let repo_key = row.repo_key.clone();
            repositories.push(row.into_descriptor(principal_account, &repo_key)?);
        }
        Ok(repositories)
    }

    /// The coordinates a tag names, by the exact index of its
    /// name: a tag that does not exist narrows nothing.
    async fn the_tag_targets(
        &self,
        principal_account: &str,
        tag: &str,
    ) -> Result<Vec<TagTarget>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.tag.tag_name, {}), first: {}) \
             @filter(eq(hodei.catalog.tag.account, {})) {{ \
             hodei.catalog.tag.repository hodei.catalog.tag.package_name \
             hodei.catalog.tag.package_version }} }}",
            dql_string(tag),
            MAX_TAG_TARGETS,
            dql_string(principal_account),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut targets = Vec::with_capacity(nodes.len());
        for node in &nodes {
            targets.push(TagTarget {
                repository: node_text(node, "hodei.catalog.tag.repository")?,
                package_name: node_text(node, "hodei.catalog.tag.package_name")?,
                package_version: node_text(node, "hodei.catalog.tag.package_version")?,
            });
        }
        Ok(targets)
    }

    /// The window of rows of ONE repository, by the exact indexes: the
    /// account and the repository at the root (the index that serves them), the
    /// rest in the filter of the same index. The page is **by key**
    /// (HSP-191): with a cursor, the window starts AFTER the key that
    /// was already seen, and an insertion before that key does not move it.
    async fn the_asset_window(
        &self,
        principal_account: &str,
        repo_key: &str,
        query: &SearchQuery,
        scope: WindowScope<'_>,
        window: usize,
    ) -> Result<Vec<AssetRow>, CatalogError> {
        let WindowScope {
            target,
            after_path,
            granted,
        } = scope;
        let mut filters = vec![format!(
            "eq(hodei.catalog.asset.account, {})",
            dql_string(principal_account)
        )];
        if let Some(name) = &query.package_name {
            filters.push(format!(
                "eq(hodei.catalog.asset.package_name, {})",
                dql_string(name)
            ));
        }
        if let Some(version) = &query.package_version {
            filters.push(format!(
                "eq(hodei.catalog.asset.package_version, {})",
                dql_string(version)
            ));
        }
        if let Some(digest) = &query.digest {
            filters.push(format!(
                "eq(hodei.catalog.asset.digest_algorithm, \"{algorithm}\") AND \
                 eq(hodei.catalog.asset.digest, {value})",
                algorithm = digest.algorithm,
                value = dql_string(&digest.value),
            ));
        }
        if let Some(granted) = granted {
            let values = granted
                .values
                .iter()
                .map(|value| dql_string(value))
                .collect::<Vec<_>>()
                .join(", ");
            filters.push(format!(
                "eq(hodei.catalog.asset.digest_algorithm, \"{algorithm}\") AND \
                 eq(hodei.catalog.asset.digest, [{values}])",
                algorithm = granted.algorithm,
            ));
        }
        if let Some(text) = &query.text {
            let pattern = the_text_pattern(text)?;
            filters.push(format!(
                "(regexp(hodei.catalog.asset.package_name, {pattern}) OR \
                 regexp(hodei.catalog.asset.asset_path, {pattern}))"
            ));
        }
        if let Some(target) = target {
            filters.push(format!(
                "eq(hodei.catalog.asset.package_name, {name}) AND \
                 eq(hodei.catalog.asset.package_version, {version})",
                name = dql_string(&target.package_name),
                version = dql_string(&target.package_version),
            ));
        }
        if let Some(after) = after_path {
            filters.push(format!(
                "gt(hodei.catalog.asset.asset_path, {})",
                dql_string(after)
            ));
        }
        let query_dql = format!(
            "{{ q(func: eq(hodei.catalog.asset.repository, {repository}), \
             orderasc: hodei.catalog.asset.asset_path, first: {window}) \
             @filter({filters}) {{ {ASSET_FIELDS} }} }}",
            repository = dql_string(repo_key),
            filters = filters.join(" AND "),
        );
        let response = self
            .client
            .execute_dql(&query_dql)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        nodes
            .iter()
            .map(AssetRow::from_node)
            .collect::<Result<Vec<_>, _>>()
    }

    /// How many rows remain to be walked, counted in the adapter with an
    /// index that only asks for the path: the same bounded count as the
    /// Surreal reference.
    async fn the_remaining_count(
        &self,
        principal_account: &str,
        repo_key: &str,
        query: &SearchQuery,
        scope: WindowScope<'_>,
        cap: usize,
    ) -> Result<usize, CatalogError> {
        let rows = self
            .the_asset_window(principal_account, repo_key, query, scope, cap + 1)
            .await?;
        Ok(rows.len())
    }
}

/// How many digests with an active grant ONE query of the window names.
///
/// The query is text and the list goes in the filter: the group bounds the size
/// of what is sent, and the groups of a repository are merged in the
/// adapter by path order.
const GRANTED_DIGESTS_PER_QUERY: usize = 64;

/// How many digests with an active grant a trusted-channel repository may have in order to
/// search it. Beyond that it is not a search: it is walking the
/// whole ledger, and it is said with a request error, just like
/// tags shared by too many repositories.
const MAX_GRANTED_DIGESTS: usize = 2048;

/// What narrows a repository's window besides the query: the
/// tag's coordinates, the key the cursor already saw and the group of
/// granted digests.
#[derive(Clone, Copy, Default)]
struct WindowScope<'a> {
    target: Option<&'a TagTarget>,
    after_path: Option<&'a str>,
    granted: Option<&'a GrantedDigests>,
}

/// A group of granted digests of the same algorithm.
struct GrantedDigests {
    algorithm: DigestAlgorithm,
    values: Vec<String>,
}

/// The DQL pattern `/text/i` for a text the client writes.
///
/// The text is searched as a **case-insensitive substring**, just like
/// the reference, so every character with a regular expression
/// meaning is escaped and the pattern is never an expression the client
/// controls. Control characters are not searchable text.
fn the_text_pattern(text: &str) -> Result<String, CatalogError> {
    if text.chars().any(char::is_control) {
        return Err(CatalogError::invalid(
            "the text of the search has control characters",
        ));
    }
    let mut pattern = String::with_capacity(text.len() + 4);
    pattern.push('/');
    for character in text.chars() {
        if matches!(
            character,
            '\\' | '/'
                | '.'
                | '+'
                | '*'
                | '?'
                | '('
                | ')'
                | '|'
                | '['
                | ']'
                | '{'
                | '}'
                | '^'
                | '$'
        ) {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push_str("/i");
    Ok(pattern)
}

impl DgraphCatalogStore {
    /// The groups of digests by which a repository is walked.
    ///
    /// An open repository is a single group without restriction. One of the
    /// trusted channel only serves what has an active grant **now**:
    /// the digests are read from the ledger (a read bounded by account and
    /// repository), one per content even if several decisions granted it,
    /// and split into groups by algorithm. Without a ledger there is no
    /// way to know what is granted and the search fails closed: an empty
    /// page would say there is nothing.
    async fn the_digest_groups(
        &self,
        principal_account: &str,
        repository: &RepositoryDescriptor,
        query: &SearchQuery,
    ) -> Result<Vec<Option<GrantedDigests>>, CatalogError> {
        if ServeChannel::of(&repository.config) != ServeChannel::Trusted {
            return Ok(vec![None]);
        }
        let ledger = self.trust.as_ref().ok_or_else(|| {
            CatalogError::unavailable("the trusted channel cannot be searched without the grants")
        })?;
        let now = Utc::now();
        let grants = ledger
            .grants_of(principal_account, &repository.repo_key)
            .await?;
        let mut seen: std::collections::HashSet<ContentDigest> = std::collections::HashSet::new();
        let mut by_algorithm: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for grant in grants {
            if !grant.is_active_at(now) {
                continue;
            }
            // A search by digest only looks at that digest's grant.
            if query
                .digest
                .as_ref()
                .is_some_and(|asked| asked != &grant.digest)
            {
                continue;
            }
            if !seen.insert(grant.digest.clone()) {
                continue;
            }
            by_algorithm
                .entry(grant.digest.algorithm.to_string())
                .or_default()
                .push(grant.digest.value.clone());
        }
        if seen.len() > MAX_GRANTED_DIGESTS {
            return Err(CatalogError::invalid(
                "too many grants in the repository to search the trusted channel",
            ));
        }
        let mut groups = Vec::new();
        for (algorithm, values) in by_algorithm {
            let algorithm = match algorithm.as_str() {
                "sha256" => DigestAlgorithm::Sha256,
                "sha512" => DigestAlgorithm::Sha512,
                other => {
                    return Err(CatalogError::corrupt(
                        "grant",
                        other,
                        "the digest algorithm of a grant is unknown",
                    ));
                }
            };
            for chunk in values.chunks(GRANTED_DIGESTS_PER_QUERY) {
                groups.push(Some(GrantedDigests {
                    algorithm: algorithm.clone(),
                    values: chunk.to_vec(),
                }));
            }
        }
        Ok(groups)
    }

    /// The results of the page that have an active grant: pairs
    /// (repository, digest). One ledger read per repository of the
    /// page, bounded by the digests the page names.
    async fn the_granted_hits(
        &self,
        principal_account: &str,
        rows: &[AssetRow],
    ) -> Result<std::collections::HashSet<(String, ContentDigest)>, CatalogError> {
        let mut granted = std::collections::HashSet::new();
        let Some(ledger) = self.trust.as_ref() else {
            return Ok(granted);
        };
        let mut by_repository: std::collections::BTreeMap<&str, Vec<ContentDigest>> =
            std::collections::BTreeMap::new();
        for row in rows {
            let digests = by_repository.entry(row.repository.as_str()).or_default();
            if !digests.contains(&row.digest) {
                digests.push(row.digest.clone());
            }
        }
        let now = Utc::now();
        for (repository, digests) in by_repository {
            for digest in ledger
                .active_digests(principal_account, repository, &digests, now)
                .await?
            {
                granted.insert((repository.to_string(), digest));
            }
        }
        Ok(granted)
    }
}

/// Whether a filter with its own index beats the path in order. The graph's search
/// ALWAYS comes out in path order (`orderasc: asset_path`),
/// which is total within the account: the order is the same for all
/// filters, and a page neither repeats nor loses a result.
fn the_attribute_matches(
    attribute: &RepositoryAttribute,
    repository: &RepositoryDescriptor,
) -> bool {
    match attribute {
        RepositoryAttribute::Format(format) => repository.format == *format,
        RepositoryAttribute::Mode(mode) => repository.mode == *mode,
        RepositoryAttribute::Channel(channel) => ServeChannel::of(&repository.config) == *channel,
    }
}

/// The format of the repository that came out in the page, from the descriptor that
/// the account already named.
fn repository_format(candidates: &[&RepositoryDescriptor], repository: &str) -> RepositoryFormat {
    candidates
        .iter()
        .find(|candidate| candidate.repo_key == repository)
        .map(|candidate| candidate.format)
        .unwrap_or(RepositoryFormat::Raw)
}

#[async_trait]
impl CatalogSearch for DgraphCatalogStore {
    async fn repositories(
        &self,
        principal_account: &str,
    ) -> Result<Vec<RepositoryDescriptor>, CatalogError> {
        self.the_repository_listing(principal_account).await
    }

    async fn search(
        &self,
        principal_account: &str,
        repositories: &[RepositoryDescriptor],
        query: &SearchQuery,
    ) -> Result<SearchPage, CatalogError> {
        query
            .validate()
            .map_err(|error| CatalogError::invalid(error.to_string()))?;

        // A principal that cannot read even one repository sees nothing: the
        // search is not the way back around the permission.
        if repositories.is_empty() {
            return Ok(SearchPage::empty(query));
        }
        // The repositories that take part, in the order in which they
        // are walked. The attribute is decided with the descriptor, as in the
        // reference: the engine does not have to resolve it.
        let mut candidates: Vec<&RepositoryDescriptor> = repositories.iter().collect();
        candidates.sort_by(|left, right| left.repo_key.cmp(&right.repo_key));
        if let Some(named) = &query.repository {
            candidates.retain(|repository| &repository.repo_key == named);
        }
        if let Some(attribute) = &query.attribute {
            candidates.retain(|repository| the_attribute_matches(attribute, repository));
        }
        if candidates.is_empty() {
            return Ok(SearchPage::empty(query));
        }
        // The tag is resolved first: what narrows is its
        // coordinates, not its name.
        let targets = match &query.tag {
            Some(tag) => self.the_tag_targets(principal_account, tag).await?,
            None => Vec::new(),
        };
        if query.tag.is_some() && targets.is_empty() {
            return Ok(SearchPage::empty(query));
        }

        // The cursor: the page starts AFTER the key already seen, in the
        // total order (repository, path) in which the rows come out.
        let cursor = match &query.cursor {
            Some(cursor) => Some(parse_search_cursor(cursor).map_err(|error| {
                CatalogError::invalid(format!("the cursor cannot be read: {error}"))
            })?),
            None => None,
        };

        // The window that is requested, and one more row to know whether there are more.
        let window = query.offset + query.limit + 1;

        let mut collected: Vec<AssetRow> = Vec::new();
        let mut counted = 0usize;
        let mut exact = true;

        for repository in &candidates {
            // The key already seen leaves behind the repositories that come before
            // in the order and narrows its own.
            let after_path = match cursor.as_ref() {
                Some((cursor_repository, cursor_path)) => {
                    if repository.repo_key.as_str() < cursor_repository.as_str() {
                        continue;
                    }
                    (repository.repo_key.as_str() == cursor_repository.as_str())
                        .then_some(cursor_path.as_str())
                }
                None => None,
            };

            // The tag, in this repository. A tag that does not point
            // here narrows nothing here.
            let target = targets
                .iter()
                .find(|target| target.repository == repository.repo_key);
            if query.tag.is_some() && target.is_none() {
                continue;
            }

            // An open repository is walked in full; one served by the
            // trusted channel, only through the digests with an active
            // grant, in bounded groups.
            let groups = self
                .the_digest_groups(principal_account, repository, query)
                .await?;
            let mut repository_rows: Vec<AssetRow> = Vec::new();
            for group in &groups {
                let scope = WindowScope {
                    target,
                    after_path,
                    granted: group.as_ref(),
                };
                let rows = self
                    .the_asset_window(
                        principal_account,
                        &repository.repo_key,
                        query,
                        scope,
                        window,
                    )
                    .await?;

                if exact {
                    if rows.len() < window {
                        counted += rows.len();
                    } else {
                        let remaining = hodei_catalog::search::MAX_COUNTED.saturating_sub(counted);
                        if remaining == 0 {
                            exact = false;
                        } else {
                            let found = self
                                .the_remaining_count(
                                    principal_account,
                                    &repository.repo_key,
                                    query,
                                    scope,
                                    remaining,
                                )
                                .await?;
                            if found > remaining {
                                counted += remaining;
                                exact = false;
                            } else {
                                counted += found;
                            }
                        }
                    }
                }
                repository_rows.extend(rows);
            }
            // The groups are disjoint sets of digests and each one comes out
            // in path order: the union is sorted by path, which
            // is unique within the repository, and the window of the whole
            // account is the one that is kept.
            repository_rows.sort_by(|left, right| left.asset_path.cmp(&right.asset_path));
            collected.extend(repository_rows);
            collected.truncate(window);
        }

        if counted > hodei_catalog::search::MAX_COUNTED {
            exact = false;
        }

        // The page: the whole window, minus what remains behind. The cut
        // is over the total order of the rows: a page neither repeats nor
        // loses a result, and whoever asks by key re-paginates without
        // gaps.
        let start = if query.cursor.is_some() {
            0
        } else {
            query.offset.min(collected.len())
        };
        let end = if query.cursor.is_some() {
            query.limit.min(collected.len())
        } else {
            (query.offset + query.limit).min(collected.len())
        };
        let more = if query.cursor.is_some() {
            collected.len() > query.limit
        } else {
            collected.len() > query.offset + query.limit
        };
        let rows = &collected[start..end];

        // Trust is said by the grant ledger, not by the reference: the
        // reference keeps what admission said on publishing and never
        // changes, and a promotion or a revocation has to be seen on the
        // next request without rewriting anything.
        let granted = self.the_granted_hits(principal_account, rows).await?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let asset = AssetRow::into_record(
                row.clone(),
                principal_account,
                &row.repository,
                &row.asset_path,
            )?;
            let format = repository_format(&candidates, &asset.repository);
            let trust_state = if granted.contains(&(asset.repository.clone(), asset.digest.clone()))
            {
                TrustState::Trusted
            } else {
                TrustState::Quarantined
            };
            results.push(SearchHit {
                repository: asset.repository,
                format,
                asset_path: asset.asset_path,
                package_name: asset.coordinates.package_name,
                package_version: asset.coordinates.package_version,
                digest: asset.digest,
                size_bytes: asset.size_bytes,
                trust_state,
                updated_at: asset.updated_at,
            });
        }

        Ok(SearchPage {
            total: if exact { Some(counted) } else { None },
            more,
            limit: query.limit,
            offset: query.offset,
            results,
        })
    }
}

// ---------------------------------------------------------------------------
// The audit outbox over the graph.
// ---------------------------------------------------------------------------

/// The row of an event as it is in the graph.
#[derive(Debug, Clone)]
struct OutboxRow {
    event_id: String,
    tenant: String,
    actor: String,
    correlation_id: String,
    action: String,
    resource: String,
    digest: Option<String>,
    result: String,
    occurred_at: DateTime<Utc>,
    state: String,
    attempts: u64,
    lease_owner: Option<String>,
    lease_until: Option<DateTime<Utc>>,
    last_error: Option<String>,
    delivered_at: Option<DateTime<Utc>>,
}

/// The result of an event, said as text.
fn the_result(value: &str) -> AuditResult {
    match value {
        "allowed" => AuditResult::Allowed,
        "denied" => AuditResult::Denied,
        "conflict" => AuditResult::Conflict,
        _ => AuditResult::Failed,
    }
}

/// The state of an entry, said as text.
fn the_state(value: &str) -> EntryState {
    match value {
        "claimed" => EntryState::Claimed,
        "delivered" => EntryState::Delivered,
        _ => EntryState::Pending,
    }
}

/// An optional text of a node.
fn node_option_text(node: &serde_json::Value, field: &str) -> Option<String> {
    node.get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// An optional moment of a node.
fn node_option_time(node: &serde_json::Value, field: &str) -> Option<DateTime<Utc>> {
    node_option_text(node, field).and_then(|text| {
        DateTime::parse_from_rfc3339(&text)
            .ok()
            .map(|instant| instant.with_timezone(&Utc))
    })
}

impl OutboxRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        Ok(Self {
            event_id: node_text(node, "hodei.catalog.outbox.key")?,
            tenant: node_text(node, "hodei.catalog.outbox.tenant")?,
            actor: node_text(node, "hodei.catalog.outbox.actor")?,
            correlation_id: node_text(node, "hodei.catalog.outbox.correlation_id")?,
            action: node_text(node, "hodei.catalog.outbox.action")?,
            resource: node_text(node, "hodei.catalog.outbox.resource")?,
            digest: node_option_text(node, "hodei.catalog.outbox.digest"),
            result: node_text(node, "hodei.catalog.outbox.result")?,
            occurred_at: node_time(node, "hodei.catalog.outbox.occurred_at")?,
            state: node_text(node, "hodei.catalog.outbox.state")?,
            attempts: node
                .get("hodei.catalog.outbox.attempts")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            lease_owner: node_option_text(node, "hodei.catalog.outbox.lease_owner"),
            lease_until: node_option_time(node, "hodei.catalog.outbox.lease_until"),
            last_error: node_option_text(node, "hodei.catalog.outbox.last_error"),
            delivered_at: node_option_time(node, "hodei.catalog.outbox.delivered_at"),
        })
    }

    /// The event of the row, reconstructed.
    fn the_event(&self) -> AuditEvent {
        AuditEvent::new(
            self.event_id.clone(),
            AuditContext::new(
                self.actor.clone(),
                self.tenant.clone(),
                self.correlation_id.clone(),
            ),
            self.occurred_at,
            self.action.clone(),
            self.resource.clone(),
            self.digest.clone(),
            the_result(&self.result),
        )
    }

    /// The entry of the row, with its state.
    fn the_entry(&self) -> AuditEntry {
        AuditEntry {
            event: self.the_event(),
            state: the_state(&self.state),
            attempts: u32::try_from(self.attempts).unwrap_or(u32::MAX),
            lease_owner: self.lease_owner.clone(),
            lease_until: self.lease_until,
            last_error: self.last_error.clone(),
            delivered_at: self.delivered_at,
        }
    }
}

/// The storage failure, said as an audit error.
fn the_audit_error(error: impl std::fmt::Display) -> AuditError {
    AuditError::StoreUnavailable {
        detail: error.to_string(),
    }
}

/// The audit outbox over the graph. The **write** side is held
/// by the catalog (the event's row is written inside the
/// transaction that publishes); what is here is the **delivery** side:
/// claiming an event, marking it delivered and returning it to the market when
/// the delivery did not happen, each with its one-piece compare-and-set.
#[derive(Clone)]
pub struct DgraphAuditOutbox {
    client: Arc<GraphClient>,
}

impl std::fmt::Debug for DgraphAuditOutbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DgraphAuditOutbox").finish_non_exhaustive()
    }
}

impl DgraphAuditOutbox {
    /// The outbox over an already connected and authenticated client.
    pub fn new(client: Arc<GraphClient>) -> Self {
        Self { client }
    }

    /// The row of an event, if it is there.
    async fn the_row(&self, event_id: &str) -> Result<Option<OutboxRow>, AuditError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.outbox.key, {})) {{ {OUTBOX_FIELDS} }} }}",
            dql_string(event_id),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(the_audit_error)?;
        match single_row(response.json.get("q"), "outbox", event_id.to_string()) {
            Ok(None) => Ok(None),
            Ok(Some(node)) => OutboxRow::from_node(&node)
                .map(Some)
                .map_err(the_audit_error),
            Err(error) => Err(AuditError::StoreUnavailable {
                detail: error.to_string(),
            }),
        }
    }

    /// The upsert that says an event only once: the `@upsert` index of
    /// the key decides between two writers, and the re-read decides whether the
    /// second one said the same thing.
    fn put_upsert(&self, event: &AuditEvent) -> hodei_graph_client::GraphUpsertTransaction {
        hodei_graph_client::GraphUpsertTransaction {
            query: format!(
                "{{ ev as var(func: eq(hodei.catalog.outbox.key, {}))\n \
                 evq(func: eq(hodei.catalog.outbox.key, {})) {{ hodei.catalog.outbox.key }} }}",
                dql_string(&event.event_id),
                dql_string(&event.event_id),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(ev), 0)",
                outbox_event_nquads(event),
            )],
            commit_now: true,
        }
    }

    /// The upsert that claims a pending event (or one whose lease expired).
    fn claim_upsert(
        &self,
        event_id: &str,
        worker: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> hodei_graph_client::GraphUpsertTransaction {
        let until = lease_until.to_rfc3339_opts(SecondsFormat::Millis, true);
        let at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let claim = |binding: &str| {
            format!(
                "uid({binding}) <hodei.catalog.outbox.state> \"claimed\" .\n\
                 uid({binding}) <hodei.catalog.outbox.lease_owner> {} .\n\
                 uid({binding}) <hodei.catalog.outbox.lease_until> \"{until}\" .\n",
                dql_string(worker),
            )
        };
        hodei_graph_client::GraphUpsertTransaction {
            query: format!(
                "{{ ep as var(func: eq(hodei.catalog.outbox.key, {})) @filter(eq(hodei.catalog.outbox.state, \"pending\"))\n \
                 ex as var(func: eq(hodei.catalog.outbox.key, {})) @filter(eq(hodei.catalog.outbox.state, \"claimed\") AND le(hodei.catalog.outbox.lease_until, \"{at}\")) }}",
                dql_string(event_id),
                dql_string(event_id),
            ),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(ep), 1)", claim("ep")),
                GraphMutationBlock::set_if("eq(len(ep), 0) AND eq(len(ex), 1)", claim("ex")),
            ],
            commit_now: true,
        }
    }

    /// The upsert that marks the delivery, with the worker in the condition: a
    /// lease that expired is no longer a permission to mark anything.
    fn mark_delivered_upsert(
        &self,
        event_id: &str,
        worker: &str,
        at: DateTime<Utc>,
    ) -> hodei_graph_client::GraphUpsertTransaction {
        let stamp = at.to_rfc3339_opts(SecondsFormat::Millis, true);
        hodei_graph_client::GraphUpsertTransaction {
            query: format!(
                "{{ md as var(func: eq(hodei.catalog.outbox.key, {})) @filter(eq(hodei.catalog.outbox.state, \"claimed\") AND eq(hodei.catalog.outbox.lease_owner, {})) }}",
                dql_string(event_id),
                dql_string(worker),
            ),
            blocks: vec![GraphMutationBlock {
                cond: Some("eq(len(md), 1)".to_owned()),
                set_nquads: format!(
                    "uid(md) <hodei.catalog.outbox.state> \"delivered\" .\n\
                     uid(md) <hodei.catalog.outbox.delivered_at> \"{stamp}\" .\n",
                ),
                delete_nquads: "uid(md) <hodei.catalog.outbox.lease_owner> * .\n\
                     uid(md) <hodei.catalog.outbox.lease_until> * .\n"
                    .to_owned(),
            }],
            commit_now: true,
        }
    }

    /// The upsert that returns an event to the market, with the worker and the
    /// attempts already counted in the condition: a one-piece compare-and-set
    /// that cannot contradict another's delivery.
    fn mark_undelivered_upsert(
        &self,
        event_id: &str,
        worker: &str,
        detail: &str,
        attempts: u64,
    ) -> hodei_graph_client::GraphUpsertTransaction {
        let next = attempts + 1;
        hodei_graph_client::GraphUpsertTransaction {
            query: format!(
                "{{ mu as var(func: eq(hodei.catalog.outbox.key, {})) @filter(eq(hodei.catalog.outbox.state, \"claimed\") AND eq(hodei.catalog.outbox.lease_owner, {}) AND eq(hodei.catalog.outbox.attempts, {attempts})) }}",
                dql_string(event_id),
                dql_string(worker),
            ),
            blocks: vec![GraphMutationBlock {
                cond: Some("eq(len(mu), 1)".to_owned()),
                set_nquads: format!(
                    "uid(mu) <hodei.catalog.outbox.state> \"pending\" .\n\
                     uid(mu) <hodei.catalog.outbox.attempts> \"{next}\" .\n\
                     uid(mu) <hodei.catalog.outbox.last_error> {} .\n",
                    dql_string(&hodei_audit::sanitize_text(detail)),
                ),
                delete_nquads: "uid(mu) <hodei.catalog.outbox.lease_owner> * .\n\
                     uid(mu) <hodei.catalog.outbox.lease_until> * .\n"
                    .to_owned(),
            }],
            commit_now: true,
        }
    }
}

#[async_trait]
impl AuditOutbox for DgraphAuditOutbox {
    async fn put_pending(&self, event: &AuditEvent) -> Result<(), AuditError> {
        // Saying an event twice is saying it once. Before blaming the
        // storage, what is there is looked at.
        if let Some(existing) = self.the_row(&event.event_id).await? {
            return if existing.the_event().same_fact(event) {
                Ok(())
            } else {
                Err(AuditError::StoreUnavailable {
                    detail: format!(
                        "the event {} is already said with another content",
                        event.event_id
                    ),
                })
            };
        }
        for _attempt in 0..5 {
            match self.client.upsert(self.put_upsert(event)).await {
                Ok(response) => {
                    // The block is only applied if the row was free
                    // INSIDE the transaction: if it was not, the row
                    // appeared between the read and the write, and what the
                    // row says decides.
                    let applied = response
                        .json
                        .get("evq")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|nodes| nodes.is_empty());
                    if applied {
                        return Ok(());
                    }
                    if let Some(existing) = self.the_row(&event.event_id).await? {
                        return if existing.the_event().same_fact(event) {
                            Ok(())
                        } else {
                            Err(AuditError::StoreUnavailable {
                                detail: format!(
                                    "the event {} is already said with another content",
                                    event.event_id
                                ),
                            })
                        };
                    }
                }
                Err(GraphClientError::UpsertConflict { detail }) => {
                    // Two writers said the same identifier at the
                    // same time: the index decided; the re-read says whether the
                    // second one said the same thing.
                    tracing::debug!(event_id = %event.event_id, %detail, "the outbox write conflicted");
                }
                Err(other) => return Err(the_audit_error(other)),
            }
        }
        Err(AuditError::StoreUnavailable {
            detail: "the event could not be said after the attempts".to_owned(),
        })
    }

    async fn claim(
        &self,
        event_id: &str,
        worker: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<AuditEntry>, AuditError> {
        self.client
            .upsert(self.claim_upsert(event_id, worker, lease_until, now))
            .await
            .map_err(the_audit_error)?;
        // The state after the commit answers: two workers ask for the
        // same thing and the loser sees that the row is no longer theirs.
        let row = self.the_row(event_id).await?;
        Ok(row
            .filter(|row| {
                row.state == EntryState::Claimed.as_str()
                    && row.lease_owner.as_deref() == Some(worker)
            })
            .map(|row| row.the_entry()))
    }

    async fn mark_delivered(
        &self,
        event_id: &str,
        worker: &str,
        at: DateTime<Utc>,
    ) -> Result<(), AuditError> {
        // What is already true is not changed.
        if let Some(row) = self.the_row(event_id).await? {
            if row.state == EntryState::Delivered.as_str() {
                return Ok(());
            }
            if row.state != EntryState::Claimed.as_str() {
                return Err(AuditError::StoreUnavailable {
                    detail: format!("the event {event_id} is not claimed, so nobody delivered it"),
                });
            }
        }
        self.client
            .upsert(self.mark_delivered_upsert(event_id, worker, at))
            .await
            .map_err(the_audit_error)?;
        let row = self.the_row(event_id).await?;
        match row {
            Some(row) if row.state == EntryState::Delivered.as_str() => Ok(()),
            _ => Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is not claimed by {worker}, so nobody delivered it"
                ),
            }),
        }
    }

    async fn mark_undelivered(
        &self,
        event_id: &str,
        worker: &str,
        detail: &str,
    ) -> Result<(), AuditError> {
        // A delivery that already happened cannot be contradicted.
        let Some(row) = self.the_row(event_id).await? else {
            return Err(AuditError::StoreUnavailable {
                detail: format!("the event {event_id} is not there, so nobody can say it failed"),
            });
        };
        if row.state == EntryState::Delivered.as_str() {
            return Ok(());
        }
        if row.state != EntryState::Claimed.as_str() {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is not claimed by {worker}, so nobody can say it failed"
                ),
            });
        }
        self.client
            .upsert(self.mark_undelivered_upsert(event_id, worker, detail, row.attempts))
            .await
            .map_err(the_audit_error)?;
        let row = self.the_row(event_id).await?;
        match row {
            Some(row) if row.state == EntryState::Pending.as_str() => Ok(()),
            _ => Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is not claimed by {worker}, so nobody can say it failed"
                ),
            }),
        }
    }

    async fn pending(&self, now: DateTime<Utc>) -> Result<Vec<AuditEntry>, AuditError> {
        let at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let query = format!(
            "{{ pend(func: eq(hodei.catalog.outbox.state, \"pending\")) {{ {OUTBOX_FIELDS} }}\n \
             expired(func: eq(hodei.catalog.outbox.state, \"claimed\")) @filter(le(hodei.catalog.outbox.lease_until, \"{at}\")) {{ {OUTBOX_FIELDS} }} }}"
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(the_audit_error)?;
        let mut entries = Vec::new();
        for root in ["pend", "expired"] {
            let nodes = response
                .json
                .get(root)
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            for node in &nodes {
                let row = OutboxRow::from_node(node).map_err(the_audit_error)?;
                entries.push(row.the_entry());
            }
        }
        entries.sort_by(|left, right| left.event.event_id.cmp(&right.event.event_id));
        Ok(entries)
    }

    async fn entry(&self, event_id: &str) -> Result<Option<AuditEntry>, AuditError> {
        Ok(self.the_row(event_id).await?.map(|row| row.the_entry()))
    }
}

const APT_SNAPSHOT_MAX_PAYLOAD: usize = 900_000;

fn apt_namespace(key: &AptSnapshotKey) -> String {
    format!(
        "{}|{}|{}|{}",
        key_part(&key.account),
        key_part(&key.group),
        key_part(&key.distribution),
        key_part(&key.principal_subject)
    )
}

fn apt_revision_key(namespace: &str, revision: u64) -> String {
    format!("{namespace}|{revision}")
}

fn apt_snapshot_value(snapshot: &AptGroupSnapshot) -> serde_json::Value {
    let reference = |r: &AptCasReference| {
        serde_json::json!({
            "account": r.location.account,
            "scope": match r.location.scope { DigestScope::Global => "global", DigestScope::AccountPartitioned => "account_partitioned" },
            "algorithm": r.location.digest.algorithm.to_string(),
            "digest": r.location.digest.value,
            "size": r.size_bytes
        })
    };
    serde_json::json!({
        "release": reference(&snapshot.release), "signature": reference(&snapshot.signature),
        "packages": snapshot.packages_by_arch.iter().map(|(k,v)| (k.clone(), reference(v))).collect::<serde_json::Map<_,_>>(),
        "pool": snapshot.pool_by_path.iter().map(|(k,v)| (k.clone(), reference(v))).collect::<serde_json::Map<_,_>>(),
        "members": snapshot.members.iter().map(|m| serde_json::json!({"repository":m.repository,"package":m.package,"version":m.version,"content":reference(&m.content)})).collect::<Vec<_>>()
    })
}

fn apt_read_reference(value: &serde_json::Value) -> Result<AptCasReference, CatalogError> {
    let account = value["account"]
        .as_str()
        .ok_or_else(|| CatalogError::corrupt("APT snapshot", "account", "missing"))?
        .to_owned();
    let scope = match value["scope"].as_str() {
        Some("global") => DigestScope::Global,
        Some("account_partitioned") => DigestScope::AccountPartitioned,
        _ => return Err(CatalogError::corrupt("APT snapshot", "scope", "invalid")),
    };
    let algorithm = match value["algorithm"].as_str() {
        Some("sha256") => DigestAlgorithm::Sha256,
        Some("sha512") => DigestAlgorithm::Sha512,
        _ => {
            return Err(CatalogError::corrupt(
                "APT snapshot",
                "algorithm",
                "invalid",
            ));
        }
    };
    Ok(AptCasReference {
        location: BlobLocation::new(
            &account,
            scope,
            ContentDigest {
                algorithm,
                value: value["digest"]
                    .as_str()
                    .ok_or_else(|| CatalogError::corrupt("APT snapshot", "digest", "missing"))?
                    .to_owned(),
            },
        ),
        size_bytes: value["size"]
            .as_u64()
            .ok_or_else(|| CatalogError::corrupt("APT snapshot", "size", "invalid"))?,
    })
}

fn apt_snapshot_from_row(row: &serde_json::Value) -> Result<AptGroupSnapshot, CatalogError> {
    let payload: serde_json::Value = serde_json::from_str(
        row["hodei.catalog.apt_snapshot.payload"]
            .as_str()
            .ok_or_else(|| CatalogError::corrupt("APT snapshot", "payload", "missing"))?,
    )
    .map_err(|e| CatalogError::corrupt("APT snapshot", "payload", e.to_string()))?;
    let revision = row["hodei.catalog.apt_snapshot.revision"]
        .as_u64()
        .ok_or_else(|| CatalogError::corrupt("APT snapshot", "revision", "invalid"))?;
    let digest_text = row["hodei.catalog.apt_snapshot.digest"]
        .as_str()
        .ok_or_else(|| CatalogError::corrupt("APT snapshot", "digest", "missing"))?;
    let (alg, value) = digest_text
        .split_once(':')
        .ok_or_else(|| CatalogError::corrupt("APT snapshot", "digest", "invalid"))?;
    let algorithm = match alg {
        "sha256" => DigestAlgorithm::Sha256,
        "sha512" => DigestAlgorithm::Sha512,
        _ => return Err(CatalogError::corrupt("APT snapshot", "digest", "algorithm")),
    };
    let parse_map = |v: &serde_json::Value| -> Result<
        std::collections::BTreeMap<String, AptCasReference>,
        CatalogError,
    > {
        v.as_object()
            .ok_or_else(|| CatalogError::corrupt("APT snapshot", "references", "invalid"))?
            .iter()
            .map(|(k, v)| Ok((k.clone(), apt_read_reference(v)?)))
            .collect()
    };
    let members = payload["members"]
        .as_array()
        .ok_or_else(|| CatalogError::corrupt("APT snapshot", "members", "invalid"))?
        .iter()
        .map(|m| {
            Ok(AptSnapshotMember {
                repository: m["repository"]
                    .as_str()
                    .ok_or_else(|| CatalogError::corrupt("APT snapshot", "member", "repository"))?
                    .to_owned(),
                package: m["package"]
                    .as_str()
                    .ok_or_else(|| CatalogError::corrupt("APT snapshot", "member", "package"))?
                    .to_owned(),
                version: m["version"]
                    .as_str()
                    .ok_or_else(|| CatalogError::corrupt("APT snapshot", "member", "version"))?
                    .to_owned(),
                content: apt_read_reference(&m["content"])?,
            })
        })
        .collect::<Result<Vec<_>, CatalogError>>()?;
    Ok(AptGroupSnapshot {
        revision,
        digest: ContentDigest {
            algorithm,
            value: value.to_owned(),
        },
        release: apt_read_reference(&payload["release"])?,
        signature: apt_read_reference(&payload["signature"])?,
        packages_by_arch: parse_map(&payload["packages"])?,
        pool_by_path: parse_map(&payload["pool"])?,
        members,
    })
}

#[async_trait]
impl AptSnapshotStore for DgraphCatalogStore {
    async fn current_apt_snapshot(
        &self,
        key: &AptSnapshotKey,
    ) -> Result<Option<AptGroupSnapshot>, CatalogError> {
        let ns = apt_namespace(key);
        let q = format!(
            "{{ p(func:eq(hodei.catalog.apt_pointer.key,{})) {{ hodei.catalog.apt_pointer.revision }} }}",
            dql_string(&ns)
        );
        let r = self
            .client
            .execute_dql(&q)
            .await
            .map_err(|e| CatalogError::unavailable(e.to_string()))?;
        let Some(rev) = r.json["p"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|p| p["hodei.catalog.apt_pointer.revision"].as_u64())
        else {
            return Ok(None);
        };
        self.apt_snapshot_by_revision(key, rev).await
    }
    async fn apt_snapshot_by_revision(
        &self,
        key: &AptSnapshotKey,
        revision: u64,
    ) -> Result<Option<AptGroupSnapshot>, CatalogError> {
        let sk = apt_revision_key(&apt_namespace(key), revision);
        let q = format!(
            "{{ s(func:eq(hodei.catalog.apt_snapshot.key,{})) {{ hodei.catalog.apt_snapshot.namespace hodei.catalog.apt_snapshot.revision hodei.catalog.apt_snapshot.digest hodei.catalog.apt_snapshot.payload }} }}",
            dql_string(&sk)
        );
        let r = self
            .client
            .execute_dql(&q)
            .await
            .map_err(|e| CatalogError::unavailable(e.to_string()))?;
        let Some(row) = r.json["s"].as_array().and_then(|a| a.first()) else {
            return Ok(None);
        };
        if row["hodei.catalog.apt_snapshot.namespace"].as_str() != Some(apt_namespace(key).as_str())
        {
            return Ok(None);
        }
        Ok(Some(apt_snapshot_from_row(row)?))
    }
    async fn apt_snapshot_by_packages_hash(
        &self,
        key: &AptSnapshotKey,
        architecture: &str,
        digest: &ContentDigest,
    ) -> Result<Option<AptGroupSnapshot>, CatalogError> {
        let ns = apt_namespace(key);
        let q = format!(
            "{{ h(func:eq(hodei.catalog.apt_packages_hash.namespace,{}), orderdesc:hodei.catalog.apt_packages_hash.revision, first:1) @filter(eq(hodei.catalog.apt_packages_hash.architecture,{}) AND eq(hodei.catalog.apt_packages_hash.digest,{})) {{ hodei.catalog.apt_packages_hash.snapshot_key }} }}",
            dql_string(&ns),
            dql_string(architecture),
            dql_string(&format!("{}:{}", digest.algorithm, digest.value))
        );
        let r = self
            .client
            .execute_dql(&q)
            .await
            .map_err(|e| CatalogError::unavailable(e.to_string()))?;
        let Some(sk) = r.json["h"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v["hodei.catalog.apt_packages_hash.snapshot_key"].as_str())
        else {
            return Ok(None);
        };
        let Some(rev) = sk.rsplit('|').next().and_then(|s| s.parse::<u64>().ok()) else {
            return Err(CatalogError::corrupt(
                "APT snapshot",
                "hash index",
                "bad snapshot key",
            ));
        };
        self.apt_snapshot_by_revision(key, rev).await
    }
    async fn publish_apt_snapshot(
        &self,
        key: &AptSnapshotKey,
        expected_revision: Option<u64>,
        snapshot: AptGroupSnapshot,
    ) -> Result<(), CatalogError> {
        let ns = apt_namespace(key);
        let sk = apt_revision_key(&ns, snapshot.revision);
        if snapshot.revision != expected_revision.map_or(1, |r| r.saturating_add(1)) {
            return Err(CatalogError::PreconditionFailed {
                kind: "APT snapshot",
                key: ns,
                detail: "revision must follow expected revision".into(),
            });
        }
        let refs = std::iter::once(&snapshot.release)
            .chain(std::iter::once(&snapshot.signature))
            .chain(snapshot.packages_by_arch.values())
            .chain(snapshot.pool_by_path.values())
            .chain(snapshot.members.iter().map(|m| &m.content));
        if refs.into_iter().any(|r| r.location.account != key.account) {
            return Err(CatalogError::invalid(
                "APT snapshot blob belongs to another account",
            ));
        }
        if snapshot.members.len() > 10_000
            || snapshot.pool_by_path.len() > 50_000
            || snapshot.packages_by_arch.len() > 256
        {
            return Err(CatalogError::invalid(
                "APT snapshot exceeds collection limits",
            ));
        }
        let payload = apt_snapshot_value(&snapshot).to_string();
        if payload.len() > APT_SNAPSHOT_MAX_PAYLOAD {
            return Err(CatalogError::invalid(
                "APT snapshot payload exceeds 900000 bytes",
            ));
        }
        let nsq = dql_string(&ns);
        let skq = dql_string(&sk);
        let ptrset = if expected_revision.is_none() {
            format!(
                "_:ptr <hodei.catalog.apt_pointer.revision> \"{}\" .\n",
                snapshot.revision
            )
        } else {
            format!(
                "uid(ptr) <hodei.catalog.apt_pointer.revision> \"{}\" .\n",
                snapshot.revision
            )
        };
        let pointer = if expected_revision.is_none() {
            format!("_:ptr <hodei.catalog.apt_pointer.key> {nsq} .\n{ptrset}")
        } else {
            ptrset
        };
        let mut set = format!(
            "_:s <hodei.catalog.apt_snapshot.key> {skq} .\n_:s <hodei.catalog.apt_snapshot.namespace> {nsq} .\n_:s <hodei.catalog.apt_snapshot.revision> \"{}\" .\n_:s <hodei.catalog.apt_snapshot.digest> {} .\n_:s <hodei.catalog.apt_snapshot.payload> {} .\n",
            snapshot.revision,
            dql_string(&format!(
                "{}:{}",
                snapshot.digest.algorithm, snapshot.digest.value
            )),
            dql_string(&payload)
        );
        set.push_str(&pointer);
        let mut apt_roots = vec![
            ("release".to_owned(), &snapshot.release),
            ("signature".to_owned(), &snapshot.signature),
        ];
        apt_roots.extend(
            snapshot
                .packages_by_arch
                .iter()
                .map(|(architecture, reference)| (format!("packages/{architecture}"), reference)),
        );
        apt_roots.extend(
            snapshot
                .pool_by_path
                .iter()
                .map(|(path, reference)| (format!("pool/{path}"), reference)),
        );
        apt_roots.extend(
            snapshot
                .members
                .iter()
                .enumerate()
                .map(|(index, member)| (format!("member/{index}"), &member.content)),
        );
        for (index, (subject, reference)) in apt_roots.iter().enumerate() {
            let root_key = format!("{ns}|{}|{}", snapshot.revision, key_part(subject));
            set.push_str(&format!(
                "_:ar{index} <dgraph.type> \"CatalogAptSnapshotRoot\" .\n_:ar{index} <hodei.catalog.apt_snapshot_root.key> {} .\n_:ar{index} <hodei.catalog.apt_snapshot_root.account> {} .\n_:ar{index} <hodei.catalog.apt_snapshot_root.repository> {} .\n_:ar{index} <hodei.catalog.apt_snapshot_root.subject> {} .\n_:ar{index} <hodei.catalog.apt_snapshot_root.digest_algorithm> {} .\n_:ar{index} <hodei.catalog.apt_snapshot_root.digest> {} .\n_:ar{index} <hodei.catalog.apt_snapshot_root.size_bytes> \"{}\" .\n",
                dql_string(&root_key),
                dql_string(&key.account),
                dql_string(&key.group),
                dql_string(subject),
                dql_string(&reference.location.digest.algorithm.to_string()),
                dql_string(&reference.location.digest.value),
                reference.size_bytes,
            ));
        }
        for (index, (arch, cas)) in snapshot.packages_by_arch.iter().enumerate() {
            let hk = format!(
                "{ns}|{}|{}:{}|{}",
                key_part(arch),
                cas.location.digest.algorithm,
                cas.location.digest.value,
                snapshot.revision
            );
            set.push_str(&format!("_:h{index} <hodei.catalog.apt_packages_hash.key> {} .\n_:h{index} <hodei.catalog.apt_packages_hash.snapshot_key> {skq} .\n_:h{index} <hodei.catalog.apt_packages_hash.namespace> {nsq} .\n_:h{index} <hodei.catalog.apt_packages_hash.architecture> {} .\n_:h{index} <hodei.catalog.apt_packages_hash.digest> {} .\n_:h{index} <hodei.catalog.apt_packages_hash.revision> \"{}\" .\n", dql_string(&hk), dql_string(arch), dql_string(&format!("{}:{}", cas.location.digest.algorithm, cas.location.digest.value)), snapshot.revision));
        }
        let condition = match expected_revision {
            None => "eq(len(ptr),0)".to_owned(),
            Some(_) => "eq(len(ptr),1)".to_owned(),
        };
        let query = if let Some(expected) = expected_revision {
            format!(
                "{{ ptr as var(func:eq(hodei.catalog.apt_pointer.key,{nsq})) @filter(eq(hodei.catalog.apt_pointer.revision,{expected})) s as var(func:eq(hodei.catalog.apt_snapshot.key,{skq})) }}"
            )
        } else {
            format!(
                "{{ ptr as var(func:eq(hodei.catalog.apt_pointer.key,{nsq})) s as var(func:eq(hodei.catalog.apt_snapshot.key,{skq})) }}"
            )
        };
        let transaction = GraphUpsertTransaction {
            query,
            blocks: vec![GraphMutationBlock::set_if(
                format!("{condition} AND eq(len(s),0)"),
                set,
            )],
            commit_now: true,
        };
        match self.client.upsert(transaction).await {
            Ok(_) => {}
            Err(GraphClientError::UpsertConflict { .. }) => {
                return Err(CatalogError::PreconditionFailed {
                    kind: "APT snapshot",
                    key: ns.clone(),
                    detail: "expected revision mismatch or immutable key already exists".into(),
                });
            }
            Err(e) => return Err(CatalogError::unavailable(e.to_string())),
        }
        let saved = self
            .apt_snapshot_by_revision(key, snapshot.revision)
            .await?;
        if saved.as_ref() != Some(&snapshot) {
            return Err(CatalogError::PreconditionFailed {
                kind: "APT snapshot",
                key: ns,
                detail: "revision row was not committed".into(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_pattern_is_a_literal_case_insensitive_regex() {
        assert_eq!(the_text_pattern("LabPkg").unwrap(), "/LabPkg/i");
        assert_eq!(the_text_pattern("a.b").unwrap(), "/a\\.b/i");
        assert_eq!(the_text_pattern("dist/x").unwrap(), "/dist\\/x/i");
        assert_eq!(the_text_pattern("(1)+[x]").unwrap(), "/\\(1\\)\\+\\[x\\]/i");
        assert_eq!(the_text_pattern("a\\b").unwrap(), "/a\\\\b/i");
        assert_eq!(the_text_pattern("a b-c_d").unwrap(), "/a b-c_d/i");
    }

    #[test]
    fn the_text_pattern_refuses_control_characters() {
        assert!(matches!(
            the_text_pattern("a\nb"),
            Err(CatalogError::InvalidRequest { .. })
        ));
        assert!(the_text_pattern("a\tb").is_err());
    }

    #[test]
    fn guard_part_escapes_the_debian_charset() {
        assert_eq!(guard_part("1.2+dfsg"), "1.2@2Bdfsg");
        assert_eq!(guard_part("1.0.0~rc1"), "1.0.0@7Erc1");
        assert_eq!(guard_part("plain/path.deb"), "plain/path.deb");
        assert_eq!(guard_part("a%b"), "a@25b");
        assert_eq!(guard_part("a@b"), "a@@b");
    }

    #[test]
    fn guard_part_is_injective() {
        // An escaped byte must not collide with a verbatim one, and two
        // different escapes must not collide with each other.
        assert_ne!(guard_part("a+b"), guard_part("a@2B"));
        assert_ne!(guard_part("a~b"), guard_part("a@7E"));
        assert_ne!(guard_part("a+b~c"), guard_part("a@2Bb~c"));
    }

    #[test]
    fn the_apt_guard_scope_opens_for_debian_versions() {
        // The regression HSP-195 fixes: a Debian pool path with `+` and `~`
        // used to be refused by the guard on Dgraph only, while Surreal
        // accepted it.
        let scope = asset_guard_scope(
            "alice",
            "debian",
            "pool/main/h/hello_1.0.0~rc1+deb12_amd64.deb",
        )
        .expect("a debian version is guardable");
        assert_eq!(
            scope.subject(),
            "asset/debian/pool/main/h/hello_1.0.0@7Erc1@2Bdeb12_amd64.deb"
        );
    }

    #[test]
    fn a_go_module_path_with_the_case_escape_is_guardable() {
        // HSP-314 P4: the canonical GOPROXY escape (`!azure` for `Azure`)
        // used to be refused by the guard charset on Dgraph only (the
        // publication answered 400 "not well formed"), while Surreal
        // accepted it.
        let scope = asset_guard_scope("alice", "go", "github.com/!azure/azure-sdk/@v/v1.0.0.zip")
            .expect("an escaped go module path is guardable");
        assert_eq!(
            scope.subject(),
            "asset/go/github.com/@21azure/azure-sdk/@@v/v1.0.0.zip"
        );
        // The map stays injective for the new escape: a lowercase path, a
        // case-escaped path and a path that spells the escape differ.
        assert_ne!(guard_part("a!b"), guard_part("ab"));
        assert_ne!(guard_part("a!b"), guard_part("a@21b"));
        // Any byte outside the charset is escaped, including non-ASCII.
        assert_eq!(guard_part("a b"), "a@20b");
        assert_eq!(guard_part("\u{e9}"), "@C3@A9");
    }

    #[test]
    fn two_debian_versions_keep_two_distinct_guards() {
        let left = asset_guard_scope("alice", "debian", "pool/main/h/hello_1.0_amd64.deb").unwrap();
        let right =
            asset_guard_scope("alice", "debian", "pool/main/h/hello_1.0~1_amd64.deb").unwrap();
        assert_ne!(
            hodei_graph_client::guard::guard_key(&left),
            hodei_graph_client::guard::guard_key(&right)
        );
    }

    #[test]
    fn keys_still_carry_the_raw_path() {
        // The escape lives only on the guard subject: the stored composite
        // key keeps the path verbatim so reads and listings are unchanged.
        let key = asset_key(
            "alice",
            "debian",
            "pool/main/h/hello_1.0.0~rc1+deb12_amd64.deb",
        );
        assert!(key.contains("hello_1.0.0~rc1+deb12_amd64.deb"));
    }
}
