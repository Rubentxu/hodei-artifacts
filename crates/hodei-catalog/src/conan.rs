//! The **conan** grammar of the Conan 2 remote protocol.
//!
//! The **pure** part lives here: the route table that the Conan 2 client speaks
//! against a remote with revisions, and the map between those routes and the
//! catalog's asset paths. The dialect is fixed by the client
//! (`conan/internal/rest/rest_routes.py` of Conan 2.20.1) and was verified by the
//! spike against a reference remote: `conan/internal/rest/client_routes.py`
//! assembles the URLs on `{remote}/v2/`, and the ping lives at `/v1/ping`.
//!
//! The versioned identity is that of the protocol itself: the **recipe revision**
//! (`rrev`) and the **package revision** (`prev`) are opaque path segments
//! that the server keeps as is. The `package_id` travels in its own
//! position and the binaries of a `package_id` live under a prefix that is only
//! its own: nothing mixes the bytes of two `package_id`s of the same recipe.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};

/// The versioned identifier of a recipe: `name/version@user/channel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeRef {
    /// The name of the conan package.
    pub name: String,
    /// The version.
    pub version: String,
    /// The user, `_` when the client carries none.
    pub user: String,
    /// The channel, `_` when the client carries none.
    pub channel: String,
}

impl RecipeRef {
    /// The reference exactly as the client prints it in listings.
    ///
    /// `_` means "no user" and "no channel": the client strips them
    /// again when reading.
    pub fn render(&self) -> String {
        format!(
            "{}/{}@{}/{}",
            self.name, self.version, self.user, self.channel
        )
    }

    /// The asset prefix of a recipe (without revision or file).
    pub fn recipe_prefix(&self) -> String {
        format!(
            "recipes/{}/{}/{}/{}",
            self.name, self.version, self.user, self.channel
        )
    }

    /// The asset prefix of a recipe revision.
    pub fn revision_prefix(&self, revision: &str) -> String {
        format!("{}/{revision}", self.recipe_prefix())
    }

    /// The asset path of a recipe file.
    pub fn recipe_file_path(&self, revision: &str, file: &str) -> String {
        format!("{}/{file}", self.revision_prefix(revision))
    }

    /// The asset prefix of the packages of a recipe revision.
    ///
    /// The binaries live in their own tree, next to the one of the recipes:
    /// `packages/{n}/{v}/{u}/{c}/{rrev}/{pid}/{prev}/{file}`. A
    /// `package_id` does not share a prefix with another, and none shares a
    /// prefix with the recipe files.
    pub fn packages_prefix(&self, revision: &str) -> String {
        format!(
            "packages/{}/{}/{}/{}/{}",
            self.name, self.version, self.user, self.channel, revision
        )
    }

    /// The asset prefix of a package_id of a revision.
    pub fn package_prefix(&self, revision: &str, package_id: &str) -> String {
        format!("{}/{package_id}", self.packages_prefix(revision))
    }

    /// The asset prefix of a package revision.
    pub fn package_revision_prefix(
        &self,
        revision: &str,
        package_id: &str,
        package_revision: &str,
    ) -> String {
        format!(
            "{}/{package_revision}",
            self.package_prefix(revision, package_id)
        )
    }

    /// The asset path of a file of a package.
    pub fn package_file_path(
        &self,
        revision: &str,
        package_id: &str,
        package_revision: &str,
        file: &str,
    ) -> String {
        format!(
            "{}/{file}",
            self.package_revision_prefix(revision, package_id, package_revision)
        )
    }
}

/// The timestamp that the remote returns in the revision
/// documents, in the format the client reads (`datetime.fromisoformat`
/// accepts the trailing `Z`).
pub fn the_timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// What can be requested on the conan surface.
///
/// The table is that of the client's `rest_routes.py`: the oracle of the
/// dialect. A recipe name is repeated in every route because the
/// client does not use short routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /v1/ping`: the client reads the capabilities from the header.
    Ping,
    /// `GET /v2/users/authenticate`: Basic in, token out.
    Authenticate,
    /// `GET /v2/users/check_credentials`: checks the token.
    CheckCredentials,
    /// `GET /v2/conans/search`: the recipe search, with its `q` pattern.
    Search,
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/latest`: the latest recipe revision.
    RecipeLatest(RecipeRef),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions`: the recipe revisions.
    RecipeRevisions(RecipeRef),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/files`: the
    /// recipe snapshot.
    RecipeSnapshot(RecipeRef, String),
    /// `GET|PUT /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/files/{path}`:
    /// a recipe file, with its relative path as is.
    RecipeFile(RecipeRef, String, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages`: the
    /// package ids of a recipe revision.
    PackageIds(RecipeRef, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/search`:
    /// the package id search (the client's).
    PackageSearch(RecipeRef, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/{pid}/latest`:
    /// the latest package revision of that package id.
    PackageLatest(RecipeRef, String, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/{pid}/revisions`:
    /// the package revisions of that package id.
    PackageRevisions(RecipeRef, String, String),
    /// `GET /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/{pid}/revisions/{prev}/files`:
    /// the package snapshot.
    PackageSnapshot(RecipeRef, String, String, String),
    /// `GET|PUT /v2/conans/{n}/{v}/{u}/{c}/revisions/{rrev}/packages/{pid}/revisions/{prev}/files/{path}`:
    /// a package file.
    PackageFile(RecipeRef, String, String, String, String),
}

/// A component name of the conan protocol: recipe name, version,
/// user, channel, revision, package id.
///
/// Conan writes them in the path without encoding, and a `..` or an empty
/// segment is not a name: it is a disguised path. Revisions and
/// package ids are hexadecimal in practice, but any token without a
/// slash is what the server accepts: whoever invents the value is the
/// client, not the server.
pub fn valid_component(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

/// A file of the recipe or of the package.
///
/// Files can nest (`metadata/sign`), but each segment is a
/// literal name: no slash, no hidden escape, no empty segment.
pub fn valid_file_path(text: &str) -> bool {
    !text.is_empty() && text.len() <= 255 && text.split('/').all(valid_component)
}

fn the_ref(name: &str, version: &str, user: &str, channel: &str) -> Option<RecipeRef> {
    for part in [name, version, user, channel] {
        if !valid_component(part) {
            return None;
        }
    }
    Some(RecipeRef {
        name: name.to_string(),
        version: version.to_string(),
        user: user.to_string(),
        channel: channel.to_string(),
    })
}

/// The requested path, already interpreted.
///
/// The input is what remains of `/conan/{repositorio}/` in the URI, without
/// decoding: the protocol grammar is the only one that may look at
/// a `%2f`. The query does not come here.
pub fn parse(rest: &str) -> Option<Route> {
    let segments: Vec<&str> = rest.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        ["v1", "ping"] => Some(Route::Ping),
        ["v2", "users", "authenticate"] => Some(Route::Authenticate),
        ["v2", "users", "check_credentials"] => Some(Route::CheckCredentials),
        ["v2", "conans", "search"] => Some(Route::Search),
        ["v2", "conans", name, version, user, channel, "latest"] => {
            Some(Route::RecipeLatest(the_ref(name, version, user, channel)?))
        }
        ["v2", "conans", name, version, user, channel, "revisions"] => Some(
            Route::RecipeRevisions(the_ref(name, version, user, channel)?),
        ),
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "files",
        ] if valid_component(revision) => Some(Route::RecipeSnapshot(
            the_ref(name, version, user, channel)?,
            revision.to_string(),
        )),
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "files",
            rest @ ..,
        ] if valid_component(revision) && !rest.is_empty() => {
            let file = rest.join("/");
            if !valid_file_path(&file) {
                return None;
            }
            Some(Route::RecipeFile(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                file,
            ))
        }
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
        ] if valid_component(revision) => Some(Route::PackageIds(
            the_ref(name, version, user, channel)?,
            revision.to_string(),
        )),
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            "search",
        ] if valid_component(revision) => Some(Route::PackageSearch(
            the_ref(name, version, user, channel)?,
            revision.to_string(),
        )),
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            package_id,
            "latest",
        ] if valid_component(revision) && valid_component(package_id) => {
            Some(Route::PackageLatest(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                package_id.to_string(),
            ))
        }
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            package_id,
            "revisions",
        ] if valid_component(revision) && valid_component(package_id) => {
            Some(Route::PackageRevisions(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                package_id.to_string(),
            ))
        }
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            package_id,
            "revisions",
            package_revision,
            "files",
        ] if valid_component(revision)
            && valid_component(package_id)
            && valid_component(package_revision) =>
        {
            Some(Route::PackageSnapshot(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                package_id.to_string(),
                package_revision.to_string(),
            ))
        }
        [
            "v2",
            "conans",
            name,
            version,
            user,
            channel,
            "revisions",
            revision,
            "packages",
            package_id,
            "revisions",
            package_revision,
            "files",
            rest @ ..,
        ] if valid_component(revision)
            && valid_component(package_id)
            && valid_component(package_revision)
            && !rest.is_empty() =>
        {
            let file = rest.join("/");
            if !valid_file_path(&file) {
                return None;
            }
            Some(Route::PackageFile(
                the_ref(name, version, user, channel)?,
                revision.to_string(),
                package_id.to_string(),
                package_revision.to_string(),
                file,
            ))
        }
        _ => None,
    }
}

/// The response of a recipe or package snapshot.
///
/// The client reads `files` and discards the rest; the relative `url`s are the
/// ones it assembles from the routes of the table, and the server does not need to
/// tell it anything that the dialect already fixes.
pub fn snapshot_document(files: &[(String, u64)]) -> Value {
    let files: Value = files
        .iter()
        .map(|(name, size)| (name.clone(), json!({ "size": size })))
        .collect::<serde_json::Map<_, _>>()
        .into();
    json!({ "files": files, "download_urls": {} })
}

/// The document of a revision list (recipe or package).
pub fn revisions_document(revisions: &[(String, DateTime<Utc>)]) -> Value {
    json!({
        "revisions": revisions
            .iter()
            .map(|(revision, time)| {
                json!({ "revision": revision, "time": the_timestamp(*time) })
            })
            .collect::<Vec<_>>(),
    })
}

/// The document of the latest revision: just one, with its time.
pub fn revision_document(revision: &str, time: DateTime<Utc>) -> Value {
    json!({ "revision": revision, "time": the_timestamp(time) })
}

/// The response of the recipe search.
pub fn search_document(results: &[RecipeRef]) -> Value {
    json!({ "results": results.iter().map(RecipeRef::render).collect::<Vec<_>>() })
}

/// The `q` pattern of the search query.
///
/// The query arrives in wire form (`q=hello%2F%2A`); the value is
/// decoded **once** and without interpreting anything else: the pattern is glob
/// text, not an expression that the server invents.
pub fn the_query_of(query: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        if name != "q" {
            return None;
        }
        Some(percent_decode(value))
    })
}

/// A query value decoded, once.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (
                (bytes[index + 1] as char).to_digit(16),
                (bytes[index + 2] as char).to_digit(16),
            )
        {
            out.push((high * 16 + low) as u8);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The client's recipe search, applied to the known recipes.
///
/// The pattern is what the client sends in `q`: a reference with
/// wildcards (`hello/*`, `*/1.0.0@hodei/stable`). The match is a glob
/// over the printed reference; without a pattern, everything.
pub fn the_search_matches(pattern: &str, reference: &RecipeRef) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let rendered = reference.render();
    // The client asks for the comparison without distinguishing case (the reference
    // remote applies it in its search engine); here we apply it
    // the same way: a listing is a read, not an identity.
    the_glob(
        &rendered.to_ascii_lowercase(),
        &pattern.to_ascii_lowercase(),
    )
}

fn the_glob(text: &str, pattern: &str) -> bool {
    match pattern.split_once('*') {
        None => text == pattern,
        Some((before, after)) => {
            let (Some(start), Some(end)) = (text.find(before), text.rfind(after)) else {
                return false;
            };
            before == &text[start..start + before.len()]
                && after == &text[end..]
                && start + before.len() <= end
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn the_ref() -> RecipeRef {
        RecipeRef {
            name: "hello".into(),
            version: "1.0.0".into(),
            user: "hodei".into(),
            channel: "stable".into(),
        }
    }

    #[test]
    fn every_route_of_the_client_dialect_parses() {
        for (path, expected) in [
            ("v1/ping", Route::Ping),
            ("v2/users/authenticate", Route::Authenticate),
            ("v2/users/check_credentials", Route::CheckCredentials),
            ("v2/conans/search", Route::Search),
            (
                "v2/conans/hello/1.0.0/hodei/stable/latest",
                Route::RecipeLatest(the_ref()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions",
                Route::RecipeRevisions(the_ref()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/files",
                Route::RecipeSnapshot(the_ref(), "abc123".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/files/conanfile.py",
                Route::RecipeFile(the_ref(), "abc123".into(), "conanfile.py".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages",
                Route::PackageIds(the_ref(), "abc123".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/search",
                Route::PackageSearch(the_ref(), "abc123".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/pid1/latest",
                Route::PackageLatest(the_ref(), "abc123".into(), "pid1".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/pid1/revisions",
                Route::PackageRevisions(the_ref(), "abc123".into(), "pid1".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/pid1/revisions/prev1/files",
                Route::PackageSnapshot(the_ref(), "abc123".into(), "pid1".into(), "prev1".into()),
            ),
            (
                "v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/packages/pid1/revisions/prev1/files/conan_package.tgz",
                Route::PackageFile(
                    the_ref(),
                    "abc123".into(),
                    "pid1".into(),
                    "prev1".into(),
                    "conan_package.tgz".into(),
                ),
            ),
        ] {
            assert_eq!(parse(path), Some(expected), "{path} does not parse");
        }
    }

    #[test]
    fn a_nested_file_of_a_package_parses_whole() {
        let route = parse(
            "v2/conans/hello/1.0.0/hodei/stable/revisions/r1/packages/pid1/revisions/prev1/files/metadata/sign",
        )
        .expect("the nested file parses");
        assert_eq!(
            route,
            Route::PackageFile(
                the_ref(),
                "r1".into(),
                "pid1".into(),
                "prev1".into(),
                "metadata/sign".into()
            )
        );
    }

    #[test]
    fn a_path_that_hides_a_traversal_is_not_a_route() {
        assert_eq!(parse("v2/conans/../1.0.0/hodei/stable/latest"), None);
        assert_eq!(
            parse("v2/conans/hello/1.0.0/hodei/stable/revisions/r1/files/..%2fx"),
            None
        );
        assert_eq!(parse("v2/conans/hello//hodei/stable/latest"), None);
    }

    #[test]
    fn the_asset_paths_keep_every_package_id_apart() {
        let reference = the_ref();
        let left = reference.package_file_path("r1", "pid1", "prev1", "conan_package.tgz");
        let right = reference.package_file_path("r1", "pid2", "prev2", "conan_package.tgz");
        assert_ne!(left, right);
        assert!(left.starts_with("packages/hello/1.0.0/hodei/stable/r1/pid1/prev1/"));
        assert_eq!(
            reference.recipe_file_path("r1", "conanfile.py"),
            "recipes/hello/1.0.0/hodei/stable/r1/conanfile.py"
        );
    }

    #[test]
    fn the_query_of_the_wire_decodes_once() {
        assert_eq!(the_query_of("q=hello%2F%2A").as_deref(), Some("hello/*"));
        assert_eq!(the_query_of("q=%2A"), Some("*".to_string()));
        assert_eq!(the_query_of("ignorecase=False"), None);
        assert_eq!(the_query_of(""), None);
    }

    #[test]
    fn the_search_matches_the_patterns_the_client_sends() {
        assert!(the_search_matches("hello/*", &the_ref()));
        assert!(the_search_matches("hello/1.0.0@hodei/stable", &the_ref()));
        assert!(the_search_matches("*", &the_ref()));
        assert!(
            the_search_matches("HELLO/*", &the_ref()),
            "the match is not case sensitive"
        );
        assert!(!the_search_matches("goodbye/*", &the_ref()));
        assert!(!the_search_matches("hello/1.0.1", &the_ref()));
    }
}
