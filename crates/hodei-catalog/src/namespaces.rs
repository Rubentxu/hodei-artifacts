//! Namespace reservation: which names belong to the house and never come from a
//! public upstream.
//!
//! The *dependency confusion* attack is simple: someone publishes on the
//! public registry a name the organization uses privately, with a
//! higher version, and a resolver that prefers the newest pulls the
//! attacker's package. The defense is **reservation**: a namespace declared reserved
//! is served from the internal repositories or not served at all, and the public
//! upstream does not take part even if it has more versions.
//!
//! This module is pure: it reads the policy from the repository configuration
//! and judges a path. Whoever resolves (the proxy and the groups) asks here
//! **always**, before looking at cache, receipts or network: the query is
//! mandatory in resolution, and a reserved path cannot dodge it
//! with a positive TTL, nor a negative one, nor without network.
//!
//! # The policy
//!
//! It lives in `config_settings.namespaces`:
//!
//! ```json
//! { "namespaces": {
//!     "reserved": ["com.acme", "@acme", "acme-internal"],
//!     "on_conflict": "deny"
//! } }
//! ```
//!
//! - `reserved` are the reserved namespaces, in the natural writing of
//!   each format: a groupId with dots in Maven 2 (`com.acme`, which also reserves
//!   its subgroups), `@scope` or name in npm, project name
//!   in PyPI (compared normalized, as PEP 503 says).
//! - `on_conflict` says what to do when the sources disagree about a
//!   reserved name. The only value is `deny`: the refusal is **explained**
//!   (it names the reservation and the conflicting sources) and there is no fallback: a
//!   down upstream never turns the refusal into a public answer.
//! - An unknown key, an empty namespace or a policy that is not
//!   understood is an error: an unreadable security setting does not degrade
//!   to "no reservation".
//!
//! # How a path is judged
//!
//! [`NamespacePolicy::judge`] receives the asset path and the format, and
//! says whether it falls under a reservation. The judgement is deterministic: the same
//! inputs give the same verdict, and the verdict names the rule that
//! applied (which reserved namespace matched and, in PyPI, with which alias it was
//! written).

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

use crate::model::{RepositoryConfig, RepositoryFormat};
use crate::pypi;

/// The cap on the namespaces a policy reserves.
pub const MAX_RESERVED: usize = 256;

/// The cap on the length of a reserved namespace.
const MAX_NAME_LEN: usize = 214;

/// Why a namespace policy is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the namespace policy is not valid: {0}")]
pub struct NamespacePolicyError(pub String);

fn invalid(detail: impl Into<String>) -> NamespacePolicyError {
    NamespacePolicyError(detail.into())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    reserved: Vec<String>,
    #[serde(default)]
    on_conflict: Option<String>,
}

/// What to do when the sources disagree about a reserved name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnConflict {
    /// The refusal is explained and there is no fallback: the reservation always wins.
    #[default]
    Deny,
}

impl OnConflict {
    /// The stable name it is written and read with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "deny",
        }
    }
}

/// The verdict on a path.
///
/// It names the rule that applied: which reservation matched and, if the path arrived
/// written with an alias that normalizes the same (PyPI), how it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// The path falls under no reservation.
    Unreserved,
    /// The path falls under the reservation of namespace `matched`.
    Reserved {
        /// The reserved namespace that matched, as it was declared.
        matched: String,
        /// The writing of the path that normalizes the same, if it was not literal.
        ///
        /// In PyPI, `Flask_Login` and `flask.login` are the same project as
        /// `flask-login`: if the policy reserves `flask-login` and the request
        /// arrives as `Flask_Login`, the alias is what is named in the
        /// explanation.
        alias_of: Option<String>,
    },
}

impl Judgement {
    /// Whether the path falls under a reservation.
    pub fn is_reserved(&self) -> bool {
        matches!(self, Self::Reserved { .. })
    }
}

/// The namespace policy of a repository, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespacePolicy {
    reserved: Vec<String>,
    on_conflict: OnConflict,
}

impl NamespacePolicy {
    /// The policy that a repository's configuration declares.
    ///
    /// `Ok(None)` means "declares none": without the `namespaces` key there is no
    /// reservation and resolution is as usual. A declaration that exists
    /// and is not understood is an error, never an empty policy.
    pub fn of(config: &RepositoryConfig) -> Result<Option<Self>, NamespacePolicyError> {
        let Some(policy) = config.config_settings.get("namespaces") else {
            return Ok(None);
        };
        Self::parse(policy).map(Some)
    }

    /// Validates a policy written as JSON.
    pub fn parse(value: &Value) -> Result<Self, NamespacePolicyError> {
        let raw: RawPolicy = serde_json::from_value(value.clone())
            .map_err(|error| invalid(format!("it cannot be read: {error}")))?;
        if raw.reserved.is_empty() {
            return Err(invalid("a policy reserves at least one namespace"));
        }
        if raw.reserved.len() > MAX_RESERVED {
            return Err(invalid(format!(
                "a policy reserves at most {MAX_RESERVED} namespaces"
            )));
        }
        let mut seen = HashSet::new();
        for name in &raw.reserved {
            let well_formed = !name.is_empty()
                && name.len() <= MAX_NAME_LEN
                && !name.chars().any(|c| c.is_whitespace() || c.is_control())
                && !name.contains('/');
            if !well_formed {
                return Err(invalid(format!("{name:?} is not a namespace")));
            }
            if !seen.insert(name.as_str()) {
                return Err(invalid(format!("the namespace {name:?} is reserved twice")));
            }
        }
        let on_conflict = match raw.on_conflict.as_deref() {
            None => OnConflict::default(),
            Some(value) if value == OnConflict::Deny.as_str() => OnConflict::Deny,
            Some(other) => {
                return Err(invalid(format!(
                    "on_conflict is {:?}, and the only conflict the policy knows is {:?}",
                    other,
                    OnConflict::Deny.as_str()
                )));
            }
        };
        Ok(Self {
            reserved: raw.reserved,
            on_conflict,
        })
    }

    /// The reserved namespaces, as they were declared.
    pub fn reserved(&self) -> &[String] {
        &self.reserved
    }

    /// What to do on a source conflict.
    pub fn on_conflict(&self) -> OnConflict {
        self.on_conflict
    }

    /// The verdict on a repository path, according to its format.
    ///
    /// The path is the asset's (relative to the repository). The judgement does not look at
    /// cache or provenance: it is the question resolution always asks itself,
    /// before deciding where the content comes from.
    pub fn judge(&self, format: RepositoryFormat, path: &str) -> Judgement {
        match format {
            RepositoryFormat::Maven2 => self.judge_maven(path),
            RepositoryFormat::Npm => self.judge_npm(path),
            RepositoryFormat::PyPi => self.judge_pypi(path),
            // OCI has no notion of a namespace with aliases: the namespace is
            // the first segment of the **repository name**, not the path of
            // an asset within it. A repository is called "team/demo" and its
            // namespace is "team"; the judgement is made on the name, and that is why
            // the same name "manifests/sha256:abc" --which is an asset path
            // and not a name-- matches no reservation.
            //
            // And the reservation cannot carry slashes: a namespace is the first
            // segment, and "team/demo" is not a declarable namespace. That is why the
            // rule is the flat one, which already compares the first segment by
            // segment prefix: "team" covers "team/demo" and "team/demo/x",
            // and does not cover "teamx" nor "teammate".
            RepositoryFormat::Oci => self.judge_flat(path),
            RepositoryFormat::NuGet => self.judge_nuget(path),
            // The other formats have no notion of a namespace with
            // aliases: the first segment of the path is the name, and it is
            // compared as is.
            _ => self.judge_flat(path),
        }
    }

    /// Maven 2: a groupId with dots covers its directory prefix and its
    /// subgroups.
    ///
    /// The Maven path is the groupId with `/` instead of `.`; `com.acme` reserves
    /// `com/acme/...`, and also `com/acme/plugin/...` (the subgroup
    /// `com.acme.plugin`). The comparison is by segments: `com/acme-x/...`
    /// does not fall under the reservation of `com.acme`.
    fn judge_maven(&self, path: &str) -> Judgement {
        for reserved in &self.reserved {
            let prefix = reserved.replace('.', "/");
            if path
                .strip_prefix(prefix.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
            {
                return Judgement::Reserved {
                    matched: reserved.clone(),
                    alias_of: None,
                };
            }
        }
        Judgement::Unreserved
    }

    /// npm: a `@scope` reserves its whole scope; a name reserves the
    /// package.
    ///
    /// The path of a scoped package starts with two segments
    /// (`@scope/pkg/...`); that of an unscoped one, with one. The name is
    /// compared in lowercase, as npm writes its own.
    fn judge_npm(&self, path: &str) -> Judgement {
        let name = package_of_npm_path(path);
        if name.is_empty() {
            return Judgement::Unreserved;
        }
        let name = name.to_ascii_lowercase();
        for reserved in &self.reserved {
            let candidate = reserved.to_ascii_lowercase();
            if name == candidate
                || (candidate.starts_with('@')
                    && name
                        .strip_prefix(candidate.as_str())
                        .is_some_and(|rest| rest.starts_with('/')))
            {
                return Judgement::Reserved {
                    matched: reserved.clone(),
                    alias_of: None,
                };
            }
        }
        Judgement::Unreserved
    }

    /// PyPI: the project name is the first segment, and it is compared
    /// normalized as PEP 503 says.
    ///
    /// `Flask_Login`, `flask.login` and `flask-login` are the same project:
    /// a reservation of `flask-login` governs all three, and the alias the
    /// request arrived with is named in the verdict.
    fn judge_pypi(&self, path: &str) -> Judgement {
        let project = path.split('/').next().unwrap_or_default();
        if project.is_empty() {
            return Judgement::Unreserved;
        }
        let normalized = pypi::normalize(project);
        for reserved in &self.reserved {
            if normalized == pypi::normalize(reserved) {
                let alias_of = (project != *reserved).then(|| project.to_string());
                return Judgement::Reserved {
                    matched: reserved.clone(),
                    alias_of,
                };
            }
        }
        Judgement::Unreserved
    }

    /// NuGet: package ids are case-insensitive, and proxy/catalog paths
    /// carry the id under `flatcontainer/{id}/...` or `registrations/{id}/...`.
    fn judge_nuget(&self, path: &str) -> Judgement {
        let id = if let Some(rest) = path.strip_prefix("flatcontainer/") {
            rest.split('/').next().unwrap_or_default()
        } else if let Some(rest) = path.strip_prefix("registrations/") {
            rest.split('/').next().unwrap_or_default()
        } else if !path.contains('/') {
            path
        } else {
            ""
        };
        if id.is_empty() {
            return Judgement::Unreserved;
        }
        let normalized = id.to_ascii_lowercase();
        for reserved in &self.reserved {
            if normalized == reserved.to_ascii_lowercase() {
                let alias_of = (id != *reserved).then(|| id.to_string());
                return Judgement::Reserved {
                    matched: reserved.clone(),
                    alias_of,
                };
            }
        }
        Judgement::Unreserved
    }

    /// The formats without a namespace of their own: the first segment of the
    /// path is the name, and it is compared literally.
    fn judge_flat(&self, path: &str) -> Judgement {
        let name = path.split('/').next().unwrap_or_default();
        if name.is_empty() {
            return Judgement::Unreserved;
        }
        for reserved in &self.reserved {
            if name == reserved {
                return Judgement::Reserved {
                    matched: reserved.clone(),
                    alias_of: None,
                };
            }
        }
        Judgement::Unreserved
    }
}

/// The npm package name that heads a path.
///
/// `@scope/pkg/-/pkg-1.0.0.tgz` is `@scope/pkg`; `pkg/...` is `pkg`; what
/// does not reach a name (`@solo`, the empty path) belongs to no package.
fn package_of_npm_path(path: &str) -> &str {
    if let Some(rest) = path.strip_prefix('@') {
        let (scope, after) = match rest.split_once('/') {
            Some(parts) => parts,
            None => return "",
        };
        let (base, _) = after.split_once('/').unwrap_or((after, ""));
        if base.is_empty() {
            return "";
        }
        // The name measures what there is from the start to the end of the
        // second segment.
        let length = 1 + scope.len() + 1 + base.len();
        return &path[..length];
    }
    path.split('/').next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy(settings: Value) -> Result<Option<NamespacePolicy>, NamespacePolicyError> {
        let mut config = RepositoryConfig::new(crate::DigestScope::Global);
        config.config_settings = settings;
        NamespacePolicy::of(&config)
    }

    fn reserved(names: &[&str]) -> NamespacePolicy {
        NamespacePolicy::parse(&json!({ "reserved": names })).expect("a valid policy")
    }

    #[test]
    fn a_repository_without_namespace_settings_has_no_policy() {
        assert_eq!(policy(json!({})).unwrap(), None);
        let mut config = RepositoryConfig::new(crate::DigestScope::Global);
        config.config_settings = json!({ "proxy": { "upstream": { "id": "u" } } });
        assert_eq!(NamespacePolicy::of(&config).unwrap(), None);
    }

    #[test]
    fn a_policy_declares_its_namespaces_and_the_conflict_it_knows() {
        let policy = reserved(&["com.acme", "@acme", "acme-internal"]);
        assert_eq!(policy.reserved(), ["com.acme", "@acme", "acme-internal"]);
        assert_eq!(policy.on_conflict(), OnConflict::Deny);

        let explicit = NamespacePolicy::parse(&json!({
            "reserved": ["a"],
            "on_conflict": "deny",
        }))
        .expect("the declared conflict is the one it knows");
        assert_eq!(explicit.on_conflict(), OnConflict::Deny);
    }

    #[test]
    fn what_cannot_be_read_is_refused_instead_of_defaulted() {
        for bad in [
            json!({}),
            json!({ "reserved": [] }),
            json!({ "reserved": ["a"], "on_conflict": "prefer_internal" }),
            json!({ "reserved": ["a"], "unknown": 1 }),
            json!({ "reserved": [""] }),
            json!({ "reserved": ["a", "a"] }),
            json!({ "reserved": ["with space"] }),
            json!({ "reserved": ["with/slash"] }),
            json!(["a"]),
            json!("reserved"),
        ] {
            assert!(NamespacePolicy::parse(&bad).is_err(), "{bad}");
        }
        let too_many: Vec<String> = (0..=MAX_RESERVED).map(|n| format!("n{n}")).collect();
        assert!(NamespacePolicy::parse(&json!({ "reserved": too_many })).is_err());
    }

    #[test]
    fn a_settings_key_nobody_can_read_breaks_the_policy() {
        assert!(policy(json!({ "namespaces": "strict" })).is_err());
        assert!(policy(json!({ "namespaces": { "reserve": ["a"] } })).is_err());
    }

    #[test]
    fn a_maven_group_id_covers_its_directories_and_its_subgroups() {
        let policy = reserved(&["com.acme"]);
        for path in [
            "com/acme/lib/1.0/lib-1.0.pom",
            "com/acme/lib/1.0/lib-1.0.pom.sha1",
            "com/acme/lib/maven-metadata.xml",
            "com/acme/plugin/2.0/plugin-2.0.jar",
        ] {
            assert_eq!(
                policy.judge(RepositoryFormat::Maven2, path),
                Judgement::Reserved {
                    matched: "com.acme".into(),
                    alias_of: None,
                },
                "{path}"
            );
        }
        for free in [
            "org/other/lib/1.0/lib-1.0.pom",
            "com/acmevil/lib/1.0/lib-1.0.pom",
            "com/acme-x/lib/1.0/lib-1.0.pom",
            "lib.bin",
        ] {
            assert_eq!(
                policy.judge(RepositoryFormat::Maven2, free),
                Judgement::Unreserved,
                "{free}"
            );
        }
        // The root of the reserved group is also reserved: everything that falls under
        // `acme/` belongs to the `acme` namespace.
        assert_eq!(
            reserved(&["acme"]).judge(RepositoryFormat::Maven2, "acme/"),
            Judgement::Reserved {
                matched: "acme".into(),
                alias_of: None,
            }
        );
    }

    #[test]
    fn an_npm_scope_governs_everything_under_it_and_a_name_governs_itself() {
        let policy = reserved(&["@acme", "left-pad"]);
        for path in [
            "@acme/pkg",
            "@acme/pkg/-/pkg-1.0.0.tgz",
            "@ACME/other/-/other-1.0.0.tgz",
            "left-pad",
            "left-pad/-/left-pad-1.0.0.tgz",
        ] {
            assert!(
                policy.judge(RepositoryFormat::Npm, path).is_reserved(),
                "{path}"
            );
        }
        for free in ["@acmevillain/pkg", "left-padx", "other-pkg"] {
            assert_eq!(
                policy.judge(RepositoryFormat::Npm, free),
                Judgement::Unreserved,
                "{free}"
            );
        }
        // An incomplete scope prefix is not a package name.
        assert_eq!(
            policy.judge(RepositoryFormat::Npm, "@acme"),
            Judgement::Unreserved
        );
    }

    #[test]
    fn a_pypi_reservation_is_matched_the_way_pep_503_normalizes() {
        let policy = reserved(&["acme-utils"]);
        for path in [
            "acme-utils",
            "acme-utils/acme_utils-1.0-py3-none-any.whl",
            "Acme_Utils",
            "acme.utils/acme.utils-2.0.tar.gz",
            "ACME-UTILS",
        ] {
            let judgement = policy.judge(RepositoryFormat::PyPi, path);
            assert!(judgement.is_reserved(), "{path}");
            if let Judgement::Reserved { alias_of, .. } = judgement {
                let alias = path.split('/').next().unwrap_or_default();
                assert_eq!(alias_of, (alias != "acme-utils").then(|| alias.to_string()));
            }
        }
        assert_eq!(
            policy.judge(RepositoryFormat::PyPi, "acme-utils-extras"),
            Judgement::Unreserved
        );
        assert_eq!(
            policy.judge(RepositoryFormat::PyPi, "other/acme-utils-1.0.whl"),
            Judgement::Unreserved
        );
    }

    #[test]
    fn a_format_without_namespaces_judges_the_first_segment_literally() {
        let policy = reserved(&["lab"]);
        assert!(
            policy
                .judge(RepositoryFormat::Raw, "lab/file.bin")
                .is_reserved()
        );
        assert_eq!(
            policy.judge(RepositoryFormat::Raw, "laboratory/file.bin"),
            Judgement::Unreserved
        );
    }

    #[test]
    fn an_oci_reservation_is_matched_over_the_name_of_the_repository() {
        // A reserved namespace cannot carry slashes: it is the first
        // segment, and "team/demo" is not a declarable namespace. The reservation
        // that protects the group is written "team", and it covers "team/demo".
        let policy = reserved(&["team"]);
        for name in ["team", "team/demo", "team/demo/extra"] {
            assert!(
                policy.judge(RepositoryFormat::Oci, name).is_reserved(),
                "{name}"
            );
        }
        for free in ["teamx", "teammate", "other/demo"] {
            assert_eq!(
                policy.judge(RepositoryFormat::Oci, free),
                Judgement::Unreserved,
                "{free}"
            );
        }
        // The judgement is made over the name of the repository, and not over
        // the path of an asset inside it.
        assert_eq!(
            policy.judge(RepositoryFormat::Oci, "manifests/sha256:abc"),
            Judgement::Unreserved
        );
    }

    #[test]
    fn the_verdict_names_its_rule_and_the_alias_it_saw() {
        let policy = reserved(&["com.acme"]);
        let judgement = policy.judge(RepositoryFormat::Maven2, "com/acme/lib/1.0/lib-1.0.pom");
        assert_eq!(
            judgement,
            Judgement::Reserved {
                matched: "com.acme".into(),
                alias_of: None,
            }
        );
    }

    #[test]
    fn nuget_judges_package_ids_case_insensitively_across_routes() {
        let policy = reserved(&["Acme.Internal"]);
        assert_eq!(
            policy.judge(RepositoryFormat::NuGet, "acme.internal"),
            Judgement::Reserved {
                matched: "Acme.Internal".into(),
                alias_of: Some("acme.internal".into()),
            }
        );
        assert_eq!(
            policy.judge(
                RepositoryFormat::NuGet,
                "flatcontainer/acme.internal/1.0.0/acme.internal.nupkg"
            ),
            Judgement::Reserved {
                matched: "Acme.Internal".into(),
                alias_of: Some("acme.internal".into()),
            }
        );
        assert_eq!(
            policy.judge(
                RepositoryFormat::NuGet,
                "registrations/acme.internal/index.json"
            ),
            Judgement::Reserved {
                matched: "Acme.Internal".into(),
                alias_of: Some("acme.internal".into()),
            }
        );
        assert_eq!(
            policy.judge(RepositoryFormat::NuGet, "v3/index.json"),
            Judgement::Unreserved
        );
        assert_eq!(
            policy.judge(RepositoryFormat::NuGet, "acme.other"),
            Judgement::Unreserved
        );
    }
}
