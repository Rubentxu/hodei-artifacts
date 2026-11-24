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

impl Match {
    /// The name of the answer.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Affected => "affected",
            Self::Unaffected => "unaffected",
            Self::Unknown => "unknown",
        }
    }
}

/// Decides whether a version is affected by an advisory.
///
/// The answer is `Unknown` whenever the facts are not enough, and never
/// `Unaffected` out of ignorance.
pub fn match_version(affected: &OsvAffected, version: &str) -> Match {
    explain(affected, version).0
}

/// Decides whether a version is affected and says WHY.
///
/// The answer of `match_version` and the reason are one reading and not
/// two: the justification is what the range said, told in the order the
/// range gave, so that an oracle can check the set AND the reason for
/// every member of it.
pub fn explain(affected: &OsvAffected, version: &str) -> (Match, String) {
    // The advisory may name the versions one by one. That is the most
    // exact thing it can say, and it comes first.
    if affected
        .versions
        .iter()
        .any(|candidate| candidate == version)
    {
        return (
            Match::Affected,
            format!("the advisory names the version {version} one by one"),
        );
    }

    if affected.ranges.is_empty() {
        // The advisory talks about the package and says nothing about
        // which versions. That is not a statement that every version is
        // fine; it is no statement at all.
        return (
            Match::Unknown,
            "the advisory says nothing about which versions are affected".to_string(),
        );
    }

    let Some(parsed) = parse(&affected.package.ecosystem, version) else {
        // The version is not one this system can order. Nobody knows.
        return (
            Match::Unknown,
            format!(
                "the version {version} cannot be ordered by the rules of {}",
                affected.package.ecosystem
            ),
        );
    };

    let mut concluded: Option<(Match, String)> = None;
    for range in &affected.ranges {
        match match_range(range, &affected.package.ecosystem, &parsed) {
            (RangeOutcome::Affected, reason) => return (Match::Affected, reason),
            (RangeOutcome::Unaffected, reason) => concluded = Some((Match::Unaffected, reason)),
            (RangeOutcome::Unknown, _) => {}
        }
    }

    concluded.unwrap_or_else(|| {
        (
            Match::Unknown,
            "no range of the advisory could be read for this version".to_string(),
        )
    })
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

/// Reads a version for comparison, with the rule of its ecosystem.
///
/// A version the ecosystem rule cannot read is not read, and the caller
/// says that nobody knows. A version like `1.2.3` is read; a version like
/// `2024.05.01-SNAPSHOT` is not, and pretending that it is would be
/// comparing things that do not have an order. Maven's `1.2` and PyPI's
/// `1.0.0rc1` are read, because their ecosystems say what those mean.
pub fn parse(ecosystem: &str, version: &str) -> Option<Version> {
    let trimmed = version.trim();
    if trimmed.is_empty() || trimmed == "0" {
        // The `0` is what OSV uses to say "from the beginning". It is
        // handled by the range, not as a version.
        return None;
    }
    let ordered = normalize(ecosystem, trimmed)?;
    Some(Version {
        raw: version.to_string(),
        ordered,
    })
}

/// Decides what one range says about a version.
///
/// The boundaries of the range are read with the rule of the ecosystem
/// the package belongs to: a Maven range that ends at `1.2` ends at
/// `1.2.0`, and a PyPI range that ends at `2.1` ends at `2.1.0`. The
/// version being compared is read the same way -- the inventory and the
/// advisory write the same package the same way, and reading one with the
/// rule and the other without it would be comparing things that do not
/// have the same order.
fn match_range(range: &OsvRange, ecosystem: &str, version: &Version) -> (RangeOutcome, String) {
    if range.range_type != "ECOSYSTEM" {
        // The Git ranges are about commits and not about versions, and
        // Hodei does not compare commits. Nobody knows.
        return (
            RangeOutcome::Unknown,
            format!(
                "the range is of type {} and its bounds are not versions",
                range.range_type
            ),
        );
    }

    let mut introduced: Option<semver::Version> = None;
    let mut fixed: Option<semver::Version> = None;
    let mut last_affected: Option<semver::Version> = None;

    for event in &range.events {
        match (&event.introduced, &event.fixed, &event.last_affected) {
            (Some(at), _, _) => {
                // The `0` means "from the beginning".
                if at.trim() != "0" {
                    match normalize(ecosystem, at) {
                        Some(parsed) => introduced = Some(parsed),
                        None => {
                            return (
                                RangeOutcome::Unknown,
                                format!("the introduced bound {at} cannot be read"),
                            );
                        }
                    }
                }
            }
            (_, Some(at), _) => match normalize(ecosystem, at) {
                Some(parsed) => fixed = Some(parsed),
                None => {
                    return (
                        RangeOutcome::Unknown,
                        format!("the fixed bound {at} cannot be read"),
                    );
                }
            },
            (_, _, Some(at)) => match normalize(ecosystem, at) {
                Some(parsed) => last_affected = Some(parsed),
                None => {
                    return (
                        RangeOutcome::Unknown,
                        format!("the last-affected bound {at} cannot be read"),
                    );
                }
            },
            _ => {}
        }
    }

    if introduced.is_none() && fixed.is_none() && last_affected.is_none() {
        return (
            RangeOutcome::Unknown,
            "the range has no bounds that could be read".to_string(),
        );
    }

    if let Some(introduced) = &introduced
        && &version.ordered < introduced
    {
        // The version is before the range starts.
        return (
            RangeOutcome::Unaffected,
            format!(
                "the version {} is before the range introduced at {introduced}",
                version.raw
            ),
        );
    }

    if let Some(fixed) = &fixed
        && &version.ordered >= fixed
    {
        // The version is at or after the fix.
        return (
            RangeOutcome::Unaffected,
            format!(
                "the version {} is at or after the fix at {fixed}",
                version.raw
            ),
        );
    }

    if let Some(last_affected) = &last_affected
        && &version.ordered > last_affected
    {
        // The version is after the last affected one.
        return (
            RangeOutcome::Unaffected,
            format!(
                "the version {} is after the last affected one at {last_affected}",
                version.raw
            ),
        );
    }

    if introduced.is_some() || fixed.is_some() || last_affected.is_some() {
        let mut bounds = Vec::new();
        if let Some(introduced) = &introduced {
            bounds.push(format!("introduced {introduced}"));
        }
        if let Some(fixed) = &fixed {
            bounds.push(format!("fixed at {fixed}"));
        }
        if let Some(last_affected) = &last_affected {
            bounds.push(format!("last affected at {last_affected}"));
        }
        (
            RangeOutcome::Affected,
            format!(
                "the version {} is inside the range ({})",
                version.raw,
                bounds.join(", ")
            ),
        )
    } else {
        (
            RangeOutcome::Unknown,
            "the range says nothing about this version".to_string(),
        )
    }
}

/// The version of a package, as the advisory names it.
pub fn version_of(affected: &OsvAffected) -> Option<&str> {
    affected.versions.first().map(String::as_str)
}

/// The version of the package, read for the ecosystem it belongs to.
///
/// The three ecosystems Hodei reads do not write versions the same way:
///
/// - Maven writes `1.2` where a semantic version needs `1.2.0`, and
///   suffixes releases with `.Final`, `.GA` and `.RELEASE`.
/// - npm writes semantic versions, which are read as they come.
/// - PyPI writes PEP 440 versions: `1.0.0rc1` is a pre-release of
///   `1.0.0` and `1.0.0.dev1` is a development one, and both can be
///   ordered against the release the same way semantic pre-releases can.
///   A post-release (`1.0.0.post1`) is strictly after `1.0.0` and strictly
///   before `1.0.1`, and no semantic version says that: it is not read,
///   and the answer is that nobody knows. An epoch (`1!1.0`) is not read
///   either.
///
/// A version that cannot be read is not forced into a shape it does not
/// have; the caller answers `Unknown`, which is what the whole module is
/// for.
pub fn normalize(ecosystem: &str, version: &str) -> Option<semver::Version> {
    let trimmed = version.trim();
    if trimmed.is_empty() {
        return None;
    }

    // The rule of the ecosystem comes first for the ones that write
    // versions the semantic reading would take for something else: a
    // Maven `1.4.1-GA` is the release `1.4.1` and not a pre-release of
    // it, and a PyPI `1.0.0rc1` is a pre-release written without the
    // separator. Everything else is read as it is written, because
    // forcing a rule the ecosystem does not use would be inventing.
    match ecosystem {
        "Maven" => normalize_maven(trimmed),
        "PyPI" => normalize_pypi(trimmed),
        _ => semver::Version::parse(trimmed).ok(),
    }
}

/// The PEP 440 ways of writing a version, read for ordering.
fn normalize_pypi(version: &str) -> Option<semver::Version> {
    // An epoch is a version with a number in front of it that orders the
    // whole thing differently: `1!1.0` after `2.0`. Nobody can order it
    // here, and saying so is the honest answer.
    if version.contains('!') {
        return None;
    }

    // PEP 440 writes the pre-releases with and without a separator, and
    // the names are written out in full and as one letter. The one
    // letters mean the same as the full names -- `a` is `alpha`, `b` is
    // `beta`, `c` is the same release candidate as `rc` -- and they are
    // read to the SAME name, because `1.0.0c1` and `1.0.0rc1` are one
    // version and reading them as two would be inventing an order.
    //
    // A post-release (`1.0.0.post1`) is strictly after `1.0.0` and
    // strictly before `1.0.1`, and no semantic version says that: it is
    // not read, and the answer is that nobody knows.
    //
    // The longer names are tried first, so that the `a` of `alpha` is not
    // taken for the short `a`.
    let names: [(&str, &str); 7] = [
        ("alpha", "alpha"),
        ("beta", "beta"),
        ("dev", "dev"),
        ("rc", "rc"),
        ("a", "alpha"),
        ("b", "beta"),
        ("c", "rc"),
    ];

    let lowered = version.to_ascii_lowercase();
    for (suffix, name) in names {
        let Some((base, number)) = split_prefixed(&lowered, suffix) else {
            continue;
        };
        let padded = pad_minor(base)?;
        let pre = if number.is_empty() {
            name.to_string()
        } else {
            format!("{name}.{number}")
        };
        return semver::Version::parse(&format!("{padded}-{pre}")).ok();
    }

    // A plain PyPI release is written with two parts as often as with
    // three: `0.9` is `0.9.0`.
    let padded = pad_minor(version)?;
    semver::Version::parse(&padded).ok()
}

/// The release part and the pre-release number of a version written with
/// the suffix the caller names: `1.0.0rc1` with `rc` gives `("1.0.0",
/// "1")`, and `1.0.0.dev1` gives `("1.0.0", "1")` with the separator
/// folded away.
fn split_prefixed<'a>(version: &'a str, suffix: &str) -> Option<(&'a str, &'a str)> {
    let at = version.rfind(suffix)?;
    let base = version[..at].trim_end_matches(['.', '-']);
    if base.is_empty() || !is_release(base) {
        return None;
    }
    let number = &version[at + suffix.len()..];
    if !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((base, number))
}

/// Whether the text is a release: digits and dots, with no empty part.
fn is_release(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| c.is_ascii_digit() || c == '.')
        && !text.starts_with('.')
        && !text.ends_with('.')
        && !text.contains("..")
}

/// The Maven ways of writing a version, read for ordering.
fn normalize_maven(version: &str) -> Option<semver::Version> {
    // The release suffixes Maven carries and that say nothing about the
    // order: `1.2.3.Final`, `1.4.1-GA` and `2.0.0.RELEASE` are the
    // versions `1.2.3`, `1.4.1` and `2.0.0`.
    let mut text = version.to_string();
    for suffix in ["Final", "GA", "RELEASE"] {
        if let Some(stripped) = text.strip_suffix(suffix) {
            text = stripped.trim_end_matches(['.', '-', '_']).to_string();
            break;
        }
    }

    // Maven writes `1.2` where the semantic reading needs `1.2.0`.
    let padded = pad_minor(&text)?;

    // A `2.0.0-SNAPSHOT` is read as the pre-release of `2.0.0` it is in
    // the Maven order, because semver says the same: the plain reading
    // below covers it.
    semver::Version::parse(&padded).ok()
}

/// The version that needs its missing parts to be read: `1.2` is
/// `1.2.0` and `2` is `2.0.0`.
///
/// Maven and PyPI both leave the trailing zeros out of a numeric
/// version -- Maven's `2` and `1.2` name what the semantic reading
/// writes as `2.0.0` and `1.2.0` -- and padding them is what the
/// ecosystem means, not an invention. A version with more than three
/// numeric parts is NOT padded: `1.2.3.4` says something the semantic
/// reading cannot hear, and the caller answers that nobody knows.
fn pad_minor(version: &str) -> Option<String> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() > 3 || parts.is_empty() {
        return Some(version.to_string());
    }
    if !parts
        .iter()
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    {
        return Some(version.to_string());
    }
    let mut padded = parts
        .iter()
        .map(|part| part.to_string())
        .collect::<Vec<_>>();
    while padded.len() < 3 {
        padded.push("0".to_string());
    }
    Some(padded.join("."))
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

    #[test]
    fn maven_versions_are_read_with_their_own_rules() {
        // A minor without the patch part is the version its ecosystem
        // says: `1.2` is `1.2.0`.
        assert_eq!(
            normalize("Maven", "1.2"),
            semver::Version::parse("1.2.0").ok()
        );
        // The release suffixes say nothing about the order.
        assert_eq!(
            normalize("Maven", "1.2.3.Final"),
            semver::Version::parse("1.2.3").ok()
        );
        assert_eq!(
            normalize("Maven", "2.0.0.RELEASE"),
            semver::Version::parse("2.0.0").ok()
        );
        assert_eq!(
            normalize("Maven", "1.4.1-GA"),
            semver::Version::parse("1.4.1").ok()
        );
        // A SNAPSHOT is the pre-release of the version it names: Maven
        // puts it before the release, and the semantic reading says the
        // same thing.
        assert_eq!(
            normalize("Maven", "2.0.0-SNAPSHOT"),
            semver::Version::parse("2.0.0-SNAPSHOT").ok()
        );
    }

    #[test]
    fn npm_versions_are_read_as_they_come() {
        assert_eq!(
            normalize("npm", "1.2.3"),
            semver::Version::parse("1.2.3").ok()
        );
        assert_eq!(
            normalize("npm", "1.2.3-beta.2"),
            semver::Version::parse("1.2.3-beta.2").ok()
        );
        // npm does not write two-part releases: the rule that pads them
        // is Maven's, not npm's, and forcing it would be inventing.
        assert_eq!(normalize("npm", "1.2"), None);
    }

    #[test]
    fn pypi_versions_are_read_with_their_own_rules() {
        // The pre-releases, with and without the separator, are read to
        // the name of the stage they belong to.
        assert_eq!(
            normalize("PyPI", "1.0.0rc1"),
            semver::Version::parse("1.0.0-rc.1").ok()
        );
        assert_eq!(
            normalize("PyPI", "1.0.0-rc.1"),
            semver::Version::parse("1.0.0-rc.1").ok()
        );
        assert_eq!(
            normalize("PyPI", "1.0.0c1"),
            normalize("PyPI", "1.0.0rc1"),
            "the short c is the release candidate it is in PEP 440"
        );
        assert_eq!(
            normalize("PyPI", "1.0.0a2"),
            semver::Version::parse("1.0.0-alpha.2").ok()
        );
        assert_eq!(
            normalize("PyPI", "1.0.0b2"),
            semver::Version::parse("1.0.0-beta.2").ok()
        );
        assert_eq!(
            normalize("PyPI", "2.1"),
            semver::Version::parse("2.1.0").ok()
        );
        // A post-release cannot be said with a semantic version: it is
        // after 1.0.0 and before 1.0.1, and nobody reads it.
        assert_eq!(normalize("PyPI", "1.0.0.post1"), None);
        // An epoch is not read either.
        assert_eq!(normalize("PyPI", "1!1.0"), None);
    }

    #[test]
    fn a_range_ending_at_a_two_part_maven_version_ends_at_the_padded_one() {
        let document = r#"{
          "id": "GHSA-test-0020",
          "affected": [
            {
              "package": {"ecosystem": "Maven", "name": "org.example:lib-m"},
              "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "1.2"}]}]
            }
          ]
        }"#;
        let affected = affected_of(document);
        assert_eq!(match_version(&affected, "1.1.9"), Match::Affected);
        assert_eq!(
            match_version(&affected, "1.2.0"),
            Match::Unaffected,
            "the fix at `1.2` fixes `1.2.0`, which is what Maven means"
        );
    }

    #[test]
    fn a_pypi_rc_before_the_release_is_inside_the_range_the_release_fixes() {
        let document = r#"{
          "id": "GHSA-test-0021",
          "affected": [
            {
              "package": {"ecosystem": "PyPI", "name": "example-pkg"},
              "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "1.0.0"}]}]
            }
          ]
        }"#;
        let affected = affected_of(document);
        assert_eq!(
            match_version(&affected, "1.0.0rc1"),
            Match::Affected,
            "the release candidate is before the release that fixes it"
        );
        assert_eq!(match_version(&affected, "1.0.0"), Match::Unaffected);
    }
}
