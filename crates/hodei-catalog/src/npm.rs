//! The pure part of the npm format: names, routes, publish
//! documents and packuments.
//!
//! Nothing here touches disk, network or catalog: these are functions over strings and
//! JSON, and for that reason they are tested without infrastructure. The service uses them to
//! decide what the client wants (`route`), whether a publish document
//! is admissible (`PublishDocument`) and what the packument looks like after
//! each operation.
//!
//! # The packument is a snapshot, not a view
//!
//! Unlike `maven-metadata.xml`, a packument carries data that cannot
//! be derived from the files (each version's `package.json`, the
//! dist-tags, the deprecations). That is why a hosted repository stores **every state**
//! of the packument as content addressed by its digest and moves the
//! route pointer: publishing, moving a dist-tag or deprecating create a new
//! snapshot and do not rewrite the bytes of the previous ones.
//!
//! # Layout
//!
//! An asset's path is the protocol's, the same in hosted and proxy:
//!
//! ```text
//! {name}                        the packument
//! {name}/-/{base}-{version}.tgz     the tarball
//! ```
//!
//! where `name` may be `@scope/base`.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

/// The maximum length of a package name (complete, with scope).
pub const MAX_NAME_LEN: usize = 214;

/// The maximum length of a dist-tag.
pub const MAX_TAG_LEN: usize = 64;

/// The dist-tag that npm resolves when no other is requested.
pub const LATEST: &str = "latest";

/// The suffix of the packument snapshot coordinates.
///
/// `#` cannot appear in an npm name, so the snapshot coordinates
/// never collide with those of a tarball.
pub const PACKUMENT_COORDINATES_SUFFIX: &str = "#packument";

/// The names that npm reserves even if they satisfy the rest of the rules.
const RESERVED: &[&str] = &["node_modules", "favicon.ico"];

/// Why a name, a route or a document is not admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NpmError {
    /// The package name does not satisfy npm's rules.
    InvalidName(&'static str),
    /// The route is none of those the protocol speaks.
    InvalidPath(&'static str),
    /// The document is not understood or is not admissible.
    InvalidDocument(&'static str),
    /// The dist-tag is not admissible.
    InvalidTag(&'static str),
    /// The version is already published and a version is not rewritten.
    VersionExists(String),
    /// What was requested does not exist in the packument.
    UnknownVersion(String),
    /// The document tries to remove something: that is an unpublish.
    Unpublish,
}

impl NpmError {
    /// The reason, as it is told to the client.
    pub fn message(&self) -> String {
        match self {
            Self::InvalidName(reason)
            | Self::InvalidPath(reason)
            | Self::InvalidDocument(reason)
            | Self::InvalidTag(reason) => (*reason).to_string(),
            Self::VersionExists(version) => {
                format!("You cannot publish over the previously published versions: {version}.")
            }
            Self::UnknownVersion(version) => format!("the version {version} does not exist"),
            Self::Unpublish => {
                "unpublishing is not allowed in this registry; deprecate the version instead"
                    .to_string()
            }
        }
    }
}

/// A package name, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageName {
    full: String,
    /// The scope with `@`, if it has one.
    scope: Option<String>,
    base: String,
}

impl PackageName {
    /// Reads a name. `strict` applies the rules of a **new** package
    /// (lowercase only); a proxy admits what the upstream already published,
    /// which includes old names with uppercase letters.
    pub fn parse(name: &str, strict: bool) -> Result<Self, NpmError> {
        if name.is_empty() {
            return Err(NpmError::InvalidName("the package name is empty"));
        }
        if name.len() > MAX_NAME_LEN {
            return Err(NpmError::InvalidName("the package name is too long"));
        }
        let (scope, base) = match name.strip_prefix('@') {
            Some(rest) => {
                let Some((scope, base)) = rest.split_once('/') else {
                    return Err(NpmError::InvalidName(
                        "a scoped package name is @scope/name",
                    ));
                };
                (Some(format!("@{scope}")), base)
            }
            None => (None, name),
        };
        let part_ok = |part: &str| {
            !part.is_empty()
                && !part.starts_with('.')
                && !part.starts_with('_')
                && part.bytes().all(|byte| {
                    byte.is_ascii_digit()
                        || byte.is_ascii_lowercase()
                        || (!strict && byte.is_ascii_uppercase())
                        || matches!(byte, b'-' | b'.' | b'_' | b'~')
                })
        };
        if let Some(scope) = &scope
            && !part_ok(&scope[1..])
        {
            return Err(NpmError::InvalidName("the scope is not a valid name"));
        }
        if !part_ok(base) {
            return Err(NpmError::InvalidName(
                "the package name has characters that npm does not allow",
            ));
        }
        if RESERVED.contains(&base) {
            return Err(NpmError::InvalidName("the package name is reserved"));
        }
        Ok(Self {
            full: name.to_string(),
            scope,
            base: base.to_string(),
        })
    }

    /// The full name, with scope.
    pub fn full(&self) -> &str {
        &self.full
    }

    /// The scope with `@`, if it has one.
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    /// The name without scope.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// The tarball file name of a version.
    pub fn tarball_file(&self, version: &str) -> String {
        format!("{}-{version}.tgz", self.base)
    }

    /// The path of a version's tarball inside the repository.
    pub fn tarball_path(&self, version: &str) -> String {
        format!("{}/-/{}", self.full, self.tarball_file(version))
    }
}

/// What the client requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /-/ping`.
    Ping,
    /// `GET /-/whoami`.
    Whoami,
    /// The packument of a package.
    Packument {
        /// The package whose packument is asked for.
        name: PackageName,
    },
    /// The tarball of a package.
    Tarball {
        /// The package the tarball belongs to.
        name: PackageName,
        /// The name of the file inside the package.
        file: String,
    },
    /// All the dist-tags of a package.
    DistTags {
        /// The package whose dist-tags are asked for.
        name: PackageName,
    },
    /// A dist-tag of a package.
    DistTag {
        /// The package the tag belongs to.
        name: PackageName,
        /// The tag and the version it points at.
        tag: String,
    },
    /// `DELETE` of an entire package (`/{name}/-rev/{rev}`).
    UnpublishPackage {
        /// The package that is withdrawn.
        name: PackageName,
    },
    /// `DELETE` of a tarball (`/{name}/-/{file}/-rev/{rev}`).
    UnpublishTarball {
        /// The package the tarball belongs to.
        name: PackageName,
        /// The name of the file inside the package.
        file: String,
    },
}

impl Route {
    /// The path of the asset backing the request, if the request reads
    /// repository content.
    pub fn asset_path(&self) -> Option<String> {
        match self {
            Self::Packument { name } | Self::DistTags { name } | Self::DistTag { name, .. } => {
                Some(name.full().to_string())
            }
            Self::Tarball { name, file } => Some(format!("{}/-/{file}", name.full())),
            _ => None,
        }
    }
}

/// Decodes a segment: only `%2f` (the slash of a scoped name) and
/// `%40` (the at sign) make sense in an npm name.
fn decode_segment(segment: &str) -> Result<String, NpmError> {
    let mut out = String::with_capacity(segment.len());
    let bytes = segment.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            let escape = segment
                .get(index + 1..index + 3)
                .map(str::to_ascii_lowercase);
            match escape.as_deref() {
                Some("2f") => out.push('/'),
                Some("40") => out.push('@'),
                _ => {
                    return Err(NpmError::InvalidPath(
                        "the path has an escape npm never uses",
                    ));
                }
            }
            index += 3;
        } else {
            out.push(byte as char);
            index += 1;
        }
    }
    Ok(out)
}

fn path_char_ok(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, '-' | '.' | '_' | '~' | '@' | '%' | '+' | ':')
}

/// A tarball file name: what the upstream publishes, without
/// traversing directories.
fn tarball_file_ok(file: &str) -> bool {
    file.len() <= 255
        && file.ends_with(".tgz")
        && !file.starts_with('.')
        && file.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+')
        })
}

/// Reads the path following `/npm/{repository}/`, **without decoding**.
pub fn route(rest: &str) -> Result<Route, NpmError> {
    if rest.is_empty() || rest.len() > 1024 {
        return Err(NpmError::InvalidPath("the path is empty or too long"));
    }
    if !rest
        .chars()
        .all(|character| path_char_ok(character) || character == '/')
    {
        return Err(NpmError::InvalidPath(
            "the path has characters npm never uses",
        ));
    }
    let raw: Vec<&str> = rest.split('/').collect();
    if raw.iter().any(|segment| segment.is_empty()) {
        return Err(NpmError::InvalidPath("the path has an empty segment"));
    }
    let segments: Vec<String> = raw
        .iter()
        .map(|segment| decode_segment(segment))
        .collect::<Result<_, _>>()?;
    // A segment may carry a slash (the scope's `%2f`) only if it is
    // the one in the name: `@scope/base`.
    if segments
        .iter()
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(NpmError::InvalidPath("the path has a dot segment"));
    }

    if segments[0] == "-" {
        return match segments.as_slice() {
            [_, ping] if ping == "ping" => Ok(Route::Ping),
            [_, whoami] if whoami == "whoami" => Ok(Route::Whoami),
            [_, package, name, tags] if package == "package" && tags == "dist-tags" => {
                Ok(Route::DistTags {
                    name: PackageName::parse(name, false)?,
                })
            }
            [_, package, name, tags, tag] if package == "package" && tags == "dist-tags" => {
                Ok(Route::DistTag {
                    name: PackageName::parse(name, false)?,
                    tag: valid_tag(tag)?.to_string(),
                })
            }
            _ => Err(NpmError::InvalidPath("this npm endpoint is not served")),
        };
    }

    // The name: `@scope%2fbase` arrives in one segment, `@scope/base` in
    // two; both are the same package.
    let (name_text, tail): (String, &[String]) =
        if segments[0].starts_with('@') && !segments[0].contains('/') {
            match segments.get(1) {
                Some(base) => (format!("{}/{}", segments[0], base), &segments[2..]),
                None => {
                    return Err(NpmError::InvalidName(
                        "a scoped package name is @scope/name",
                    ));
                }
            }
        } else {
            (segments[0].clone(), &segments[1..])
        };
    let name = PackageName::parse(&name_text, false)?;
    match tail {
        [] => Ok(Route::Packument { name }),
        [dash, file] if dash == "-" => {
            if !tarball_file_ok(file) {
                return Err(NpmError::InvalidPath("the tarball name is not valid"));
            }
            Ok(Route::Tarball {
                name,
                file: file.clone(),
            })
        }
        [rev, _] if rev == "-rev" => Ok(Route::UnpublishPackage { name }),
        [dash, file, rev, _] if dash == "-" && rev == "-rev" => {
            if !tarball_file_ok(file) {
                return Err(NpmError::InvalidPath("the tarball name is not valid"));
            }
            Ok(Route::UnpublishTarball {
                name,
                file: file.clone(),
            })
        }
        _ => Err(NpmError::InvalidPath("this npm endpoint is not served")),
    }
}

/// Reads a semver version. npm requires a complete, valid version.
pub fn valid_version(version: &str) -> Result<&str, NpmError> {
    if version.len() > 256 || semver::Version::parse(version).is_err() {
        return Err(NpmError::InvalidDocument(
            "the version is not a valid semver",
        ));
    }
    Ok(version)
}

/// A dist-tag: a short name that is neither a version nor a range.
pub fn valid_tag(tag: &str) -> Result<&str, NpmError> {
    if tag.is_empty() || tag.len() > MAX_TAG_LEN {
        return Err(NpmError::InvalidTag("the dist-tag is empty or too long"));
    }
    if !tag
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
    {
        return Err(NpmError::InvalidTag(
            "the dist-tag has characters npm never uses",
        ));
    }
    // A tag that reads as a version or as a range would make
    // `npm install pkg@x` ambiguous: npm forbids it.
    if semver::Version::parse(tag).is_ok() || semver::VersionReq::parse(tag).is_ok() {
        return Err(NpmError::InvalidTag(
            "a dist-tag cannot be a version or a range",
        ));
    }
    Ok(tag)
}

/// An attachment of the publish document, already decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The file name.
    pub file: String,
    /// The content: the tarball.
    pub bytes: Vec<u8>,
}

/// The request of a `PUT /{name}`.
///
/// It is a **publication** if it carries the tarball and a **metadata
/// update** (deprecate) if it does not. Anything else is an error.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// A new version with its tarball.
    Publish(Box<NewVersion>),
    /// `deprecated` changes on versions that already exist.
    Metadata(Box<Document>),
}

/// The version being published.
#[derive(Debug, Clone, PartialEq)]
pub struct NewVersion {
    /// The package name.
    pub name: PackageName,
    /// The version.
    pub version: String,
    /// The `package.json` the client declares, untouched.
    pub manifest: Map<String, Value>,
    /// The dist-tags the client requests for this version.
    pub tags: Vec<String>,
    /// The tarball.
    pub attachment: Attachment,
    /// The document fields that describe the package.
    pub document: Document,
}

/// The document as it arrives, already shaped.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The package name.
    pub name: PackageName,
    /// The entire object.
    pub value: Map<String, Value>,
}

/// Reads the body of a `PUT /{name}`.
///
/// `expected` is the route's name: the document cannot talk about another
/// package. `decode` converts the attachment's base64 (the service decides the
/// codec; there are no hash or base64 dependencies here).
pub fn parse_update(
    expected: &PackageName,
    body: &[u8],
    decode: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Update, NpmError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| NpmError::InvalidDocument("the body is not a JSON document"))?;
    let Value::Object(object) = value else {
        return Err(NpmError::InvalidDocument("the body is not a JSON object"));
    };
    let declared = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or(NpmError::InvalidDocument("the document has no name"))?;
    if declared != expected.full() {
        return Err(NpmError::InvalidDocument(
            "the document is for another package than the path",
        ));
    }
    let document = Document {
        name: expected.clone(),
        value: object,
    };
    let attachments = document
        .value
        .get("_attachments")
        .and_then(Value::as_object)
        .filter(|attachments| !attachments.is_empty());
    let Some(attachments) = attachments else {
        return Ok(Update::Metadata(Box::new(document)));
    };
    if attachments.len() != 1 {
        return Err(NpmError::InvalidDocument(
            "a publication carries exactly one tarball",
        ));
    }
    let versions = document
        .value
        .get("versions")
        .and_then(Value::as_object)
        .ok_or(NpmError::InvalidDocument("the document has no versions"))?;
    if versions.len() != 1 {
        return Err(NpmError::InvalidDocument(
            "a publication carries exactly one version",
        ));
    }
    let (version, manifest) = versions.iter().next().expect("one version");
    valid_version(version)?;
    let Value::Object(manifest) = manifest else {
        return Err(NpmError::InvalidDocument("the version is not an object"));
    };
    if manifest.get("name").and_then(Value::as_str) != Some(expected.full())
        || manifest.get("version").and_then(Value::as_str) != Some(version.as_str())
    {
        return Err(NpmError::InvalidDocument(
            "the version does not say the name and version it is published under",
        ));
    }
    let (file, attachment) = attachments.iter().next().expect("one attachment");
    // `libnpmpublish` calls the attachment `{full name}-{version}.tgz`
    // (`@acme/widget-1.0.0.tgz` if scoped); the registry stores it as
    // `{base}-{version}.tgz`. Both forms are accepted and the second is stored.
    let basename = expected.tarball_file(version);
    if *file != basename && *file != format!("{}-{version}.tgz", expected.full()) {
        return Err(NpmError::InvalidDocument(
            "the tarball is not called {name}-{version}.tgz",
        ));
    }
    let file = &basename;
    let data = attachment
        .get("data")
        .and_then(Value::as_str)
        .ok_or(NpmError::InvalidDocument("the tarball has no data"))?;
    let bytes = decode(data).ok_or(NpmError::InvalidDocument("the tarball is not base64"))?;
    if bytes.is_empty() {
        return Err(NpmError::InvalidDocument("the tarball is empty"));
    }
    if let Some(length) = attachment.get("length").and_then(Value::as_u64)
        && length != bytes.len() as u64
    {
        return Err(NpmError::InvalidDocument(
            "the tarball is not as long as it says",
        ));
    }
    let tags = match document.value.get("dist-tags").and_then(Value::as_object) {
        Some(tags) => {
            let mut chosen = Vec::new();
            for (tag, target) in tags {
                valid_tag(tag)?;
                if target.as_str() != Some(version.as_str()) {
                    return Err(NpmError::InvalidTag(
                        "a publication only tags the version it publishes",
                    ));
                }
                chosen.push(tag.clone());
            }
            if chosen.is_empty() {
                vec![LATEST.to_string()]
            } else {
                chosen
            }
        }
        None => vec![LATEST.to_string()],
    };
    Ok(Update::Publish(Box::new(NewVersion {
        name: expected.clone(),
        version: version.clone(),
        manifest: manifest.clone(),
        tags,
        attachment: Attachment {
            file: file.clone(),
            bytes,
        },
        document,
    })))
}

/// What the service knows about the tarball when publishing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TarballFacts {
    /// `sha1` in hexadecimal: npm's historical `shasum`.
    pub shasum: String,
    /// `sha512-{base64}`: npm's `integrity`.
    pub integrity: String,
}

/// Checks what the client declares about its tarball against what was computed.
pub fn check_declared_dist(
    manifest: &Map<String, Value>,
    facts: &TarballFacts,
) -> Result<(), NpmError> {
    let Some(dist) = manifest.get("dist").and_then(Value::as_object) else {
        return Ok(());
    };
    if let Some(shasum) = dist.get("shasum").and_then(Value::as_str)
        && !shasum.eq_ignore_ascii_case(&facts.shasum)
    {
        return Err(NpmError::InvalidDocument(
            "the shasum does not match the tarball",
        ));
    }
    if let Some(integrity) = dist.get("integrity").and_then(Value::as_str)
        && integrity != facts.integrity
    {
        return Err(NpmError::InvalidDocument(
            "the integrity does not match the tarball",
        ));
    }
    Ok(())
}

/// Who published, when, and with what correlation: the origin of the version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The verified subject that publishes.
    pub publisher: String,
    /// The instant, in RFC 3339.
    pub at: String,
    /// The request correlation.
    pub correlation: String,
}

fn versions_of(packument: &mut Map<String, Value>) -> &mut Map<String, Value> {
    object_entry(packument, "versions")
}

fn object_entry<'a>(
    packument: &'a mut Map<String, Value>,
    key: &str,
) -> &'a mut Map<String, Value> {
    let slot = packument
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !slot.is_object() {
        *slot = Value::Object(Map::new());
    }
    slot.as_object_mut().expect("an object")
}

/// The packument of a package that does not yet have versions.
pub fn empty_packument(name: &PackageName, now: &str) -> Map<String, Value> {
    let mut packument = Map::new();
    packument.insert("_id".into(), Value::String(name.full().to_string()));
    packument.insert("name".into(), Value::String(name.full().to_string()));
    packument.insert("dist-tags".into(), Value::Object(Map::new()));
    packument.insert("versions".into(), Value::Object(Map::new()));
    let mut time = Map::new();
    time.insert("created".into(), Value::String(now.to_string()));
    packument.insert("time".into(), Value::Object(time));
    packument
}

/// Reads the stored packument.
pub fn parse_packument(bytes: &[u8]) -> Result<Map<String, Value>, NpmError> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(object)) => Ok(object),
        _ => Err(NpmError::InvalidDocument(
            "the packument is not a JSON object",
        )),
    }
}

/// Whether the packument already has the version.
pub fn has_version(packument: &Map<String, Value>, version: &str) -> bool {
    packument
        .get("versions")
        .and_then(Value::as_object)
        .is_some_and(|versions| versions.contains_key(version))
}

/// Adds a published version to the packument.
///
/// A version is **not rewritten**: if it already exists, it is an error even if the
/// content is the same, which is what npm expects from a registry.
pub fn add_version(
    packument: &mut Map<String, Value>,
    new: &NewVersion,
    facts: &TarballFacts,
    origin: &Origin,
) -> Result<(), NpmError> {
    if has_version(packument, &new.version) {
        return Err(NpmError::VersionExists(new.version.clone()));
    }
    let mut manifest = new.manifest.clone();
    let mut dist = Map::new();
    dist.insert("shasum".into(), Value::String(facts.shasum.clone()));
    dist.insert("integrity".into(), Value::String(facts.integrity.clone()));
    // The absolute URL depends on who asks: it is set when serving.
    dist.insert(
        "tarball".into(),
        Value::String(new.name.tarball_path(&new.version)),
    );
    manifest.insert("dist".into(), Value::Object(dist));
    manifest.insert(
        "_id".into(),
        Value::String(format!("{}@{}", new.name.full(), new.version)),
    );
    // The origin is set by the service and not the client: what the client
    // says about itself is not a provenance.
    let mut hodei = Map::new();
    hodei.insert(
        "publishedBy".into(),
        Value::String(origin.publisher.clone()),
    );
    hodei.insert("publishedAt".into(), Value::String(origin.at.clone()));
    hodei.insert(
        "correlation".into(),
        Value::String(origin.correlation.clone()),
    );
    manifest.insert("_hodei".into(), Value::Object(hodei));
    versions_of(packument).insert(new.version.clone(), Value::Object(manifest));

    let tags = object_entry(packument, "dist-tags");
    for tag in &new.tags {
        tags.insert(tag.clone(), Value::String(new.version.clone()));
    }
    // The package description is that of whatever was last called `latest`.
    if new.tags.iter().any(|tag| tag == LATEST) {
        for field in [
            "description",
            "readme",
            "keywords",
            "license",
            "homepage",
            "repository",
            "bugs",
            "author",
        ] {
            if let Some(value) = new.document.value.get(field) {
                packument.insert(field.to_string(), value.clone());
            }
        }
    }
    let time = object_entry(packument, "time");
    time.insert("modified".into(), Value::String(origin.at.clone()));
    time.insert(new.version.clone(), Value::String(origin.at.clone()));
    Ok(())
}

/// Moves (or creates) a dist-tag.
///
/// The target must be a version that exists: a tag pointing at
/// nothing breaks `npm install pkg@tag`.
pub fn set_dist_tag(
    packument: &mut Map<String, Value>,
    tag: &str,
    version: &str,
    at: &str,
) -> Result<bool, NpmError> {
    valid_tag(tag)?;
    if !has_version(packument, version) {
        return Err(NpmError::UnknownVersion(version.to_string()));
    }
    let tags = object_entry(packument, "dist-tags");
    if tags.get(tag).and_then(Value::as_str) == Some(version) {
        return Ok(false);
    }
    tags.insert(tag.to_string(), Value::String(version.to_string()));
    object_entry(packument, "time").insert("modified".into(), Value::String(at.to_string()));
    Ok(true)
}

/// Removes a dist-tag. `latest` is not removed: it is what
/// `npm install pkg` resolves.
pub fn remove_dist_tag(
    packument: &mut Map<String, Value>,
    tag: &str,
    at: &str,
) -> Result<bool, NpmError> {
    valid_tag(tag)?;
    if tag == LATEST {
        return Err(NpmError::InvalidTag(
            "the latest dist-tag cannot be removed",
        ));
    }
    let tags = object_entry(packument, "dist-tags");
    if tags.remove(tag).is_none() {
        return Ok(false);
    }
    object_entry(packument, "time").insert("modified".into(), Value::String(at.to_string()));
    Ok(true)
}

/// Applies to a packument what a document without tarball says.
///
/// Only changing `deprecated` of versions that already exist is admitted. Removing
/// a version from the document is an unpublish, and adding one without a tarball has
/// no bytes to serve: both are rejected.
pub fn apply_metadata(
    packument: &mut Map<String, Value>,
    document: &Document,
    at: &str,
) -> Result<bool, NpmError> {
    let incoming = document
        .value
        .get("versions")
        .and_then(Value::as_object)
        .ok_or(NpmError::InvalidDocument("the document has no versions"))?;
    let known: BTreeSet<String> = packument
        .get("versions")
        .and_then(Value::as_object)
        .map(|versions| versions.keys().cloned().collect())
        .unwrap_or_default();
    if known.iter().any(|version| !incoming.contains_key(version)) {
        return Err(NpmError::Unpublish);
    }
    if incoming.keys().any(|version| !known.contains(version)) {
        return Err(NpmError::InvalidDocument(
            "a version cannot be added without its tarball",
        ));
    }
    let mut changed = false;
    for (version, entry) in incoming {
        let message = entry
            .get("deprecated")
            .and_then(Value::as_str)
            .unwrap_or("");
        let current = packument
            .get("versions")
            .and_then(Value::as_object)
            .and_then(|versions| versions.get(version))
            .and_then(|entry| entry.get("deprecated"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if message == current {
            continue;
        }
        if message.len() > 4096 {
            return Err(NpmError::InvalidDocument(
                "the deprecation message is too long",
            ));
        }
        let Some(Value::Object(target)) = versions_of(packument).get_mut(version) else {
            continue;
        };
        if message.is_empty() {
            target.remove("deprecated");
        } else {
            target.insert("deprecated".into(), Value::String(message.to_string()));
        }
        changed = true;
    }
    if changed {
        object_entry(packument, "time").insert("modified".into(), Value::String(at.to_string()));
    }
    Ok(changed)
}

/// The packument as it is served: each `dist.tarball` points at **this**
/// repository, and not at the upstream or the relative path that is stored.
///
/// `base` is `{scheme}://{host}/npm/{repository}` without trailing slash and `name`
/// the name the client requested: never the document's, which in a proxy is
/// written by the upstream and cannot be trusted to build a path.
pub fn rewrite_tarballs(packument: &mut Map<String, Value>, name: &str, base: &str) {
    let Some(versions) = packument.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for entry in versions.values_mut() {
        let Some(dist) = entry.get_mut("dist").and_then(Value::as_object_mut) else {
            continue;
        };
        let file = dist
            .get("tarball")
            .and_then(Value::as_str)
            .and_then(|url| url.rsplit('/').next())
            .filter(|file| tarball_file_ok(file));
        if let Some(file) = file {
            let url = format!("{base}/{name}/-/{file}");
            dist.insert("tarball".into(), Value::String(url));
        }
    }
}

/// The dist-tags of a packument.
pub fn dist_tags(packument: &Map<String, Value>) -> Value {
    packument
        .get("dist-tags")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()))
}

/// Whether an asset's path is a tarball's: the immutable content of
/// an npm repository. Everything else is a packument, which is republished.
pub fn is_tarball_path(asset_path: &str) -> bool {
    let mut segments = asset_path.rsplit('/');
    let (Some(file), Some(dash)) = (segments.next(), segments.next()) else {
        return false;
    };
    dash == "-" && file.ends_with(".tgz")
}

/// The configuration of an npm repository.
///
/// It lives under the `npm` key of the repository settings:
///
/// ```json
/// { "npm": { "scopes": ["@acme"] } }
/// ```
///
/// Without `scopes`, a hosted repository admits any package. With `scopes`, only those
/// hanging from one of those scopes: it is the npm way of separating who
/// can publish what (one registry and one token per scope).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpmConfig {
    /// The scopes the repository admits, with their at sign.
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl NpmConfig {
    /// The configuration of some repository settings.
    ///
    /// Without an `npm` key, the default one; a key that is not understood is an
    /// error, never the default.
    pub fn of_settings(settings: &Value) -> Result<Self, String> {
        let Some(value) = settings.get("npm") else {
            return Ok(Self::default());
        };
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|error| format!("the npm settings cannot be read: {error}"))?;
        for scope in &config.scopes {
            let body = scope.strip_prefix('@').unwrap_or("");
            let valid = !body.is_empty()
                && body.len() <= MAX_NAME_LEN
                && body.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'-' | b'.' | b'_' | b'~')
                });
            if !valid {
                return Err(format!(
                    "the npm scope {scope:?} is not a valid scope (use @name)"
                ));
            }
        }
        Ok(config)
    }

    /// Whether the repository admits publishing that package.
    pub fn admits(&self, name: &PackageName) -> bool {
        if self.scopes.is_empty() {
            return true;
        }
        match name.scope() {
            Some(scope) => self
                .scopes
                .iter()
                .any(|allowed| allowed.trim_start_matches('@') == scope.trim_start_matches('@')),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn name(text: &str) -> PackageName {
        PackageName::parse(text, true).expect("a valid name")
    }

    fn b64(text: &str) -> Option<Vec<u8>> {
        // A test codec: the service injects the real one.
        Some(text.as_bytes().to_vec())
    }

    #[test]
    fn names_follow_the_npm_rules() {
        assert_eq!(name("lodash").full(), "lodash");
        let scoped = name("@acme/widget");
        assert_eq!(scoped.scope(), Some("@acme"));
        assert_eq!(scoped.base(), "widget");
        assert_eq!(
            scoped.tarball_path("1.0.0"),
            "@acme/widget/-/widget-1.0.0.tgz"
        );
        for bad in [
            "",
            "UPPER",
            ".hidden",
            "_hidden",
            "has space",
            "a/b",
            "@acme",
            "@/x",
            "@acme/",
            "node_modules",
            "a%20b",
        ] {
            assert!(
                PackageName::parse(bad, true).is_err(),
                "{bad:?} must be refused"
            );
        }
        // A proxy admits what the upstream already published: old names.
        assert!(PackageName::parse("JSONStream", false).is_ok());
        assert!(PackageName::parse(&"a".repeat(215), false).is_err());
    }

    #[test]
    fn routes_read_every_form_the_npm_cli_sends() {
        assert_eq!(route("-/ping").unwrap(), Route::Ping);
        assert_eq!(route("-/whoami").unwrap(), Route::Whoami);
        let Route::Packument { name } = route("@acme%2fwidget").unwrap() else {
            panic!("a packument");
        };
        assert_eq!(name.full(), "@acme/widget");
        let Route::Packument { name } = route("@acme/widget").unwrap() else {
            panic!("a packument");
        };
        assert_eq!(name.full(), "@acme/widget");
        let Route::Tarball { name, file } = route("@acme/widget/-/widget-1.0.0.tgz").unwrap()
        else {
            panic!("a tarball");
        };
        assert_eq!(
            (name.full(), file.as_str()),
            ("@acme/widget", "widget-1.0.0.tgz")
        );
        assert!(matches!(
            route("-/package/@acme%2fwidget/dist-tags").unwrap(),
            Route::DistTags { .. }
        ));
        let Route::DistTag { tag, .. } = route("-/package/lodash/dist-tags/beta").unwrap() else {
            panic!("a dist-tag");
        };
        assert_eq!(tag, "beta");
        assert!(matches!(
            route("lodash/-rev/3-abc").unwrap(),
            Route::UnpublishPackage { .. }
        ));
        assert!(matches!(
            route("lodash/-/lodash-1.0.0.tgz/-rev/3-abc").unwrap(),
            Route::UnpublishTarball { .. }
        ));
    }

    #[test]
    fn routes_refuse_traversal_escapes_and_unknown_endpoints() {
        for bad in [
            "",
            "../x",
            "a/../b",
            "a//b",
            "lodash/%2e%2e",
            "lodash%00",
            "lodash\\x",
            "lodash/-/../x.tgz",
            "lodash/-/x.txt",
            "-/user/org.couchdb.user:x",
            "-/package/lodash/dist-tags/1.0.0",
            "lodash/other",
        ] {
            assert!(route(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn dist_tags_cannot_look_like_versions() {
        assert!(valid_tag("beta").is_ok());
        assert!(valid_tag("next-2").is_ok());
        for bad in ["", "1.0.0", "^1.0", "has space", "a/b", &"x".repeat(65)] {
            assert!(valid_tag(bad).is_err(), "{bad:?}");
        }
    }

    fn publish_doc(version: &str, tarball: &str) -> Vec<u8> {
        let file = format!("widget-{version}.tgz");
        serde_json::to_vec(&json!({
            "_id": "widget",
            "name": "widget",
            "description": "a widget",
            "dist-tags": {"latest": version},
            "versions": {version: {"name": "widget", "version": version, "description": "a widget"}},
            "_attachments": {file: {"content_type": "application/octet-stream", "data": tarball, "length": tarball.len()}},
        }))
        .unwrap()
    }

    fn facts() -> TarballFacts {
        TarballFacts {
            shasum: "ab".repeat(20),
            integrity: "sha512-AAAA".into(),
        }
    }

    fn origin() -> Origin {
        Origin {
            publisher: "alice".into(),
            at: "2026-01-01T00:00:00Z".into(),
            correlation: "c-1".into(),
        }
    }

    #[test]
    fn a_publication_needs_one_version_one_tarball_and_matching_names() {
        let widget = name("widget");
        let Update::Publish(new) =
            parse_update(&widget, &publish_doc("1.0.0", "tarball"), &b64).unwrap()
        else {
            panic!("a publication");
        };
        assert_eq!(new.version, "1.0.0");
        assert_eq!(new.tags, vec!["latest".to_string()]);
        assert_eq!(new.attachment.bytes, b"tarball");

        // A package other than the route's.
        assert!(parse_update(&name("other"), &publish_doc("1.0.0", "t"), &b64).is_err());
        // An attachment that is not named like the version.
        let mut doc: Value = serde_json::from_slice(&publish_doc("1.0.0", "t")).unwrap();
        doc["_attachments"] = json!({"widget-9.9.9.tgz": {"data": "t"}});
        assert!(parse_update(&widget, &serde_json::to_vec(&doc).unwrap(), &b64).is_err());
        // A version that is not semver.
        assert!(parse_update(&widget, &publish_doc("latest", "t"), &b64).is_err());
        // A tag pointing at another version.
        let mut doc: Value = serde_json::from_slice(&publish_doc("1.0.0", "t")).unwrap();
        doc["dist-tags"] = json!({"latest": "0.0.1"});
        assert!(parse_update(&widget, &serde_json::to_vec(&doc).unwrap(), &b64).is_err());
        // An empty attachment and a body that is not JSON.
        assert!(parse_update(&widget, &publish_doc("1.0.0", ""), &b64).is_err());
        assert!(parse_update(&widget, b"not json", &b64).is_err());
    }

    #[test]
    fn a_scoped_publication_names_its_tarball_as_libnpmpublish_does() {
        let scoped = name("@acme/widget");
        let document = |file: &str| {
            serde_json::to_vec(&json!({
                "name": "@acme/widget",
                "versions": {"1.0.0": {"name": "@acme/widget", "version": "1.0.0"}},
                "_attachments": {file: {"data": "tarball"}},
            }))
            .unwrap()
        };
        // The real client sends the full name; the registry stores the base.
        for file in ["@acme/widget-1.0.0.tgz", "widget-1.0.0.tgz"] {
            let Update::Publish(new) = parse_update(&scoped, &document(file), &b64).unwrap() else {
                panic!("a publication");
            };
            assert_eq!(new.attachment.file, "widget-1.0.0.tgz");
        }
        assert!(parse_update(&scoped, &document("@acme/widget-9.9.9.tgz"), &b64).is_err());
    }

    #[test]
    fn the_declared_integrity_has_to_be_the_computed_one() {
        let mut manifest = Map::new();
        assert!(check_declared_dist(&manifest, &facts()).is_ok());
        manifest.insert(
            "dist".into(),
            json!({"shasum": "ab".repeat(20), "integrity": "sha512-AAAA"}),
        );
        assert!(check_declared_dist(&manifest, &facts()).is_ok());
        manifest.insert("dist".into(), json!({"integrity": "sha512-BBBB"}));
        assert!(check_declared_dist(&manifest, &facts()).is_err());
        manifest.insert("dist".into(), json!({"shasum": "00"}));
        assert!(check_declared_dist(&manifest, &facts()).is_err());
    }

    fn published(version: &str) -> (Map<String, Value>, NewVersion) {
        let widget = name("widget");
        let Update::Publish(new) =
            parse_update(&widget, &publish_doc(version, "tarball"), &b64).unwrap()
        else {
            panic!("a publication");
        };
        (empty_packument(&widget, "2026-01-01T00:00:00Z"), *new)
    }

    #[test]
    fn publishing_adds_the_version_with_our_integrity_and_origin() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        let entry = &packument["versions"]["1.0.0"];
        assert_eq!(entry["dist"]["integrity"], "sha512-AAAA");
        assert_eq!(entry["dist"]["tarball"], "widget/-/widget-1.0.0.tgz");
        assert_eq!(entry["_hodei"]["publishedBy"], "alice");
        assert_eq!(packument["dist-tags"]["latest"], "1.0.0");
        assert_eq!(packument["description"], "a widget");
        assert_eq!(packument["time"]["1.0.0"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn a_published_version_is_never_overwritten() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        let again = add_version(&mut packument, &new, &facts(), &origin());
        assert_eq!(again, Err(NpmError::VersionExists("1.0.0".into())));
        assert!(again.unwrap_err().message().contains("cannot publish over"));
    }

    #[test]
    fn dist_tags_move_only_to_versions_that_exist_and_latest_stays() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        assert!(set_dist_tag(&mut packument, "beta", "1.0.0", "t").unwrap());
        assert!(!set_dist_tag(&mut packument, "beta", "1.0.0", "t").unwrap());
        assert_eq!(
            set_dist_tag(&mut packument, "beta", "9.9.9", "t"),
            Err(NpmError::UnknownVersion("9.9.9".into()))
        );
        assert!(set_dist_tag(&mut packument, "1.0.0", "1.0.0", "t").is_err());
        assert!(remove_dist_tag(&mut packument, "beta", "t").unwrap());
        assert!(!remove_dist_tag(&mut packument, "beta", "t").unwrap());
        assert!(remove_dist_tag(&mut packument, "latest", "t").is_err());
    }

    fn metadata(versions: Value) -> Document {
        Document {
            name: name("widget"),
            value: json!({"name": "widget", "versions": versions})
                .as_object()
                .unwrap()
                .clone(),
        }
    }

    #[test]
    fn deprecating_sets_and_clears_a_message_without_touching_the_rest() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        let deprecate = metadata(json!({"1.0.0": {"deprecated": "use 2.x"}}));
        assert!(apply_metadata(&mut packument, &deprecate, "t").unwrap());
        assert_eq!(packument["versions"]["1.0.0"]["deprecated"], "use 2.x");
        assert_eq!(
            packument["versions"]["1.0.0"]["dist"]["integrity"],
            "sha512-AAAA"
        );
        assert!(!apply_metadata(&mut packument, &deprecate, "t").unwrap());
        let clear = metadata(json!({"1.0.0": {"deprecated": ""}}));
        assert!(apply_metadata(&mut packument, &clear, "t").unwrap());
        assert!(packument["versions"]["1.0.0"].get("deprecated").is_none());
    }

    #[test]
    fn a_document_that_drops_a_version_is_an_unpublish_and_is_refused() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        assert_eq!(
            apply_metadata(&mut packument, &metadata(json!({})), "t"),
            Err(NpmError::Unpublish)
        );
        // Adding a version without a tarball is not admitted either.
        let extra = metadata(json!({"1.0.0": {}, "2.0.0": {}}));
        assert!(matches!(
            apply_metadata(&mut packument, &extra, "t"),
            Err(NpmError::InvalidDocument(_))
        ));
        assert!(has_version(&packument, "1.0.0"));
    }

    #[test]
    fn serving_points_every_tarball_at_this_repository() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        rewrite_tarballs(
            &mut packument,
            "widget",
            "http://127.0.0.1:8080/npm/npm-all",
        );
        assert_eq!(
            packument["versions"]["1.0.0"]["dist"]["tarball"],
            "http://127.0.0.1:8080/npm/npm-all/widget/-/widget-1.0.0.tgz"
        );
        // A packument from an upstream: its URL is replaced by ours.
        let mut upstream = json!({
            "name": "@acme/lib",
            "versions": {"2.0.0": {"dist": {"tarball": "https://registry.npmjs.org/@acme/lib/-/lib-2.0.0.tgz"}}}
        })
        .as_object()
        .unwrap()
        .clone();
        rewrite_tarballs(&mut upstream, "@acme/lib", "http://hodei/npm/proxy");
        // The document's name does not decide the route.
        let mut hostile = json!({
            "name": "a/../../otro",
            "versions": {"2.0.0": {"dist": {"tarball": "https://x/y/-/lib-2.0.0.tgz"}}}
        })
        .as_object()
        .unwrap()
        .clone();
        rewrite_tarballs(&mut hostile, "@acme/lib", "http://hodei/npm/proxy");
        assert_eq!(
            hostile["versions"]["2.0.0"]["dist"]["tarball"],
            "http://hodei/npm/proxy/@acme/lib/-/lib-2.0.0.tgz"
        );
        assert_eq!(
            upstream["versions"]["2.0.0"]["dist"]["tarball"],
            "http://hodei/npm/proxy/@acme/lib/-/lib-2.0.0.tgz"
        );
    }

    #[test]
    fn only_tarballs_are_immutable_content() {
        assert!(is_tarball_path("lodash/-/lodash-4.17.21.tgz"));
        assert!(is_tarball_path("@acme/lib/-/lib-1.0.0.tgz"));
        assert!(!is_tarball_path("lodash"));
        assert!(!is_tarball_path("@acme/lib"));
        assert!(!is_tarball_path("lodash/-/lodash-4.17.21.json"));
    }

    #[test]
    fn the_scope_policy_restricts_what_a_repository_admits() {
        use serde_json::json;
        let open = NpmConfig::of_settings(&json!({})).unwrap();
        let scoped = NpmConfig::of_settings(&json!({"npm": {"scopes": ["@acme"]}})).unwrap();
        let acme = PackageName::parse("@acme/tool", true).unwrap();
        let other = PackageName::parse("@other/tool", true).unwrap();
        let plain = PackageName::parse("left-pad", true).unwrap();
        assert!(open.admits(&plain) && open.admits(&other));
        assert!(scoped.admits(&acme));
        assert!(!scoped.admits(&other));
        assert!(!scoped.admits(&plain));
    }

    #[test]
    fn a_malformed_npm_setting_is_an_error_not_the_default() {
        use serde_json::json;
        for settings in [
            json!({"npm": {"scopes": ["acme"]}}),
            json!({"npm": {"scopes": ["@"]}}),
            json!({"npm": {"surprise": true}}),
            json!({"npm": 3}),
        ] {
            assert!(NpmConfig::of_settings(&settings).is_err(), "{settings}");
        }
    }
}
