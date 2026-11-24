//! The **galaxy** grammar of the Ansible Galaxy collection API v3.
//!
//! Here lives the **pure** part of the protocol: the route table the
//! `ansible-galaxy` client speaks against a Galaxy server, the v3 JSON
//! documents, and the reading of the collection artifact identity. The
//! dialect is fixed by the client (`ansible/galaxy/api.py` of
//! ansible-core 2.19.6) and it was verified against that source: the
//! client discovers the API at `{server}/api/` (`available_versions`),
//! reads the collection and its DRF-paginated versions under
//! `{server}/api/v3/collections/{ns}/{name}/...`, downloads the artifact
//! from the `download_url` of the version document while checking the
//! sha256 the document declares, and publishes a multipart form with the
//! artifact and its sha256 to `api/v3/artifacts/collections/`, waiting on
//! an import task it polls at `api/v3/imports/collections/{task}/`.
//!
//! The versioned identity is the one the protocol itself names: the
//! fully qualified collection name `{namespace}.{name}` plus a version.
//! The artifact is a gzipped tarball whose `MANIFEST.json` at the root of
//! the archive carries the identity — the server reads it on publish and
//! keeps the artifact bytes immutable, so the sha256 the client verified
//! on install is the sha256 of what was published.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use std::io::Read;

/// The fully qualified identity of a collection: `namespace.name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionRef {
    /// The collection namespace.
    pub namespace: String,
    /// The collection name.
    pub name: String,
}

impl CollectionRef {
    /// How the client names the collection in a requirement.
    pub fn fqcn(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }

    /// The prefix of the catalog asset of every version of the
    /// collection: `artifacts/{ns}/{name}/`.
    pub fn asset_prefix(&self) -> String {
        format!("artifacts/{}/{}", self.namespace, self.name)
    }

    /// The catalog asset of one version of the collection.
    pub fn asset_path(&self, version: &str) -> String {
        format!("{}/{version}.tar.gz", self.asset_prefix())
    }

    /// The name of the artifact file as the protocol writes it in the
    /// download URL and in the version document.
    pub fn artifact_file(&self, version: &str) -> String {
        format!("{}-{}-{version}.tar.gz", self.namespace, self.name)
    }

    /// The collection back from an artifact file name
    /// `{ns}-{name}-{version}.tar.gz`. Neither the namespace nor the name
    /// carry a dash in the grammar, so the first two segments are theirs
    /// and the whole rest is the version, dashes included.
    pub fn from_artifact_file(file: &str) -> Option<(Self, String)> {
        let rest = file.strip_suffix(".tar.gz")?;
        let (namespace, rest) = rest.split_once('-')?;
        let (name, version) = rest.split_once('-')?;
        let reference = Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
        };
        if !valid_namespace_name(&reference.namespace)
            || !valid_namespace_name(&reference.name)
            || !valid_version(version)
        {
            return None;
        }
        if reference.artifact_file(version) != file {
            return None;
        }
        Some((reference, version.to_string()))
    }
}

/// The identity a published artifact declares about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionIdentity {
    /// The collection namespace.
    pub namespace: String,
    /// The collection name.
    pub name: String,
    /// The version the artifact was built as.
    pub version: String,
    /// The dependencies the collection declares: `{fqcn: requirement}`.
    pub dependencies: Value,
}

impl CollectionIdentity {
    /// The fully qualified collection name the identity names.
    pub fn fqcn(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }
}

/// The timestamp the documents carry, in the format the client reads
/// (`datetime.strptime` with `%Y-%m-%dT%H:%M:%SZ`).
pub fn the_timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// A collection namespace or name.
///
/// The grammar of the client and of the reference server (`galaxy_ng`)
/// allows letters, digits and underscores; a dash would break the artifact
/// file name, so it is not in the grammar. A name that is empty or hides a
/// path is not a collection.
pub fn valid_namespace_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// A collection version.
///
/// The client resolves versions with `packaging`, so the token is a
/// version string: letters, digits and the separators a version carries.
/// A version that hides a path is not a version.
pub fn valid_version(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-' | b'_'))
}

/// A publish task identifier, as the server hands one back.
pub fn valid_task_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

/// A sha256 digest, as the artifact URLs and the publish form carry it.
pub fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The page of the versions listing.
///
/// The client asks with `?limit={COLLECTION_PAGE_SIZE}` and follows
/// `links.next`; the pagination is the offset one the reference server
/// speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// The page size the client asked for.
    pub limit: usize,
    /// The offset the client asked for.
    pub offset: usize,
}

/// The page size the client asks with (`COLLECTION_PAGE_SIZE` of the
/// client, 100), and the largest one this release answers.
pub const MAX_PAGE_SIZE: usize = 100;

/// The `limit` and `offset` of the query, with the defaults the protocol
/// has when the client sends none.
pub fn the_page_of(query: &str) -> Page {
    let mut page = Page {
        limit: MAX_PAGE_SIZE,
        offset: 0,
    };
    for pair in query.split('&') {
        if let Some((name, value)) = pair.split_once('=')
            && let Ok(number) = value.parse::<usize>()
        {
            match name {
                "limit" => page.limit = number.clamp(1, MAX_PAGE_SIZE),
                "offset" => page.offset = number,
                _ => {}
            }
        }
    }
    page
}

/// What can be asked on the galaxy surface.
///
/// The table is the one the client speaks: the API root it discovers, the
/// collection documents and the paginated versions, the artifact of a
/// version, the publish form and the import task it polls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /api/`: the discovery document with `available_versions`.
    ApiRoot,
    /// `GET /api/v3/collections/{ns}/{name}/`: the collection document.
    CollectionDetail(CollectionRef),
    /// `GET /api/v3/collections/{ns}/{name}/versions/`: the paginated
    /// versions. The query carries `limit` and `offset`.
    CollectionVersions(CollectionRef),
    /// `GET /api/v3/collections/{ns}/{name}/versions/{version}/`: one
    /// version, with its `download_url` and its `artifact.sha256`.
    CollectionVersion(CollectionRef, String),
    /// `POST /api/v3/artifacts/collections/`: the publish form.
    Publish,
    /// `GET /api/v3/imports/collections/{task}/`: the import task the
    /// publish returned.
    ImportTask(String),
    /// `GET /artifact/{sha256}/{file}`: the artifact bytes. The digest is
    /// the one the version document declared for it.
    Artifact(String, CollectionRef, String),
}

/// The route asked, already interpreted.
///
/// The input is what remains of `/galaxy/{repository}/` in the URI, without
/// decoding: the grammar of the protocol is the only thing that may look
/// at a `%2f`. A trailing slash is part of the client's own URLs and is
/// not grammar; the query does not come here.
pub fn parse(rest: &str) -> Option<Route> {
    let rest = rest.trim_start_matches('/').trim_end_matches('/');
    if rest.is_empty() {
        return None;
    }
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        ["api"] => Some(Route::ApiRoot),
        ["api", "v3", "artifacts", "collections"] => Some(Route::Publish),
        ["api", "v3", "imports", "collections", task] if valid_task_id(task) => {
            Some(Route::ImportTask(task.to_string()))
        }
        ["api", "v3", "collections", namespace, name] => {
            Some(Route::CollectionDetail(the_ref(namespace, name)?))
        }
        ["api", "v3", "collections", namespace, name, "versions"] => {
            Some(Route::CollectionVersions(the_ref(namespace, name)?))
        }
        [
            "api",
            "v3",
            "collections",
            namespace,
            name,
            "versions",
            version,
        ] if valid_version(version) => Some(Route::CollectionVersion(
            the_ref(namespace, name)?,
            version.to_string(),
        )),
        ["artifact", digest, file] if valid_sha256(digest) => {
            let (reference, version) = CollectionRef::from_artifact_file(file)?;
            Some(Route::Artifact(digest.to_string(), reference, version))
        }
        _ => None,
    }
}

fn the_ref(namespace: &str, name: &str) -> Option<CollectionRef> {
    if !valid_namespace_name(namespace) || !valid_namespace_name(name) {
        return None;
    }
    Some(CollectionRef {
        namespace: namespace.to_string(),
        name: name.to_string(),
    })
}

/// The download URL of one version: an absolute path from the server
/// root, the shape the client accepts (`urljoin` with the API root keeps
/// it), carrying the digest of the artifact so the URL names what it
/// serves.
pub fn the_download_url(
    base: &str,
    digest: &str,
    reference: &CollectionRef,
    version: &str,
) -> String {
    format!(
        "{base}/artifact/{digest}/{}",
        reference.artifact_file(version)
    )
}

/// The link of the next page of versions: an absolute path, because the
/// client refuses a pagination link without one.
pub fn the_next_link(base: &str, reference: &CollectionRef, page: Page) -> String {
    format!(
        "{base}/api/v3/collections/{}/{}/versions/?limit={}&offset={}",
        reference.namespace, reference.name, page.limit, page.offset
    )
}

/// The discovery document. The client reads `available_versions` and
/// speaks the largest version it shares with the server; this release
/// serves v3 and only v3.
pub fn api_root_document() -> Value {
    json!({ "available_versions": { "v3": "v3/" } })
}

/// The document of a collection. The client reads `updated_at` to decide
/// whether its cached version listing is stale; the identity names what
/// the document is about.
pub fn collection_document(
    base: &str,
    reference: &CollectionRef,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Value {
    json!({
        "href": format!("{base}/api/v3/collections/{}/{}/", reference.namespace, reference.name),
        "namespace": { "name": reference.namespace },
        "name": reference.name,
        "created_at": the_timestamp(created_at),
        "updated_at": the_timestamp(updated_at),
    })
}

/// One entry of the versions listing.
#[derive(Debug, Clone)]
pub struct VersionEntry {
    /// The version.
    pub version: String,
    /// When the version was published.
    pub created_at: DateTime<Utc>,
}

/// The versions ordered the way a listing presents them: newest first,
/// with semantic ordering when the version is a semantic one and the
/// string order when it is not.
pub fn the_sorted_versions(versions: &mut [VersionEntry]) {
    versions.sort_by(|a, b| {
        let (left, right) = (
            semver::Version::parse(&a.version),
            semver::Version::parse(&b.version),
        );
        match (left, right) {
            (Ok(left), Ok(right)) => right.cmp(&left),
            _ => b.version.cmp(&a.version),
        }
    });
}

/// The page of versions, in the DRF shape the client reads: `data` for
/// the entries and `links.next` for the page that follows. The client
/// refuses a `links.next` that is not an absolute URL or an absolute
/// path, so a page without more entries carries `null`.
pub fn versions_page_document(
    base: &str,
    reference: &CollectionRef,
    entries: &[VersionEntry],
    page: Page,
    next: Option<&str>,
) -> Value {
    let data: Vec<Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "href": format!(
                    "{base}/api/v3/collections/{}/{}/versions/{}/",
                    reference.namespace, reference.name, entry.version
                ),
                "version": entry.version,
                "created_at": the_timestamp(entry.created_at),
            })
        })
        .collect();
    json!({
        "meta": { "count": entries.len() },
        "links": {
            "first": the_next_link(base, reference, Page { offset: 0, ..page }),
            "previous": Value::Null,
            "next": next.map(Value::from).unwrap_or(Value::Null),
        },
        "data": data,
    })
}

/// The document of one version. The client reads `namespace.name`,
/// `collection.name`, `version`, `download_url`, `artifact.sha256`,
/// `metadata.dependencies` and `href`; the download URL is an absolute
/// path and the sha256 is the digest of the artifact bytes in the store,
/// the very one the client verifies after downloading.
pub fn version_document(
    base: &str,
    reference: &CollectionRef,
    version: &str,
    digest: &str,
    size: u64,
    dependencies: &Value,
) -> Value {
    json!({
        "href": format!(
            "{base}/api/v3/collections/{}/{}/versions/{version}/",
            reference.namespace, reference.name
        ),
        "namespace": { "name": reference.namespace },
        "collection": { "name": reference.name },
        "version": version,
        "download_url": the_download_url(base, digest, reference, version),
        "artifact": {
            "filename": reference.artifact_file(version),
            "sha256": digest,
            "size": size,
        },
        "metadata": { "dependencies": dependencies, "tags": [] },
        "signatures": [],
    })
}

/// The answer of a publish: the import task the client will poll.
pub fn publish_document(task: &str) -> Value {
    json!({ "task": task })
}

/// The document of an import task.
///
/// The publish of this release imports synchronously: by the time the
/// client reads the task, the import has finished, and the document says
/// so. `finished_at` is what the client waits for.
pub fn import_task_document(task: &str, finished_at: DateTime<Utc>) -> Value {
    json!({
        "id": task,
        "state": "completed",
        "finished_at": the_timestamp(finished_at),
        "messages": [],
        "error": Value::Null,
    })
}

/// The error document of the v3 dialect, the shape the client turns into
/// its error message (`errors[].detail`).
pub fn error_document(code: &str, detail: &str) -> Value {
    json!({ "errors": [{ "code": code, "detail": detail }] })
}

/// The identity a collection artifact declares, read from its bytes.
///
/// The artifact is a tarball, gzipped when the client built it
/// (`ansible-galaxy collection build` writes a `.tar.gz`), whose members
/// sit at the root of the archive. The `MANIFEST.json` at the root
/// carries `collection_info` with the namespace, the name, the version
/// and the dependencies. An artifact that is not a readable tarball of
/// this shape is not a collection artifact: the caller answers for it.
pub fn the_collection_identity(artifact: &[u8]) -> Option<CollectionIdentity> {
    the_collection_identity_checked(artifact).ok()
}

/// The most a gzipped artifact may expand to when its identity is read.
///
/// The reader inflates the whole tarball to find `MANIFEST.json`, so a
/// gzip of zeros could turn a small upload into gigabytes of memory. The
/// limit is generous for a collection (they are source and docs) and finite.
pub const MAX_EXPANDED_BYTES: usize = 256 * 1024 * 1024;

/// Why the identity of an artifact could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityError {
    /// The artifact expands past [`MAX_EXPANDED_BYTES`]: a decompression
    /// bomb, or a collection no one should publish here.
    TooLarge,
    /// The artifact is not a readable collection tarball of the shape the
    /// dialect names.
    Unreadable,
}

/// [`the_collection_identity`], with the reason it failed.
pub fn the_collection_identity_checked(
    artifact: &[u8],
) -> Result<CollectionIdentity, IdentityError> {
    the_collection_identity_within(artifact, MAX_EXPANDED_BYTES)
}

/// [`the_collection_identity_checked`] with an explicit expansion limit.
pub fn the_collection_identity_within(
    artifact: &[u8],
    limit: usize,
) -> Result<CollectionIdentity, IdentityError> {
    let tar = the_tar_of(artifact, limit)?;
    let unreadable = IdentityError::Unreadable;
    let manifest = the_member(&tar, "MANIFEST.json").ok_or(unreadable)?;
    let value: Value = serde_json::from_slice(manifest).map_err(|_| unreadable)?;
    let info = value.get("collection_info").ok_or(unreadable)?;
    let text = |key: &str| info.get(key).and_then(Value::as_str).ok_or(unreadable);
    let namespace = text("namespace")?;
    let name = text("name")?;
    let version = text("version")?;
    if !valid_namespace_name(namespace) || !valid_namespace_name(name) || !valid_version(version) {
        return Err(unreadable);
    }
    Ok(CollectionIdentity {
        namespace: namespace.to_string(),
        name: name.to_string(),
        version: version.to_string(),
        dependencies: info.get("dependencies").cloned().unwrap_or(json!({})),
    })
}

/// The tarball inside an artifact: gunzipped when the magic says gzip,
/// the bytes as they are when it does not. The inflation is bounded by
/// `limit`: one byte past it is already too much.
fn the_tar_of(artifact: &[u8], limit: usize) -> Result<Vec<u8>, IdentityError> {
    if artifact.len() >= 2 && artifact[0] == 0x1f && artifact[1] == 0x8b {
        let mut tar = Vec::new();
        flate2::read::GzDecoder::new(artifact)
            .take(limit as u64 + 1)
            .read_to_end(&mut tar)
            .map_err(|_| IdentityError::Unreadable)?;
        if tar.len() > limit {
            return Err(IdentityError::TooLarge);
        }
        Ok(tar)
    } else {
        Ok(artifact.to_vec())
    }
}

/// The content of the member `wanted` of a tar archive, read off the
/// ustar records the format is made of: a 512-byte header with the name
/// and the size, the bytes that follow it, and the padding that rounds
/// the record up. A header of all zeros ends the archive.
fn the_member<'a>(tar: &'a [u8], wanted: &str) -> Option<&'a [u8]> {
    let mut offset = 0usize;
    while offset + 512 <= tar.len() {
        let header = &tar[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            return None;
        }
        let name = the_member_name(header)?;
        let size = the_member_size(header)?;
        let data_start = offset + 512;
        let data_end = data_start.checked_add(size)?;
        if data_end > tar.len() {
            return None;
        }
        let typeflag = header[156];
        if name == wanted && (typeflag == b'0' || typeflag == 0) {
            return Some(&tar[data_start..data_end]);
        }
        offset = data_end + the_padding_of(size);
    }
    None
}

/// The name of a member: the 100-byte field, prefixed by the prefix
/// field when the archive is a ustar one and the name did not fit.
fn the_member_name(header: &[u8]) -> Option<String> {
    let name = the_text_field(&header[0..100])?;
    let prefix = the_text_field(&header[345..500]).unwrap_or_default();
    if prefix.is_empty() {
        Some(name)
    } else {
        Some(format!("{prefix}/{name}"))
    }
}

/// The size of a member, from the octal field.
fn the_member_size(header: &[u8]) -> Option<usize> {
    let field = the_text_field(&header[124..136])?;
    usize::from_str_radix(field.trim_matches([' ', '\0']), 8).ok()
}

/// The bytes that pad a member of that size up to a whole record.
fn the_padding_of(size: usize) -> usize {
    (512 - (size % 512)) % 512
}

/// A text field of a header: the bytes up to the first NUL, read as
/// UTF-8 when they are one.
fn the_text_field(field: &[u8]) -> Option<String> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    std::str::from_utf8(&field[..end]).ok().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn the_ref() -> CollectionRef {
        CollectionRef {
            namespace: "hodei".into(),
            name: "demo".into(),
        }
    }

    #[test]
    fn every_route_of_the_client_dialect_parses() {
        for (path, expected) in [
            ("api", Route::ApiRoot),
            ("api/", Route::ApiRoot),
            (
                "api/v3/collections/hodei/demo/",
                Route::CollectionDetail(the_ref()),
            ),
            (
                "api/v3/collections/hodei/demo/versions/",
                Route::CollectionVersions(the_ref()),
            ),
            (
                "api/v3/collections/hodei/demo/versions/1.0.0/",
                Route::CollectionVersion(the_ref(), "1.0.0".into()),
            ),
            ("api/v3/artifacts/collections/", Route::Publish),
            (
                "api/v3/imports/collections/838d1308-a8f4-402c-95cb-7823f3806cd8/",
                Route::ImportTask("838d1308-a8f4-402c-95cb-7823f3806cd8".into()),
            ),
            (
                "artifact/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/hodei-demo-1.0.0.tar.gz",
                Route::Artifact(
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                    the_ref(),
                    "1.0.0".into(),
                ),
            ),
        ] {
            assert_eq!(parse(path), Some(expected), "{path} does not parse");
        }
    }

    #[test]
    fn a_version_with_a_prerelease_parses_whole() {
        let route = parse("api/v3/collections/hodei/demo/versions/1.0.0-beta.1+build/");
        assert_eq!(
            route,
            Some(Route::CollectionVersion(
                the_ref(),
                "1.0.0-beta.1+build".into()
            ))
        );
    }

    #[test]
    fn a_path_that_hides_a_traversal_is_not_a_route() {
        assert_eq!(parse("api/v3/collections/../demo"), None);
        assert_eq!(parse("api/v3/collections/hodei//versions/"), None);
        assert_eq!(parse("api/v3/imports/collections/../.."), None);
        assert_eq!(parse("artifact/short/hodei-demo-1.0.0.tar.gz"), None);
        assert_eq!(parse("artifact/../hodei/demo/1.0.0.tar.gz"), None);
    }

    #[test]
    fn a_name_that_the_artifact_file_cannot_carry_is_not_a_route() {
        // The dash is not in the namespace or name grammar, and that is
        // what keeps the artifact file name unambiguous: the first two
        // segments are theirs, and everything after is the version.
        assert_eq!(parse("api/v3/collections/hodei-two/demo"), None);
        assert_eq!(parse("api/v3/collections/hodei/demo-two"), None);
        let route = parse(
            "artifact/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/hodei-demo-two-1.0.0.tar.gz",
        );
        assert_eq!(
            route,
            Some(Route::Artifact(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                the_ref(),
                "two-1.0.0".into(),
            )),
            "the parse is deterministic: the rest after the name is the version"
        );
    }

    #[test]
    fn the_page_of_the_query_defaults_like_the_client_asks() {
        assert_eq!(
            the_page_of("limit=100"),
            Page {
                limit: 100,
                offset: 0
            }
        );
        assert_eq!(
            the_page_of("limit=10&offset=10"),
            Page {
                limit: 10,
                offset: 10
            }
        );
        assert_eq!(
            the_page_of(""),
            Page {
                limit: 100,
                offset: 0
            }
        );
        // A limit above the one the client asks with is clamped, not
        // honored: the page the server builds must stay one the client
        // can follow.
        assert_eq!(
            the_page_of("limit=1000").limit,
            100,
            "the limit is clamped to the page size of the protocol"
        );
    }

    #[test]
    fn the_documents_carry_the_fields_the_client_reads() {
        let base = "/galaxy/repo";
        let root = api_root_document();
        assert_eq!(root["available_versions"]["v3"], "v3/");

        let document = collection_document(
            base,
            &the_ref(),
            DateTime::parse_from_rfc3339("2026-11-20T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            DateTime::parse_from_rfc3339("2026-11-20T11:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );
        assert_eq!(document["namespace"]["name"], "hodei");
        assert_eq!(document["name"], "demo");
        assert_eq!(document["created_at"], "2026-11-20T10:00:00Z");
        assert_eq!(document["updated_at"], "2026-11-20T11:00:00Z");

        let version =
            version_document(base, &the_ref(), "1.0.0", &"a".repeat(64), 1234, &json!({}));
        assert_eq!(version["namespace"]["name"], "hodei");
        assert_eq!(version["collection"]["name"], "demo");
        assert_eq!(version["version"], "1.0.0");
        assert_eq!(
            version["download_url"],
            format!(
                "/galaxy/repo/artifact/{}/hodei-demo-1.0.0.tar.gz",
                "a".repeat(64)
            )
        );
        assert_eq!(version["artifact"]["sha256"], "a".repeat(64));
        assert_eq!(version["artifact"]["size"], 1234);
        // The download URL is an absolute path: the one shape the client
        // accepts next to an absolute URL.
        assert!(version["download_url"].as_str().unwrap().starts_with('/'));
    }

    #[test]
    fn the_versions_page_follows_the_drf_shape() {
        let base = "/galaxy/repo";
        let entries = vec![
            VersionEntry {
                version: "1.1.0".into(),
                created_at: Utc::now(),
            },
            VersionEntry {
                version: "1.0.0".into(),
                created_at: Utc::now(),
            },
        ];
        let page = versions_page_document(
            base,
            &the_ref(),
            &entries,
            Page {
                limit: 1,
                offset: 1,
            },
            Some(&the_next_link(
                base,
                &the_ref(),
                Page {
                    limit: 1,
                    offset: 2,
                },
            )),
        );
        assert_eq!(page["meta"]["count"], 2);
        assert_eq!(page["data"].as_array().unwrap().len(), 2);
        assert_eq!(page["data"][0]["version"], "1.1.0");
        let next = page["links"]["next"].as_str().unwrap();
        assert!(
            next.starts_with('/'),
            "the pagination link is an absolute path, the shape the client accepts"
        );
        assert!(next.contains("limit=1&offset=2"));
        let last = versions_page_document(
            base,
            &the_ref(),
            &entries,
            Page {
                limit: 100,
                offset: 0,
            },
            None,
        );
        assert!(last["links"]["next"].is_null());
    }

    #[test]
    fn the_versions_order_newest_first_by_semantics() {
        let mut entries = vec![
            VersionEntry {
                version: "1.10.0".into(),
                created_at: Utc::now(),
            },
            VersionEntry {
                version: "1.9.0".into(),
                created_at: Utc::now(),
            },
            VersionEntry {
                version: "2.0.0".into(),
                created_at: Utc::now(),
            },
        ];
        the_sorted_versions(&mut entries);
        let names: Vec<&str> = entries.iter().map(|entry| entry.version.as_str()).collect();
        assert_eq!(names, vec!["2.0.0", "1.10.0", "1.9.0"]);
    }

    /// A ustar member written the way the format is read back: header,
    /// content and the padding that rounds the record up.
    fn the_member_bytes(name: &str, content: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0u8; 512];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{:011o}", content.len());
        bytes[124..124 + size.len()].copy_from_slice(size.as_bytes());
        bytes[148..156].copy_from_slice(b"        ");
        bytes[156] = b'0';
        bytes[257..262].copy_from_slice(b"ustar");
        let checksum: u32 = bytes.iter().map(|byte| *byte as u32).sum();
        let checksum = format!("{:06o}\0 ", checksum);
        bytes[148..156].copy_from_slice(checksum.as_bytes());
        let mut member = bytes;
        member.extend_from_slice(content);
        let padding = the_padding_of(content.len());
        member.extend(std::iter::repeat_n(0u8, padding));
        member
    }

    /// G6: a gzip of zeros that expands past the limit is refused as too
    /// large, and what stays under it still reads.
    #[test]
    fn a_gzip_that_expands_past_the_limit_is_refused() {
        let mut bomb = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        bomb.write_all(&vec![0u8; 8 * 1024 * 1024])
            .expect("the zeros");
        let bomb = bomb.finish().expect("the gzip finish");
        assert!(bomb.len() < 64 * 1024, "the bomb is small on the wire");
        assert_eq!(
            the_collection_identity_within(&bomb, 1024 * 1024),
            Err(IdentityError::TooLarge)
        );
        // Under the limit it is only an unreadable tarball, not a bomb.
        assert_eq!(
            the_collection_identity_within(&bomb, 16 * 1024 * 1024),
            Err(IdentityError::Unreadable)
        );
    }

    #[test]
    fn the_identity_of_a_gzipped_artifact_is_the_manifest_one() {
        let manifest = json!({
            "collection_info": {
                "namespace": "hodei",
                "name": "demo",
                "version": "1.0.0",
                "dependencies": { "hodei.base": ">=1.0.0,<2.0.0" },
            },
            "format": 1,
        });
        let mut tar = Vec::new();
        tar.extend_from_slice(&the_member_bytes(
            "MANIFEST.json",
            manifest.to_string().as_bytes(),
        ));
        tar.extend_from_slice(&the_member_bytes("README.md", b"# demo\n"));
        tar.extend_from_slice(&[0u8; 1024]);
        let mut gzipped = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzipped.write_all(&tar).expect("the gzip write");
        let artifact = gzipped.finish().expect("the gzip finish");

        let identity = the_collection_identity(&artifact).expect("the identity");
        assert_eq!(identity.namespace, "hodei");
        assert_eq!(identity.name, "demo");
        assert_eq!(identity.version, "1.0.0");
        assert_eq!(identity.dependencies["hodei.base"], ">=1.0.0,<2.0.0");
    }

    #[test]
    fn the_identity_of_a_plain_tarball_is_read_the_same() {
        let manifest = json!({
            "collection_info": {
                "namespace": "hodei",
                "name": "demo",
                "version": "1.0.0",
                "dependencies": {},
            },
        });
        let mut tar = Vec::new();
        tar.extend_from_slice(&the_member_bytes(
            "MANIFEST.json",
            manifest.to_string().as_bytes(),
        ));
        tar.extend_from_slice(&[0u8; 512]);
        let identity = the_collection_identity(&tar).expect("the identity");
        assert_eq!(identity.fqcn(), "hodei.demo");
    }

    #[test]
    fn an_artifact_that_is_not_a_collection_is_not_an_identity() {
        assert_eq!(the_collection_identity(b"not a tarball"), None);
        assert_eq!(the_collection_identity(&[]), None);
        // A truncated gzip: the magic says gzip, the stream does not end.
        assert_eq!(the_collection_identity(&[0x1f, 0x8b, 0x08]), None);
        // A tarball whose manifest is not the document the format built.
        let mut tar = Vec::new();
        tar.extend_from_slice(&the_member_bytes("MANIFEST.json", b"{not json"));
        assert_eq!(the_collection_identity(&tar), None);
    }

    #[test]
    fn the_artifact_file_round_trips_with_its_version() {
        let (reference, version) =
            CollectionRef::from_artifact_file("hodei-demo-1.0.0.tar.gz").expect("the file");
        assert_eq!(reference, the_ref());
        assert_eq!(version, "1.0.0");
        assert_eq!(reference.artifact_file(&version), "hodei-demo-1.0.0.tar.gz");
        // A version that carries dashes keeps them: the first two
        // segments are the namespace and the name, the rest is the
        // version, whole.
        let (reference, version) =
            CollectionRef::from_artifact_file("hodei-demo-1.0.0-beta.1.tar.gz").expect("the file");
        assert_eq!(version, "1.0.0-beta.1");
        assert_eq!(
            reference.artifact_file(&version),
            "hodei-demo-1.0.0-beta.1.tar.gz"
        );
        assert_eq!(CollectionRef::from_artifact_file("hodei.tar.gz"), None);
        assert_eq!(CollectionRef::from_artifact_file("a-b-c/d.tar.gz"), None);
    }
}
