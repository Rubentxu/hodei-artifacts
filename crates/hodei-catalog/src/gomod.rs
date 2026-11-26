//! The pure grammar of the Go Modules **GOPROXY** protocol.
//!
//! A `go` repository serves four things per module, all under the path
//! `{module}/@v/`: the version list (`list`), the version document
//! (`.info`), its `go.mod` (`.mod`) and the canonical zip (`.zip`). In addition
//! there is `{module}/@latest`, which points to the latest published version.
//!
//! The protocol does not know writing: `go` only reads. Publishing to a
//! hosted repository is leaving the three artifacts at the same path with `PUT`, and the
//! list is assembled from what is in the catalog, as in cargo's sparse
//! index: nothing is rewritten, every version is forever.
//!
//! Module paths are case-sensitive and the protocol
//! escapes them: `H` is written `!h`. This module validates the name **already
//! escaped**, which is the canonical form it is stored in.

use serde_json::{Value, json};

/// What the protocol cannot accept.
#[derive(Debug, PartialEq, Eq)]
pub enum GoError {
    /// The path does not belong to GOPROXY.
    InvalidPath(String),
    /// The module name is not a valid escaped module path.
    InvalidModule(String),
    /// The version is not a semver with `v`.
    InvalidVersion(String),
    /// The published document does not match what its path announces.
    InvalidDocument(String),
}

impl GoError {
    /// What the client is told.
    pub fn message(&self) -> String {
        match self {
            Self::InvalidPath(what) => {
                format!("the path is not part of the GOPROXY protocol: {what}")
            }
            Self::InvalidModule(what) => format!("not a valid module path: {what}"),
            Self::InvalidVersion(what) => format!("not a valid module version: {what}"),
            Self::InvalidDocument(what) => format!("the published document is not usable: {what}"),
        }
    }

    /// A bad path is a 404: the client cannot distinguish "there is
    /// nothing here" from "GOPROXY is not spoken here", and no route map
    /// publishes the structure of the service.
    pub fn is_path_error(&self) -> bool {
        matches!(self, Self::InvalidPath(_))
    }
}

/// What the client can ask for at `/go/{repository}/...`.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// The versions of the module: `GET {module}/@v/list`.
    List {
        /// The module path, in its escaped form.
        module: String,
    },
    /// The document of a version: `GET {module}/@v/{version}.info`.
    Info {
        /// The module path, in its escaped form.
        module: String,
        /// The requested version.
        version: String,
    },
    /// The `go.mod` of a version: `GET {module}/@v/{version}.mod`.
    Mod {
        /// The module path, in its escaped form.
        module: String,
        /// The requested version.
        version: String,
    },
    /// The module zip: `GET {module}/@v/{version}.zip`.
    Zip {
        /// The module path, in its escaped form.
        module: String,
        /// The requested version.
        version: String,
    },
    /// The latest version: `GET {module}/@latest`.
    Latest {
        /// The module path, in its escaped form.
        module: String,
    },
}

/// The content type each artifact is served with.
pub fn content_type(extension: &str) -> &'static str {
    match extension {
        "info" => "application/json",
        "mod" => "text/plain; charset=utf-8",
        "zip" => "application/zip",
        _ => "text/plain; charset=utf-8",
    }
}

impl Route {
    /// The protocol path after `{repository}/`, undecoded.
    pub fn parse(rest: &str) -> Result<Route, GoError> {
        let segments = split(rest)?;
        if segments.is_empty() {
            return Err(GoError::InvalidPath("an empty path".into()));
        }
        if segments.last().is_some_and(|last| *last == "@latest") {
            let module = join_module(&segments[..segments.len() - 1])?;
            return Ok(Route::Latest { module });
        }
        let at = segments
            .iter()
            .rposition(|segment| *segment == "@v")
            .ok_or_else(|| GoError::InvalidPath("the path has no @v segment".into()))?;
        if at + 1 != segments.len() - 1 {
            return Err(GoError::InvalidPath(
                "there is exactly one file after @v".into(),
            ));
        }
        let module = join_module(&segments[..at])?;
        let file = segments[at + 1];
        if file == "list" {
            return Ok(Route::List { module });
        }
        for extension in ["info", "mod", "zip"] {
            if let Some(version) = file.strip_suffix(&format!(".{extension}")) {
                let version = valid_version(version)?;
                return Ok(match extension {
                    "info" => Self::Info { module, version },
                    "mod" => Self::Mod { module, version },
                    _ => Self::Zip { module, version },
                });
            }
        }
        Err(GoError::InvalidPath(format!(
            "{file:?} is neither list nor a .info, .mod or .zip version file"
        )))
    }

    /// The path where this version's published artifact lives, or `None`
    /// if the path does not speak of a specific version.
    pub fn asset_path(&self) -> Option<String> {
        match self {
            Self::Info { module, version }
            | Self::Mod { module, version }
            | Self::Zip { module, version } => {
                let extension = match self {
                    Self::Info { .. } => "info",
                    Self::Mod { .. } => "mod",
                    _ => "zip",
                };
                Some(format!("{module}/@v/{version}.{extension}"))
            }
            Self::List { module } | Self::Latest { module } => Some(format!("{module}/@v/")),
        }
    }

    /// The module the path is looking for, if it is looking for one.
    pub fn module(&self) -> &str {
        match self {
            Self::List { module }
            | Self::Info { module, .. }
            | Self::Mod { module, .. }
            | Self::Zip { module, .. }
            | Self::Latest { module } => module,
        }
    }
}

/// An escaped module path: segments of lowercase ASCII with their
/// dots, hyphens and underscores, and the uppercase letters in the form `!x`.
///
/// A colon is allowed in the first segment because hosts with a port
/// (`localhost:8080/folder`) are valid module paths.
pub fn valid_module(module: &str) -> Result<String, GoError> {
    if module.is_empty() || module.len() > 512 {
        return Err(GoError::InvalidModule(module.to_string()));
    }
    for segment in module.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment == "@v" {
            return Err(GoError::InvalidModule(module.to_string()));
        }
        let mut escaped = false;
        for byte in segment.bytes() {
            if escaped {
                if !byte.is_ascii_lowercase() {
                    return Err(GoError::InvalidModule(module.to_string()));
                }
                escaped = false;
            } else if byte == b'!' {
                escaped = true;
            } else if byte.is_ascii_uppercase() {
                // `go` never asks for an unescaped capital: it escapes it
                // as `!x`. A path with the raw uppercase letter is not a
                // syntax error the client can fix by
                // retrying: it is a path that names nothing, and it is
                // answered as what it is for the protocol: a 404.
                // Accepting it as a name would also create an alias of the
                // escaped module, and an alias is another way for the
                // same version to exist in two different places.
                return Err(GoError::InvalidPath(format!(
                    "the module path {module:?} carries a capital that is not escaped"
                )));
            } else if !(byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'-' | b'_' | b'~' | b':'))
            {
                return Err(GoError::InvalidModule(module.to_string()));
            }
        }
        if escaped || segment.starts_with('.') || segment.ends_with('.') {
            return Err(GoError::InvalidModule(module.to_string()));
        }
    }
    Ok(module.to_string())
}

/// A module version: the `v` and a strict semver.
pub fn valid_version(version: &str) -> Result<String, GoError> {
    if parse_version(version).is_none() {
        return Err(GoError::InvalidVersion(version.to_string()));
    }
    Ok(version.to_string())
}

/// The canonical `.info` document: the version and the moment it was first uploaded
/// to the registry, in RFC 3339.
pub fn info_document(version: &str, time: &str) -> Result<Value, GoError> {
    let version = valid_version(version)?;
    chrono::DateTime::parse_from_rfc3339(time)
        .map_err(|_| GoError::InvalidDocument(format!("the time {time:?} is not RFC 3339")))?;
    Ok(json!({ "Version": version, "Time": time }))
}

/// Checks a published `.info` against the version its path announces.
pub fn check_info(body: &[u8], version: &str) -> Result<(), GoError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| GoError::InvalidDocument("the info document is not JSON".into()))?;
    if value["Version"].as_str() != Some(version) {
        return Err(GoError::InvalidDocument(format!(
            "the info document does not declare version {version}"
        )));
    }
    let time = value["Time"].as_str().unwrap_or_default();
    chrono::DateTime::parse_from_rfc3339(time)
        .map_err(|_| GoError::InvalidDocument("the info time is not RFC 3339".into()))?;
    Ok(())
}

/// The unescaped form of a module path: `!x` goes back to `X`. It is the
/// form in which the `go.mod` and the inside of the zip name the module.
pub fn unescape(module: &str) -> String {
    let bytes = module.as_bytes();
    let mut out = String::with_capacity(module.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'!' && index + 1 < bytes.len() {
            out.push(bytes[index + 1].to_ascii_uppercase() as char);
            index += 2;
        } else {
            out.push(bytes[index] as char);
            index += 1;
        }
    }
    out
}

/// Checks a published `.mod` against the module its path announces: the
/// first `module` directive of the file has to say exactly the same
/// thing. A mute or lying `.mod` does not get in: `go` would use it to
/// resolve dependencies under another identity.
pub fn check_mod(body: &[u8], module: &str) -> Result<(), GoError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| GoError::InvalidDocument("the go.mod is not utf-8".into()))?;
    let declared = declared_module(text)
        .ok_or_else(|| GoError::InvalidDocument("the go.mod declares no module".into()))?;
    if declared != unescape(module) {
        return Err(GoError::InvalidDocument(format!(
            "the go.mod declares {declared:?}, the route says {module:?}"
        )));
    }
    Ok(())
}

/// The module a `go.mod` declares, unescaped: the directive can
/// be written with uppercase letters because the `go.mod` lives inside the zip and the
/// path's route lives outside.
pub fn declared_module(go_mod: &str) -> Option<String> {
    for line in go_mod.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module") {
            let declared = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            return (!declared.is_empty()).then_some(declared);
        }
    }
    None
}

/// The version number without the `v` for ordering: a proxy's listing
/// returns already-valid versions; what is not valid is silently ignored,
/// as `go` itself does.
pub fn parse_version(version: &str) -> Option<semver::Version> {
    semver::Version::parse(version.strip_prefix('v')?).ok()
}

fn split(rest: &str) -> Result<Vec<&str>, GoError> {
    let mut segments = Vec::new();
    for segment in rest.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                return Err(GoError::InvalidPath(
                    "the path escapes the repository".into(),
                ));
            }
            other => segments.push(other),
        }
    }
    Ok(segments)
}

fn join_module(segments: &[&str]) -> Result<String, GoError> {
    let module = segments.join("/");
    valid_module(&module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_are_the_four_artifacts_and_latest() {
        assert_eq!(
            Route::parse("example.com/hello/@v/list").expect("list"),
            Route::List {
                module: "example.com/hello".into()
            }
        );
        assert_eq!(
            Route::parse("example.com/hello/@v/v1.0.0.info").expect("info"),
            Route::Info {
                module: "example.com/hello".into(),
                version: "v1.0.0".into()
            }
        );
        assert_eq!(
            Route::parse("example.com/hello/@v/v1.0.0.mod").expect("mod"),
            Route::Mod {
                module: "example.com/hello".into(),
                version: "v1.0.0".into()
            }
        );
        assert_eq!(
            Route::parse("github.com/!burnt!sushi/toml/@v/v1.3.2.zip").expect("zip"),
            Route::Zip {
                module: "github.com/!burnt!sushi/toml".into(),
                version: "v1.3.2".into()
            }
        );
        assert_eq!(
            Route::parse("example.com/hello/@latest").expect("latest"),
            Route::Latest {
                module: "example.com/hello".into()
            }
        );
    }

    #[test]
    fn pseudo_versions_and_incompatible_are_versions_too() {
        for version in [
            "v0.0.0-20191109021931-daa7c04131f0",
            "v2.0.0+incompatible",
            "v1.0.0-rc.1",
        ] {
            let route = Route::parse(&format!("example.com/m/@v/{version}.mod"))
                .unwrap_or_else(|error| panic!("{version} must parse: {error:?}"));
            assert_eq!(
                route.asset_path(),
                Some(format!("example.com/m/@v/{version}.mod"))
            );
        }
    }

    #[test]
    fn wild_paths_are_not_goproxy() {
        for hostile in [
            "",
            "example.com/hello",
            "example.com/hello/@v",
            "example.com/hello/@v/v1.0.0.0.0",
            "example.com/hello/@v/v1.0.0.rar",
            "example.com/hello/@v/1.0.0.info",
            "example.com/../@v/list",
            "example.com/@v/v1.0.0.info/extra",
        ] {
            assert!(
                Route::parse(hostile).is_err(),
                "{hostile:?} must be refused as a GOPROXY path"
            );
        }
    }

    #[test]
    fn modules_are_escaped_lowercase_paths() {
        assert_eq!(
            valid_module("golang.org/x/text").expect("plain"),
            "golang.org/x/text"
        );
        assert_eq!(
            valid_module("github.com/!burnt!sushi/toml").expect("escaped"),
            "github.com/!burnt!sushi/toml"
        );
        assert_eq!(
            valid_module("localhost:8080/folder/mod").expect("port"),
            "localhost:8080/folder/mod"
        );
        for hostile in [
            "",
            "Upper/Case",
            "example.com/un!escaped!",
            "example.com/!uPPER",
            "example.com//hello",
            "example.com/./hello",
            "example.com/@v",
        ] {
            assert!(
                valid_module(hostile).is_err(),
                "{hostile:?} must be refused"
            );
        }
    }

    #[test]
    fn the_info_document_is_version_and_time() {
        let document = info_document("v1.2.3", "2026-11-17T12:00:00Z").expect("document");
        assert_eq!(document["Version"], "v1.2.3");
        assert!(info_document("v1.2.3", "ayer").is_err());
        assert!(info_document("1.2.3", "2026-11-17T12:00:00Z").is_err());

        check_info(document.to_string().as_bytes(), "v1.2.3").expect("consistent");
        assert!(check_info(document.to_string().as_bytes(), "v9.9.9").is_err());
    }

    #[test]
    fn the_go_mod_must_declare_the_module_of_its_route() {
        check_mod(
            b"module example.com/hello\n\ngo 1.22\n",
            "example.com/hello",
        )
        .expect("mod");
        // The go.mod writes the real name; the path, the escaped form.
        check_mod(
            b"module github.com/BurntSushi/toml\n",
            "github.com/!burnt!sushi/toml",
        )
        .expect("the escape is folded before comparing");
        assert_eq!(
            unescape("github.com/!burnt!sushi/toml"),
            "github.com/BurntSushi/toml"
        );
        assert_eq!(unescape("example.com/plain"), "example.com/plain");
        assert!(check_mod(b"go 1.22\n", "example.com/hello").is_err());
        assert_eq!(
            declared_module("module \"quoted/x\"\ngo 1.22").as_deref(),
            Some("quoted/x")
        );
    }

    #[test]
    fn versions_order_like_the_go_tool() {
        let versions = ["v1.0.0", "v1.10.0", "v1.2.0"];
        let mut by_semver: Vec<_> = versions
            .iter()
            .filter_map(|version| parse_version(version).map(|parsed| (parsed, *version)))
            .collect();
        by_semver.sort();
        assert_eq!(
            by_semver
                .iter()
                .map(|(_, version)| *version)
                .collect::<Vec<_>>(),
            vec!["v1.0.0", "v1.2.0", "v1.10.0"],
            "the list is ordered by semver, not by bytes"
        );
    }

    #[test]
    fn content_types_answer_what_the_protocol_promises() {
        assert_eq!(content_type("info"), "application/json");
        assert_eq!(content_type("mod"), "text/plain; charset=utf-8");
        assert_eq!(content_type("zip"), "application/zip");
    }
}
