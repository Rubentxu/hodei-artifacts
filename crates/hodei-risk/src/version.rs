//! The matching of a version against the ranges of an advisory.
//!
//! This is where a lot of systems go wrong, and the reason is that they
//! answer "not affected" when they do not know. A version that cannot be
//! compared, a range that cannot be interpreted, an ecosystem whose
//! versions do not follow the rules: **none of those mean that the
//! artefact is safe**. They mean that nobody knows, and a system that
//! cannot say "nobody knows" will happily report that a deployment with
//! four hundred dependencies is fine.
//!
//! So the answer has three values and not two, and the third one is the
//! interesting one.

use crate::osv::{OsvAffected, OsvRange};

/// What the matching found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Match {
    /// The version is inside the affected range.
    Affected,
    /// The version is outside the affected range.
    Unaffected,
    /// Nobody knows.
    ///
    /// The version could not be read, the range could not be
    /// interpreted, or the advisory says nothing about this package.
    Unknown,
}

/// Decides whether a version is affected by an advisory.
///
/// The answer is `Unknown` whenever the facts are not enough, and never
/// `Unaffected` out of ignorance.
pub fn match_version(affected: &OsvAffected, version: &str) -> Match {
    // The advisory may name the versions one by one. That is the most
    // exact thing it can say, and it comes first.
    if affected
        .versions
        .iter()
        .any(|candidate| candidate == version)
    {
        return Match::Affected;
    }

    if affected.ranges.is_empty() {
        // The advisory talks about the package and says nothing about
        // which versions. That is not a statement that every version is
        // fine; it is no statement at all.
        return Match::Unknown;
    }

    let Some(parsed) = parse(version) else {
        // The version is not one this system can order. Nobody knows.
        return Match::Unknown;
    };

    let mut concluded = None;
    for range in &affected.ranges {
        match match_range(range, &parsed) {
            RangeOutcome::Affected => return Match::Affected,
            RangeOutcome::Unaffected => concluded = Some(Match::Unaffected),
            RangeOutcome::Unknown => {}
        }
    }

    concluded.unwrap_or(Match::Unknown)
}

/// What one range says.
enum RangeOutcome {
    Affected,
    Unaffected,
    Unknown,
}

/// The version, read for comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// The version as it was written.
    pub raw: String,
    /// The version, ordered.
    pub ordered: semver::Version,
}

/// Reads a version for comparison.
///
/// A version that is not a semantic version is not read, and the caller
/// says that nobody knows. A version like `1.2.3` is read; a version like
/// `2024.05.01-SNAPSHOT` is not, and pretending that it is would be
/// comparing things that do not have an order.
pub fn parse(version: &str) -> Option<Version> {
    let trimmed = version.trim();
    if trimmed.is_empty() || trimmed == "0" {
        // The `0` is what OSV uses to say "from the beginning". It is
        // handled by the range, not as a version.
        return None;
    }
    let ordered = semver::Version::parse(trimmed).ok()?;
    Some(Version {
        raw: version.to_string(),
        ordered,
    })
}

/// Decides what one range says about a version.
fn match_range(range: &OsvRange, version: &Version) -> RangeOutcome {
    if range.range_type != "ECOSYSTEM" {
        // The Git ranges are about commits and not about versions, and
        // Hodei does not compare commits. Nobody knows.
        return RangeOutcome::Unknown;
    }

    let mut introduced: Option<semver::Version> = None;
    let mut fixed: Option<semver::Version> = None;
    let mut last_affected: Option<semver::Version> = None;

    for event in &range.events {
        match (&event.introduced, &event.fixed, &event.last_affected) {
            (Some(at), _, _) => {
                // The `0` means "from the beginning".
                if at.trim() != "0" {
                    match semver::Version::parse(at.trim()) {
                        Ok(parsed) => introduced = Some(parsed),
                        Err(_) => return RangeOutcome::Unknown,
                    }
                }
            }
            (_, Some(at), _) => match semver::Version::parse(at.trim()) {
                Ok(parsed) => fixed = Some(parsed),
                Err(_) => return RangeOutcome::Unknown,
            },
            (_, _, Some(at)) => match semver::Version::parse(at.trim()) {
                Ok(parsed) => last_affected = Some(parsed),
                Err(_) => return RangeOutcome::Unknown,
            },
            _ => {}
        }
    }

    if introduced.is_none() && fixed.is_none() && last_affected.is_none() {
        return RangeOutcome::Unknown;
    }

    if let Some(introduced) = &introduced
        && &version.ordered < introduced
    {
        // The version is before the range starts.
        return RangeOutcome::Unaffected;
    }

    if let Some(fixed) = &fixed
        && &version.ordered >= fixed
    {
        // The version is at or after the fix.
        return RangeOutcome::Unaffected;
    }

    if let Some(last_affected) = &last_affected
        && &version.ordered > last_affected
    {
        // The version is after the last affected one.
        return RangeOutcome::Unaffected;
    }

    if introduced.is_some() || fixed.is_some() || last_affected.is_some() {
        RangeOutcome::Affected
    } else {
        RangeOutcome::Unknown
    }
}

/// The version of a package, as the advisory names it.
pub fn version_of(affected: &OsvAffected) -> Option<&str> {
    affected.versions.first().map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osv::read_osv;

    const ADVISORY: &str = r#"{
      "id": "GHSA-test-0002",
      "affected": [
        {
          "package": {"ecosystem": "Maven", "name": "org.example:lib-b"},
          "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.0.1"}]}]
        }
      ]
    }"#;

    const ADVISORY_WITH_LAST_AFFECTED: &str = r#"{
      "id": "GHSA-test-0003",
      "affected": [
        {
          "package": {"ecosystem": "Maven", "name": "org.example:lib-b"},
          "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "1.0.0"}, {"last_affected": "1.9.9"}]}]
        }
      ]
    }"#;

    fn affected_of(document: &str) -> OsvAffected {
        let advisory = read_osv(document).expect("the document is read");
        advisory.affected.into_iter().next().expect("it has one")
    }

    #[test]
    fn a_version_inside_the_range_is_affected() {
        let affected = affected_of(ADVISORY);
        assert_eq!(match_version(&affected, "1.5.0"), Match::Affected);
        assert_eq!(match_version(&affected, "2.0.0"), Match::Affected);
    }

    #[test]
    fn a_version_outside_the_range_is_not_affected() {
        let affected = affected_of(ADVISORY);
        assert_eq!(match_version(&affected, "2.0.1"), Match::Unaffected);
        assert_eq!(match_version(&affected, "3.1.4"), Match::Unaffected);
    }

    #[test]
    fn a_range_that_ends_at_last_affected_ends_there() {
        let affected = affected_of(ADVISORY_WITH_LAST_AFFECTED);
        assert_eq!(match_version(&affected, "1.9.9"), Match::Affected);
        assert_eq!(match_version(&affected, "2.0.0"), Match::Unaffected);
    }

    #[test]
    fn a_version_that_cannot_be_read_is_unknown_and_not_safe() {
        let affected = affected_of(ADVISORY);
        assert_eq!(
            match_version(&affected, "2024.05.01-SNAPSHOT"),
            Match::Unknown,
            "a version nobody can order is not a version nobody is affected by"
        );
    }

    #[test]
    fn a_package_the_advisory_says_nothing_about_is_unknown() {
        let affected = OsvAffected {
            package: crate::osv::OsvPackage {
                ecosystem: "Maven".to_string(),
                name: "org.example:lib-b".to_string(),
                purl: None,
            },
            ranges: Vec::new(),
            versions: Vec::new(),
            ecosystem_specific: serde_json::Value::Null,
        };
        assert_eq!(
            match_version(&affected, "1.0.0"),
            Match::Unknown,
            "the advisory says nothing about the versions, and that is not a statement that they are fine"
        );
    }

    #[test]
    fn a_version_named_one_by_one_is_affected() {
        let mut affected = affected_of(ADVISORY);
        affected.versions = vec!["1.2.3".to_string()];
        assert_eq!(match_version(&affected, "1.2.3"), Match::Affected);
    }
}
