//! The **hf** grammar of the Hugging Face Hub read dialect.
//!
//! This holds the **pure** part: the route table that the `hf` CLI
//! (`huggingface_hub` 1.x, which speaks httpx) builds against a hub, the
//! JSON revision documents and the resolve metadata headers.
//! The dialect is fixed by the client and was verified by the HSP-169 spike
//! against huggingface.co and against a local fixture (FMT-07 of
//! `uat/UAT-FORMATS.md`):
//!
//! 1. `GET /api/models/{repo_id}/revision/{rev}`: the revision
//!    document, with `id`, `sha` and `siblings[]` (`rfilename`). The `sha`
//!    is the commit that the client later uses in resolves.
//! 2. `HEAD /{repo_id}/resolve/{rev}/{rfilename}`: a `302` whose
//!    metadata travels in headers: `X-Repo-Commit` (the commit),
//!    `X-Linked-Etag` (the file's sha256, quoted),
//!    `X-Linked-Size` (its length) and `Location`, **relative** (a path
//!    without scheme or host, the classic hub form) towards the blob.
//! 3. `HEAD`/`GET {Location}`: the metadata is read from the final response
//!    of the redirect and the bytes are streamed; the response `ETag` is
//!    the sha256 of the content.
//!
//! The client does **not** emit `X-Xet-*` when the server does not announce it
//! (server decision): the dialect falls back to the classic HTTP route. And on
//! that route the client ONLY verifies the size (it does not check the declared
//! sha256): integrity is guaranteed by the server's CAS
//! (connection with HSP-105), which refuses to serve bytes that are not its
//! digest.
//!
//! A model in the catalog is the set of assets
//! `artifacts/{owner}/{name}/{rfilename}`; the revision commit is
//! the digest of the snapshot listing (`rfilename sha256`, sorted): it is
//! deterministic and only changes when the content it declares changes.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// The identity of a model: `owner/name`, the client's `repo_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// The model owner.
    pub owner: String,
    /// The model name.
    pub name: String,
}

impl ModelRef {
    /// The `repo_id` as the client names it.
    pub fn repo_id(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// The catalog prefix of all the model's files:
    /// `artifacts/{owner}/{name}/`.
    pub fn asset_prefix(&self) -> String {
        format!("artifacts/{}/{}", self.owner, self.name)
    }

    /// The catalog asset of a model file.
    pub fn asset_path(&self, filename: &str) -> String {
        format!("{}/{filename}", self.asset_prefix())
    }
}

/// A part of the `repo_id` (`owner` or `name`).
///
/// The hub grammar: letters, digits, hyphen, dot and underscore, and
/// starts with a letter or digit. A leading dot is a hidden repo in the
/// real hub, and `.` and `..` hide paths: none of that is a repo.
pub fn valid_repo_part(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 96
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && text
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
}

/// A sibling's `rfilename`: the file path inside the model.
///
/// It may contain subdirectories (the hub stores paths, not names), so
/// each segment must be a legitimate segment: not empty, not `..`,
/// not a Windows separator, and no URI reference delimiters. The list
/// is the one that makes the path an unambiguous reference, the
/// same as the one for raw content.
pub fn valid_filename(text: &str) -> bool {
    if text.is_empty() || text.len() > 1024 {
        return false;
    }
    let segments: Vec<&str> = text.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return false;
    }
    segments.iter().all(|segment| valid_segment(segment))
}

/// A segment of an `rfilename`: the RFC 3986 `pchar`s, the same list
/// as the one for the hub's raw content.
fn valid_segment(segment: &str) -> bool {
    if segment.is_empty() || segment == "." || segment == ".." {
        return false;
    }
    segment.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b':'
                    | b'@'
            )
    })
}

/// A sha256, as carried by the `X-Linked-Etag`, the `ETag` and the blob
/// path.
pub fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// A revision that the read dialect names: the `main` branch or a
/// 40-hex commit. Another branch does not exist in this release's hosted, and
/// a commit that is not the snapshot's sha256 is a revision that the
/// hub never served.
pub fn valid_revision(text: &str) -> bool {
    text == "main" || valid_sha256(text)
}

/// What the CLI can request on the surface of an hf repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /api/models/{owner}/{name}/revision/{rev}`: the model's
    /// revision document.
    ApiRevision(ModelRef, String),
    /// `HEAD`/`GET /{owner}/{name}/resolve/{rev}/{rfilename}`: the
    /// file metadata (the resolve's `302`) and, on `GET`, the
    /// bytes without redirect.
    Resolve(ModelRef, String, String),
    /// `HEAD`/`GET /cdn-lfs/{owner}/{name}/{sha256}/{rfilename}`: the
    /// final blob metadata and its streamed bytes. The path digest
    /// names the content: the blob's identity is its sha256, exact
    /// CAS.
    Blob(ModelRef, String, String),
}

/// The requested route, already interpreted.
///
/// The input is what remains of `/hf/{repository}/` in the URI, **not**
/// decoded: the protocol grammar is the only one that may look at
/// a `%2f`. A trailing slash is not part of the client grammar; the query
/// does not get here either.
pub fn parse(rest: &str) -> Option<Route> {
    let rest = rest.trim_start_matches('/').trim_end_matches('/');
    if rest.is_empty() {
        return None;
    }
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        ["api", "models", owner, name, "revision", rev] if valid_revision(rev) => {
            Some(Route::ApiRevision(the_model(owner, name)?, rev.to_string()))
        }
        [owner, name, "resolve", rev, rest @ ..] if valid_revision(rev) && !rest.is_empty() => {
            let filename = rest.join("/");
            if !valid_filename(&filename) {
                return None;
            }
            Some(Route::Resolve(
                the_model(owner, name)?,
                rev.to_string(),
                filename,
            ))
        }
        ["cdn-lfs", owner, name, digest, rest @ ..] if valid_sha256(digest) && !rest.is_empty() => {
            let filename = rest.join("/");
            if !valid_filename(&filename) {
                return None;
            }
            Some(Route::Blob(
                the_model(owner, name)?,
                digest.to_string(),
                filename,
            ))
        }
        _ => None,
    }
}

fn the_model(owner: &str, name: &str) -> Option<ModelRef> {
    if !valid_repo_part(owner) || !valid_repo_part(name) {
        return None;
    }
    Some(ModelRef {
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

/// The commit of a model snapshot: the digest of the listing that
/// forms it, `rfilename sha256` in order. Two snapshots with the same
/// content are the same commit, and the commit only changes when
/// the content it declares changes: the revision is content-addressed like the
/// CAS that serves it.
pub fn snapshot_commit(entries: &[(&str, &str)]) -> String {
    let mut listing: Vec<String> = entries
        .iter()
        .map(|(filename, digest)| format!("{filename} {digest}"))
        .collect();
    listing.sort();
    let mut hasher = Sha256::new();
    for line in listing {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

/// The timestamp carried by the documents, in the format the
/// client reads without inspecting (`ISO 8601` with seconds).
pub fn the_timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The model revision document. The client reads `sha` (the commit
/// it will use in resolves) and `siblings[]` (`rfilename`): the rest is
/// courtesy of the real document.
pub fn revision_document(
    model: &ModelRef,
    commit: &str,
    last_modified: DateTime<Utc>,
    siblings: &[String],
) -> Value {
    json!({
        "id": model.repo_id(),
        "sha": commit,
        "lastModified": the_timestamp(last_modified),
        "siblings": siblings
            .iter()
            .map(|filename| json!({ "rfilename": filename }))
            .collect::<Vec<Value>>(),
    })
}

/// The metadata carried by the resolve and blob headers: the
/// commit, the file's sha256 and its length. The blob's identity is
/// its sha256: it is the etag the client uses to name its cache and the
/// digest the CAS guarantees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobMeta {
    /// The commit of the snapshot that declares the file.
    pub commit: String,
    /// The sha256 of the content, without quotes.
    pub sha256: String,
    /// The length of the content, in bytes.
    pub size: u64,
}

impl BlobMeta {
    /// The dialect headers, in wire order: `X-Repo-Commit`,
    /// `X-Linked-Etag` (the sha256 in quotes, as the real hub
    /// writes it) and `X-Linked-Size`.
    pub fn the_linked_headers(&self) -> [(&'static str, String); 3] {
        [
            ("x-repo-commit", self.commit.clone()),
            ("x-linked-etag", format!("\"{}\"", self.sha256)),
            ("x-linked-size", self.size.to_string()),
        ]
    }
}

/// The resolve `Location`: a **path** from the server root,
/// without scheme or host, the form the classic hub delivers and the one the
/// client resolves against the host of the URL it requested. It carries the
/// model, the digest and the file because the URL has to name what it
/// serves.
pub fn the_download_location(
    base: &str,
    model: &ModelRef,
    meta: &BlobMeta,
    filename: &str,
) -> String {
    format!(
        "{base}/cdn-lfs/{}/{}/{}/{}",
        model.owner, model.name, meta.sha256, filename
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn the_model() -> ModelRef {
        ModelRef {
            owner: "hodei".into(),
            name: "demo-model".into(),
        }
    }

    #[test]
    fn every_route_of_the_client_dialect_parses() {
        for (path, expected) in [
            (
                "api/models/hodei/demo-model/revision/main",
                Route::ApiRevision(the_model(), "main".into()),
            ),
            (
                "api/models/hodei/demo-model/revision/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9",
                Route::ApiRevision(
                    the_model(),
                    "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
                ),
            ),
            (
                "hodei/demo-model/resolve/main/model.safetensors",
                Route::Resolve(the_model(), "main".into(), "model.safetensors".into()),
            ),
            (
                "hodei/demo-model/resolve/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/config.json",
                Route::Resolve(
                    the_model(),
                    "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
                    "config.json".into(),
                ),
            ),
            (
                "hodei/demo-model/resolve/main/subdir/tokenizer.json",
                Route::Resolve(the_model(), "main".into(), "subdir/tokenizer.json".into()),
            ),
            (
                "cdn-lfs/hodei/demo-model/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/model.safetensors",
                Route::Blob(
                    the_model(),
                    "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
                    "model.safetensors".into(),
                ),
            ),
        ] {
            assert_eq!(parse(path), Some(expected), "{path} does not parse");
        }
    }

    #[test]
    fn a_path_that_hides_a_traversal_is_not_a_route() {
        assert_eq!(parse("api/models/../demo-model/revision/main"), None);
        assert_eq!(parse("hodei/demo-model/resolve/main/../config.json"), None);
        assert_eq!(parse("hodei/demo-model/resolve/main/sub//file"), None);
        assert_eq!(
            parse("cdn-lfs/hodei/demo-model/short/model.safetensors"),
            None
        );
        assert_eq!(parse("hodei/demo-model/resolve/dev/config.json"), None);
        assert_eq!(parse("hodei/demo-model/resolve/main/"), None);
        assert_eq!(
            parse(
                "cdn-lfs/hodei/demo-model/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/"
            ),
            None
        );
    }

    #[test]
    fn a_filename_the_wire_cannot_carry_is_not_a_route() {
        // The backslash is a separator on some systems, and a space is the
        // same name written two ways: the hub grammar does not
        // carry them.
        assert_eq!(parse("hodei/demo-model/resolve/main/a\\b"), None);
        assert_eq!(parse("hodei/demo-model/resolve/main/a b"), None);
        // A `?` or a `#` changes the meaning of the reference.
        assert_eq!(parse("hodei/demo-model/resolve/main/a%3fb"), None);
        // And a segment encoded twice still carries encoding.
        assert_eq!(parse("hodei/demo-model/resolve/main/a%252fb"), None);
    }

    #[test]
    fn the_blob_route_carries_the_model_its_digest_and_the_file() {
        // The blob path carries the model, the digest and the file: whoever
        // serves it looks up the asset, checks that its content is the one
        // the path names and serves nothing that is not its digest.
        let route = parse(
            "cdn-lfs/hodei/demo-model/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/subdir/model.safetensors",
        );
        assert_eq!(
            route,
            Some(Route::Blob(
                the_model(),
                "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
                "subdir/model.safetensors".into()
            ))
        );
    }

    #[test]
    fn the_repo_parts_of_the_hub_grammar() {
        assert!(valid_repo_part("hodei"));
        assert!(valid_repo_part("demo-model_v2.1"));
        assert!(!valid_repo_part(""));
        assert!(!valid_repo_part("."));
        assert!(!valid_repo_part(".."));
        assert!(!valid_repo_part(".hidden"));
        assert!(!valid_repo_part("a/b"));
        assert!(!valid_repo_part("a b"));
    }

    #[test]
    fn the_document_of_a_revision_carries_the_fields_the_client_reads() {
        let commit = snapshot_commit(&[
            ("config.json", "e4d31dec"),
            ("model.safetensors", "9f215d17"),
        ]);
        let document = revision_document(
            &the_model(),
            &commit,
            DateTime::parse_from_rfc3339("2026-11-20T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            &["config.json".into(), "model.safetensors".into()],
        );
        assert_eq!(document["id"], "hodei/demo-model");
        assert_eq!(document["sha"], commit);
        assert_eq!(document["lastModified"], "2026-11-20T10:00:00Z");
        let siblings = document["siblings"].as_array().unwrap();
        assert_eq!(siblings.len(), 2);
        assert_eq!(siblings[0]["rfilename"], "config.json");
        assert_eq!(siblings[1]["rfilename"], "model.safetensors");
    }

    #[test]
    fn the_commit_of_a_snapshot_is_its_sorted_listing() {
        let one = snapshot_commit(&[
            ("config.json", "e4d31dec"),
            ("model.safetensors", "9f215d17"),
        ]);
        let two = snapshot_commit(&[
            ("model.safetensors", "9f215d17"),
            ("config.json", "e4d31dec"),
        ]);
        assert_eq!(one, two, "el orden del listado no cambia el commit");
        let other = snapshot_commit(&[("config.json", "e4d31dec")]);
        assert_ne!(one, other, "otro contenido es otro commit");
        assert_eq!(one.len(), 64, "el commit es un sha256");
    }

    #[test]
    fn the_linked_headers_are_the_ones_the_wire_carries() {
        let meta = BlobMeta {
            commit: "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91".into(),
            sha256: "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
            size: 268435456,
        };
        let headers = meta.the_linked_headers();
        assert_eq!(headers[0].0, "x-repo-commit");
        assert_eq!(headers[0].1, "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91");
        assert_eq!(headers[1].0, "x-linked-etag");
        assert_eq!(
            headers[1].1, "\"9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9\"",
            "el etag va entre comillas, como lo escribe el hub real"
        );
        assert_eq!(headers[2].0, "x-linked-size");
        assert_eq!(headers[2].1, "268435456");
    }

    #[test]
    fn the_download_location_is_a_relative_path_with_the_digest() {
        let meta = BlobMeta {
            commit: "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91".into(),
            sha256: "9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9".into(),
            size: 268435456,
        };
        let location =
            the_download_location("/hf/hf-hosted", &the_model(), &meta, "model.safetensors");
        assert_eq!(
            location,
            "/hf/hf-hosted/cdn-lfs/hodei/demo-model/9f215d1758e5893b6cb9cce25a044849dce7f0f0b80218f92b0a20ceec7a73c9/model.safetensors"
        );
        assert!(
            location.starts_with('/'),
            "la Location es una ruta, no una URL"
        );
        assert!(
            !location.contains("://"),
            "la Location no anuncia un anfitrion"
        );
    }

    #[test]
    fn the_asset_path_of_a_file_sits_under_its_model_prefix() {
        let model = the_model();
        assert_eq!(model.asset_prefix(), "artifacts/hodei/demo-model");
        assert_eq!(
            model.asset_path("config.json"),
            "artifacts/hodei/demo-model/config.json"
        );
        assert_eq!(
            model.asset_path("subdir/tokenizer.json"),
            "artifacts/hodei/demo-model/subdir/tokenizer.json"
        );
    }
}
