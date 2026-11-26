//! The grammar of the Terraform/OpenTofu **module registry protocol v1**.
//!
//! A private module registry speaks three reads and one write
//! specific to the hub, all hanging off the surface of a **hosted**
//! repository of the Terraform format:
//!
//! 1. Service discovery: the hub answers at `/.well-known/terraform.json`
//!    `{"modules.v1": "<protocol base>"}`. The client always requests
//!    `https://{host}/.well-known/terraform.json` (the scheme is fixed by
//!    `terraform-svchost/disco`), and a host without TLS declares the service
//!    directly in its CLI config (`host` + `services`), which is the
//!    disco's `ForceHostServices`.
//! 2. `GET {base}/{namespace}/{name}/{system}/versions`: the version
//!    index, paginable; the client only reads `modules[0].versions[]`.
//! 3. `GET {base}/{namespace}/{name}/{system}/{version}/download`: a
//!    response **without a body** whose `X-Terraform-Get` is a go-getter
//!    source. If the value starts with `/`, `./` or `../` the client
//!    resolves it against the URL it requested, and the artifact download is
//!    done by go-getter, which does **not** carry the registry credentials: the
//!    artifact authenticates with netrc (Basic) or with the same Bearer.
//! 4. `POST publish/{...}/{version}`: publishing is defined by the hub
//!    (the CLI does not know how to publish modules): one complete tarball per version,
//!    immutable, with the sha256 computed at commit in the receipt.
//!
//! Only the grammar lives here: which paths exist and which segments are
//! legitimately a namespace, a name, a system, a version or a
//! file name. As in the mirror, a path arrives **undecoded**
//! and a segment with percent-escape, Windows separator, empty or
//! `..` is a traversal attempt, and is rejected before touching the
//! catalog.

use serde_json::{Value, json};

/// What can go wrong when reading a module registry path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleRegistryError {
    /// The path does not belong to the registry protocol.
    InvalidPath(String),
    /// The namespace, the name or the system are not valid.
    InvalidCoordinates(String),
    /// The version is not strict semver.
    InvalidVersion(String),
}

impl ModuleRegistryError {
    /// The message that explains the rejection.
    pub fn message(&self) -> String {
        match self {
            Self::InvalidPath(path) => {
                format!("the path `{path}` is not a module registry protocol path")
            }
            Self::InvalidCoordinates(name) => {
                format!("the module coordinates `{name}` are not valid")
            }
            Self::InvalidVersion(version) => {
                format!("the version `{version}` is not strict semver")
            }
        }
    }

    /// Whether the rejection is about the path and not about who asks for it: a path that the
    /// protocol does not know is a resource that does not exist.
    pub fn is_path_error(&self) -> bool {
        matches!(self, Self::InvalidPath(_))
    }
}

impl std::fmt::Display for ModuleRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ModuleRegistryError {}

/// The prefixes this protocol claims over the surface of a
/// Terraform repository. A **hosted** repository speaks registry and a
/// **proxy** speaks mirror: the modality decides the dialect, and the prefix
/// only separates the registry paths from the mirror ones.
pub const REGISTRY_PREFIXES: [&str; 3] = ["v1/modules/", "publish/", "artifacts/"];

/// Whether the rest of the path of a repository is a module registry
/// path and not a provider mirror one.
pub fn is_registry_path(rest: &str) -> bool {
    REGISTRY_PREFIXES
        .iter()
        .any(|prefix| rest.starts_with(prefix))
}

/// The address of a module within the registry: `namespace/name/system`.
///
/// The client builds it from the source `host/namespace/name/
/// system` and asks by the last three segments; the hub stores them as
/// published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRef {
    /// The namespace of the module.
    pub namespace: String,
    /// The name of the module.
    pub name: String,
    /// The target system (`local`, `aws`, `kubernetes`, ...).
    pub system: String,
}

impl ModuleRef {
    /// The source the client sees, without the host: the one the versions
    /// document declares as `source`.
    pub fn source(&self) -> String {
        format!("{}/{}/{}", self.namespace, self.name, self.system)
    }

    /// The name of the package in the catalog.
    pub fn package_name(&self) -> String {
        self.source()
    }

    /// The path of the artifact of a version in the CAS: a versioned
    /// and immutable tarball.
    pub fn artifact_path(&self, version: &str) -> String {
        format!(
            "artifacts/{}/{}/{}/{}.tar.gz",
            self.namespace, self.name, self.system, version
        )
    }
}

/// A module registry path, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The version index of a module.
    Versions {
        /// The address of the module.
        reference: ModuleRef,
    },
    /// The download of a version: the response carries the `X-Terraform-Get`.
    Download {
        /// The address of the module.
        reference: ModuleRef,
        /// The requested version.
        version: String,
    },
    /// The tarball of a version.
    Artifact {
        /// The address of the module.
        reference: ModuleRef,
        /// The requested version.
        version: String,
    },
    /// The hub publication: a complete tarball for a new version.
    Publish {
        /// The address of the module.
        reference: ModuleRef,
        /// The version being published.
        version: String,
    },
}

impl Route {
    /// The path that states its own name.
    ///
    /// The path arrives **undecoded**: the protocol rules are the
    /// only ones that interpret a `%`, and there is nothing to interpret there.
    /// An empty segment, a `..`, a backslash or a percent
    /// escape is a traversal, not a path.
    pub fn parse(rest: &str) -> Result<Self, ModuleRegistryError> {
        if rest.contains('%') || rest.contains('\\') {
            return Err(ModuleRegistryError::InvalidPath(rest.to_string()));
        }
        let segments: Vec<&str> = rest.split('/').collect();
        if segments.iter().any(|segment| segment.is_empty()) {
            return Err(ModuleRegistryError::InvalidPath(rest.to_string()));
        }
        match segments.as_slice() {
            ["v1", "modules", namespace, name, system, "versions"] => {
                let reference = the_coordinates(namespace, name, system)?;
                Ok(Self::Versions { reference })
            }
            [
                "v1",
                "modules",
                namespace,
                name,
                system,
                version,
                "download",
            ] => {
                let reference = the_coordinates(namespace, name, system)?;
                valid_version(version)?;
                Ok(Self::Download {
                    reference,
                    version: version.to_string(),
                })
            }
            ["publish", namespace, name, system, version] => {
                let reference = the_coordinates(namespace, name, system)?;
                valid_version(version)?;
                Ok(Self::Publish {
                    reference,
                    version: version.to_string(),
                })
            }
            ["artifacts", namespace, name, system, file] => {
                let reference = the_coordinates(namespace, name, system)?;
                let Some(version) = file.strip_suffix(".tar.gz") else {
                    return Err(ModuleRegistryError::InvalidPath(rest.to_string()));
                };
                valid_version(version)?;
                Ok(Self::Artifact {
                    reference,
                    version: version.to_string(),
                })
            }
            _ => Err(ModuleRegistryError::InvalidPath(rest.to_string())),
        }
    }

    /// The version that the path names, if it names one.
    pub fn version(&self) -> Option<&str> {
        match self {
            Self::Versions { .. } => None,
            Self::Download { version, .. }
            | Self::Artifact { version, .. }
            | Self::Publish { version, .. } => Some(version),
        }
    }
}

/// A namespace, a name or a system: lowercase, digits, hyphens and
/// underscores, without a hyphen at the edge. The same rules as the mirror, which
/// are the ones the client uses for its sources.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.ends_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn the_coordinates(
    namespace: &str,
    name: &str,
    system: &str,
) -> Result<ModuleRef, ModuleRegistryError> {
    if !valid_name(namespace) || !valid_name(name) || !valid_name(system) {
        return Err(ModuleRegistryError::InvalidCoordinates(format!(
            "{namespace}/{name}/{system}"
        )));
    }
    Ok(ModuleRef {
        namespace: namespace.to_string(),
        name: name.to_string(),
        system: system.to_string(),
    })
}

/// The version: strict semver, without a leading `v` and without spaces. The
/// same rules as the provider mirror.
fn valid_version(version: &str) -> Result<(), ModuleRegistryError> {
    let core = |text: &str| {
        let numbers: Vec<&str> = text.split('.').collect();
        numbers.len() == 3
            && numbers
                .iter()
                .all(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
    };
    let identifiers = |text: &str| {
        text.split('.')
            .all(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    };
    let (release, _) = match version.split_once(['-', '+']) {
        Some((release, rest)) => (release, rest),
        None => (version, ""),
    };
    let ok = core(release)
        && match version.split_once('+') {
            Some((_, build)) => identifiers(build),
            None => true,
        }
        && match version.split_once('-') {
            Some((_, rest)) => {
                let pre = rest.split('+').next().unwrap_or_default();
                identifiers(pre)
            }
            None => true,
        };
    if ok {
        Ok(())
    } else {
        Err(ModuleRegistryError::InvalidVersion(version.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

/// The service discovery document. This release **does not declare
/// `providers.v1`**: the origin provider registry is not implemented
/// (AC3 not implemented), and a service that cannot be served is not
/// announced.
pub fn discovery_document(module_base: &str) -> Value {
    json!({ "modules.v1": module_base })
}

/// The versions document of a module. The client reads
/// `modules[0].versions[].version` and sorts by semver; `root` is empty
/// because the dependencies of a registry module are declared by its
/// configuration, not the index.
pub fn versions_document(reference: &ModuleRef, versions: &[&str]) -> Value {
    let versions: Vec<Value> = versions
        .iter()
        .map(|version| {
            json!({
                "version": version,
                "root": { "providers": [], "dependencies": [] },
                "submodules": [],
            })
        })
        .collect();
    json!({
        "meta": {
            "limit": 100,
            "current_offset": 0,
            "next_offset": null,
            "prev_offset": null,
        },
        "modules": [
            {
                "source": reference.source(),
                "versions": versions,
            }
        ],
    })
}

/// The go-getter source of a version download: the path of the
/// artifact on this surface, **relative** (the client resolves it
/// against the URL it requested), with the checksum that go-getter verifies when
/// downloading. The trailing `.tar.gz` is what makes go-getter treat the
/// download as file + decompression, which is the only way it
/// verifies checksums (`checksum cannot be specified for directory
/// download` if the path does not end in a known archive).
pub fn download_location(
    repository_key: &str,
    reference: &ModuleRef,
    version: &str,
    sha256: &str,
) -> String {
    format!(
        "/terraform/{repository_key}/{}?checksum=sha256:{sha256}",
        reference.artifact_path(version)
    )
}

/// The receipt of a publication: the identity of the version and the sha256
/// that the hub computed at commit, the same one the download later declares.
pub fn publish_receipt(
    reference: &ModuleRef,
    version: &str,
    sha256: &str,
    size_bytes: u64,
) -> Value {
    json!({
        "namespace": reference.namespace,
        "name": reference.name,
        "system": reference.system,
        "version": version,
        "sha256": sha256,
        "size_bytes": size_bytes,
        "path": reference.artifact_path(version),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> ModuleRef {
        ModuleRef {
            namespace: "hodei".to_string(),
            name: "demo".to_string(),
            system: "local".to_string(),
        }
    }

    #[test]
    fn the_protocol_paths_parse() {
        assert_eq!(
            Route::parse("v1/modules/hodei/demo/local/versions"),
            Ok(Route::Versions {
                reference: reference()
            })
        );
        assert_eq!(
            Route::parse("v1/modules/hodei/demo/local/1.0.0/download"),
            Ok(Route::Download {
                reference: reference(),
                version: "1.0.0".to_string(),
            })
        );
        assert_eq!(
            Route::parse("artifacts/hodei/demo/local/1.0.0.tar.gz"),
            Ok(Route::Artifact {
                reference: reference(),
                version: "1.0.0".to_string(),
            })
        );
        assert_eq!(
            Route::parse("publish/hodei/demo/local/1.0.0"),
            Ok(Route::Publish {
                reference: reference(),
                version: "1.0.0".to_string(),
            })
        );
    }

    #[test]
    fn the_registry_prefixes_are_ours() {
        assert!(is_registry_path("v1/modules/hodei/demo/local/versions"));
        assert!(is_registry_path("publish/hodei/demo/local/1.0.0"));
        assert!(is_registry_path("artifacts/hodei/demo/local/1.0.0.tar.gz"));
        assert!(!is_registry_path("index.json"));
        assert!(!is_registry_path(
            "registry.terraform.io/hashicorp/random/index.json"
        ));
    }

    #[test]
    fn a_traversal_is_not_a_path() {
        for hostile in [
            "v1/modules/hodei/../demo/local/versions",
            "v1/modules//demo/local/versions",
            "v1/modules/hodei/demo/local/1.0.0/../download",
            "artifacts/hodei/demo/local/..%2f1.0.0.tar.gz",
            "publish/hodei\\demo/local/1.0.0",
            "v1/modules/hodei/demo/local/1.0.0/download/",
            "artifacts/hodei/demo/local/1.0.0.zip",
            "",
            "v1/modules/hodei/demo/local",
        ] {
            let _ = Route::parse(hostile).expect_err("a hostile path is refused");
        }
    }

    #[test]
    fn a_bad_coordinate_or_version_is_refused() {
        assert!(Route::parse("v1/modules/Hodei/demo/local/versions").is_err());
        assert!(Route::parse("v1/modules/-hodei/demo/local/versions").is_err());
        assert!(Route::parse("v1/modules/hodei/demo/local/v1/download").is_err());
        assert!(Route::parse("v1/modules/hodei/demo/local/1.0/download").is_err());
        assert!(Route::parse("publish/hodei/demo/local/1.0.0-.0").is_err());
    }

    #[test]
    fn a_prerelease_version_is_a_version() {
        assert!(Route::parse("publish/hodei/demo/local/1.0.0-rc.1").is_ok());
    }

    #[test]
    fn the_documents_say_what_the_client_reads() {
        let discovery = discovery_document("/terraform/modules-hosted/v1/modules/");
        assert_eq!(
            discovery["modules.v1"],
            "/terraform/modules-hosted/v1/modules/"
        );
        assert!(
            discovery.get("providers.v1").is_none(),
            "AC3: no providers declared"
        );

        let versions = versions_document(&reference(), &["1.0.0", "2.0.0"]);
        assert_eq!(versions["modules"][0]["source"], "hodei/demo/local");
        assert_eq!(versions["modules"][0]["versions"][0]["version"], "1.0.0");

        let location = download_location("modules-hosted", &reference(), "1.0.0", "abc");
        assert_eq!(
            location,
            "/terraform/modules-hosted/artifacts/hodei/demo/local/1.0.0.tar.gz?checksum=sha256:abc"
        );

        let receipt = publish_receipt(&reference(), "1.0.0", "abc", 10);
        assert_eq!(receipt["sha256"], "abc");
        assert_eq!(receipt["path"], "artifacts/hodei/demo/local/1.0.0.tar.gz");
    }

    #[test]
    fn the_artifact_path_is_versioned_and_flat() {
        let reference = reference();
        assert_eq!(
            reference.artifact_path("1.0.0"),
            "artifacts/hodei/demo/local/1.0.0.tar.gz"
        );
        assert_eq!(reference.package_name(), "hodei/demo/local");
    }
}
