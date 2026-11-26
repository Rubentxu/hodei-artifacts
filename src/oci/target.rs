//! The OCI path split, and the rules for the name and the digest.
//!
//! The distribution path is read **raw**, from the URI as it arrived:
//! `/v2/<name>/blobs/<digest>`, `/v2/<name>/manifests/<reference>`,
//! `/v2/<name>/blobs/uploads/<uuid>`.
//!
//! And there is a detail the protocol makes clear and that many
//! implementations do not honor: the repository name may
//! contain `/`. A repository is called `team/demo`, and its path is
//! `/v2/team/demo/blobs/sha256:...`.
//!
//! That is why the split does not look for the **first** `blobs` or `manifests`, but the
//! **last**: what comes before it is the name, and what comes after is
//! the resource class and the reference. A repository named
//! `blobs/app` would still be a valid name, not a resource
//! class.
//!
//! The path is read raw on purpose: an already-decoded `%2e%2e` cannot
//! be refuted with an extractor that decodes, and a traversal is a
//! traversal.

use crate::oci::failure::OciFailure;

/// What the path names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A blob of the repository, by digest.
    Blob {
        /// The repository name.
        name: String,
        /// The reference: the digest, with its algorithm.
        reference: String,
    },
    /// A manifest of the repository, by digest or by tag.
    Manifest {
        /// The repository name.
        name: String,
        /// The reference: the digest or the tag.
        reference: String,
    },
    /// The repository's tag list: `<name>/tags/list`.
    Tags {
        /// The repository name.
        name: String,
    },
    /// The referrers of a manifest: `<name>/referrers/<digest>`.
    Referrers {
        /// The repository name.
        name: String,
        /// The digest of the manifest being asked about.
        digest: String,
    },
    /// The **start** of an upload: `<name>/blobs/uploads/`.
    ///
    /// There is no session here yet: this path is the one that opens one.
    UploadStart {
        /// The repository name.
        name: String,
    },
    /// An upload **session** in progress: `<name>/blobs/uploads/<uuid>`.
    UploadSession {
        /// The repository name.
        name: String,
        /// The opaque session identifier.
        session: String,
    },
}

impl Target {
    /// The repository name, whatever the resource class.
    pub fn the_name(&self) -> &str {
        match self {
            Self::Blob { name, .. }
            | Self::Manifest { name, .. }
            | Self::Tags { name }
            | Self::Referrers { name, .. }
            | Self::UploadStart { name }
            | Self::UploadSession { name, .. } => name,
        }
    }
}

/// The prefix the distribution surface carries.
const PREFIX: &str = "/v2/";

/// What the path names, without the surface prefix.
///
/// The prefix is removed and the rest is split into levels. An empty level at
/// the end is the trailing slash of `/blobs/uploads/`, and is not a level.
fn the_levels(path: &str) -> Option<Vec<&str>> {
    let rest = path.strip_prefix(PREFIX)?;
    let mut levels: Vec<&str> = rest.split('/').collect();
    // The trailing slash is not a level: `/blobs/uploads/` and `/blobs/uploads`
    // are the same request.
    while levels.last().is_some_and(|level| level.is_empty()) {
        levels.pop();
    }
    if levels.iter().any(|level| level.is_empty()) {
        // An empty level in the middle (`team//demo`) is not a name.
        return None;
    }
    Some(levels)
}

/// The index of the level that names the resource class.
///
/// It is the **last** level that is `blobs`, `manifests`, `tags` or
/// `referrers` and has something after it: what comes after it is the
/// reference.
fn the_kind_index(levels: &[&str]) -> Option<usize> {
    levels
        .iter()
        .enumerate()
        .rev()
        .find(|(index, level)| {
            matches!(**level, "blobs" | "manifests" | "tags" | "referrers")
                && *index + 1 < levels.len()
        })
        .map(|(index, _)| index)
}

/// What the path names.
///
/// The name is validated BEFORE looking at the catalog: a `..` does not get to
/// be a query, and a malformed name is a client error.
pub fn the_target(path: &str) -> Result<Target, OciFailure> {
    let levels = the_levels(path)
        .ok_or_else(|| OciFailure::name_invalid("the path is not an OCI resource path"))?;
    let index = the_kind_index(&levels).ok_or_else(|| {
        OciFailure::name_invalid("the path does not name a resource kind and a reference")
    })?;
    let name = levels[..index].join("/");
    the_name(&name)?;
    let rest = &levels[index + 1..];

    match levels[index] {
        "manifests" => {
            let reference = rest.join("/");
            if reference.is_empty() {
                return Err(OciFailure::name_invalid(
                    "the path does not name a reference",
                ));
            }
            Ok(Target::Manifest { name, reference })
        }
        // The tag list: `<name>/tags/list`. The `list` is part
        // of the protocol and not a resource name, so without it this is not
        // the tag list: it is a path that names nothing.
        "tags" => {
            if rest != ["list"] {
                return Err(OciFailure::name_invalid(
                    "the path does not name the list of tags",
                ));
            }
            Ok(Target::Tags { name })
        }
        // The referrers: `<name>/referrers/<digest>`. The digest is
        // checked here, with the same rule as a blob's: a
        // digest with another syntax is a client error, and not a
        // search with no result.
        "referrers" => {
            if rest.len() != 1 {
                return Err(OciFailure::name_invalid(
                    "the path does not name a manifest to ask the referrers of",
                ));
            }
            // The syntax is checked here: a digest with another syntax
            // is a client error, and the protocol says so with a
            // `400`, not with an empty list.
            the_digest_of_reference(rest[0])?;
            Ok(Target::Referrers {
                name,
                digest: rest[0].to_string(),
            })
        }
        "blobs" => {
            // The upload surface lives under `blobs/uploads`: that is what
            // says this is not a blob called `uploads`.
            if rest.first() == Some(&"uploads") {
                return match rest.len() {
                    1 => Ok(Target::UploadStart { name }),
                    2 => {
                        the_session_id(rest[1])?;
                        Ok(Target::UploadSession {
                            name,
                            session: rest[1].to_string(),
                        })
                    }
                    _ => Err(OciFailure::name_invalid(
                        "the upload path names neither a session nor its start",
                    )),
                };
            }
            let reference = rest.join("/");
            if reference.is_empty() {
                return Err(OciFailure::name_invalid(
                    "the path does not name a reference",
                ));
            }
            Ok(Target::Blob { name, reference })
        }
        other => Err(OciFailure::unsupported(format!(
            "the resource kind {other:?} is not part of the distribution API"
        ))),
    }
}

/// The identifier of an upload session.
///
/// It is a UUID: the client receives it in the `Location` and returns it as
/// is. That it is a UUID is not a whim: it is what makes the
/// identifier **unguessable**, and a session that can be
/// guessed is a session that someone else can write to.
///
/// And what cannot be guessed cannot be smuggled in either: an
/// identifier that is not a UUID is not a session, and is answered as
/// if it did not exist.
pub fn the_session_id(session: &str) -> Result<(), OciFailure> {
    let parsed = uuid::Uuid::parse_str(session)
        .map_err(|_| OciFailure::blob_upload_unknown("the upload session does not exist"))?;
    // The canonical UUID is the one returned: a client that sends the
    // uppercase version or with braces receives the same error as one
    // that sends anything else.
    if parsed.hyphenated().to_string() != session {
        return Err(OciFailure::blob_upload_unknown(
            "the upload session does not exist",
        ));
    }
    Ok(())
}

/// The repository name, with its rules.
///
/// An OCI name is lowercase letters, digits and separators, with levels
/// separated by a slash. A `..` is not a level: it is a traversal, and is
/// rejected before looking at the catalog.
pub fn the_name(name: &str) -> Result<(), OciFailure> {
    if name.is_empty() || name.len() > 255 {
        return Err(OciFailure::name_invalid(
            "the repository name is empty or too long",
        ));
    }
    if name.starts_with('/') || name.ends_with('/') {
        return Err(OciFailure::name_invalid(
            "the repository name does not start or end with a separator",
        ));
    }
    for segment in name.split('/') {
        if segment.is_empty() {
            return Err(OciFailure::name_invalid(
                "the repository name has an empty level",
            ));
        }
        if segment == "." || segment == ".." {
            return Err(OciFailure::name_invalid(
                "the repository name does not carry navigation levels",
            ));
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        {
            return Err(OciFailure::name_invalid(
                "the repository name only admits lowercase letters, digits, '.', '_' and '-'",
            ));
        }
    }
    Ok(())
}

/// The digest the reference names.
///
/// A blob or manifest reference by digest carries the algorithm and
/// the hexadecimal (`sha256:<64 hex>`). Without the prefix, the reference is
/// a **tag**, which is mutable: the bare digest is not accepted as a
/// digest, because a client that believes it asks by digest and receives
/// mutable content gets a manifest that may have changed
/// between the request and the use.
///
/// Whatever does not fit is a client error, and the OCI code says so.
pub fn the_digest_of_reference(reference: &str) -> Result<String, OciFailure> {
    if !reference.starts_with("sha256:") {
        return Err(OciFailure::digest_invalid(format!(
            "{reference:?} does not name a digest: a digest carries its algorithm"
        )));
    }
    let digest = hodei_hub::oci::the_digest_of(reference)
        .ok_or_else(|| {
            OciFailure::digest_invalid(format!(
                "{reference:?} is not a content digest the registry understands"
            ))
        })?
        .value;
    Ok(digest)
}

/// Whether the reference is a digest or a tag.
///
/// A reference with `:` is a digest (the algorithm carries it); without it,
/// it is a tag, which is mutable.
pub fn is_a_digest(reference: &str) -> bool {
    reference.contains(':')
}

/// The name of a tag, with its rules.
///
/// The protocol says a tag is a word: it starts with an
/// alphanumeric or an underscore, and continues with alphanumerics, dots,
/// hyphens and underscores, up to 128 characters.
///
/// The rule is not a whim: the tag is a **key** in the
/// catalog, and a key with slashes, spaces or control characters is not
/// a tag: it is a route nobody asked for. And the name that is
/// accepted is the one the protocol accepts, so that a real client does not
/// have to learn two rules.
pub fn the_tag_name(tag: &str) -> Result<(), OciFailure> {
    let mut characters = tag.chars();
    let first = characters
        .next()
        .ok_or_else(|| OciFailure::manifest_invalid("the tag of the manifest is empty"))?;
    let starts_well = first.is_ascii_alphanumeric() || first == '_';
    if !starts_well {
        return Err(OciFailure::manifest_invalid(format!(
            "the tag {tag:?} does not start with a letter, a digit or an underscore"
        )));
    }
    if tag.len() > 128 {
        return Err(OciFailure::manifest_invalid(format!(
            "the tag {tag:?} is longer than the 128 characters the protocol admits"
        )));
    }
    let rest_well = characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'));
    if !rest_well {
        return Err(OciFailure::manifest_invalid(format!(
            "the tag {tag:?} carries a character that is not a letter, a digit, a '.', a '_' or a '-'"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name may have levels: the last `blobs` or `manifests` is
    /// the one that rules, not the first.
    #[test]
    fn the_last_kind_wins_and_the_name_keeps_its_levels() {
        let target = the_target("/v2/team/demo/blobs/sha256:abc").expect("the target");
        assert_eq!(
            target,
            Target::Blob {
                name: "team/demo".to_string(),
                reference: "sha256:abc".to_string(),
            }
        );

        // A repository called `blobs/app` is still a name.
        let target = the_target("/v2/blobs/app/manifests/v1").expect("the target");
        assert_eq!(
            target,
            Target::Manifest {
                name: "blobs/app".to_string(),
                reference: "v1".to_string(),
            }
        );

        // And the last one rules when both appear.
        let target = the_target("/v2/team/manifests/blobs/sha256:abc").expect("the target");
        assert_eq!(
            target,
            Target::Blob {
                name: "team/manifests".to_string(),
                reference: "sha256:abc".to_string(),
            }
        );
    }

    /// The upload surface distinguishes the start of the session.
    #[test]
    fn the_upload_path_tells_the_start_from_the_session() {
        let start = the_target("/v2/team/demo/blobs/uploads/").expect("the start");
        assert_eq!(
            start,
            Target::UploadStart {
                name: "team/demo".to_string()
            }
        );

        // Without the trailing slash it is the same request.
        let start = the_target("/v2/team/demo/blobs/uploads").expect("the start");
        assert_eq!(
            start,
            Target::UploadStart {
                name: "team/demo".to_string()
            }
        );

        let session = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";
        let target =
            the_target(&format!("/v2/team/demo/blobs/uploads/{session}")).expect("the session");
        assert_eq!(
            target,
            Target::UploadSession {
                name: "team/demo".to_string(),
                session: session.to_string(),
            }
        );
    }

    /// A session identifier that is not a UUID is not a session.
    #[test]
    fn a_session_id_that_is_not_a_uuid_is_not_a_session() {
        let error = the_target("/v2/team/demo/blobs/uploads/..%2Fetc%2Fpasswd")
            .expect_err("the traversal is not a session");
        assert_eq!(error.the_code(), "BLOB_UPLOAD_UNKNOWN");
    }

    /// The traversal is rejected before touching the catalog.
    #[test]
    fn a_traversal_name_is_refused_before_anything_else() {
        let error = the_target("/v2/..%2Fescape/manifests/v1").expect_err("the traversal");
        assert_eq!(error.the_code(), "NAME_INVALID");
    }

    /// A digest without its algorithm is not a digest.
    #[test]
    fn a_digest_without_its_algorithm_is_refused() {
        let error = the_digest_of_reference("not-a-digest").expect_err("the digest");
        assert_eq!(error.the_code(), "DIGEST_INVALID");
    }
}
