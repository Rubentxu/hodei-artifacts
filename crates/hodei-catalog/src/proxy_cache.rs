//! The cache policy of a proxy repository and the freshness decision.
//!
//! This module is **pure**: it does not touch the network, the clock or the store. It receives
//! the policy, what the provenance ledger knows about a path, and the time, and
//! answers one question: what to do with this request. Whoever executes
//! the decision (fetch, revalidate, serve, reject) is the service.
//!
//! # The configuration
//!
//! It lives in `config_settings.proxy.cache`, next to `proxy.upstream`:
//!
//! ```json
//! { "proxy": { "cache": {
//!     "ttl": { "content_secs": null, "metadata_secs": 300 },
//!     "metadata_paths": ["maven-metadata.xml", "dists/"],
//!     "negative_secs": 60,
//!     "stale_if_error_secs": 3600,
//!     "offline": false
//! } } }
//! ```
//!
//! - **Two kinds of path.** *Content* (a jar, a layer) is identified
//!   by its bytes and does not change: its default TTL is "does not expire". *Metadata*
//!   (an index, a `maven-metadata.xml`) is mutable by
//!   nature: it is what gets revalidated. A `metadata_paths` entry without a
//!   trailing `/` is the **name of the last segment**; with a trailing `/`, a
//!   path prefix.
//! - **TTL.** Absent = does not expire (never revalidated by time); `0` =
//!   revalidated on every read. An expired TTL serves nothing by itself:
//!   it triggers a conditional revalidation (`If-None-Match`,
//!   `If-Modified-Since`) and a `304` renews the receipt without moving bytes.
//! - **`stale_if_error_secs`.** How long **beyond** the TTL the cached
//!   content is served if the upstream fails. `0` (default) = expired content
//!   is never served.
//! - **`negative_secs`.** How long a `404` is remembered. `0` (default) =
//!   not remembered: a `404` is not a guarantee of permanent absence.
//! - **`offline`.** The repository does not go out to the network: it serves only what it already
//!   has with provenance.

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use serde::Deserialize;
use serde_json::Value;

use crate::model::RepositoryFormat;
use crate::proxy::{ChecksumEvidence, ProvenanceReceipt, ReceiptOutcome, UpstreamConfigError};

/// The cap on any policy duration: ten years.
pub const MAX_CACHE_SECS: u64 = 315_360_000;

/// The metadata names that are recognized if the policy does not declare
/// `metadata_paths`: indexes that the usual formats republish.
pub const DEFAULT_METADATA_NAMES: &[&str] = &[
    "maven-metadata.xml",
    "repomd.xml",
    "Packages",
    "Packages.gz",
    "Packages.xz",
    "Sources",
    "Sources.gz",
    "Sources.xz",
    "Release",
    "Release.gpg",
    "InRelease",
    "index.json",
];

/// Suffixes of the files that accompany a piece of metadata (its checksum or its
/// signature): they are republished with it and treated the same.
const SIDECAR_SUFFIXES: &[&str] = &[
    ".md5", ".sha1", ".sha256", ".sha512", ".asc", ".sig", ".gpg",
];

fn invalid(reason: impl Into<String>) -> UpstreamConfigError {
    UpstreamConfigError(reason.into())
}

/// Whether the path is a file of a `-SNAPSHOT` version with Maven's
/// floating name (`.../1.0-SNAPSHOT/widget-1.0-SNAPSHOT.jar`).
///
/// That name points to the latest build and the upstream republishes it:
/// a change is expected, not a breach. Files with a
/// timestamp (`widget-1.0-20240101.120000-1.jar`) do not carry the name of
/// the version and remain immutable.
fn is_floating_snapshot(asset_path: &str) -> bool {
    let mut segments = asset_path.rsplit('/');
    let (Some(file), Some(directory), Some(artifact)) =
        (segments.next(), segments.next(), segments.next())
    else {
        return false;
    };
    if !directory.ends_with("-SNAPSHOT") {
        return false;
    }
    // `{artifactId}-{version}` and then the extension or a classifier.
    let floating = format!("{artifact}-{directory}");
    file.strip_prefix(floating.as_str())
        .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('-'))
}

/// What kind of path it is: how a change is treated depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    /// Bytes identified by their digest that must not change.
    ///
    /// An upstream that delivers **other** bytes for the same path is not
    /// updating: it is in breach, and the content is put in
    /// quarantine.
    Content,
    /// An index that the upstream republishes. A change is expected.
    Metadata,
}

/// The cache policy of a proxy repository, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachePolicy {
    /// Freshness seconds of the content. `None` = does not expire.
    pub content_ttl: Option<u64>,
    /// Freshness seconds of the metadata. `None` = does not expire.
    pub metadata_ttl: Option<u64>,
    /// Last-segment names (without trailing `/`) and prefixes (with trailing
    /// `/`) that are metadata.
    pub metadata_paths: Vec<String>,
    /// Seconds a `404` is remembered. `0` = not remembered.
    pub negative_secs: u64,
    /// Seconds beyond the TTL during which content is served if the upstream
    /// fails. `0` = never.
    pub stale_if_error_secs: u64,
    /// Whether the repository works without network.
    pub offline: bool,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            content_ttl: None,
            metadata_ttl: None,
            metadata_paths: DEFAULT_METADATA_NAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            negative_secs: 0,
            stale_if_error_secs: 0,
            offline: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCache {
    #[serde(default)]
    ttl: Option<RawTtl>,
    #[serde(default)]
    metadata_paths: Option<Vec<String>>,
    #[serde(default)]
    negative_secs: Option<u64>,
    #[serde(default)]
    stale_if_error_secs: Option<u64>,
    #[serde(default)]
    offline: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTtl {
    #[serde(default)]
    content_secs: Option<u64>,
    #[serde(default)]
    metadata_secs: Option<u64>,
}

impl CachePolicy {
    /// Validates the policy written as JSON.
    pub fn parse(value: &Value) -> Result<Self, UpstreamConfigError> {
        let raw: RawCache = serde_json::from_value(value.clone())
            .map_err(|error| invalid(format!("the cache settings cannot be read: {error}")))?;

        let (content_ttl, metadata_ttl) = match raw.ttl {
            Some(ttl) => (ttl.content_secs, ttl.metadata_secs),
            None => (None, None),
        };
        let negative_secs = raw.negative_secs.unwrap_or(0);
        let stale_if_error_secs = raw.stale_if_error_secs.unwrap_or(0);
        for (name, seconds) in [
            ("ttl.content_secs", content_ttl.unwrap_or(0)),
            ("ttl.metadata_secs", metadata_ttl.unwrap_or(0)),
            ("negative_secs", negative_secs),
            ("stale_if_error_secs", stale_if_error_secs),
        ] {
            if seconds > MAX_CACHE_SECS {
                return Err(invalid(format!(
                    "{name} cannot be more than {MAX_CACHE_SECS} seconds"
                )));
            }
        }

        let metadata_paths = match raw.metadata_paths {
            Some(paths) => {
                if paths.len() > 64 {
                    return Err(invalid("metadata_paths cannot have more than 64 entries"));
                }
                for path in &paths {
                    Self::check_metadata_path(path)?;
                }
                paths
            }
            None => Self::default().metadata_paths,
        };

        Ok(Self {
            content_ttl,
            metadata_ttl,
            metadata_paths,
            negative_secs,
            stale_if_error_secs,
            offline: raw.offline,
        })
    }

    fn check_metadata_path(raw: &str) -> Result<(), UpstreamConfigError> {
        let name = raw.strip_suffix('/').unwrap_or(raw);
        let prefix = raw.ends_with('/');
        let bad = name.is_empty()
            || raw.starts_with('/')
            || raw.contains('\\')
            || raw.contains('?')
            || raw.contains('#')
            || raw.chars().any(char::is_control)
            || name.split('/').any(|segment| segment.is_empty() || segment == ".." || segment == ".")
            // Without a trailing slash it is a segment name: it carries no slashes.
            || (!prefix && name.contains('/'));
        if bad {
            return Err(invalid(
                "a metadata path is a file name, or a relative prefix ending in /",
            ));
        }
        Ok(())
    }

    /// The kind of a relative path.
    pub fn kind_of(&self, asset_path: &str) -> CacheKind {
        let last = asset_path.rsplit('/').next().unwrap_or(asset_path);
        // `maven-metadata.xml.sha1` changes when `maven-metadata.xml` changes:
        // it is decided by the name it accompanies.
        let named = SIDECAR_SUFFIXES
            .iter()
            .find_map(|suffix| last.strip_suffix(suffix))
            .filter(|base| !base.is_empty())
            .unwrap_or(last);
        let metadata = self.metadata_paths.iter().any(|entry| {
            if entry.ends_with('/') {
                asset_path.starts_with(entry.as_str())
            } else {
                last == entry || named == entry
            }
        });
        if metadata || is_floating_snapshot(asset_path) {
            CacheKind::Metadata
        } else {
            CacheKind::Content
        }
    }

    /// The kind of a path according to the repository format.
    ///
    /// In npm the immutable content is the tarballs (`{name}/-/{file}.tgz`)
    /// and everything else is a packument, which the upstream republishes. The rest of
    /// formats are governed by the policy's metadata paths.
    pub fn kind_in(&self, format: RepositoryFormat, asset_path: &str) -> CacheKind {
        match format {
            RepositoryFormat::Npm => {
                if crate::npm::is_tarball_path(asset_path) {
                    CacheKind::Content
                } else {
                    CacheKind::Metadata
                }
            }
            RepositoryFormat::PyPi => {
                // A project's document (`{project}`) is metadata: the
                // upstream publishes new versions. Distributions
                // (`{project}/{file}`) are immutable content.
                if asset_path.contains('/') {
                    CacheKind::Content
                } else {
                    CacheKind::Metadata
                }
            }
            RepositoryFormat::Terraform => {
                // Provider zips are immutable: a published version
                // does not change its bytes. The protocol's JSON documents
                // (`index.json`, a provider's `index.json`, `{version}.json`)
                // are revalidatable: the upstream publishes new versions.
                if asset_path.ends_with(".zip") {
                    CacheKind::Content
                } else {
                    CacheKind::Metadata
                }
            }
            _ => self.kind_of(asset_path),
        }
    }

    /// The TTL that applies to a kind of path.
    pub fn ttl_of(&self, kind: CacheKind) -> Option<u64> {
        match kind {
            CacheKind::Content => self.content_ttl,
            CacheKind::Metadata => self.metadata_ttl,
        }
    }
}

/// A receipt that backs the content in the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    /// The upstream that backed it.
    pub upstream_id: String,
    /// The configuration version it was backed with.
    pub config_version: u64,
    /// When it was fetched or revalidated.
    pub at: DateTime<Utc>,
    /// The upstream's entity tag.
    pub etag: Option<String>,
    /// The upstream's `Last-Modified`.
    pub last_modified: Option<String>,
    /// What the content was checked with then.
    pub checksum: ChecksumEvidence,
}

/// What the ledger knows about the content that is at a path today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    /// The digest of the content the catalog serves for the path.
    pub digest: ContentDigest,
    /// The bytes of that content.
    pub size_bytes: u64,
    /// The most recent receipt that backs **that** digest, if there is any.
    ///
    /// Without it, the content is in the catalog but has no demonstrable
    /// provenance: it is not served offline and is revalidated online.
    pub confirmation: Option<Confirmation>,
    /// The configuration version under which the upstream delivered bytes that
    /// are not the expected ones, if the last receipt is a quarantine one.
    pub quarantined_at: Option<u64>,
}

impl CacheEntry {
    /// What the receipts of a path (most recent first) say about
    /// the digest the catalog serves today.
    pub fn of(
        asset_digest: &ContentDigest,
        size_bytes: u64,
        receipts: &[ProvenanceReceipt],
    ) -> Self {
        let quarantined_at = receipts
            .first()
            .filter(|receipt| receipt.outcome == ReceiptOutcome::Quarantined)
            .map(|receipt| receipt.config_version);
        let confirmation = receipts
            .iter()
            .find(|receipt| {
                receipt.outcome != ReceiptOutcome::Quarantined && receipt.digest == *asset_digest
            })
            .map(|receipt| Confirmation {
                upstream_id: receipt.upstream_id.clone(),
                config_version: receipt.config_version,
                at: receipt.fetched_at,
                etag: receipt.etag.clone(),
                last_modified: receipt.last_modified.clone(),
                checksum: receipt.checksum.clone(),
            });
        Self {
            digest: asset_digest.clone(),
            size_bytes,
            confirmation,
            quarantined_at,
        }
    }
}

/// What is needed to revalidate a content that is already held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    /// The digest that is held.
    pub digest: ContentDigest,
    /// The bytes of that content.
    pub size_bytes: u64,
    /// What it was checked with when it was backed, if there is a current backing: it is
    /// what a `304` inherits, since it brings no body to check.
    pub checksum: Option<ChecksumEvidence>,
    /// The `ETag` validator the upstream gave, if it gave one and it is still valid.
    pub etag: Option<String>,
    /// The `Last-Modified` validator, likewise.
    pub last_modified: Option<String>,
    /// Whether the content is immutable: other bytes for the same path are a
    /// breach by the upstream and not an update.
    ///
    /// A revalidation done because the configuration changed after
    /// a quarantine is **not** immutable: whoever administers changed the
    /// configuration knowing there was retained content, and that change is
    /// the decision to accept what the upstream serves now.
    pub immutable: bool,
}

/// What is done with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheDecision {
    /// What is there is served, without touching the upstream.
    Fresh,
    /// The upstream is asked whether what is there is still valid.
    Revalidate(Known),
    /// There is nothing: it is fetched.
    Fetch,
    /// The upstream delivered other bytes for an immutable content and that
    /// configuration has not been changed since.
    Quarantined,
    /// The path no longer falls under any scope: the policy change hides it.
    OutOfScope,
    /// Without network: what is there is served, which has provenance.
    OfflineServe,
    /// Without network and without servable content with provenance.
    OfflineMiss,
}

/// The context of a request.
#[derive(Debug, Clone)]
pub struct Context<'a> {
    /// The repository format.
    pub format: RepositoryFormat,
    /// The policy in force.
    pub policy: &'a CachePolicy,
    /// The relative path.
    pub asset_path: &'a str,
    /// The upstream in force.
    pub upstream_id: &'a str,
    /// Whether the path today falls under some upstream scope.
    pub in_scope: bool,
    /// The current version of the repository configuration.
    pub config_version: u64,
    /// Whether the network cannot be used (the policy or the service).
    pub offline: bool,
    /// The time of the decision.
    pub now: DateTime<Utc>,
    /// The kind of the path, when whoever decides already knows it.
    ///
    /// The OCI surface knows it: a manifest requested by **tag** is
    /// metadata --the tag moves-- and the same manifest requested by
    /// **digest** is content --the digest does not move--. Without this, both
    /// would be governed by the same rule, and the digest would end up moving.
    pub kind: Option<CacheKind>,
}

/// The age of a confirmation, in seconds (never negative).
fn age_secs(confirmation: &Confirmation, now: DateTime<Utc>) -> u64 {
    now.signed_duration_since(confirmation.at)
        .num_seconds()
        .max(0) as u64
}

/// Decides what to do with a request for a proxy path.
pub fn decide(ctx: &Context<'_>, entry: Option<&CacheEntry>) -> CacheDecision {
    if !ctx.in_scope {
        return CacheDecision::OutOfScope;
    }
    let Some(entry) = entry else {
        return if ctx.offline {
            CacheDecision::OfflineMiss
        } else {
            CacheDecision::Fetch
        };
    };

    // A quarantine lasts as long as the configuration does not change: it is the
    // only lever of whoever administers to say "now yes".
    let mut forced = false;
    if let Some(version) = entry.quarantined_at {
        if ctx.offline || version == ctx.config_version {
            return CacheDecision::Quarantined;
        }
        forced = true;
    }

    let current = entry
        .confirmation
        .as_ref()
        .filter(|confirmation| confirmation.upstream_id == ctx.upstream_id);

    if ctx.offline {
        return match current {
            Some(_) => CacheDecision::OfflineServe,
            None => CacheDecision::OfflineMiss,
        };
    }

    let kind = ctx
        .kind
        .unwrap_or_else(|| ctx.policy.kind_in(ctx.format, ctx.asset_path));
    let known = |with_validators: bool| Known {
        digest: entry.digest.clone(),
        size_bytes: entry.size_bytes,
        checksum: current
            .filter(|_| with_validators)
            .map(|c| c.checksum.clone()),
        etag: current
            .filter(|_| with_validators)
            .and_then(|c| c.etag.clone()),
        last_modified: current
            .filter(|_| with_validators)
            .and_then(|c| c.last_modified.clone()),
        immutable: kind == CacheKind::Content && !forced,
    };

    let Some(confirmation) = current else {
        return CacheDecision::Revalidate(known(false));
    };
    // A new configuration (another URL, another `require_checksum`...) inherits neither
    // validators nor evidence: a `304` from the new origin cannot vouch for
    // what was downloaded under the old one, and a `200` is checked with today's
    // policy. It costs one download per path and configuration change.
    if forced || confirmation.config_version != ctx.config_version {
        return CacheDecision::Revalidate(known(false));
    }
    match ctx.policy.ttl_of(kind) {
        Some(ttl) if age_secs(confirmation, ctx.now) >= ttl => {
            CacheDecision::Revalidate(known(true))
        }
        _ => CacheDecision::Fresh,
    }
}

/// Whether, after an upstream failure, the cached content can be served.
///
/// Only if there is provenance from the current upstream, the policy allows it
/// (`stale_if_error_secs > 0`) and the content has not been expired for longer than that
/// window. An absent TTL does not expire: the window never runs out.
pub fn stale_allowed(ctx: &Context<'_>, entry: &CacheEntry) -> bool {
    if ctx.policy.stale_if_error_secs == 0 || entry.quarantined_at.is_some() || !ctx.in_scope {
        return false;
    }
    let Some(confirmation) = entry
        .confirmation
        .as_ref()
        .filter(|confirmation| confirmation.upstream_id == ctx.upstream_id)
    else {
        return false;
    };
    let kind = ctx
        .kind
        .unwrap_or_else(|| ctx.policy.kind_in(ctx.format, ctx.asset_path));
    match ctx.policy.ttl_of(kind) {
        None => true,
        Some(ttl) => {
            age_secs(confirmation, ctx.now) < ttl.saturating_add(ctx.policy.stale_if_error_secs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn digest(byte: char) -> ContentDigest {
        ContentDigest::of_bytes(byte.to_string().as_bytes())
    }

    fn at(offset_secs: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000 + offset_secs, 0).expect("time")
    }

    fn policy(value: Value) -> CachePolicy {
        CachePolicy::parse(&value).expect("valid")
    }

    fn confirmation(version: u64, secs: i64) -> Confirmation {
        Confirmation {
            upstream_id: "central".into(),
            config_version: version,
            at: at(secs),
            etag: Some("\"v1\"".into()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".into()),
            checksum: ChecksumEvidence::ComputedOnly,
        }
    }

    fn entry(version: u64, secs: i64) -> CacheEntry {
        CacheEntry {
            digest: digest('a'),
            size_bytes: 7,
            confirmation: Some(confirmation(version, secs)),
            quarantined_at: None,
        }
    }

    fn ctx<'a>(policy: &'a CachePolicy, path: &'a str, now: i64) -> Context<'a> {
        Context {
            format: RepositoryFormat::Maven2,
            policy,
            asset_path: path,
            upstream_id: "central",
            in_scope: true,
            config_version: 1,
            offline: false,
            now: at(now),
            kind: None,
        }
    }

    #[test]
    fn npm_tarballs_are_content_and_packuments_are_metadata() {
        let policy = CachePolicy::default();
        let npm = RepositoryFormat::Npm;
        assert_eq!(policy.kind_in(npm, "lodash"), CacheKind::Metadata);
        assert_eq!(policy.kind_in(npm, "@acme/lib"), CacheKind::Metadata);
        assert_eq!(
            policy.kind_in(npm, "lodash/-/lodash-4.17.21.tgz"),
            CacheKind::Content
        );
        // The other formats do not change criterion.
        assert_eq!(
            policy.kind_in(RepositoryFormat::Maven2, "a/b/maven-metadata.xml"),
            CacheKind::Metadata
        );
        assert_eq!(
            policy.kind_in(RepositoryFormat::Raw, "lodash"),
            CacheKind::Content
        );
    }

    #[test]
    fn the_defaults_never_expire_nor_remember_nor_serve_stale() {
        let policy = CachePolicy::default();
        assert_eq!(policy.content_ttl, None);
        assert_eq!(policy.metadata_ttl, None);
        assert_eq!(policy.negative_secs, 0);
        assert_eq!(policy.stale_if_error_secs, 0);
        assert!(!policy.offline);
        assert_eq!(CachePolicy::parse(&json!({})).expect("empty"), policy);
    }

    #[test]
    fn what_cannot_be_read_is_refused() {
        let cases = [
            json!({"ttl": {"content_secs": -1}}),
            json!({"ttl": {"content_secs": MAX_CACHE_SECS + 1}}),
            json!({"negative_secs": MAX_CACHE_SECS + 1}),
            json!({"stale_if_error_secs": "soon"}),
            json!({"ttl": {"metadata": 5}}),
            json!({"unknown": true}),
            json!({"metadata_paths": ["/abs/"]}),
            json!({"metadata_paths": ["a/b"]}),
            json!({"metadata_paths": ["../x/"]}),
            json!({"metadata_paths": [""]}),
            json!({"metadata_paths": ["/"]}),
            json!({"metadata_paths": ["a//b/"]}),
        ];
        for case in cases {
            assert!(
                CachePolicy::parse(&case).is_err(),
                "{case} should be refused"
            );
        }
    }

    #[test]
    fn a_path_is_metadata_by_name_or_by_prefix_and_otherwise_content() {
        let policy = policy(json!({"metadata_paths": ["maven-metadata.xml", "dists/"]}));
        assert_eq!(
            policy.kind_of("acme/lib/maven-metadata.xml"),
            CacheKind::Metadata
        );
        assert_eq!(policy.kind_of("maven-metadata.xml"), CacheKind::Metadata);
        assert_eq!(policy.kind_of("dists/stable/Release"), CacheKind::Metadata);
        assert_eq!(
            policy.kind_of("acme/lib/1.0/lib-1.0.jar"),
            CacheKind::Content
        );
        // The name is of a whole segment: not an ending.
        assert_eq!(policy.kind_of("x-maven-metadata.xml"), CacheKind::Content);
        // The prefix is a segment prefix: `dists-x/` is not `dists/`.
        assert_eq!(policy.kind_of("dists-x/a"), CacheKind::Content);
    }

    #[test]
    fn the_checksum_and_signature_of_a_metadata_follow_its_kind() {
        let policy = policy(json!({}));
        for path in [
            "acme/lib/maven-metadata.xml.sha1",
            "acme/lib/maven-metadata.xml.asc",
            "dists/stable/Release.gpg",
            "dists/stable/InRelease",
            "dists/stable/main/Packages.xz",
        ] {
            assert_eq!(policy.kind_of(path), CacheKind::Metadata, "{path}");
        }
        // An artifact's checksum is as immutable as the artifact.
        assert_eq!(
            policy.kind_of("acme/lib/1.0/lib-1.0.jar.sha1"),
            CacheKind::Content
        );
        assert_eq!(policy.kind_of(".sha1"), CacheKind::Content);
    }

    #[test]
    fn without_an_entry_it_fetches_online_and_misses_offline() {
        let policy = CachePolicy::default();
        assert_eq!(decide(&ctx(&policy, "a/b", 0), None), CacheDecision::Fetch);
        let mut offline = ctx(&policy, "a/b", 0);
        offline.offline = true;
        assert_eq!(decide(&offline, None), CacheDecision::OfflineMiss);
    }

    #[test]
    fn a_path_outside_the_scopes_is_hidden_even_if_it_is_cached() {
        let policy = CachePolicy::default();
        let mut out = ctx(&policy, "a/b", 0);
        out.in_scope = false;
        assert_eq!(decide(&out, Some(&entry(1, 0))), CacheDecision::OutOfScope);
        assert_eq!(decide(&out, None), CacheDecision::OutOfScope);
    }

    #[test]
    fn a_fresh_entry_is_served_and_an_expired_one_is_revalidated_with_validators() {
        let policy = policy(json!({"ttl": {"content_secs": 100, "metadata_secs": 10}}));
        let entry = entry(1, 0);

        assert_eq!(
            decide(&ctx(&policy, "a/lib.jar", 99), Some(&entry)),
            CacheDecision::Fresh
        );
        let CacheDecision::Revalidate(known) =
            decide(&ctx(&policy, "a/lib.jar", 100), Some(&entry))
        else {
            panic!("expired content should be revalidated");
        };
        assert_eq!(known.digest, digest('a'));
        assert_eq!(known.etag.as_deref(), Some("\"v1\""));
        assert!(known.last_modified.is_some());
        assert!(known.immutable);

        // The metadata expires sooner and is not immutable.
        assert_eq!(
            decide(&ctx(&policy, "a/maven-metadata.xml", 9), Some(&entry)),
            CacheDecision::Fresh
        );
        let CacheDecision::Revalidate(known) =
            decide(&ctx(&policy, "a/maven-metadata.xml", 10), Some(&entry))
        else {
            panic!("expired metadata should be revalidated");
        };
        assert!(!known.immutable);
    }

    #[test]
    fn a_zero_ttl_revalidates_every_time_and_no_ttl_never() {
        let always = policy(json!({"ttl": {"content_secs": 0}}));
        assert!(matches!(
            decide(&ctx(&always, "a/b", 0), Some(&entry(1, 0))),
            CacheDecision::Revalidate(_)
        ));
        let never = CachePolicy::default();
        assert_eq!(
            decide(&ctx(&never, "a/b", 10_000_000), Some(&entry(1, 0))),
            CacheDecision::Fresh
        );
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_make_an_entry_expire() {
        let policy = policy(json!({"ttl": {"content_secs": 10}}));
        assert_eq!(
            decide(&ctx(&policy, "a/b", -500), Some(&entry(1, 0))),
            CacheDecision::Fresh
        );
    }

    #[test]
    fn a_new_config_version_revalidates_even_if_fresh() {
        let policy = CachePolicy::default();
        let mut now = ctx(&policy, "a/b", 1);
        now.config_version = 2;
        let CacheDecision::Revalidate(known) = decide(&now, Some(&entry(1, 0))) else {
            panic!("a new configuration should be revalidated");
        };
        // And without inheriting validators: another configuration may point to another
        // origin or demand other evidence.
        assert_eq!(known.etag, None);
    }

    #[test]
    fn content_without_a_receipt_of_the_current_upstream_is_revalidated_without_validators() {
        let policy = CachePolicy::default();
        let no_receipt = CacheEntry {
            digest: digest('a'),
            size_bytes: 7,
            confirmation: None,
            quarantined_at: None,
        };
        let CacheDecision::Revalidate(known) = decide(&ctx(&policy, "a/b", 0), Some(&no_receipt))
        else {
            panic!("no provenance means revalidate");
        };
        assert!(known.etag.is_none() && known.last_modified.is_none());

        let mut other = entry(1, 0);
        other
            .confirmation
            .as_mut()
            .expect("confirmation")
            .upstream_id = "elsewhere".into();
        let CacheDecision::Revalidate(known) = decide(&ctx(&policy, "a/b", 0), Some(&other)) else {
            panic!("another upstream means revalidate");
        };
        // The validators of another upstream are not shown to this one.
        assert!(known.etag.is_none());
    }

    #[test]
    fn offline_serves_only_what_has_provenance_from_the_current_upstream() {
        let policy = policy(json!({"ttl": {"content_secs": 1}}));
        let mut offline = ctx(&policy, "a/b", 1_000_000);
        offline.offline = true;
        offline.config_version = 9;

        // The TTL and configuration drift do not matter without network.
        assert_eq!(
            decide(&offline, Some(&entry(1, 0))),
            CacheDecision::OfflineServe
        );

        let no_receipt = CacheEntry {
            digest: digest('a'),
            size_bytes: 7,
            confirmation: None,
            quarantined_at: None,
        };
        assert_eq!(
            decide(&offline, Some(&no_receipt)),
            CacheDecision::OfflineMiss
        );

        let mut other = entry(1, 0);
        other
            .confirmation
            .as_mut()
            .expect("confirmation")
            .upstream_id = "elsewhere".into();
        assert_eq!(decide(&offline, Some(&other)), CacheDecision::OfflineMiss);
    }

    /// An import receipt confirms the content just like a fetch:
    /// what authorizes serving it without network is the repository's current
    /// configuration, not how the bytes arrived.
    #[test]
    fn an_imported_receipt_of_the_current_upstream_is_served_offline() {
        let policy = policy(json!({"ttl": {"content_secs": 1}}));
        let mut offline = ctx(&policy, "a/b", 1_000_000);
        offline.offline = true;
        offline.config_version = 4;

        let served = digest('a');
        let mut receipt = ProvenanceReceipt {
            id: "prv-import".into(),
            account: "acme".into(),
            repository: "central".into(),
            asset_path: "a/b".into(),
            upstream_id: "central".into(),
            source_url: "https://repo.example.com/a/b".into(),
            config_version: 4,
            digest: served.clone(),
            size_bytes: 1,
            checksum: ChecksumEvidence::ComputedOnly,
            status: 200,
            redirects: 0,
            etag: None,
            last_modified: None,
            outcome: ReceiptOutcome::Imported,
            actor: "u".into(),
            correlation_id: "c".into(),
            fetched_at: at(0),
        };
        let entry = CacheEntry::of(&served, 1, std::slice::from_ref(&receipt));
        assert_eq!(decide(&offline, Some(&entry)), CacheDecision::OfflineServe);

        // What is not the current upstream is not served without network either, and an
        // imported one opens no exceptions.
        receipt.upstream_id = "elsewhere".into();
        let entry = CacheEntry::of(&served, 1, std::slice::from_ref(&receipt));
        assert_eq!(decide(&offline, Some(&entry)), CacheDecision::OfflineMiss);
    }

    #[test]
    fn a_quarantine_lasts_until_the_configuration_changes() {
        let policy = CachePolicy::default();
        let mut quarantined = entry(1, 0);
        quarantined.quarantined_at = Some(1);

        assert_eq!(
            decide(&ctx(&policy, "a/b", 5), Some(&quarantined)),
            CacheDecision::Quarantined
        );
        // With a new configuration the upstream is asked again, and
        // whatever it serves now is no longer judged immutable.
        let mut changed = ctx(&policy, "a/b", 5);
        changed.config_version = 2;
        let CacheDecision::Revalidate(known) = decide(&changed, Some(&quarantined)) else {
            panic!("a new configuration should reopen the question");
        };
        assert!(!known.immutable);
        // Without network it is never lifted: it would be serving what was rejected.
        changed.offline = true;
        assert_eq!(
            decide(&changed, Some(&quarantined)),
            CacheDecision::Quarantined
        );
    }

    #[test]
    fn stale_is_served_only_inside_the_window_and_with_provenance() {
        let policy = policy(json!({
            "ttl": {"content_secs": 100},
            "stale_if_error_secs": 50,
        }));
        let entry = entry(1, 0);
        assert!(stale_allowed(&ctx(&policy, "a/b", 149), &entry));
        assert!(!stale_allowed(&ctx(&policy, "a/b", 150), &entry));

        // By default never.
        let strict = CachePolicy::default();
        assert!(!stale_allowed(&ctx(&strict, "a/b", 0), &entry));

        // Without provenance from the current upstream, neither.
        let mut other = entry.clone();
        other
            .confirmation
            .as_mut()
            .expect("confirmation")
            .upstream_id = "elsewhere".into();
        assert!(!stale_allowed(&ctx(&policy, "a/b", 0), &other));

        // Nor what was put in quarantine.
        let mut quarantined = entry.clone();
        quarantined.quarantined_at = Some(1);
        assert!(!stale_allowed(&ctx(&policy, "a/b", 0), &quarantined));
    }

    #[test]
    fn the_entry_is_read_from_the_receipts_of_the_digest_the_catalog_serves() {
        use crate::proxy::ChecksumEvidence;
        let receipt = |digest: ContentDigest, outcome, secs: i64, version| ProvenanceReceipt {
            id: format!("prv-{secs}"),
            account: "acme".into(),
            repository: "central".into(),
            asset_path: "a/b".into(),
            upstream_id: "central".into(),
            source_url: "https://repo.example.com/a/b".into(),
            config_version: version,
            digest,
            size_bytes: 1,
            checksum: ChecksumEvidence::ComputedOnly,
            status: 200,
            redirects: 0,
            etag: Some(format!("\"e{secs}\"")),
            last_modified: None,
            outcome,
            actor: "u".into(),
            correlation_id: "c".into(),
            fetched_at: at(secs),
        };
        let served = digest('a');
        let receipts = vec![
            receipt(digest('b'), ReceiptOutcome::Quarantined, 30, 2),
            receipt(served.clone(), ReceiptOutcome::Revalidated, 20, 1),
            receipt(served.clone(), ReceiptOutcome::Fetched, 10, 1),
        ];
        let entry = CacheEntry::of(&served, 1, &receipts);
        assert_eq!(entry.quarantined_at, Some(2));
        let confirmation = entry.confirmation.expect("confirmed");
        // The quarantine receipt confirms nothing; the most recent one that does rules.
        assert_eq!(confirmation.at, at(20));
        assert_eq!(confirmation.etag.as_deref(), Some("\"e20\""));

        // A receipt for another digest does not back the one being served.
        let strangers = vec![receipt(digest('c'), ReceiptOutcome::Fetched, 1, 1)];
        assert!(
            CacheEntry::of(&served, 1, &strangers)
                .confirmation
                .is_none()
        );
        assert!(CacheEntry::of(&served, 1, &[]).confirmation.is_none());

        // A quarantine that is no longer the last one does not count.
        let healed = vec![
            receipt(served.clone(), ReceiptOutcome::Revalidated, 40, 3),
            receipt(digest('b'), ReceiptOutcome::Quarantined, 30, 2),
        ];
        assert_eq!(CacheEntry::of(&served, 1, &healed).quarantined_at, None);
    }

    #[test]
    fn a_floating_snapshot_is_mutable_and_a_timestamped_one_is_not() {
        let policy = CachePolicy::default();
        for path in [
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-SNAPSHOT.jar",
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-SNAPSHOT.pom",
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-SNAPSHOT-sources.jar",
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-SNAPSHOT.jar.sha1",
        ] {
            assert_eq!(policy.kind_of(path), CacheKind::Metadata, "{path}");
        }
        for path in [
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-20240101.120000-1.jar",
            "org/acme/widget/1.0/widget-1.0.jar",
            "widget-1.0-SNAPSHOT.jar",
            "org/acme/widget/1.0-SNAPSHOT/readme.txt",
            // The version name inside another name is not floating.
            "org/acme/lib/1.0-SNAPSHOT/lib-11.0-SNAPSHOT.jar",
            "org/acme/lib/1.0-SNAPSHOT/other-1.0-SNAPSHOT.jar",
            "org/acme/lib/1.0-SNAPSHOT/lib-1.0-SNAPSHOTS.jar",
        ] {
            assert_eq!(policy.kind_of(path), CacheKind::Content, "{path}");
        }
    }
}
