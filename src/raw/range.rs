//! The request for a span of the content.
//!
//! RFC 9110 defines byte ranges. What this module decides is
//! **what is served**: a valid range is served with 206 and its
//! `Content-Range`, a range that cannot be satisfied is answered with 416
//! and the `Content-Range` that says how much the content has, and a `Range`
//! that is not understood is ignored, which is what the RFC allows.
//!
//! What is NOT done is serving a half range: a 206 with fewer
//! bytes than the client asked for is a 206 that means nothing.

/// What the client requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeRequest {
    /// There is no range: the whole content is served.
    Whole,
    /// A range, with the first byte and the last byte **included**.
    Single { first: u64, last: u64 },
}

/// Why a range cannot be served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeError {
    /// The range does not fit in the content, or is not well written.
    Unsatisfiable,
}

impl RangeError {
    /// What is told to the client.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Unsatisfiable => "the requested range is not satisfiable",
        }
    }
}

/// Which range the `Range` header requests.
///
/// The whole content is answered with `Whole`. A `Range` with another
/// unit, or with several ranges, is **ignored**: RFC 9110 allows it, and
/// a server that does not know how to do what it is asked can say it does
/// not know, but cannot tell a half truth.
pub fn the_range(header: Option<&str>, length: u64) -> Result<RangeRequest, RangeError> {
    let Some(header) = header else {
        return Ok(RangeRequest::Whole);
    };
    let header = header.trim();
    // The unit is case-insensitive (RFC 9110): `Bytes=0-9` is a
    // range, not another unit. What is compared is the name of the
    // unit, not how it is written.
    let Some((unit, specification)) = header.split_once('=') else {
        // It does not even have the shape of a unit: it is not understood, and what
        // is not understood is ignored.
        return Ok(RangeRequest::Whole);
    };
    if !unit.trim().eq_ignore_ascii_case("bytes") {
        // Another unit: not understood, and what is not understood is ignored.
        return Ok(RangeRequest::Whole);
    }
    let specification = specification.trim();
    if specification.contains(',') {
        // Several ranges: this release serves one. The request is ignored and
        // the whole content is served, which is what the RFC allows.
        return Ok(RangeRequest::Whole);
    }
    let Some((first, last)) = specification.split_once('-') else {
        return Err(RangeError::Unsatisfiable);
    };
    let first = first.trim();
    let last = last.trim();

    if first.is_empty() {
        // `bytes=-N`: the last N bytes.
        let suffix: u64 = last.parse().map_err(|_| RangeError::Unsatisfiable)?;
        if suffix == 0 || length == 0 {
            return Err(RangeError::Unsatisfiable);
        }
        let first = length.saturating_sub(suffix);
        return Ok(RangeRequest::Single {
            first,
            last: length - 1,
        });
    }

    let first: u64 = first.parse().map_err(|_| RangeError::Unsatisfiable)?;
    let last: u64 = if last.is_empty() {
        // `bytes=N-`: from N to the end.
        if length == 0 {
            return Err(RangeError::Unsatisfiable);
        }
        length - 1
    } else {
        last.parse().map_err(|_| RangeError::Unsatisfiable)?
    };

    if first > last || first >= length {
        // The RFC says an invalid range may be ignored, and one that
        // cannot be satisfied is answered with 416. Both cases are
        // answered the same, because in both the client asked for something that
        // does not exist.
        return Err(RangeError::Unsatisfiable);
    }
    Ok(RangeRequest::Single {
        first,
        last: last.min(length - 1),
    })
}

/// The `Content-Range` header of a range.
pub fn the_content_range(first: u64, last: u64, length: u64) -> String {
    format!("bytes {first}-{last}/{length}")
}

/// The `Content-Range` header of a content for which the requested
/// range cannot be served.
pub fn the_unsatisfiable_range(length: u64) -> String {
    format!("bytes */{length}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_range_the_whole_content_is_served() {
        assert_eq!(the_range(None, 100), Ok(RangeRequest::Whole));
        assert_eq!(
            the_range(Some("bytes=0-9"), 100),
            Ok(RangeRequest::Single { first: 0, last: 9 })
        );
    }

    #[test]
    fn the_unit_of_the_range_does_not_distinguish_capitals() {
        // RFC 9110: unit names are case-insensitive.
        assert_eq!(
            the_range(Some("Bytes=0-9"), 100),
            Ok(RangeRequest::Single { first: 0, last: 9 })
        );
        assert_eq!(
            the_range(Some("BYTES=0-9"), 100),
            Ok(RangeRequest::Single { first: 0, last: 9 })
        );
        // But another unit is still another unit.
        assert_eq!(the_range(Some("items=0-9"), 100), Ok(RangeRequest::Whole));
    }

    #[test]
    fn a_range_that_starts_and_ends_is_served() {
        assert_eq!(
            the_range(Some("bytes=10-19"), 100),
            Ok(RangeRequest::Single {
                first: 10,
                last: 19
            })
        );
    }

    #[test]
    fn a_range_without_an_end_goes_to_the_end() {
        assert_eq!(
            the_range(Some("bytes=90-"), 100),
            Ok(RangeRequest::Single {
                first: 90,
                last: 99
            })
        );
    }

    #[test]
    fn a_suffix_range_takes_the_last_bytes() {
        assert_eq!(
            the_range(Some("bytes=-10"), 100),
            Ok(RangeRequest::Single {
                first: 90,
                last: 99
            })
        );
    }

    #[test]
    fn a_range_that_past_the_end_is_not_satisfiable() {
        assert_eq!(
            the_range(Some("bytes=100-200"), 100),
            Err(RangeError::Unsatisfiable)
        );
        assert_eq!(
            the_range(Some("bytes=100-"), 100),
            Err(RangeError::Unsatisfiable)
        );
    }

    #[test]
    fn an_invalid_range_is_not_satisfiable() {
        assert_eq!(
            the_range(Some("bytes=abc"), 100),
            Err(RangeError::Unsatisfiable)
        );
        assert_eq!(
            the_range(Some("bytes=10-5"), 100),
            Err(RangeError::Unsatisfiable)
        );
    }

    #[test]
    fn a_range_in_another_unit_is_ignored() {
        assert_eq!(the_range(Some("items=0-9"), 100), Ok(RangeRequest::Whole));
    }

    #[test]
    fn several_ranges_are_ignored_and_the_whole_is_served() {
        assert_eq!(
            the_range(Some("bytes=0-9,20-29"), 100),
            Ok(RangeRequest::Whole)
        );
    }

    #[test]
    fn the_content_range_says_what_is_served() {
        assert_eq!(the_content_range(0, 9, 100), "bytes 0-9/100");
        assert_eq!(the_unsatisfiable_range(100), "bytes */100");
    }
}
