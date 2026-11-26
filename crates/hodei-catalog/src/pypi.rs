//! The pure part of the PyPI format: normalized names (PEP 503),
//! distributions, a project's document and the two ways of
//! publishing it (the PEP 503 HTML and the PEP 691 JSON).
//!
//! Nothing here touches disk, network or catalog: these are functions over strings and
//! JSON, and that is why they are tested without infrastructure.
//!
//! # A project's document
//!
//! A project's index is what the client sees: the files that exist,
//! with their hash, their size, the Python version they need and whether they are
//! marked as *yanked*. It is a **document**, not a view of the catalog:
//! it is written when published and rewritten when a version
//! is withdrawn. The distribution bytes live separately, at their own
//! path, and are not touched when the document is rewritten.
//!
//! # The two formats
//!
//! - **HTML** (PEP 503): a page with one link per file. The hash goes
//!   in the fragment (`#sha256=...`) and the Python version in
//!   `data-requires-python`.
//! - **JSON** (PEP 691): a document with `meta`, `name` and `files`. The hash
//!   goes in `hashes` and the Python version in `requires-python`.
//!
//! Both say the same thing: a file that appears in the HTML appears in the
//! JSON with the same hash and the same *yanked* state. What is negotiated is
//! the representation, not the content.
//!
//! # The HTML is ours, not the upstream's
//!
//! An index that comes from another one is repainted here: the HTML that comes out
//! of this service is written by this page, with the values escaped. An
//! upstream may publish whatever it wants in its HTML; what the client
//! receives from here it cannot inject.

use serde_json::{Map, Value};

/// The suffix of a project document's coordinates.
///
/// The `#` cannot appear in a project name, so the
/// document's coordinates never collide with those of a distribution.
pub const PROJECT_COORDINATES_SUFFIX: &str = "#project";

/// The Simple API content type in JSON (PEP 691).
pub const JSON_CONTENT_TYPE: &str = "application/vnd.pypi.simple.v1+json";

/// The Simple API content type in HTML (PEP 503).
pub const HTML_CONTENT_TYPE: &str = "application/vnd.pypi.simple.v1+html";

/// The Simple API content type in JSON, in its *latest* version.
///
/// PEP 691 defines it as the same representation as version `1.0`,
/// and a client may request it instead of the other.
pub const LATEST_JSON_CONTENT_TYPE: &str = "application/vnd.pypi.simple.latest+json";

/// The Simple API version that is advertised.
pub const API_VERSION: &str = "1.0";

/// The maximum length of a project name.
pub const MAX_PROJECT_LEN: usize = 255;

/// Why a name, a file, a version or a document is not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PypiError {
    /// The project name does not meet the PEP 503 rules.
    #[error("the project name is not valid: {0}")]
    InvalidName(String),
    /// The file name is not that of a distribution.
    #[error("the file name is not a valid distribution: {0}")]
    InvalidFileName(String),
    /// The version is not a PEP 440 version.
    #[error("the version is not valid: {0}")]
    InvalidVersion(String),
    /// The upload is missing a required field.
    #[error("the upload is missing the field {0}")]
    MissingField(&'static str),
    /// The upload is missing the file.
    #[error("the upload does not carry the distribution")]
    MissingFile,
    /// What the upload declares does not match the file.
    #[error("the upload does not match the distribution: {0}")]
    Mismatch(String),
    /// The declared hash is not that of the received bytes.
    #[error("the digest of the upload does not match its content")]
    DigestMismatch,
    /// The document is not understood.
    #[error("the project document is not valid: {0}")]
    InvalidDocument(String),
    /// The file is already in the project.
    #[error("the distribution is already published in this project")]
    FileExists,
    /// The project does not have that file.
    #[error("the project does not have that distribution")]
    UnknownFile(String),
    /// The path is none of those the protocol speaks.
    #[error("the path is not part of the PyPI surface: {0}")]
    InvalidPath(String),
}

impl PypiError {
    /// The reason, as it is told to the client.
    pub fn message(&self) -> String {
        self.to_string()
    }
}

/// Normalizes a project name (PEP 503).
///
/// Uppercase stops mattering and any run of `-`, `_` or `.`
/// is a single `-`: `Flask`, `flask`, `flask_login` and `flask.login` are the
/// same project.
pub fn normalize(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    let mut separator = false;
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            // A run is a separator, and a single one.
            if !separator {
                normalized.push('-');
            }
            separator = true;
            continue;
        }
        separator = false;
        normalized.extend(character.to_lowercase());
    }
    normalized
}

/// An already normalized project name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectName(String);

impl std::fmt::Display for ProjectName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl ProjectName {
    /// Reads a name and normalizes it.
    ///
    /// The normalized name is the one used in the index path; the
    /// client may request whichever it wants, because they are all the same.
    pub fn parse(raw: &str) -> Result<Self, PypiError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(PypiError::InvalidName("the name is empty".into()));
        }
        if trimmed.len() > MAX_PROJECT_LEN {
            return Err(PypiError::InvalidName("the name is too long".into()));
        }
        if !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(PypiError::InvalidName(
                "the name has characters that PyPI does not allow".into(),
            ));
        }
        let normalized = normalize(trimmed);
        // A distribution name starts and ends with an alphanumeric:
        // that is what the canonical metadata name says.
        if normalized.is_empty()
            || !normalized
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric())
            || !normalized
                .chars()
                .last()
                .is_some_and(|character| character.is_ascii_alphanumeric())
        {
            return Err(PypiError::InvalidName(
                "the name is empty once normalized".into(),
            ));
        }
        Ok(Self(normalized))
    }

    /// The normalized name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What kind of distribution it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionKind {
    /// A *wheel*: a zip directory with the Python version in the name.
    Wheel,
    /// An *sdist*: the tarball or zip with the source code.
    Sdist,
}

/// A project's distribution, with its file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distribution {
    /// The file name, as it was published.
    pub filename: String,
    /// The project it belongs to.
    pub project: ProjectName,
    /// The version it declares.
    pub version: String,
    /// What kind it is.
    pub kind: DistributionKind,
}

impl Distribution {
    /// Reads the name of a distribution file.
    ///
    /// A wheel's name is
    /// `{distribucion}-{version}(-{build})?-{python}-{abi}-{plataforma}.whl`
    /// and an sdist's `{nombre}-{version}.tar.gz` (or `.zip`).
    ///
    /// The parts have to be what they are: a name `pip` does not
    /// accept is not advertised, because a link the client cannot
    /// install is a broken link.
    pub fn parse(filename: &str) -> Result<Self, PypiError> {
        if filename.is_empty() || filename.len() > 255 {
            return Err(PypiError::InvalidFileName(filename.to_string()));
        }
        if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
            return Err(PypiError::InvalidFileName(filename.to_string()));
        }
        if let Some(stem) = filename.strip_suffix(".whl") {
            // Not one part more or less: a wheel name has
            // five parts, or six with a build tag.
            let parts: Vec<&str> = stem.split('-').collect();
            let (version_text, tags) = match parts.len() {
                5 => (&parts[1], &parts[2..]),
                6 => {
                    if !valid_build(parts[2]) {
                        return Err(PypiError::InvalidFileName(filename.to_string()));
                    }
                    (&parts[1], &parts[3..])
                }
                _ => return Err(PypiError::InvalidFileName(filename.to_string())),
            };
            // The three tags: Python version, ABI and platform.
            if tags.len() != 3 || !tags.iter().all(|tag| valid_tag(tag)) {
                return Err(PypiError::InvalidFileName(filename.to_string()));
            }
            let version = Version::parse(version_text)
                .ok_or_else(|| PypiError::InvalidVersion(version_text.to_string()))?;
            let project = ProjectName::parse(parts[0])?;
            return Ok(Self {
                filename: filename.to_string(),
                project,
                version: version.canonical(),
                kind: DistributionKind::Wheel,
            });
        }
        for suffix in [".tar.gz", ".zip"] {
            let Some(stem) = filename.strip_suffix(suffix) else {
                continue;
            };
            let Some((name, version_text)) = stem.rsplit_once('-') else {
                return Err(PypiError::InvalidFileName(filename.to_string()));
            };
            let version = Version::parse(version_text)
                .ok_or_else(|| PypiError::InvalidVersion(version_text.to_string()))?;
            let project = ProjectName::parse(name)?;
            return Ok(Self {
                filename: filename.to_string(),
                project,
                version: version.canonical(),
                kind: DistributionKind::Sdist,
            });
        }
        Err(PypiError::InvalidFileName(filename.to_string()))
    }
}

/// A wheel's build tag: a number followed by whatever
/// comes after.
///
/// PEP 427 says it has to start with a digit, and `pip` does too:
/// its rule is a digit followed by anything that is not a hyphen.
/// That is why `1.5` and `1abc` are valid build tags.
fn valid_build(build: &str) -> bool {
    let Some(first) = build.as_bytes().first() else {
        return false;
    };
    first.is_ascii_digit() && !build.contains('-')
}

/// A wheel tag: Python version, ABI or platform.
///
/// Each tag is a set of single tags separated by a
/// dot: `py2.py3`, `cp39.cp310`, `manylinux_2_17_x86_64`.
///
/// What is accepted is what `pip` accepts: the tag is a list of
/// single tags separated by dots, and `pip` does not look at the
/// characters of each one. This surface does not look at them either: what is
/// advertised has to be installable, not merely resemble what is.
fn valid_tag(tag: &str) -> bool {
    !tag.is_empty() && !tag.contains('-')
}

/// The kind of a *pre release*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PreKind {
    /// An alpha.
    Alpha,
    /// A beta.
    Beta,
    /// A *release candidate*.
    Rc,
}

impl PreKind {
    /// The letter it is written with.
    fn letter(self) -> &'static str {
        match self {
            Self::Alpha => "a",
            Self::Beta => "b",
            Self::Rc => "rc",
        }
    }
}

/// A PEP 440 version.
///
/// The PEP defines the grammar and the **canonical** form. What is stored and
/// what is compared is the canonical form, because a client may write
/// the version however it likes (`1.0-2`, `1.0.rc1`, `v1.0`) and the file name
/// carries it canonical (`1.0.post2`, `1.0rc1`, `1.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// The epoch.
    epoch: u64,
    /// The release: its numbers.
    release: Vec<u64>,
    /// The pre release: its kind and number.
    pre: Option<(PreKind, u64)>,
    /// The post release.
    post: Option<u64>,
    /// The *development release*.
    dev: Option<u64>,
    /// The local version, already normalized.
    local: Option<String>,
}

impl Version {
    /// Reads a PEP 440 version.
    ///
    /// `None` if what is there is not a PEP 440 version.
    pub fn parse(raw: &str) -> Option<Self> {
        let text = raw.trim();
        if text.is_empty() {
            return None;
        }
        // The `v` prefix is from the PEP: `v1.0` is `1.0`.
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        // The PEP grammar is case-insensitive: `1.0RC1` is
        // `1.0rc1`. Lowercase is also the canonical form, so
        // it is normalized once and we always work in lowercase.
        let lowered = text.to_ascii_lowercase();
        // The local version goes last and is separated by a `+`.
        let (text, local) = match lowered.split_once('+') {
            Some((head, local)) => (head, Some(normalize_local(local)?)),
            None => (lowered.as_str(), None),
        };
        // The epoch: `1!2.0`.
        let (epoch, text) = match text.split_once('!') {
            Some((epoch, tail)) => {
                if epoch.is_empty() || !epoch.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                (epoch.parse().ok()?, tail)
            }
            None => (0, text),
        };
        // The release: `1`, `1.0`, `1.0.0`.
        let end = text
            .find(|character: char| !character.is_ascii_digit() && character != '.')
            .unwrap_or(text.len());
        let (mut release_text, mut rest) = text.split_at(end);
        // The dot that separates the release from its suffix (`1.0.dev1`) is not
        // part of the release: it is the suffix separator.
        if let Some(stripped) = release_text.strip_suffix('.') {
            release_text = stripped;
            rest = &text[end - 1..];
        }
        let mut release = Vec::new();
        for part in release_text.split('.') {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            release.push(part.parse().ok()?);
        }
        // The suffixes go in this order and each one once: a *pre
        // release*, a *post release* and a *development release*.
        let mut pre = None;
        let mut post = None;
        let mut dev = None;
        if let Some((kind, number, tail)) = take_marker(rest, PRE_MARKERS) {
            pre = Some((kind, number));
            rest = tail;
        }
        if let Some((number, tail)) = take_implicit_post(rest) {
            post = Some(number);
            rest = tail;
        } else if let Some((number, tail)) = take_named_marker(rest, POST_MARKERS) {
            post = Some(number);
            rest = tail;
        }
        if let Some((number, tail)) = take_named_marker(rest, DEV_MARKERS) {
            dev = Some(number);
            rest = tail;
        }
        if !rest.is_empty() {
            return None;
        }
        Some(Self {
            epoch,
            release,
            pre,
            post,
            dev,
            local,
        })
    }

    /// The canonical form of the version.
    ///
    /// It is the one the file name carries: `1.0-2` is `1.0.post2`, and
    /// `1.0+Ubuntu-1` is `1.0+ubuntu.1`.
    pub fn canonical(&self) -> String {
        let mut version = String::new();
        if self.epoch != 0 {
            version.push_str(&self.epoch.to_string());
            version.push('!');
        }
        let release: Vec<String> = self.release.iter().map(u64::to_string).collect();
        version.push_str(&release.join("."));
        if let Some((kind, number)) = self.pre {
            version.push_str(kind.letter());
            version.push_str(&number.to_string());
        }
        if let Some(post) = self.post {
            version.push_str(".post");
            version.push_str(&post.to_string());
        }
        if let Some(dev) = self.dev {
            version.push_str(".dev");
            version.push_str(&dev.to_string());
        }
        if let Some(local) = &self.local {
            version.push('+');
            version.push_str(local);
        }
        version
    }
}

impl Ord for Version {
    /// The PEP 440 ordering.
    ///
    /// A *pre release* goes before its release, a *post release* goes
    /// after, a *development release* goes before both, and the
    /// local version goes after the same version without it.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        self.epoch
            .cmp(&other.epoch)
            .then_with(|| {
                let length = self.release.len().max(other.release.len());
                for index in 0..length {
                    let left = self.release.get(index).copied().unwrap_or(0);
                    let right = other.release.get(index).copied().unwrap_or(0);
                    match left.cmp(&right) {
                        Ordering::Equal => {}
                        other => return other,
                    }
                }
                Ordering::Equal
            })
            .then_with(|| match (self.pre, other.pre) {
                (None, None) => Ordering::Equal,
                // Without a pre release it is the newest.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(&right),
            })
            .then_with(|| self.post.cmp(&other.post))
            .then_with(|| match (self.dev, other.dev) {
                (None, None) => Ordering::Equal,
                // Without a development release it is the newest.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(&right),
            })
            .then_with(|| match (&self.local, &other.local) {
                (None, None) => Ordering::Equal,
                // The local version is newer than the same one without it.
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(left), Some(right)) => left.cmp(right),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The markers of a *pre release*, from longest to shortest.
///
/// Order matters: `alpha` before `a`, and `preview` before `pre`.
const PRE_MARKERS: &[(&str, PreKind)] = &[
    ("alpha", PreKind::Alpha),
    ("a", PreKind::Alpha),
    ("beta", PreKind::Beta),
    ("b", PreKind::Beta),
    ("preview", PreKind::Rc),
    ("pre", PreKind::Rc),
    ("c", PreKind::Rc),
    ("rc", PreKind::Rc),
];

/// The markers of a *post release*, from longest to shortest.
const POST_MARKERS: &[&str] = &["post", "rev", "r"];

/// The markers of a *development release*.
const DEV_MARKERS: &[&str] = &["dev"];

/// The canonical form of a version.
///
/// `None` if what is there is not a PEP 440 version.
pub fn canonical_version(raw: &str) -> Option<String> {
    Version::parse(raw).map(|version| version.canonical())
}

/// Whether a version is a PEP 440 version.
///
/// It is not the comparator (the client takes care of that), it is the filter: an
/// index with a version that does not exist is not served.
pub fn valid_version(version: &str) -> bool {
    canonical_version(version).is_some()
}

/// The local part of a version, normalized.
///
/// Segments are separated by `.`, `-` or `_` and are written in
/// lowercase: `1.0+Ubuntu-1` is `1.0+ubuntu.1`.
///
/// A numeric segment is written without leading zeros: `01` is `1` and
/// `00` is `0`. The PEP says so, and that is why `1.0+01` and `1.0+1` are the
/// same version and the same file.
fn normalize_local(local: &str) -> Option<String> {
    if local.is_empty() {
        return None;
    }
    let mut segments = Vec::new();
    for part in local.split(['.', '-', '_']) {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return None;
        }
        let segment = part.to_ascii_lowercase();
        let segment = if segment.bytes().all(|byte| byte.is_ascii_digit()) {
            let trimmed = segment.trim_start_matches('0');
            if trimmed.is_empty() {
                "0".to_string()
            } else {
                trimmed.to_string()
            }
        } else {
            segment
        };
        segments.push(segment);
    }
    Some(segments.join("."))
}

/// A number at the start of the text.
fn take_number(text: &str) -> Option<(u64, &str)> {
    let end = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    text[..end]
        .parse()
        .ok()
        .map(|number| (number, &text[end..]))
}

/// A marker's separator: `.`, `-` or `_`.
fn take_separator(text: &str) -> &str {
    match text.as_bytes().first() {
        Some(b'.' | b'-' | b'_') => &text[1..],
        _ => text,
    }
}

/// The number that follows a marker, if there is one.
///
/// Without a separator, the number has to start with a digit: `rc1` carries
/// a number and `rcx` does not.
fn take_optional_number(text: &str) -> (u64, &str) {
    let separated = take_separator(text);
    if separated.len() != text.len() {
        return match take_number(separated) {
            Some((number, tail)) => (number, tail),
            None => (0, separated),
        };
    }
    match take_number(text) {
        Some((number, tail)) => (number, tail),
        None => (0, text),
    }
}

/// A marker with its optional number.
///
/// The separator goes before the marker and is optional; the number goes after
/// an optional separator. If the marker is not there, nothing is consumed: the
/// separator belongs to whichever marker comes.
fn take_marker<'a>(
    text: &'a str,
    names: &[(&'static str, PreKind)],
) -> Option<(PreKind, u64, &'a str)> {
    let separated = take_separator(text);
    if separated.len() != text.len() {
        for (name, kind) in names {
            if let Some(tail) = separated.strip_prefix(name) {
                let (number, tail) = take_optional_number(tail);
                return Some((*kind, number, tail));
            }
        }
        return None;
    }
    for (name, kind) in names {
        if let Some(tail) = text.strip_prefix(name) {
            let (number, tail) = take_optional_number(tail);
            return Some((*kind, number, tail));
        }
    }
    None
}

/// A marker that has no kind: that of the *post release* and that of the
/// *development release*.
///
/// It is the same rule as [`take_marker`], without the kind: the *post release*
/// and the *development release* are not distinguished by their letter.
fn take_named_marker<'a>(text: &'a str, names: &[&'static str]) -> Option<(u64, &'a str)> {
    let separated = take_separator(text);
    if separated.len() != text.len() {
        for name in names {
            if let Some(tail) = separated.strip_prefix(name) {
                let (number, tail) = take_optional_number(tail);
                return Some((number, tail));
            }
        }
        return None;
    }
    for name in names {
        if let Some(tail) = text.strip_prefix(name) {
            let (number, tail) = take_optional_number(tail);
            return Some((number, tail));
        }
    }
    None
}

/// The implicit *post release*: `1.0-1`.
///
/// The hyphen is attached and so is the number: `1.0-1` is a post release and
/// `1.0-r1` is something else.
fn take_implicit_post(text: &str) -> Option<(u64, &str)> {
    let tail = text.strip_prefix('-')?;
    take_number(tail)
}

/// A project's file, as the index sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// The file name.
    pub filename: String,
    /// The SHA-256 hash, in hexadecimal.
    pub sha256: String,
    /// The size in bytes, if known.
    pub size: Option<u64>,
    /// The Python version it needs, if it declares one.
    pub requires_python: Option<String>,
    /// The reason for the *yank*, if the version is withdrawn.
    ///
    /// `None` means the version is still live; `Some("")` means it is withdrawn
    /// with no written reason.
    pub yanked: Option<String>,
    /// Where the content comes from when it is not from this service.
    ///
    /// In a proxy it is the upstream URL; in a hosted repository there is none and the
    /// content is in the catalog.
    pub source: Option<String>,
}

impl FileEntry {
    /// The entry's JSON, in the shape of the stored document.
    ///
    /// The stored document is a Simple API document (PEP 691):
    /// the hash goes in `hashes` and the file URL in `url`.
    fn to_value(&self, url: &str) -> Value {
        let mut entry = Map::new();
        entry.insert("filename".into(), Value::String(self.filename.clone()));
        entry.insert("url".into(), Value::String(url.to_string()));
        let mut hashes = Map::new();
        hashes.insert("sha256".into(), Value::String(self.sha256.clone()));
        entry.insert("hashes".into(), Value::Object(hashes));
        if let Some(size) = self.size {
            entry.insert("size".into(), Value::from(size));
        }
        if let Some(requires_python) = &self.requires_python {
            entry.insert(
                "requires-python".into(),
                Value::String(requires_python.clone()),
            );
        }
        match &self.yanked {
            Some(reason) => {
                entry.insert("yanked".into(), Value::String(reason.clone()));
            }
            None => {
                entry.insert("yanked".into(), Value::Bool(false));
            }
        }
        Value::Object(entry)
    }

    /// Reads the entry of a Simple API document.
    fn from_value(value: &Value) -> Result<Self, PypiError> {
        let object = value
            .as_object()
            .ok_or_else(|| PypiError::InvalidDocument("a file entry is not an object".into()))?;
        let filename = object
            .get("filename")
            .and_then(Value::as_str)
            .ok_or_else(|| PypiError::InvalidDocument("a file entry has no name".into()))?
            .to_string();
        let sha256 = object
            .get("hashes")
            .and_then(|hashes| hashes.get("sha256"))
            .and_then(Value::as_str)
            .ok_or_else(|| PypiError::InvalidDocument("a file entry has no hash".into()))?
            .to_ascii_lowercase();
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(PypiError::InvalidDocument(
                "the hash of a file entry is not a SHA-256".into(),
            ));
        }
        let size = object.get("size").and_then(Value::as_u64);
        let requires_python = object
            .get("requires-python")
            .and_then(Value::as_str)
            .map(str::to_string);
        let yanked = match object.get("yanked") {
            Some(Value::String(reason)) => Some(reason.clone()),
            Some(Value::Bool(true)) => Some(String::new()),
            _ => None,
        };
        // The document's URL is where the content comes from when the
        // document is not from this service.
        let source = object
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(Self {
            filename,
            sha256,
            size,
            requires_python,
            yanked,
            source,
        })
    }
}

/// A project's document: what the index knows about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDocument {
    /// The project's normalized name.
    pub name: String,
    /// The project's files.
    pub files: Vec<FileEntry>,
}

impl ProjectDocument {
    /// An empty document.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            files: Vec::new(),
        }
    }

    /// Adds a file to the document.
    ///
    /// A file with the same name is not added twice: the document
    /// describes what is there, not what was requested.
    pub fn add(&mut self, entry: FileEntry) -> Result<(), PypiError> {
        if self
            .files
            .iter()
            .any(|file| file.filename == entry.filename)
        {
            return Err(PypiError::FileExists);
        }
        self.files.push(entry);
        Ok(())
    }

    /// The file of that name, if present.
    pub fn file(&self, filename: &str) -> Option<&FileEntry> {
        self.files.iter().find(|file| file.filename == filename)
    }

    /// The project's versions, without repeats and in order.
    pub fn versions(&self) -> Vec<String> {
        let mut versions: Vec<String> = Vec::new();
        for file in &self.files {
            let Ok(distribution) = Distribution::parse(&file.filename) else {
                continue;
            };
            if !versions.contains(&distribution.version) {
                versions.push(distribution.version.clone());
            }
        }
        versions.sort_by(|left, right| compare_versions(left, right));
        versions
    }

    /// Withdraws (or brings back to life) an entire version.
    ///
    /// The state belongs to the *release*, not the file: all the files of
    /// a version stay the same, as PEP 592 mandates.
    pub fn set_yanked(&mut self, version: &str, yanked: Option<String>) -> bool {
        let mut changed = false;
        for file in &mut self.files {
            let Ok(distribution) = Distribution::parse(&file.filename) else {
                continue;
            };
            if distribution.version != version {
                continue;
            }
            if file.yanked != yanked {
                file.yanked = yanked.clone();
                changed = true;
            }
        }
        changed
    }

    /// The document's JSON, as it is stored.
    ///
    /// The stored document is a Simple API document (PEP 691):
    /// what the index shows and what the client downloads are the same
    /// thing. `files_base` is the prefix where the files live
    /// (`/pypi/repositorio/files`).
    pub fn to_value(&self, files_base: &str) -> Value {
        let mut document = Map::new();
        let mut meta = Map::new();
        meta.insert("api-version".into(), Value::String(API_VERSION.to_string()));
        document.insert("meta".into(), Value::Object(meta));
        document.insert("name".into(), Value::String(self.name.clone()));
        document.insert(
            "files".into(),
            Value::Array(
                self.files
                    .iter()
                    .map(|file| {
                        let url = format!(
                            "{}/{}/{}",
                            files_base.trim_end_matches('/'),
                            encode_segment(&self.name),
                            encode_segment(&file.filename)
                        );
                        file.to_value(&url)
                    })
                    .collect(),
            ),
        );
        Value::Object(document)
    }

    /// Reads the stored document.
    pub fn from_value(value: &Value) -> Result<Self, PypiError> {
        let object = value
            .as_object()
            .ok_or_else(|| PypiError::InvalidDocument("the document is not an object".into()))?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| PypiError::InvalidDocument("the document has no name".into()))?
            .to_string();
        let files = object
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| PypiError::InvalidDocument("the document has no files".into()))?;
        let mut document = Self::new(name);
        for file in files {
            document.add(FileEntry::from_value(file)?)?;
        }
        Ok(document)
    }
}

/// What the client requests on the PyPI surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /simple/`: the project index.
    Root,
    /// `GET /simple/{proyecto}/`: a project's page.
    Project {
        /// The project whose page is asked for.
        project: ProjectName,
    },
    /// `GET /files/{proyecto}/{fichero}`: the distribution.
    File {
        /// The project the distribution belongs to.
        project: ProjectName,
        /// The name of the file of the distribution.
        filename: String,
    },
    /// `POST /`: the upload.
    Upload,
    /// `POST /yank`: withdraw a version.
    Yank,
}

/// Reads the rest of the path of `/pypi/{repositorio}/{resto}`.
///
/// The project goes normalized in the URL, but the client may request it with
/// whatever name it wants: `Flask` and `flask` are the same project and are
/// answered the same.
pub fn route(rest: &str) -> Result<Route, PypiError> {
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        return Ok(Route::Upload);
    }
    if rest == "simple" {
        return Ok(Route::Root);
    }
    if rest == "yank" {
        return Ok(Route::Yank);
    }
    if let Some(project) = rest.strip_prefix("simple/") {
        if project.is_empty() || project.contains('/') {
            return Err(PypiError::InvalidPath(rest.to_string()));
        }
        return Ok(Route::Project {
            project: ProjectName::parse(project)?,
        });
    }
    if let Some(path) = rest.strip_prefix("files/") {
        let Some((project, filename)) = path.split_once('/') else {
            return Err(PypiError::InvalidPath(rest.to_string()));
        };
        if project.is_empty() || filename.is_empty() || filename.contains('/') {
            return Err(PypiError::InvalidPath(rest.to_string()));
        }
        let filename = decode_segment(filename)?;
        if filename.is_empty() || filename.contains('/') || filename.contains("..") {
            return Err(PypiError::InvalidPath(rest.to_string()));
        }
        return Ok(Route::File {
            project: ProjectName::parse(project)?,
            filename,
        });
    }
    Err(PypiError::InvalidPath(rest.to_string()))
}

/// Decodes a path segment.
///
/// A `%2F` is not a separator: it is a `/` inside the name, and a file
/// name does not have one.
pub fn decode_segment(segment: &str) -> Result<String, PypiError> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(PypiError::InvalidPath(segment.to_string()));
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                .map_err(|_| PypiError::InvalidPath(segment.to_string()))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| PypiError::InvalidPath(segment.to_string()))?;
            decoded.push(byte);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).map_err(|_| PypiError::InvalidPath(segment.to_string()))
}

/// Compares two PEP 440 versions by their numeric part.
///
/// The index does not decide which version is the newest: the client does so with
/// its own comparator. Here we only order what is displayed, so
/// that `1.10` goes after `1.9`.
pub fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (Version::parse(left), Version::parse(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        // A version that is not PEP 440 is ordered as text: nobody
        // else knows it, and at least the order is stable.
        _ => left.cmp(right),
    }
}

/// How the index representation is negotiated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Representation {
    /// HTML (PEP 503).
    Html,
    /// JSON (PEP 691).
    Json,
}

/// Chooses the representation the client asks for.
///
/// Today's `pip` asks for JSON first; an old client or a browser asks for
/// HTML. Without an `Accept` header HTML is returned, which is what
/// everyone understands.
pub fn negotiate(accept: Option<&str>) -> Representation {
    let Some(accept) = accept else {
        return Representation::Html;
    };
    let mut best: Option<(f32, Representation)> = None;
    for part in accept.split(',') {
        let mut pieces = part.split(';');
        let media = pieces.next().unwrap_or("").trim().to_ascii_lowercase();
        let mut quality = 1.0f32;
        for parameter in pieces {
            let parameter = parameter.trim();
            if let Some(value) = parameter.strip_prefix("q=") {
                quality = value.parse().unwrap_or(0.0);
            }
        }
        let representation = if media == JSON_CONTENT_TYPE || media == LATEST_JSON_CONTENT_TYPE {
            Some(Representation::Json)
        } else if media == HTML_CONTENT_TYPE || media == "text/html" {
            Some(Representation::Html)
        } else {
            None
        };
        let Some(representation) = representation else {
            continue;
        };
        // A quality of zero is a refusal: the client says it does not
        // want that representation.
        if quality <= 0.0 {
            continue;
        }
        if best.is_none_or(|(current, _)| quality > current) {
            best = Some((quality, representation));
        }
    }
    best.map(|(_, representation)| representation)
        .unwrap_or(Representation::Html)
}

/// A file's URL, as the client sees it.
///
/// The `base` is the root of the surface (`http://host/pypi/repositorio`).
pub fn file_url(base: &str, project: &str, filename: &str) -> String {
    format!(
        "{}/files/{}/{}",
        base.trim_end_matches('/'),
        encode_segment(project),
        encode_segment(filename),
    )
}

/// A file's URL with its hash, which is how the index publishes it.
pub fn file_url_with_hash(base: &str, project: &str, filename: &str, sha256: &str) -> String {
    format!(
        "{}/files/{}/{}#sha256={}",
        base.trim_end_matches('/'),
        encode_segment(project),
        encode_segment(filename),
        sha256
    )
}

/// Escapes a text to embed it in HTML.
pub fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Encodes a path segment.
fn encode_segment(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// The HTML of a project's page (PEP 503).
pub fn render_project_html(document: &ProjectDocument, base: &str) -> String {
    let mut page = String::new();
    page.push_str("<!DOCTYPE html>\n<html>\n  <head>\n");
    page.push_str(&format!(
        "    <meta name=\"pypi:repository-version\" content=\"{API_VERSION}\">\n"
    ));
    page.push_str(&format!(
        "    <title>Links for {}</title>\n",
        escape_html(&document.name)
    ));
    page.push_str("  </head>\n  <body>\n");
    page.push_str(&format!(
        "    <h1>Links for {}</h1>\n",
        escape_html(&document.name)
    ));
    for file in &document.files {
        let url = file_url_with_hash(base, &document.name, &file.filename, &file.sha256);
        page.push_str(&format!("    <a href=\"{}\"", escape_html(&url)));
        if let Some(requires_python) = &file.requires_python {
            page.push_str(&format!(
                " data-requires-python=\"{}\"",
                escape_html(requires_python)
            ));
        }
        if let Some(yanked) = &file.yanked {
            page.push_str(&format!(" data-yanked=\"{}\"", escape_html(yanked)));
        }
        page.push_str(&format!(">{}</a><br/>\n", escape_html(&file.filename)));
    }
    page.push_str("  </body>\n</html>\n");
    page
}

/// The JSON of a project's page (PEP 691).
pub fn render_project_json(document: &ProjectDocument, base: &str) -> Value {
    let mut files = Vec::new();
    for file in &document.files {
        let mut entry = Map::new();
        entry.insert("filename".into(), Value::String(file.filename.clone()));
        entry.insert(
            "url".into(),
            Value::String(file_url_with_hash(
                base,
                &document.name,
                &file.filename,
                &file.sha256,
            )),
        );
        let mut hashes = Map::new();
        hashes.insert("sha256".into(), Value::String(file.sha256.clone()));
        entry.insert("hashes".into(), Value::Object(hashes));
        if let Some(requires_python) = &file.requires_python {
            entry.insert(
                "requires-python".into(),
                Value::String(requires_python.clone()),
            );
        }
        // The *yanked* state is always stated: `false` means the version
        // is still live, and the reason means it is withdrawn.
        match &file.yanked {
            Some(yanked) => {
                entry.insert("yanked".into(), Value::String(yanked.clone()));
            }
            None => {
                entry.insert("yanked".into(), Value::Bool(false));
            }
        }
        if let Some(size) = file.size {
            entry.insert("size".into(), Value::from(size));
        }
        files.push(Value::Object(entry));
    }
    let mut page = Map::new();
    let mut meta = Map::new();
    meta.insert("api-version".into(), Value::String(API_VERSION.to_string()));
    page.insert("meta".into(), Value::Object(meta));
    page.insert("name".into(), Value::String(document.name.clone()));
    page.insert("files".into(), Value::Array(files));
    page.insert(
        "versions".into(),
        Value::Array(document.versions().into_iter().map(Value::String).collect()),
    );
    Value::Object(page)
}

/// The HTML of the general index (PEP 503).
pub fn render_root_html(projects: &[String], base: &str) -> String {
    let mut page = String::new();
    page.push_str("<!DOCTYPE html>\n<html>\n  <head>\n");
    page.push_str(&format!(
        "    <meta name=\"pypi:repository-version\" content=\"{API_VERSION}\">\n"
    ));
    page.push_str("    <title>Simple index</title>\n");
    page.push_str("  </head>\n  <body>\n");
    for project in projects {
        let url = format!(
            "{}/simple/{}/",
            base.trim_end_matches('/'),
            encode_segment(project)
        );
        page.push_str(&format!(
            "    <a href=\"{}\">{}</a><br/>\n",
            escape_html(&url),
            escape_html(project)
        ));
    }
    page.push_str("  </body>\n</html>\n");
    page
}

/// The JSON of the general index (PEP 691).
pub fn render_root_json(projects: &[String]) -> Value {
    let mut page = Map::new();
    let mut meta = Map::new();
    meta.insert("api-version".into(), Value::String(API_VERSION.to_string()));
    page.insert("meta".into(), Value::Object(meta));
    page.insert(
        "projects".into(),
        Value::Array(
            projects
                .iter()
                .map(|project| {
                    let mut entry = Map::new();
                    entry.insert("name".into(), Value::String(project.clone()));
                    Value::Object(entry)
                })
                .collect(),
        ),
    );
    Value::Object(page)
}

/// What an upload declares.
///
/// The fields are those `twine` sends: the name, the version, the hash and
/// the Python version the package needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    /// The project, already normalized.
    pub project: ProjectName,
    /// The version.
    pub version: String,
    /// The file name.
    pub filename: String,
    /// The SHA-256 hash of the content.
    pub sha256: String,
    /// The Python version it needs, if it declares one.
    pub requires_python: Option<String>,
}

/// Reads an upload's fields and checks them against the file.
///
/// The check is against the bytes, not against what the client says:
/// the declared hash has to be that of what was received, and the name and
/// version have to be those of the file name.
pub fn parse_upload(
    fields: &[(String, String)],
    filename: &str,
    content_sha256: &str,
) -> Result<Upload, PypiError> {
    let lookup = |name: &str| -> Option<&str> {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    };
    match lookup(":action") {
        Some("file_upload") => {}
        Some(other) => {
            return Err(PypiError::Mismatch(format!(
                "the action {other:?} is not an upload"
            )));
        }
        None => return Err(PypiError::MissingField(":action")),
    }
    match lookup("protocol_version") {
        Some("1") => {}
        Some(other) => {
            return Err(PypiError::Mismatch(format!(
                "the protocol version {other:?} is not supported"
            )));
        }
        None => return Err(PypiError::MissingField("protocol_version")),
    }
    let declared_name = lookup("name").ok_or(PypiError::MissingField("name"))?;
    let declared_version = lookup("version").ok_or(PypiError::MissingField("version"))?;
    let declared_sha256 =
        lookup("sha256_digest").ok_or(PypiError::MissingField("sha256_digest"))?;
    if !declared_sha256.eq_ignore_ascii_case(content_sha256) {
        return Err(PypiError::DigestMismatch);
    }
    let distribution = Distribution::parse(filename)?;
    let declared_project = ProjectName::parse(declared_name)?;
    if declared_project != distribution.project {
        return Err(PypiError::Mismatch(format!(
            "the name {declared_name:?} is not the project of the file"
        )));
    }
    // The version the client declares and that of the file name are
    // compared in their canonical form: the client may write it however
    // it likes (`1.0-2`, `1.0.rc1`, `v1.0`) and the file name
    // carries it canonical (`1.0.post2`, `1.0rc1`, `1.0`).
    let declared_version = canonical_version(declared_version)
        .ok_or_else(|| PypiError::InvalidVersion(declared_version.to_string()))?;
    if declared_version != distribution.version {
        return Err(PypiError::Mismatch(format!(
            "the version {declared_version:?} is not the version of the file"
        )));
    }
    Ok(Upload {
        project: declared_project,
        version: declared_version,
        filename: filename.to_string(),
        sha256: declared_sha256.to_ascii_lowercase(),
        requires_python: lookup("requires_python")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project(name: &str) -> ProjectName {
        ProjectName::parse(name).expect("a valid name")
    }

    #[test]
    fn names_are_normalized_the_way_pep_503_says() {
        assert_eq!(normalize("Flask"), "flask");
        assert_eq!(normalize("flask_login"), "flask-login");
        assert_eq!(normalize("flask.login"), "flask-login");
        assert_eq!(normalize("Flask--Login__Now"), "flask-login-now");
        assert_eq!(project("Flask.Login").as_str(), "flask-login");
        assert_eq!(
            project("flask_login").as_str(),
            project("Flask.Login").as_str()
        );
        for bad in ["", "  ", "a b", "a/b", "a:b", "-lead", "trail-"] {
            assert!(
                ProjectName::parse(bad).is_err(),
                "{bad:?} should not be a name"
            );
        }
    }

    #[test]
    fn distributions_are_read_from_their_file_name() {
        let wheel = Distribution::parse("labpkg-1.0-py3-none-any.whl").expect("a wheel");
        assert_eq!(wheel.project.as_str(), "labpkg");
        assert_eq!(wheel.version, "1.0");
        assert_eq!(wheel.kind, DistributionKind::Wheel);

        let underscore = Distribution::parse("my_pkg-2.1.1-py3-none-any.whl").expect("a wheel");
        assert_eq!(underscore.project.as_str(), "my-pkg");

        let sdist = Distribution::parse("labpkg-1.0.tar.gz").expect("an sdist");
        assert_eq!(sdist.project.as_str(), "labpkg");
        assert_eq!(sdist.version, "1.0");
        assert_eq!(sdist.kind, DistributionKind::Sdist);

        let zip = Distribution::parse("labpkg-1.0.zip").expect("an sdist");
        assert_eq!(zip.kind, DistributionKind::Sdist);

        for bad in [
            "labpkg.whl",
            "labpkg-1.0-py3-none-any.tar.gz",
            "labpkg-not.a.version-py3-none-any.whl",
            "../evil-1.0-py3-none-any.whl",
            "labpkg-1.0-py3-none-any.whl.asc",
        ] {
            assert!(
                Distribution::parse(bad).is_err(),
                "{bad:?} should not be a distribution"
            );
        }
    }

    #[test]
    fn versions_follow_pep_440() {
        for good in [
            "1",
            "1.0",
            "1.0.0",
            "1.0.0.0",
            "2.1.3",
            "1.0a1",
            "1.0rc1",
            "1.0.dev1",
            "1.0.post1",
            "1.0-1",
            "1!2.0",
            "1.0+local.1",
            "1.0.0.post2.dev3",
        ] {
            assert!(valid_version(good), "{good:?} should be a version");
        }
        for bad in ["", "x", "1..0", "1.0.", ".1", "1.0+", "1.0+_", "1.0zz"] {
            assert!(!valid_version(bad), "{bad:?} should not be a version");
        }
    }

    #[test]
    fn the_document_keeps_one_entry_per_file() {
        let mut document = ProjectDocument::new("labpkg");
        let entry = FileEntry {
            filename: "labpkg-1.0-py3-none-any.whl".into(),
            sha256: "a".repeat(64),
            size: Some(10),
            requires_python: Some(">=3.9".into()),
            yanked: None,
            source: None,
        };
        document.add(entry.clone()).expect("the first file");
        assert_eq!(document.add(entry).unwrap_err(), PypiError::FileExists);
        assert_eq!(document.files.len(), 1);
        assert!(document.file("labpkg-1.0-py3-none-any.whl").is_some());
    }

    #[test]
    fn a_version_is_yanked_as_a_whole() {
        let mut document = ProjectDocument::new("labpkg");
        for filename in [
            "labpkg-1.0-py3-none-any.whl",
            "labpkg-1.0.tar.gz",
            "labpkg-2.0-py3-none-any.whl",
        ] {
            document
                .add(FileEntry {
                    filename: filename.into(),
                    sha256: "b".repeat(64),
                    size: None,
                    requires_python: None,
                    yanked: None,
                    source: None,
                })
                .expect("a file");
        }
        assert!(document.set_yanked("1.0", Some("broken".into())));
        assert!(!document.set_yanked("1.0", Some("broken".into())));
        assert_eq!(
            document.file("labpkg-1.0.tar.gz").unwrap().yanked,
            Some("broken".into())
        );
        assert_eq!(
            document.file("labpkg-2.0-py3-none-any.whl").unwrap().yanked,
            None
        );
    }

    #[test]
    fn the_json_and_the_html_say_the_same() {
        let mut document = ProjectDocument::new("labpkg");
        document
            .add(FileEntry {
                filename: "labpkg-1.0-py3-none-any.whl".into(),
                sha256: "c".repeat(64),
                size: Some(42),
                requires_python: Some(">=3.9".into()),
                yanked: Some("broken".into()),
                source: None,
            })
            .expect("a file");
        let base = "http://localhost:8080/pypi/repo";
        let html = render_project_html(&document, base);
        assert!(html.contains("data-requires-python=\"&gt;=3.9\""));
        assert!(html.contains("data-yanked=\"broken\""));
        assert!(html.contains(&format!("#sha256={}", "c".repeat(64))));
        let json = render_project_json(&document, base);
        let files = json.get("files").and_then(Value::as_array).expect("files");
        assert_eq!(files.len(), 1);
        let entry = &files[0];
        assert_eq!(
            entry
                .get("hashes")
                .and_then(|hashes| hashes.get("sha256"))
                .and_then(Value::as_str),
            Some("c".repeat(64).as_str())
        );
        assert_eq!(
            entry.get("requires-python").and_then(Value::as_str),
            Some(">=3.9")
        );
        assert_eq!(entry.get("yanked").and_then(Value::as_str), Some("broken"));
        assert_eq!(entry.get("size").and_then(Value::as_u64), Some(42));
        let url = entry.get("url").and_then(Value::as_str).expect("a url");
        assert!(url.starts_with("http://localhost:8080/pypi/repo/files/labpkg/"));
    }

    #[test]
    fn a_file_that_is_not_yanked_says_so() {
        let mut document = ProjectDocument::new("labpkg");
        document
            .add(FileEntry {
                filename: "labpkg-1.0-py3-none-any.whl".into(),
                sha256: "9".repeat(64),
                size: Some(11),
                requires_python: None,
                yanked: None,
                source: None,
            })
            .expect("a file");
        // The JSON says it is not withdrawn: the client decides with it.
        let json = render_project_json(&document, "http://localhost");
        let files = json.get("files").and_then(Value::as_array).expect("files");
        assert_eq!(files[0].get("yanked"), Some(&Value::Bool(false)));
        // And the HTML does not set the attribute, which is the same.
        let html = render_project_html(&document, "http://localhost");
        assert!(!html.contains("data-yanked"));
    }

    #[test]
    fn the_html_escapes_what_comes_from_the_client() {
        let mut document = ProjectDocument::new("labpkg");
        document
            .add(FileEntry {
                filename: "<script>alert(1)</script>".into(),
                sha256: "d".repeat(64),
                size: None,
                requires_python: Some("\"><script>".into()),
                yanked: None,
                source: None,
            })
            .expect("a file");
        let html = render_project_html(&document, "http://localhost");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn the_representation_is_negotiated() {
        assert_eq!(negotiate(None), Representation::Html);
        assert_eq!(
            negotiate(Some("application/vnd.pypi.simple.v1+json")),
            Representation::Json
        );
        assert_eq!(
            negotiate(Some(
                "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1, text/html;q=0.01"
            )),
            Representation::Json
        );
        assert_eq!(negotiate(Some("text/html")), Representation::Html);
        assert_eq!(
            negotiate(Some(
                "application/vnd.pypi.simple.v1+html;q=0.9, application/vnd.pypi.simple.v1+json;q=0.2"
            )),
            Representation::Html
        );
        assert_eq!(negotiate(Some("application/json")), Representation::Html);
        // The JSON `latest` version is the same representation.
        assert_eq!(
            negotiate(Some("application/vnd.pypi.simple.latest+json")),
            Representation::Json
        );
        // A quality of zero is a refusal.
        assert_eq!(
            negotiate(Some("application/vnd.pypi.simple.v1+json;q=0")),
            Representation::Html
        );
    }

    #[test]
    fn a_version_is_canonicalized_the_way_pep_440_says() {
        for (raw, canonical) in [
            ("1.0", "1.0"),
            ("1.0-2", "1.0.post2"),
            ("1.0-rc1", "1.0rc1"),
            ("1.0.rc1", "1.0rc1"),
            ("1.0_rc1", "1.0rc1"),
            ("1.0.alpha1", "1.0a1"),
            ("1.0.beta1", "1.0b1"),
            ("1.0.pre1", "1.0rc1"),
            ("1.0.c1", "1.0rc1"),
            ("v1.0", "1.0"),
            ("1.0.post", "1.0.post0"),
            ("1.0-r2", "1.0.post2"),
            ("1.2post2", "1.2.post2"),
            ("1.0+ubuntu-1", "1.0+ubuntu.1"),
            ("01.0", "1.0"),
            ("1.0.dev", "1.0.dev0"),
            // The PEP grammar is case-insensitive.
            ("1.0RC1", "1.0rc1"),
            ("1.0A1", "1.0a1"),
            ("1.0B1", "1.0b1"),
            ("1.0POST1", "1.0.post1"),
            ("1.0DEV1", "1.0.dev1"),
            ("1.0+ABC", "1.0+abc"),
            // And a numeric segment of the local version carries no zeros.
            ("1.0+01", "1.0+1"),
            ("1.0+abc.01", "1.0+abc.1"),
            ("1.0+00", "1.0+0"),
        ] {
            assert_eq!(
                canonical_version(raw).as_deref(),
                Some(canonical),
                "{raw:?}"
            );
        }
        for bad in [
            "1.0-",
            "1.0.dev1.post1",
            "1.0.dev1.dev2",
            "1.0.post1.post2",
            "1.0-1-2",
            "1.0+_x",
            "1!",
        ] {
            assert!(
                canonical_version(bad).is_none(),
                "{bad:?} should not be a version"
            );
        }
    }

    #[test]
    fn a_version_is_compared_in_its_canonical_form() {
        // The client writes the version however it likes; the file
        // name carries it canonical.
        let sha = "e".repeat(64);
        let mut fields = vec![
            (":action".to_string(), "file_upload".to_string()),
            ("protocol_version".to_string(), "1".to_string()),
            ("name".to_string(), "labpkg".to_string()),
            ("version".to_string(), "1.0-2".to_string()),
            ("sha256_digest".to_string(), sha.clone()),
        ];
        let upload =
            parse_upload(&fields, "labpkg-1.0.post2.tar.gz", &sha).expect("a valid upload");
        assert_eq!(upload.version, "1.0.post2");
        // And a version that is not the file's is rejected.
        fields[3].1 = "1.0-3".to_string();
        assert!(matches!(
            parse_upload(&fields, "labpkg-1.0.post2.tar.gz", &sha),
            Err(PypiError::Mismatch(_))
        ));
    }

    #[test]
    fn a_wheel_that_the_client_would_reject_is_not_a_distribution() {
        // A wheel's parts are what they are: not one more or less.
        assert!(Distribution::parse("labpkg-1.0-py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-1-py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-py3-none-any-extra.whl").is_err());
        assert!(Distribution::parse("labpkg-1.0-1-2-py3-none-any.whl").is_err());
        assert!(Distribution::parse("labpkg-1.0-x-py3-none-any.whl").is_err());
        assert!(Distribution::parse("labpkg-1.0-py3-none.whl").is_err());
    }

    #[test]
    fn a_wheel_that_the_client_would_install_is_a_distribution() {
        // The rule is `pip`'s: the build tag starts with
        // a digit and anything may follow, and the three tags are
        // what they are. A name that `pip` installs is not rejected here.
        assert!(Distribution::parse("labpkg-1.0-1.5-py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-1abc-py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-py3-none-any+mips.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-py2.py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-cp39-cp39-manylinux_2_17_x86_64.whl").is_ok());
    }

    #[test]
    fn an_upload_is_checked_against_its_content() {
        let sha = "e".repeat(64);
        let fields = vec![
            (":action".to_string(), "file_upload".to_string()),
            ("protocol_version".to_string(), "1".to_string()),
            ("name".to_string(), "Lab.Pkg".to_string()),
            ("version".to_string(), "1.0".to_string()),
            ("sha256_digest".to_string(), sha.clone()),
            ("requires_python".to_string(), ">=3.9".to_string()),
        ];
        let upload =
            parse_upload(&fields, "lab_pkg-1.0-py3-none-any.whl", &sha).expect("a valid upload");
        assert_eq!(upload.project.as_str(), "lab-pkg");
        assert_eq!(upload.version, "1.0");
        assert_eq!(upload.requires_python.as_deref(), Some(">=3.9"));

        let corrupted = parse_upload(&fields, "lab_pkg-1.0-py3-none-any.whl", &"f".repeat(64));
        assert_eq!(corrupted.unwrap_err(), PypiError::DigestMismatch);

        let mut other_name = fields.clone();
        other_name[2].1 = "otro".to_string();
        assert!(matches!(
            parse_upload(&other_name, "lab_pkg-1.0-py3-none-any.whl", &sha),
            Err(PypiError::Mismatch(_))
        ));

        let mut other_version = fields.clone();
        other_version[3].1 = "2.0".to_string();
        assert!(matches!(
            parse_upload(&other_version, "lab_pkg-1.0-py3-none-any.whl", &sha),
            Err(PypiError::Mismatch(_))
        ));

        let without_action: Vec<(String, String)> = fields
            .iter()
            .filter(|(field, _)| field != ":action")
            .cloned()
            .collect();
        assert_eq!(
            parse_upload(&without_action, "lab_pkg-1.0-py3-none-any.whl", &sha).unwrap_err(),
            PypiError::MissingField(":action")
        );
    }

    #[test]
    fn the_document_survives_a_round_trip() {
        let mut document = ProjectDocument::new("labpkg");
        document
            .add(FileEntry {
                filename: "labpkg-1.0-py3-none-any.whl".into(),
                sha256: "1".repeat(64),
                size: Some(7),
                requires_python: None,
                yanked: Some(String::new()),
                source: None,
            })
            .expect("a file");
        let value = document.to_value("/pypi/repo/files");
        let read = ProjectDocument::from_value(&value).expect("the document");
        assert_eq!(read.name, document.name);
        assert_eq!(read.files.len(), 1);
        assert_eq!(read.files[0].filename, document.files[0].filename);
        assert_eq!(read.files[0].sha256, document.files[0].sha256);
        assert_eq!(read.files[0].size, document.files[0].size);
        assert_eq!(read.files[0].yanked, Some(String::new()));
        // The stored URL is that of the surface, not of an upstream.
        assert_eq!(
            read.files[0].source.as_deref(),
            Some("/pypi/repo/files/labpkg/labpkg-1.0-py3-none-any.whl")
        );
    }

    #[test]
    fn a_document_of_the_upstream_is_read_the_same_way() {
        let upstream = json!({
            "meta": {"api-version": "1.0"},
            "name": "labpkg",
            "files": [{
                "filename": "labpkg-1.0-py3-none-any.whl",
                "url": "https://files.example.com/packages/ab/cd/labpkg-1.0-py3-none-any.whl",
                "hashes": {"sha256": "2".repeat(64)},
                "requires-python": ">=3.9",
                "yanked": "broken",
                "size": 99
            }]
        });
        let document = ProjectDocument::from_value(&upstream).expect("the document");
        assert_eq!(document.name, "labpkg");
        let file = &document.files[0];
        assert_eq!(file.sha256, "2".repeat(64));
        assert_eq!(file.size, Some(99));
        assert_eq!(file.requires_python.as_deref(), Some(">=3.9"));
        assert_eq!(file.yanked.as_deref(), Some("broken"));
        assert_eq!(
            file.source.as_deref(),
            Some("https://files.example.com/packages/ab/cd/labpkg-1.0-py3-none-any.whl")
        );
    }

    #[test]
    fn the_routes_of_the_surface_are_read() {
        assert_eq!(route("").unwrap(), Route::Upload);
        assert_eq!(route("simple").unwrap(), Route::Root);
        assert_eq!(route("simple/").unwrap(), Route::Root);
        assert_eq!(route("yank").unwrap(), Route::Yank);
        assert_eq!(
            route("simple/Flask.Login/").unwrap(),
            Route::Project {
                project: project("flask-login")
            }
        );
        assert_eq!(
            route("files/labpkg/labpkg-1.0%2Blocal-py3-none-any.whl").unwrap(),
            Route::File {
                project: project("labpkg"),
                filename: "labpkg-1.0+local-py3-none-any.whl".to_string()
            }
        );
        for bad in [
            "nope",
            "files/labpkg",
            "files/labpkg/a/b",
            "files/labpkg/..",
        ] {
            assert!(route(bad).is_err(), "{bad:?} should not be a route");
        }
    }

    #[test]
    fn a_document_that_is_not_ours_is_refused() {
        assert!(ProjectDocument::from_value(&json!({})).is_err());
        assert!(ProjectDocument::from_value(&json!({"name": "x", "files": [{}]})).is_err());
        assert!(
            ProjectDocument::from_value(&json!({
                "name": "x",
                "files": [{"filename": "a.whl", "sha256": "zz"}]
            }))
            .is_err()
        );
    }
}
