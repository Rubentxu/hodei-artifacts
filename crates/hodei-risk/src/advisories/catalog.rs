//! The name catalog the signals are judged against: the legitimate
//! packages of an ecosystem, their reservations, and the metadata the
//! repository said the last time.
//!
//! A signal is a comparison, and a comparison needs the other side. This
//! module holds that side in plain data: no registry, no network, no
//! script. The catalog is what the platform already knows — the names it
//! mirrors, the namespaces it reserves (HSP-143), and the metadata
//! record of the last publication — and it is a set of facts, not of
//! accusations.
//!
//! The judging semantics mirror `hodei-catalog::namespaces` (the
//! authority at resolution time): a Maven reservation covers the
//! groupId and its subgroups, npm reserves an `@scope` or an exact
//! name, and PyPI compares names normalized the way PEP 503 says.

use serde::{Deserialize, Serialize};

/// How the platform knows the ecosystem, enough to judge a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    /// The npm registry and the proxies that feed it.
    Npm,
    /// PyPI and its mirrors.
    Pypi,
    /// Maven Central and its mirrors.
    Maven,
}

impl Ecosystem {
    /// The name as the catalogs and the documents write it.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pypi => "PyPI",
            Self::Maven => "Maven",
        }
    }

    /// The ecosystem from how a document writes it, if it writes it a
    /// way this module can judge.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "npm" => Some(Self::Npm),
            "PyPI" | "pypi" => Some(Self::Pypi),
            "Maven" => Some(Self::Maven),
            _ => None,
        }
    }

    /// Whether a bare name (no group, no scope) is valid in the
    /// ecosystem.
    ///
    /// Maven cannot ship `log4j` by itself: a bare Maven name IS the
    /// anomaly, which is how the confusion attack of the CVE-2021 style
    /// gets in. npm and PyPI allow bare names, so for them a bare name
    /// is only weak evidence.
    pub fn bare_name_is_anomalous(&self) -> bool {
        matches!(self, Self::Maven)
    }
}

/// The normalized form of a package name.
///
/// The normalization is the one the ecosystem's own registry uses when
/// it compares names — it is not an invention of this module:
///
/// - PyPI: PEP 503, runs of `-`, `_` and `.` collapse to one `-`, all
///   lowercase. `Flask_Login`, `flask.login` and `FLASK--LOGIN` are one
///   name.
/// - npm: lowercase; a scoped name keeps its `@scope/` and normalizes
///   each part.
/// - Maven: the `groupId:artifactId` is kept as written; Maven names are
///   case-sensitive and do not normalize, which is also why the
///   subgroup rule lives in [`namespace_of`] instead.
pub fn normalize_name(ecosystem: Ecosystem, name: &str) -> String {
    match ecosystem {
        Ecosystem::Pypi => pypi_normalized(name),
        Ecosystem::Npm => {
            let lowered = name.to_ascii_lowercase();
            match lowered.split_once('/') {
                Some((scope, rest)) => format!("{}/{rest}", collapse_separators(scope)),
                None => collapse_separators(&lowered),
            }
        }
        Ecosystem::Maven => name.to_string(),
    }
}

fn pypi_normalized(name: &str) -> String {
    // PEP 503: replace runs of [-._]+ with a single '-', lowercase.
    collapse_separators(&name.to_ascii_lowercase())
}

fn collapse_separators(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_was_separator = false;
    for character in text.chars() {
        if matches!(character, '-' | '_' | '.') {
            if !previous_was_separator {
                out.push('-');
            }
            previous_was_separator = true;
        } else {
            out.push(character);
            previous_was_separator = false;
        }
    }
    out
}

/// The reserved namespace a bare publication lands in, if any.
///
/// A Maven package reserves at the groupId — and `com.acme` also covers
/// `com.acme.reporting`, because subgroups are part of the group. An
/// npm package reserves its `@scope` or its exact name. A PyPI package
/// reserves its normalized name. This is the judging rule of HSP-143
/// ported to the data this module reads; the authority that ENFORCES
/// the reservation at resolution time stays in the catalog crate.
pub fn namespace_of(ecosystem: Ecosystem, name: &str) -> String {
    match ecosystem {
        Ecosystem::Maven => name
            .split_once(':')
            .map(|(group, _)| group)
            .unwrap_or(name)
            .to_string(),
        Ecosystem::Npm => match name.split_once('/') {
            Some((scope, _)) => scope.to_ascii_lowercase(),
            None => normalize_name(ecosystem, name),
        },
        Ecosystem::Pypi => pypi_normalized(name),
    }
}

/// What the repository knows about the legitimate name.
///
/// This is the "known package" side of every comparison: the publisher
/// that released it before, the versions it had, and the metadata the
/// last release carried. It is read from the store's own publication
/// history — the catalog does not go and ask the upstream registry at
/// detection time, because an attacker who races the upstream is still
/// answering to what THIS repository saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownPackage {
    /// The name as the legitimate owner published it.
    pub name: String,
    /// The publisher identity of the legitimate owner.
    pub publisher: String,
    /// The versions already published, oldest first.
    pub versions: Vec<String>,
    /// The metadata of the last known release.
    pub last_metadata: KnownMetadata,
}

/// The metadata a publication declares, the fields the anomaly is read
/// from.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KnownMetadata {
    /// The declared install hooks (npm `preinstall`/`install`, PyPI
    /// setup entry points that execute at install, Maven lifecycle
    /// plugins bound to the install phase).
    pub install_hooks: Vec<String>,
    /// The declared runtime dependencies.
    pub dependencies: Vec<String>,
    /// Whether a source repository is declared.
    pub has_repository: bool,
    /// Whether the package declares provenance attestation.
    pub has_provenance: bool,
    /// The number of maintainers who can publish.
    pub maintainers: usize,
    /// How long the publisher account has existed, in days at the
    /// moment of evaluation.
    pub publisher_age_days: i64,
}

/// The catalog: what the platform knows per ecosystem.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EcosystemCatalog {
    /// The legitimate packages by normalized name.
    pub packages: Vec<KnownPackage>,
    /// The reserved namespaces, in the natural writing of the format,
    /// exactly like the `config_settings.namespaces.reserved` of HSP-143.
    pub reserved_namespaces: Vec<String>,
}

impl EcosystemCatalog {
    /// A new catalog.
    pub fn new(packages: Vec<KnownPackage>, reserved_namespaces: Vec<String>) -> Self {
        Self {
            packages,
            reserved_namespaces,
        }
    }

    /// The legitimate package with this exact normalized name, if the
    /// catalog knows one.
    pub fn known(&self, ecosystem: Ecosystem, name: &str) -> Option<&KnownPackage> {
        let needle = normalize_name(ecosystem, name);
        self.packages
            .iter()
            .find(|package| normalize_name(ecosystem, &package.name) == needle)
    }

    /// The normalized set of known names — the benign population a
    /// typosquat is close to.
    pub fn known_names(&self, ecosystem: Ecosystem) -> Vec<String> {
        self.packages
            .iter()
            .map(|package| normalize_name(ecosystem, &package.name))
            .collect()
    }

    /// Whether the publication lands in a reserved namespace.
    ///
    /// The rule is HSP-143's: Maven reserves the groupId and its
    /// subgroups, npm the `@scope` or the exact name, PyPI the
    /// normalized name.
    pub fn is_reserved(&self, ecosystem: Ecosystem, name: &str) -> bool {
        self.reserved_namespaces
            .iter()
            .any(|reserved| match ecosystem {
                Ecosystem::Maven => {
                    let group = namespace_of(ecosystem, name);
                    group == *reserved || group.starts_with(&format!("{reserved}."))
                }
                _ => {
                    normalize_name(ecosystem, reserved) == normalize_name(ecosystem, name)
                        || namespace_of(ecosystem, name) == normalize_name(ecosystem, reserved)
                }
            })
    }

    /// Whether a publisher has released before in this catalog.
    pub fn seen_publisher(&self, publisher: &str) -> bool {
        self.packages
            .iter()
            .any(|package| package.publisher == publisher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pypi_normalizes_the_way_pep_503_says() {
        assert_eq!(
            normalize_name(Ecosystem::Pypi, "Flask_Login"),
            "flask-login"
        );
        assert_eq!(
            normalize_name(Ecosystem::Pypi, "flask.login"),
            "flask-login"
        );
        assert_eq!(
            normalize_name(Ecosystem::Pypi, "FLASK--LOGIN"),
            "flask-login",
            "runs of separators collapse to one dash"
        );
    }

    #[test]
    fn maven_names_do_not_normalize_because_maven_is_case_sensitive() {
        assert_eq!(
            normalize_name(Ecosystem::Maven, "org.example:Lib-B"),
            "org.example:Lib-B"
        );
    }

    #[test]
    fn npm_scoped_names_keep_the_scope_and_lowercase() {
        assert_eq!(
            normalize_name(Ecosystem::Npm, "@ACME/WebServer"),
            "@acme/webserver"
        );
    }

    #[test]
    fn a_maven_reservation_covers_its_subgroups() {
        let catalog = EcosystemCatalog::new(Vec::new(), vec!["com.acme".to_string()]);
        assert!(catalog.is_reserved(Ecosystem::Maven, "com.acme:core"));
        assert!(
            catalog.is_reserved(Ecosystem::Maven, "com.acme.reporting:lib"),
            "the subgroup lands inside the reserved group"
        );
        assert!(!catalog.is_reserved(Ecosystem::Maven, "com.other:core"));
    }

    #[test]
    fn a_npm_reservation_covers_the_scope() {
        let catalog = EcosystemCatalog::new(Vec::new(), vec!["@acme".to_string()]);
        assert!(catalog.is_reserved(Ecosystem::Npm, "@acme/utils"));
        assert!(!catalog.is_reserved(Ecosystem::Npm, "acme-utils"));
    }

    #[test]
    fn a_pypi_reservation_compares_normalized_names() {
        let catalog = EcosystemCatalog::new(Vec::new(), vec!["acme-internal".to_string()]);
        assert!(catalog.is_reserved(Ecosystem::Pypi, "Acme_Internal"));
        assert!(!catalog.is_reserved(Ecosystem::Pypi, "requests"));
    }
}
