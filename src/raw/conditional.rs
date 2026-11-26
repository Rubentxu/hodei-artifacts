//! The conditions of a request: the `ETag` and what the client says
//! about it.
//!
//! A content's `ETag` is its **digest**: the content's identity,
//! which is the only one that can be compared without reading the bytes.
//!
//! Conditions are applied with the RFC 9110 rule, which distinguishes
//! between reading and writing:
//!
//! - On **read**, a matching `If-None-Match` means "don't send it
//!   to me again": the response is 304. A non-matching `If-Match` means
//!   "only if it is that one": the response is 412.
//! - On **write**, both failing conditions are answered with
//!   412: the request is not carried out. A 304 would be a lie, because 304 says
//!   "there is nothing new", and here there is something new: what the client
//!   wants to write.

use hodei_catalog::Precondition;
use hodei_evidence::domain::content::ContentDigest;

/// The tag of a content.
///
/// It is quoted because it is an entity tag, and the digest goes
/// inside because it is what distinguishes it from another.
pub fn the_etag(digest: &ContentDigest) -> String {
    format!("\"{}\"", digest)
}

/// Whether the header says the content is **this one**, with strong comparison.
///
/// `*` says "the one that is there", and a list of tags says "one of these".
///
/// The comparison is **strong**, which is what RFC 9110 requires for
/// `If-Match`: a weak tag `W/"..."` is not the tag that was
/// generated, and cannot say the content is that one.
pub fn the_strong_match(header: &str, etag: &str) -> bool {
    let header = header.trim();
    if header == "*" {
        return true;
    }
    let wanted = etag.trim();
    header.split(',').any(|candidate| {
        let candidate = candidate.trim();
        // A weak tag is not the tag that was generated: the digest
        // is strong, and saying two contents with the same digest are
        // the same content is exactly what is wanted.
        !candidate.starts_with("W/") && candidate == wanted
    })
}

/// Whether the header says the content is **this one**, with weak comparison.
///
/// `*` says "the one that is there", and a list of tags says "one of these".
///
/// The comparison is **weak**, which is what RFC 9110 requires for
/// `If-None-Match`: what is compared is the **opaque tag**, without the
/// `W/` prefix. A `W/"..."` and a `"..."` are the same entity for this
/// comparison, which is just what a client revalidating
/// against a cache needs.
pub fn the_weak_match(header: &str, etag: &str) -> bool {
    let header = header.trim();
    if header == "*" {
        return true;
    }
    let wanted = the_opaque_tag(etag.trim());
    header.split(',').any(|candidate| {
        let candidate = the_opaque_tag(candidate.trim());
        !candidate.is_empty() && candidate == wanted
    })
}

/// The opaque tag of an entity tag: what remains without the leading
/// `W/`.
fn the_opaque_tag(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

/// The `If-Match` header of a request.
pub fn the_if_match(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// The `If-None-Match` header of a request.
pub fn the_if_none_match(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// What a **read** condition decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    /// There was no condition, or the condition is met: the
    /// content is served.
    Serve,
    /// The client already has that content: the response is 304, which
    /// is not a failure, but the answer to "give me this only if it has changed".
    NotModified,
    /// The condition is not met: the response is 412.
    Failed,
}

/// What a **write** condition decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// There was no condition, or the condition is met: it is written.
    Write,
    /// The condition is not met: the response is 412 and **nothing** is
    /// written. Neither the bytes nor the asset.
    Failed,
}

/// The condition of a **read**.
///
/// The `etag` is `None` when there is nothing at that address: then an
/// `If-Match` cannot be met, because there is nothing to compare.
pub fn the_read_condition(
    if_match: Option<&str>,
    if_none_match: Option<&str>,
    etag: Option<&str>,
) -> ReadOutcome {
    if let Some(header) = if_match {
        let matches = etag.is_some_and(|etag| the_strong_match(header, etag));
        if !matches {
            return ReadOutcome::Failed;
        }
    }
    if let Some(header) = if_none_match
        && let Some(etag) = etag
        && the_weak_match(header, etag)
    {
        // The content is the one the client already has.
        return ReadOutcome::NotModified;
    }
    ReadOutcome::Serve
}

/// The condition of a **write**.
///
/// Both failing conditions are answered with 412, and the request is not
/// carried out: neither the content is written nor the asset touched.
///
/// And an `If-Match` on an **empty** address fails: `*` means
/// "the resource that is there", and if there is no resource, there is nothing to
/// compare.
pub fn the_write_condition(
    if_match: Option<&str>,
    if_none_match: Option<&str>,
    etag: Option<&str>,
) -> WriteOutcome {
    if let Some(header) = if_match {
        let matches = etag.is_some_and(|etag| the_strong_match(header, etag));
        if !matches {
            return WriteOutcome::Failed;
        }
    }
    if let Some(header) = if_none_match
        && let Some(etag) = etag
        && the_weak_match(header, etag)
    {
        // The content is already there: a write that says "only if it does not
        // exist" is not carried out.
        return WriteOutcome::Failed;
    }
    WriteOutcome::Write
}

/// The condition the client requires on what is there now.
///
/// It is what is carried to the catalog. The check at the boundary
/// is a **courtesy**: it says early what is going to happen. The one that counts is
/// the catalog's, because it happens **inside the transaction** that writes,
/// and there is no window there between checking and writing.
///
/// What is not understood does not travel: a tag that is not a digest cannot
/// be compared with anything, and a condition that cannot be compared
/// is not a condition.
pub fn the_precondition(
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> Option<Precondition> {
    if let Some(header) = if_match {
        let header = header.trim();
        if header == "*" {
            return Some(Precondition::Exists);
        }
        let digests = the_digests_of(header);
        if !digests.is_empty() {
            return Some(Precondition::Present(digests));
        }
    }
    if let Some(header) = if_none_match {
        let header = header.trim();
        if header == "*" {
            return Some(Precondition::Absent);
        }
        let digests = the_digests_of(header);
        if !digests.is_empty() {
            return Some(Precondition::Other(digests));
        }
    }
    None
}

/// The digests of a list of tags.
///
/// A tag is a quoted digest, and the list is separated by
/// commas. What is not a digest is left out: a list with an
/// entry that is not understood is a list that cannot be compared.
fn the_digests_of(header: &str) -> Vec<ContentDigest> {
    header
        .split(',')
        .filter_map(|tag| {
            let tag = tag.trim();
            let tag = tag.strip_prefix("W/").unwrap_or(tag).trim();
            let tag = tag.trim_matches('"');
            let (algorithm, value) = tag.split_once(':')?;
            if !algorithm.eq_ignore_ascii_case("sha256") {
                return None;
            }
            let digest = ContentDigest::sha256(value);
            if digest.is_well_formed() {
                Some(digest)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The digest of any content.
    fn a_digest() -> ContentDigest {
        ContentDigest::of_bytes(b"content")
    }

    #[test]
    fn the_etag_is_the_quoted_digest() {
        let digest = a_digest();
        let etag = the_etag(&digest);
        assert!(etag.starts_with('"') && etag.ends_with('"'));
        assert!(etag.contains(&digest.value));
    }

    #[test]
    fn the_header_matches_the_etag() {
        let etag = the_etag(&a_digest());
        assert!(the_strong_match(&etag, &etag));
        assert!(the_strong_match("*", &etag));
        assert!(!the_strong_match("\"other\"", &etag));
    }

    #[test]
    fn a_list_of_tags_matches_if_one_of_them_does() {
        let etag = the_etag(&a_digest());
        assert!(the_strong_match(&format!("\"other\", {etag}"), &etag));
    }

    #[test]
    fn a_weak_tag_is_not_the_tag_that_was_generated() {
        let etag = the_etag(&a_digest());
        let weak = format!("W/{etag}");
        assert!(!the_strong_match(&weak, &etag));
    }

    #[test]
    fn a_weak_tag_is_the_same_entity_for_the_weak_comparison() {
        // RFC 9110: `If-None-Match` is compared with the **weak** function,
        // and a leading `W/` says "shares the representation". A client
        // revalidating against a cache sends the tag this way.
        let etag = the_etag(&a_digest());
        let weak = format!("W/{etag}");
        assert!(the_weak_match(&weak, &etag));
        assert!(the_weak_match(&etag, &etag));
        assert!(!the_weak_match("\"other\"", &etag));
    }

    #[test]
    fn a_weak_read_with_a_matching_if_none_match_is_not_modified() {
        let etag = the_etag(&a_digest());
        let weak = format!("W/{etag}");
        assert_eq!(
            the_read_condition(None, Some(&weak), Some(&etag)),
            ReadOutcome::NotModified
        );
    }

    #[test]
    fn the_precondition_of_an_if_match_is_the_content_it_names() {
        let digest = a_digest();
        let etag = the_etag(&digest);
        assert_eq!(
            the_precondition(Some(&etag), None),
            Some(Precondition::Present(vec![digest.clone()]))
        );
    }

    #[test]
    fn the_precondition_of_a_star_is_the_absence_or_the_presence() {
        assert_eq!(
            the_precondition(Some("*"), None),
            Some(Precondition::Exists)
        );
        assert_eq!(
            the_precondition(None, Some("*")),
            Some(Precondition::Absent)
        );
    }

    #[test]
    fn a_condition_that_cannot_be_compared_does_not_travel() {
        // A tag that is not a digest cannot be compared with
        // anything, and a condition that cannot be compared is not a
        // condition.
        assert_eq!(the_precondition(Some("\"other\""), None), None);
        assert_eq!(the_precondition(None, Some("\"other\"")), None);
    }

    #[test]
    fn a_read_with_a_matching_if_none_match_is_not_modified() {
        let etag = the_etag(&a_digest());
        assert_eq!(
            the_read_condition(None, Some(&etag), Some(&etag)),
            ReadOutcome::NotModified
        );
    }

    #[test]
    fn a_read_with_a_wrong_if_match_fails() {
        let etag = the_etag(&a_digest());
        assert_eq!(
            the_read_condition(Some("\"other\""), None, Some(&etag)),
            ReadOutcome::Failed
        );
    }

    #[test]
    fn a_read_of_something_that_is_not_there_fails_an_if_match() {
        assert_eq!(
            the_read_condition(Some("*"), None, None),
            ReadOutcome::Failed
        );
    }

    #[test]
    fn a_write_with_a_matching_if_none_match_fails() {
        let etag = the_etag(&a_digest());
        assert_eq!(
            the_write_condition(None, Some(&etag), Some(&etag)),
            WriteOutcome::Failed
        );
    }

    #[test]
    fn a_write_with_a_matching_if_match_goes_on() {
        let etag = the_etag(&a_digest());
        assert_eq!(
            the_write_condition(Some(&etag), None, Some(&etag)),
            WriteOutcome::Write
        );
    }

    #[test]
    fn a_write_with_an_if_match_on_an_empty_path_fails() {
        // `*` means "the resource that is there", and if there is no resource, there
        // is nothing to compare.
        assert_eq!(
            the_write_condition(Some("*"), None, None),
            WriteOutcome::Failed
        );
    }

    #[test]
    fn a_write_with_an_if_none_match_on_an_empty_path_goes_on() {
        assert_eq!(
            the_write_condition(None, Some("*"), None),
            WriteOutcome::Write
        );
    }
}
