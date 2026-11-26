//! The pure part of the Maven 2 format: paths, versions, POM and metadata.
//!
//! Nothing here touches disk, network or catalog: these are functions over strings and
//! bytes, and for that reason they are tested without infrastructure. The service uses them
//! to decide whether a path is Maven, whether a version is a release or a
//! snapshot, whether a POM says the same as the path it is uploaded to, and to
//! **derive** `maven-metadata.xml` from what the catalog already knows.
//!
//! # Why metadata is derived and not stored
//!
//! `maven-metadata.xml` is a view of what is in the repository. If it
//! were stored, there would be two sources of truth (the files and the XML) and
//! two concurrent deployments would overwrite each other when rewriting it. By deriving it
//! on every read, the XML always says what is there, a failure halfway through
//! a deployment does not leave an inconsistent XML, and there is nothing to repair.
//!
//! # Layout
//!
//! ```text
//! {group/with/slashes}/{artifactId}/{version}/{artifactId}-{version}[-{classifier}].{ext}
//! {group/with/slashes}/{artifactId}/maven-metadata.xml
//! {group/with/slashes}/{artifactId}/{version}-SNAPSHOT/maven-metadata.xml
//! any of the above + .md5 | .sha1 | .sha256 | .sha512
//! ```

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

/// The name of the metadata file.
pub const METADATA_FILE: &str = "maven-metadata.xml";

/// The suffix of a snapshot version.
pub const SNAPSHOT_SUFFIX: &str = "-SNAPSHOT";

/// The maximum bytes of a POM or of a metadata document that the service reads into
/// memory.
pub const MAX_XML_BYTES: usize = 1024 * 1024;

/// The maximum bytes of a checksum file.
pub const MAX_CHECKSUM_BYTES: usize = 1024;

/// The maximum length of a version.
const MAX_VERSION_LEN: usize = 128;

/// The maximum depth of XML elements that is accepted.
const MAX_XML_DEPTH: usize = 32;

/// The maximum XML elements of a document.
const MAX_XML_ELEMENTS: usize = 20_000;

/// Maximum length of the name of an XML element.
const MAX_XML_NAME: usize = 64;

/// Maximum bytes of materialized paths when reading an XML.
const MAX_XML_PATH_BYTES: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Why a path is not a valid Maven path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// It does not have the form `group/artifact/version/file`.
    Layout,
    /// A group or artifact identifier is not valid.
    Identifier,
    /// The version is not a valid version.
    Version,
    /// The file name does not correspond to its artifact and version.
    FileName,
}

impl PathError {
    /// What is told to the client.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Layout => "the path is not a Maven 2 repository path",
            Self::Identifier => "the group or artifact identifier is not valid",
            Self::Version => "the version is not valid",
            Self::FileName => "the file name does not match the artifact and version of its path",
        }
    }
}

/// How the version of a file is written relative to its directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionKind {
    /// A fixed version (`1.2.3`).
    Release,
    /// The floating name of a snapshot (`1.0-SNAPSHOT`).
    Snapshot,
    /// A timestamped snapshot (`1.0-20240101.120000-3`).
    Timestamped {
        /// `yyyyMMdd.HHmmss`.
        stamp: String,
        /// The build number.
        build: u32,
    },
}

/// A file of an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenFile {
    /// The `groupId` (with dots).
    pub group_id: String,
    /// The `artifactId`.
    pub artifact_id: String,
    /// The version of the **directory** (`1.0-SNAPSHOT`, never the timestamp).
    pub version: String,
    /// How the version is written inside the file name.
    pub kind: VersionKind,
    /// The classifier, if any.
    pub classifier: Option<String>,
    /// The extension (`jar`, `pom`, `tar.gz`...).
    pub extension: String,
}

impl MavenFile {
    /// Whether it is the POM of the artifact (without classifier).
    pub fn is_pom(&self) -> bool {
        self.extension == "pom" && self.classifier.is_none()
    }

    /// Whether the directory version is a snapshot.
    pub fn is_snapshot(&self) -> bool {
        self.version.ends_with(SNAPSHOT_SUFFIX)
    }
}

/// What a metadata document describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataLevel {
    /// The versions of an artifact.
    Artifact {
        /// The `groupId`.
        group_id: String,
        /// The `artifactId`.
        artifact_id: String,
    },
    /// The builds of a snapshot.
    Version {
        /// The `groupId`.
        group_id: String,
        /// The `artifactId`.
        artifact_id: String,
        /// The snapshot version.
        version: String,
    },
}

/// The algorithm of a checksum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// MD5, 32 hexadecimal characters.
    Md5,
    /// SHA-1, 40.
    Sha1,
    /// SHA-256, 64.
    Sha256,
    /// SHA-512, 128.
    Sha512,
}

impl Algorithm {
    const ALL: [(&'static str, Self); 4] = [
        (".md5", Self::Md5),
        (".sha1", Self::Sha1),
        (".sha256", Self::Sha256),
        (".sha512", Self::Sha512),
    ];

    /// The hexadecimal characters of the digest.
    pub fn hex_len(self) -> usize {
        match self {
            Self::Md5 => 32,
            Self::Sha1 => 40,
            Self::Sha256 => 64,
            Self::Sha512 => 128,
        }
    }
}

/// What the checksum is of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// Of an artifact file.
    File(MavenFile),
    /// Of a metadata document.
    Metadata(MetadataLevel),
}

/// What a path of a Maven 2 repository names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// An artifact file (POM, JAR, sources, module...).
    File(MavenFile),
    /// `maven-metadata.xml`.
    Metadata(MetadataLevel),
    /// The checksum of a file or of a metadata document.
    Checksum {
        /// The algorithm.
        algorithm: Algorithm,
        /// What it is of.
        subject: Subject,
        /// The path of the thing it is the checksum of.
        subject_path: String,
    },
}

/// Whether a version is valid.
///
/// A version is written in paths and in files: only letters, digits and
/// `. _ - +`, not starting or ending with a separator, and not being `.` or `..`.
pub fn is_valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= MAX_VERSION_LEN
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
        && version
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && version
            .bytes()
            .last()
            .is_some_and(|b| b.is_ascii_alphanumeric())
}

/// A group segment: an identifier without dots, because the group
/// `com.acme` is the directory `com/acme` and there cannot be two ways of
/// writing the same coordinates.
fn is_valid_group(segments: &[&str]) -> bool {
    segments
        .iter()
        .all(|segment| is_valid_identifier(segment) && !segment.contains('.'))
}

fn is_valid_identifier(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 128
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
        && !segment.starts_with('.')
}

/// Parses a path of a Maven 2 repository.
pub fn parse_path(path: &str) -> Result<Target, PathError> {
    for (suffix, algorithm) in Algorithm::ALL {
        if let Some(subject_path) = path.strip_suffix(suffix) {
            // A checksum of a checksum does not exist.
            if Algorithm::ALL
                .iter()
                .any(|(other, _)| subject_path.ends_with(other))
            {
                return Err(PathError::Layout);
            }
            let subject = match parse_subject(subject_path)? {
                Target::File(file) => Subject::File(file),
                Target::Metadata(level) => Subject::Metadata(level),
                Target::Checksum { .. } => return Err(PathError::Layout),
            };
            return Ok(Target::Checksum {
                algorithm,
                subject,
                subject_path: subject_path.to_string(),
            });
        }
    }
    parse_subject(path)
}

fn parse_subject(path: &str) -> Result<Target, PathError> {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(PathError::Layout);
    }
    let name = *segments.last().ok_or(PathError::Layout)?;
    if name == METADATA_FILE {
        return parse_metadata_path(&segments[..segments.len() - 1]);
    }
    // group (>=1) + artifact + version + file
    if segments.len() < 4 {
        return Err(PathError::Layout);
    }
    let version = segments[segments.len() - 2];
    let artifact_id = segments[segments.len() - 3];
    let group = &segments[..segments.len() - 3];
    if !is_valid_identifier(artifact_id) || !is_valid_group(group) {
        return Err(PathError::Identifier);
    }
    if !is_valid_version(version) {
        return Err(PathError::Version);
    }
    let group_id = group.join(".");
    let (kind, rest) = split_version(name, artifact_id, version)?;
    let (classifier, extension) = if let Some(extension) = rest.strip_prefix('.') {
        (None, extension)
    } else if let Some(after) = rest.strip_prefix('-') {
        let (classifier, extension) = after.split_once('.').ok_or(PathError::FileName)?;
        if classifier.is_empty() {
            return Err(PathError::FileName);
        }
        (Some(classifier.to_string()), extension)
    } else {
        return Err(PathError::FileName);
    };
    // The extension starts with a letter: `a-1.0.1.jar` in the `1.0`
    // directory is not the file of version `1.0`.
    if !extension
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic())
    {
        return Err(PathError::FileName);
    }
    Ok(Target::File(MavenFile {
        group_id,
        artifact_id: artifact_id.to_string(),
        version: version.to_string(),
        kind,
        classifier,
        extension: extension.to_string(),
    }))
}

fn parse_metadata_path(directories: &[&str]) -> Result<Target, PathError> {
    if directories.len() < 2 {
        return Err(PathError::Layout);
    }
    if !directories.iter().all(|s| is_valid_identifier(s)) {
        return Err(PathError::Identifier);
    }
    let last = directories[directories.len() - 1];
    let group_len = directories.len()
        - if last.ends_with(SNAPSHOT_SUFFIX) {
            2
        } else {
            1
        };
    if group_len == 0 || !is_valid_group(&directories[..group_len]) {
        return Err(PathError::Identifier);
    }
    if last.ends_with(SNAPSHOT_SUFFIX) {
        // group (>=1) + artifact + snapshot version
        if directories.len() < 3 {
            return Err(PathError::Layout);
        }
        if !is_valid_version(last) {
            return Err(PathError::Version);
        }
        return Ok(Target::Metadata(MetadataLevel::Version {
            group_id: directories[..directories.len() - 2].join("."),
            artifact_id: directories[directories.len() - 2].to_string(),
            version: last.to_string(),
        }));
    }
    Ok(Target::Metadata(MetadataLevel::Artifact {
        group_id: directories[..directories.len() - 1].join("."),
        artifact_id: last.to_string(),
    }))
}

/// Splits from a file name the version and what remains after it.
fn split_version<'a>(
    name: &'a str,
    artifact_id: &str,
    version: &str,
) -> Result<(VersionKind, &'a str), PathError> {
    let rest = name
        .strip_prefix(artifact_id)
        .and_then(|rest| rest.strip_prefix('-'))
        .ok_or(PathError::FileName)?;
    if let Some(after) = rest.strip_prefix(version)
        && after.starts_with(['.', '-'])
    {
        let kind = if version.ends_with(SNAPSHOT_SUFFIX) {
            VersionKind::Snapshot
        } else {
            VersionKind::Release
        };
        return Ok((kind, after));
    }
    if let Some(base) = version.strip_suffix(SNAPSHOT_SUFFIX)
        && let Some(after) = rest
            .strip_prefix(base)
            .and_then(|rest| rest.strip_prefix('-'))
    {
        // yyyyMMdd.HHmmss-N
        let (stamp, tail) = after.split_at_checked(15).ok_or(PathError::FileName)?;
        if !is_stamp(stamp) {
            return Err(PathError::FileName);
        }
        let tail = tail.strip_prefix('-').ok_or(PathError::FileName)?;
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 {
            return Err(PathError::FileName);
        }
        let build: u32 = tail[..digits].parse().map_err(|_| PathError::FileName)?;
        return Ok((
            VersionKind::Timestamped {
                stamp: stamp.to_string(),
                build,
            },
            &tail[digits..],
        ));
    }
    Err(PathError::FileName)
}

fn is_stamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 15
        && bytes[8] == b'.'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 8 || byte.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Number(u128),
    Qualifier(String),
}

fn items(version: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut numeric = false;
    let flush = |current: &mut String, numeric: bool, out: &mut Vec<Item>| {
        if current.is_empty() {
            return;
        }
        if numeric {
            out.push(Item::Number(current.parse().unwrap_or(u128::MAX)));
        } else {
            out.push(Item::Qualifier(current.to_ascii_lowercase()));
        }
        current.clear();
    };
    for char in version.chars() {
        if matches!(char, '.' | '-' | '_' | '+') {
            flush(&mut current, numeric, &mut out);
            continue;
        }
        let is_digit = char.is_ascii_digit();
        if !current.is_empty() && is_digit != numeric {
            flush(&mut current, numeric, &mut out);
        }
        numeric = is_digit;
        current.push(char);
    }
    flush(&mut current, numeric, &mut out);
    out
}

/// The order of a qualifier relative to the published version (`5`).
fn rank(qualifier: &str) -> (u8, &str) {
    match qualifier {
        "alpha" | "a" => (0, ""),
        "beta" | "b" => (1, ""),
        "milestone" | "m" => (2, ""),
        "rc" | "cr" => (3, ""),
        "snapshot" => (4, ""),
        "" | "ga" | "final" | "release" => (5, ""),
        "sp" => (6, ""),
        other => (7, other),
    }
}

fn compare_items(left: Option<&Item>, right: Option<&Item>) -> Ordering {
    let zero = Item::Number(0);
    let release = Item::Qualifier(String::new());
    match (left, right) {
        (Some(Item::Number(a)), Some(Item::Number(b))) => a.cmp(b),
        (Some(Item::Number(_)), Some(Item::Qualifier(_))) => Ordering::Greater,
        (Some(Item::Qualifier(_)), Some(Item::Number(_))) => Ordering::Less,
        (Some(Item::Qualifier(a)), Some(Item::Qualifier(b))) => rank(a).cmp(&rank(b)),
        (Some(a), None) => compare_items(Some(a), Some(non_number(a, &zero, &release))),
        (None, Some(b)) => compare_items(Some(non_number(b, &zero, &release)), Some(b)),
        (None, None) => Ordering::Equal,
    }
}

/// The padding of a shorter version: `0` against a number, the published
/// version against a qualifier.
fn non_number<'a>(other: &Item, zero: &'a Item, release: &'a Item) -> &'a Item {
    match other {
        Item::Number(_) => zero,
        Item::Qualifier(_) => release,
    }
}

/// Compares two versions with Maven's ordering: `1.0-SNAPSHOT < 1.0 < 1.0.1`
/// and `1.9 < 1.10`.
pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let (a, b) = (items(left), items(right));
    for index in 0..a.len().max(b.len()) {
        let ordering = compare_items(a.get(index), b.get(index));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    // Equal for Maven, different as text: the tie-break is stable.
    left.cmp(right)
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Which versions a repository admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionPolicy {
    /// Releases and snapshots.
    #[default]
    Mixed,
    /// Only fixed versions.
    Release,
    /// Only snapshots.
    Snapshot,
}

/// The configuration of a Maven 2 repository.
///
/// It lives under the `maven` key of the repository settings:
///
/// ```json
/// { "maven": { "version_policy": "release", "release_redeploy": false } }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MavenConfig {
    /// Which versions it admits. By default, all.
    #[serde(default)]
    pub version_policy: VersionPolicy,
    /// Whether a release can change content. By default, no.
    #[serde(default)]
    pub release_redeploy: bool,
}

impl MavenConfig {
    /// The configuration from a repository settings object.
    ///
    /// Without a `maven` key, the default one; a key that is not understood
    /// is an error, never the default.
    pub fn of_settings(settings: &Value) -> Result<Self, String> {
        match settings.get("maven") {
            None => Ok(Self::default()),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|error| format!("the maven settings cannot be read: {error}")),
        }
    }

    /// Whether the repository admits versions of that kind.
    pub fn admits(&self, snapshot: bool) -> bool {
        match self.version_policy {
            VersionPolicy::Mixed => true,
            VersionPolicy::Release => !snapshot,
            VersionPolicy::Snapshot => snapshot,
        }
    }
}

// ---------------------------------------------------------------------------
// XML
// ---------------------------------------------------------------------------

/// Why an XML cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlError {
    /// It is not well formed.
    Malformed,
    /// It carries a `DOCTYPE` declaration, which is not accepted.
    Doctype,
    /// It is too deep or too large.
    TooComplex,
}

/// An element with text: its path (`project/parent/groupId`) and its text.
type Leaf = (String, String);

/// Reads an XML and returns the text of each element, by path.
///
/// It does not expand external entities or accept `DOCTYPE`: a POM is data that
/// comes from outside.
fn leaves(xml: &str) -> Result<Vec<Leaf>, XmlError> {
    let bytes = xml.as_bytes();
    let mut stack: Vec<(String, String)> = Vec::new();
    let mut out: Vec<Leaf> = Vec::new();
    let mut roots = 0usize;
    let mut count = 0usize;
    let mut path_bytes = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            let end = xml[index..].find('<').map_or(bytes.len(), |n| index + n);
            if let Some((_, text)) = stack.last_mut() {
                text.push_str(&decode(&xml[index..end]));
            } else if !xml[index..end].trim().is_empty() {
                return Err(XmlError::Malformed);
            }
            index = end;
            continue;
        }
        let rest = &xml[index..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").ok_or(XmlError::Malformed)?;
            index += end + 3;
        } else if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").ok_or(XmlError::Malformed)?;
            let (_, text) = stack.last_mut().ok_or(XmlError::Malformed)?;
            text.push_str(&rest[9..end]);
            index += end + 3;
        } else if rest.starts_with("<?") {
            let end = rest.find("?>").ok_or(XmlError::Malformed)?;
            index += end + 2;
        } else if rest.starts_with("<!") {
            return Err(XmlError::Doctype);
        } else if let Some(closing) = rest.strip_prefix("</") {
            let end = closing.find('>').ok_or(XmlError::Malformed)?;
            let name = closing[..end].trim();
            let (open, text) = stack.pop().ok_or(XmlError::Malformed)?;
            if open != name {
                return Err(XmlError::Malformed);
            }
            let path = stack
                .iter()
                .map(|(name, _)| name.as_str())
                .chain([name])
                .collect::<Vec<_>>()
                .join("/");
            path_bytes += path.len();
            if path_bytes > MAX_XML_PATH_BYTES {
                return Err(XmlError::TooComplex);
            }
            out.push((path, text.trim().to_string()));
            index += 2 + end + 1;
        } else {
            let end = tag_end(rest).ok_or(XmlError::Malformed)?;
            let inner = &rest[1..end];
            let self_closing = inner.ends_with('/');
            let inner = inner.strip_suffix('/').unwrap_or(inner);
            let name = inner
                .split(|char: char| char.is_whitespace())
                .next()
                .unwrap_or("");
            if name.is_empty() || !name.bytes().all(is_name_byte) {
                return Err(XmlError::Malformed);
            }
            if name.len() > MAX_XML_NAME {
                return Err(XmlError::TooComplex);
            }
            count += 1;
            if count > MAX_XML_ELEMENTS || stack.len() >= MAX_XML_DEPTH {
                return Err(XmlError::TooComplex);
            }
            if stack.is_empty() {
                roots += 1;
                if roots > 1 {
                    return Err(XmlError::Malformed);
                }
            }
            if self_closing {
                let path = stack
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .chain([name])
                    .collect::<Vec<_>>()
                    .join("/");
                path_bytes += path.len();
                if path_bytes > MAX_XML_PATH_BYTES {
                    return Err(XmlError::TooComplex);
                }
                out.push((path, String::new()));
            } else {
                stack.push((name.to_string(), String::new()));
            }
            index += end + 1;
        }
    }
    if !stack.is_empty() || roots == 0 {
        return Err(XmlError::Malformed);
    }
    Ok(out)
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
}

/// Where an opening tag ends, respecting quotes.
fn tag_end(rest: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (index, byte) in rest.bytes().enumerate().skip(1) {
        match (quote, byte) {
            (Some(open), byte) if byte == open => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(byte),
            (None, b'>') => return Some(index),
            (None, b'<') => return None,
            _ => {}
        }
    }
    None
}

fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn encode(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn first<'a>(leaves: &'a [Leaf], path: &str) -> Option<&'a str> {
    leaves
        .iter()
        .find(|(candidate, _)| candidate == path)
        .map(|(_, text)| text.as_str())
        .filter(|text| !text.is_empty())
}

// ---------------------------------------------------------------------------
// POM
// ---------------------------------------------------------------------------

/// The coordinates declared by a POM, with the parent's inheritance applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PomCoordinates {
    /// The `groupId`.
    pub group_id: String,
    /// The `artifactId`.
    pub artifact_id: String,
    /// The `version`.
    pub version: String,
}

/// Why a POM is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PomError {
    /// It is not valid XML, or it is too large.
    Xml(XmlError),
    /// It is not a `project`.
    NotAProject,
    /// Coordinates are missing.
    Incomplete,
}

impl PomError {
    /// What is told to the client.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Xml(_) => "the POM is not a valid XML document",
            Self::NotAProject => "the POM is not a Maven project",
            Self::Incomplete => "the POM does not declare its group, artifact and version",
        }
    }
}

/// The coordinates of a POM.
///
/// The `groupId` and the `version` are inherited from the parent when the project does not
/// declare them, as Maven does.
pub fn parse_pom(bytes: &[u8]) -> Result<PomCoordinates, PomError> {
    let text = std::str::from_utf8(bytes).map_err(|_| PomError::Xml(XmlError::Malformed))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let leaves = leaves(text).map_err(PomError::Xml)?;
    if !leaves
        .iter()
        .any(|(path, _)| path == "project/modelVersion")
        && !leaves.iter().any(|(path, _)| path == "project/artifactId")
    {
        return Err(PomError::NotAProject);
    }
    let artifact_id = first(&leaves, "project/artifactId").ok_or(PomError::Incomplete)?;
    let group_id = first(&leaves, "project/groupId")
        .or_else(|| first(&leaves, "project/parent/groupId"))
        .ok_or(PomError::Incomplete)?;
    let version = first(&leaves, "project/version")
        .or_else(|| first(&leaves, "project/parent/version"))
        .ok_or(PomError::Incomplete)?;
    Ok(PomCoordinates {
        group_id: group_id.to_string(),
        artifact_id: artifact_id.to_string(),
        version: version.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

/// The metadata that a client uploads, read in order to check it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredMetadata {
    /// The `groupId`, if declared.
    pub group_id: Option<String>,
    /// The `artifactId`, if declared.
    pub artifact_id: Option<String>,
    /// The `version`, if declared.
    pub version: Option<String>,
}

/// Reads metadata uploaded by a client.
pub fn parse_metadata(bytes: &[u8]) -> Result<DeclaredMetadata, XmlError> {
    let text = std::str::from_utf8(bytes).map_err(|_| XmlError::Malformed)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let leaves = leaves(text)?;
    if !leaves
        .iter()
        .any(|(path, _)| path == "metadata" || path.starts_with("metadata/"))
    {
        return Err(XmlError::Malformed);
    }
    Ok(DeclaredMetadata {
        group_id: first(&leaves, "metadata/groupId").map(str::to_string),
        artifact_id: first(&leaves, "metadata/artifactId").map(str::to_string),
        version: first(&leaves, "metadata/version").map(str::to_string),
    })
}

/// Whether what a client declares in its metadata matches its path.
pub fn metadata_matches(level: &MetadataLevel, declared: &DeclaredMetadata) -> bool {
    let (group, artifact, version) = match level {
        MetadataLevel::Artifact {
            group_id,
            artifact_id,
        } => (group_id, Some(artifact_id), None),
        MetadataLevel::Version {
            group_id,
            artifact_id,
            version,
        } => (group_id, Some(artifact_id), Some(version)),
    };
    let same = |declared: &Option<String>, expected: Option<&String>| match (declared, expected) {
        (Some(declared), Some(expected)) => declared == expected,
        _ => true,
    };
    declared.group_id.as_ref().is_none_or(|g| g == group)
        && same(&declared.artifact_id, artifact)
        && same(&declared.version, version)
}

/// A known file of the catalog: the path and when it was published.
#[derive(Debug, Clone)]
pub struct Known {
    /// The path within the repository.
    pub path: String,
    /// When it was published.
    pub updated_at: DateTime<Utc>,
}

fn stamp(moment: DateTime<Utc>) -> String {
    moment.format("%Y%m%d%H%M%S").to_string()
}

/// The versions that a repository contributes to the metadata of an artifact.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contribution {
    /// The versions it lists.
    pub versions: Vec<String>,
    /// Its `lastUpdated` (`yyyyMMddHHmmss`), if known.
    pub last_updated: Option<String>,
}

/// How many versions are accepted from an upstream's metadata: the XML is already
/// bounded in bytes, this bounds what accumulates when merging.
pub const MAX_MERGED_VERSIONS: usize = 20_000;

/// What the files of a hosted repository contribute to the artifact.
///
/// A version counts when its **POM** exists: it is the last thing a complete
/// deployment leaves, and a version with a jar and no POM cannot be
/// resolved.
pub fn hosted_contribution(group_id: &str, artifact_id: &str, files: &[Known]) -> Contribution {
    let mut versions: BTreeMap<String, DateTime<Utc>> = BTreeMap::new();
    for known in files {
        if let Ok(Target::File(file)) = parse_path(&known.path)
            && file.group_id == group_id
            && file.artifact_id == artifact_id
            && file.is_pom()
        {
            let at = versions.entry(file.version).or_insert(known.updated_at);
            *at = (*at).max(known.updated_at);
        }
    }
    let last_updated = versions.values().max().map(|at| stamp(*at));
    Contribution {
        versions: versions.into_keys().collect(),
        last_updated,
    }
}

/// What metadata served by an upstream contributes to the artifact.
///
/// The metadata must be of the requested artifact: metadata of another
/// `groupId` or `artifactId` contributes nothing. Versions with a shape
/// that Maven does not admit are discarded instead of slipping into the
/// merged metadata.
pub fn declared_contribution(
    bytes: &[u8],
    group_id: &str,
    artifact_id: &str,
) -> Result<Contribution, XmlError> {
    let text = std::str::from_utf8(bytes).map_err(|_| XmlError::Malformed)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let leaves = leaves(text)?;
    if first(&leaves, "metadata/groupId").is_some_and(|declared| declared != group_id)
        || first(&leaves, "metadata/artifactId").is_some_and(|declared| declared != artifact_id)
    {
        return Ok(Contribution::default());
    }
    let versions: Vec<String> = leaves
        .iter()
        .filter(|(path, _)| path == "metadata/versioning/versions/version")
        .map(|(_, text)| text.as_str())
        .filter(|version| is_valid_version(version))
        .take(MAX_MERGED_VERSIONS)
        .map(str::to_string)
        .collect();
    let last_updated = first(&leaves, "metadata/versioning/lastUpdated")
        .filter(|stamp| stamp.len() == 14 && stamp.bytes().all(|byte| byte.is_ascii_digit()))
        .map(str::to_string);
    Ok(Contribution {
        versions,
        last_updated,
    })
}

/// Whether metadata is of these coordinates and lists **no**
/// version (not even an invalid one): this is the case of the metadata of a
/// plugin group, which a group serves as is.
///
/// Metadata of another artifact, or with versions that were discarded,
/// does not qualify: it is not served verbatim.
pub fn lists_no_versions(bytes: &[u8], group_id: &str, artifact_id: &str) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Ok(leaves) = leaves(text) else {
        return false;
    };
    first(&leaves, "metadata/groupId").is_none_or(|declared| declared == group_id)
        && first(&leaves, "metadata/artifactId").is_none_or(|declared| declared == artifact_id)
        && !leaves
            .iter()
            .any(|(path, _)| path == "metadata/versioning/versions/version")
}

/// The metadata of an artifact from what one or several
/// repositories contribute. Returns `None` if none contributes a version.
///
/// The result is the same for the same set of versions, regardless of
/// who contributes them or in what order: `latest` and `release` come
/// from Maven's version ordering and `lastUpdated` is the greatest known.
/// If there are more than `MAX_MERGED_VERSIONS`, the newest ones are
/// kept.
pub fn merged_metadata(
    group_id: &str,
    artifact_id: &str,
    contributions: &[Contribution],
) -> Option<String> {
    let seen: BTreeSet<&String> = contributions
        .iter()
        .flat_map(|contribution| contribution.versions.iter())
        .collect();
    if seen.is_empty() {
        return None;
    }
    let mut ordered: Vec<&String> = seen.into_iter().collect();
    sort_versions(&mut ordered);
    // The cap is applied on the sorted result and the newest ones are kept:
    // this way it does not depend on which member arrives first.
    if ordered.len() > MAX_MERGED_VERSIONS {
        ordered.drain(..ordered.len() - MAX_MERGED_VERSIONS);
    }
    let latest = ordered.last().copied()?;
    let release = ordered
        .iter()
        .rev()
        .find(|version| !version.ends_with(SNAPSHOT_SUFFIX));
    let last = contributions
        .iter()
        .filter_map(|contribution| contribution.last_updated.as_deref())
        .max()
        .map(str::to_string)
        .unwrap_or_else(|| stamp(DateTime::<Utc>::UNIX_EPOCH));

    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<metadata>\n");
    xml.push_str(&format!("  <groupId>{}</groupId>\n", encode(group_id)));
    xml.push_str(&format!(
        "  <artifactId>{}</artifactId>\n",
        encode(artifact_id)
    ));
    xml.push_str("  <versioning>\n");
    xml.push_str(&format!("    <latest>{}</latest>\n", encode(latest)));
    if let Some(release) = release {
        xml.push_str(&format!("    <release>{}</release>\n", encode(release)));
    }
    xml.push_str("    <versions>\n");
    for version in &ordered {
        xml.push_str(&format!("      <version>{}</version>\n", encode(version)));
    }
    xml.push_str("    </versions>\n");
    xml.push_str(&format!("    <lastUpdated>{last}</lastUpdated>\n"));
    xml.push_str("  </versioning>\n</metadata>\n");
    Some(xml)
}

/// The metadata of an artifact, derived from its files.
///
/// Returns `None` if there is no version with a POM.
pub fn artifact_metadata(group_id: &str, artifact_id: &str, files: &[Known]) -> Option<String> {
    merged_metadata(
        group_id,
        artifact_id,
        &[hosted_contribution(group_id, artifact_id, files)],
    )
}

/// Sorts versions without ever panicking.
///
/// Maven's comparison is not transitive in all cases
/// (`1.0-alpha < 1 < 1-sp < 1.0-alpha`) and the standard library's `sort_by`
/// can abort on an ordering that is not total. Binary insertion accepts
/// any comparator and is stable; the linear cost of the shifts
/// is negligible compared to the thousands of versions that fit in an artifact.
fn sort_versions(versions: &mut Vec<&String>) {
    let mut sorted: Vec<&String> = Vec::with_capacity(versions.len());
    for version in versions.drain(..) {
        let at =
            sorted.partition_point(|other| compare_versions(other, version) != Ordering::Greater);
        sorted.insert(at, version);
    }
    *versions = sorted;
}

/// The key of a build: classifier and extension.
type LatestKey = (Option<String>, String);

/// What is known about the most recent build: timestamp, number and date.
type LatestBuild = (String, u32, DateTime<Utc>);

/// The metadata of a snapshot, derived from its timestamped files.
///
/// A build counts when it has its POM; the `snapshot` is the most
/// recent of those that count and `snapshotVersions` carries, for each
/// classifier and extension pair, its most recent build. Returns `None`
/// if there is no timestamped build.
pub fn snapshot_metadata(
    group_id: &str,
    artifact_id: &str,
    version: &str,
    files: &[Known],
) -> Option<String> {
    struct Build {
        stamp: String,
        number: u32,
        at: DateTime<Utc>,
    }
    let mut parsed: Vec<(MavenFile, String, u32, DateTime<Utc>)> = Vec::new();
    for known in files {
        if let Ok(Target::File(file)) = parse_path(&known.path)
            && file.group_id == group_id
            && file.artifact_id == artifact_id
            && file.version == version
            && let VersionKind::Timestamped { stamp, build } = file.kind.clone()
        {
            parsed.push((file, stamp, build, known.updated_at));
        }
    }
    let with_pom: HashSet<(&str, u32)> = parsed
        .iter()
        .filter(|(file, ..)| file.is_pom())
        .map(|(_, stamp, build, _)| (stamp.as_str(), *build))
        .collect();
    let newest = with_pom.iter().max().copied()?;
    let newest = Build {
        stamp: newest.0.to_string(),
        number: newest.1,
        at: parsed
            .iter()
            .filter(|(_, stamp, build, _)| with_pom.contains(&(stamp.as_str(), *build)))
            .map(|(.., at)| *at)
            .max()?,
    };

    // For each (classifier, extension), the most recent build that
    // has a POM.
    let mut latest: BTreeMap<LatestKey, LatestBuild> = BTreeMap::new();
    for (file, stamp, build, at) in &parsed {
        if !with_pom.contains(&(stamp.as_str(), *build)) {
            continue;
        }
        let key = (file.classifier.clone(), file.extension.clone());
        let candidate = (stamp.clone(), *build, *at);
        match latest.get(&key) {
            Some(current) if (&current.0, current.1) >= (&candidate.0, candidate.1) => {}
            _ => {
                latest.insert(key, candidate);
            }
        }
    }

    let base = version.strip_suffix(SNAPSHOT_SUFFIX).unwrap_or(version);
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<metadata modelVersion=\"1.1.0\">\n");
    xml.push_str(&format!("  <groupId>{}</groupId>\n", encode(group_id)));
    xml.push_str(&format!(
        "  <artifactId>{}</artifactId>\n",
        encode(artifact_id)
    ));
    xml.push_str(&format!("  <version>{}</version>\n", encode(version)));
    xml.push_str("  <versioning>\n    <snapshot>\n");
    xml.push_str(&format!(
        "      <timestamp>{}</timestamp>\n      <buildNumber>{}</buildNumber>\n",
        newest.stamp, newest.number
    ));
    xml.push_str("    </snapshot>\n");
    xml.push_str(&format!(
        "    <lastUpdated>{}</lastUpdated>\n",
        stamp(newest.at)
    ));
    xml.push_str("    <snapshotVersions>\n");
    for ((classifier, extension), (stamp_value, build, at)) in &latest {
        xml.push_str("      <snapshotVersion>\n");
        if let Some(classifier) = classifier {
            xml.push_str(&format!(
                "        <classifier>{}</classifier>\n",
                encode(classifier)
            ));
        }
        xml.push_str(&format!(
            "        <extension>{}</extension>\n        <value>{}-{}-{}</value>\n        <updated>{}</updated>\n",
            encode(extension),
            encode(base),
            stamp_value,
            build,
            stamp(*at),
        ));
        xml.push_str("      </snapshotVersion>\n");
    }
    xml.push_str("    </snapshotVersions>\n  </versioning>\n</metadata>\n");
    Some(xml)
}

/// The checksum that is served or compared: lowercase hexadecimal.
///
/// A client may write the digest followed by the file name or
/// with a line break; only the first word counts.
pub fn declared_checksum(bytes: &[u8], algorithm: Algorithm) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let word = text.split_whitespace().next()?.to_ascii_lowercase();
    (word.len() == algorithm.hex_len() && word.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> MavenFile {
        match parse_path(path).expect("a valid path") {
            Target::File(file) => file,
            other => panic!("not a file: {other:?}"),
        }
    }

    #[test]
    fn a_release_file_is_split_into_its_coordinates() {
        let file = file("org/acme/tools/widget/1.2.3/widget-1.2.3.jar");
        assert_eq!(file.group_id, "org.acme.tools");
        assert_eq!(file.artifact_id, "widget");
        assert_eq!(file.version, "1.2.3");
        assert_eq!(file.kind, VersionKind::Release);
        assert_eq!(file.classifier, None);
        assert_eq!(file.extension, "jar");
    }

    #[test]
    fn classifier_and_compound_extensions_are_recognised() {
        let sources = file("org/acme/widget/1.0/widget-1.0-sources.jar");
        assert_eq!(sources.classifier.as_deref(), Some("sources"));
        assert_eq!(sources.extension, "jar");
        let archive = file("org/acme/widget/1.0/widget-1.0-bin.tar.gz");
        assert_eq!(archive.classifier.as_deref(), Some("bin"));
        assert_eq!(archive.extension, "tar.gz");
        let signature = file("org/acme/widget/1.0/widget-1.0.jar.asc");
        assert_eq!(signature.extension, "jar.asc");
    }

    #[test]
    fn a_timestamped_snapshot_keeps_the_directory_version() {
        let file = file("org/acme/widget/1.0-SNAPSHOT/widget-1.0-20240101.120000-3.jar");
        assert_eq!(file.version, "1.0-SNAPSHOT");
        assert_eq!(
            file.kind,
            VersionKind::Timestamped {
                stamp: "20240101.120000".to_string(),
                build: 3
            }
        );
        let floating = self::file("org/acme/widget/1.0-SNAPSHOT/widget-1.0-SNAPSHOT.pom");
        assert_eq!(floating.kind, VersionKind::Snapshot);
        assert!(floating.is_pom());
    }

    #[test]
    fn metadata_and_checksums_are_recognised() {
        assert_eq!(
            parse_path("org/acme/widget/maven-metadata.xml"),
            Ok(Target::Metadata(MetadataLevel::Artifact {
                group_id: "org.acme".to_string(),
                artifact_id: "widget".to_string()
            }))
        );
        assert_eq!(
            parse_path("org/acme/widget/1.0-SNAPSHOT/maven-metadata.xml"),
            Ok(Target::Metadata(MetadataLevel::Version {
                group_id: "org.acme".to_string(),
                artifact_id: "widget".to_string(),
                version: "1.0-SNAPSHOT".to_string()
            }))
        );
        match parse_path("org/acme/widget/1.0/widget-1.0.jar.sha1") {
            Ok(Target::Checksum {
                algorithm: Algorithm::Sha1,
                subject: Subject::File(_),
                subject_path,
            }) => assert_eq!(subject_path, "org/acme/widget/1.0/widget-1.0.jar"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse_path("org/acme/widget/maven-metadata.xml.md5"),
            Ok(Target::Checksum {
                algorithm: Algorithm::Md5,
                subject: Subject::Metadata(_),
                ..
            })
        ));
        // A checksum of a checksum does not exist.
        assert!(parse_path("org/acme/widget/1.0/widget-1.0.jar.sha1.sha1").is_err());
    }

    #[test]
    fn paths_that_are_not_maven_are_refused() {
        for (path, error) in [
            ("widget.jar", PathError::Layout),
            ("a/b/c.jar", PathError::Layout),
            ("org/acme/widget/1.0/widget-2.0.jar", PathError::FileName),
            ("org/acme/widget/1.0/other-1.0.jar", PathError::FileName),
            ("org/acme/widget/1.0/widget-1.0", PathError::FileName),
            ("org/acme/widget/1.0/widget-1.0.1.jar", PathError::FileName),
            ("org/acme/widget/-1.0/widget--1.0.jar", PathError::Version),
            ("org/acme/widget/1.0 beta/x.jar", PathError::Version),
            ("org/acme/widget/1.0/widget-1.0-.jar", PathError::FileName),
            (
                "org/.hidden/widget/1.0/widget-1.0.jar",
                PathError::Identifier,
            ),
            (
                "org/acme/widget/1.0-SNAPSHOT/widget-1.0-2024.jar",
                PathError::FileName,
            ),
        ] {
            assert_eq!(parse_path(path), Err(error), "{path}");
        }
    }

    #[test]
    fn versions_are_validated() {
        for valid in [
            "1",
            "1.0",
            "1.0-SNAPSHOT",
            "2.0.0-rc.1",
            "1.0+build5",
            "1_0",
        ] {
            assert!(is_valid_version(valid), "{valid}");
        }
        for invalid in [
            "",
            ".",
            "..",
            "-1",
            "1.",
            "1 0",
            "1/0",
            "a%20b",
            &"1".repeat(200),
        ] {
            assert!(!is_valid_version(invalid), "{invalid}");
        }
    }

    #[test]
    fn versions_are_ordered_like_maven_orders_them() {
        let ordered = [
            "1.0-alpha-1",
            "1.0-beta-1",
            "1.0-rc1",
            "1.0-SNAPSHOT",
            "1.0",
            "1.0-sp1",
            "1.0.1",
            "1.1",
            "1.9",
            "1.10",
            "2",
        ];
        for pair in ordered.windows(2) {
            assert_eq!(
                compare_versions(pair[0], pair[1]),
                Ordering::Less,
                "{pair:?}"
            );
            assert_eq!(compare_versions(pair[1], pair[0]), Ordering::Greater);
        }
        assert_eq!(compare_versions("1.0", "1.0"), Ordering::Equal);
    }

    #[test]
    fn a_pom_inherits_group_and_version_from_its_parent() {
        let pom = br#"<?xml version="1.0"?>
            <!-- a comment -->
            <project xmlns="http://maven.apache.org/POM/4.0.0">
              <modelVersion>4.0.0</modelVersion>
              <parent><groupId>org.acme</groupId><artifactId>root</artifactId><version>3.1</version></parent>
              <artifactId>widget</artifactId>
              <dependencies><dependency><groupId>x</groupId><artifactId>y</artifactId><version>9</version></dependency></dependencies>
            </project>"#;
        let coordinates = parse_pom(pom).expect("a valid POM");
        assert_eq!(
            coordinates,
            PomCoordinates {
                group_id: "org.acme".to_string(),
                artifact_id: "widget".to_string(),
                version: "3.1".to_string()
            }
        );
    }

    #[test]
    fn a_pom_that_is_not_usable_is_refused() {
        assert!(matches!(
            parse_pom(b"not xml"),
            Err(PomError::Xml(XmlError::Malformed))
        ));
        assert!(matches!(
            parse_pom(b"<project><artifactId>a</artifactId>"),
            Err(PomError::Xml(XmlError::Malformed))
        ));
        assert!(matches!(
            parse_pom(
                b"<project><modelVersion>4</modelVersion><artifactId>a</artifactId></project>"
            ),
            Err(PomError::Incomplete)
        ));
        assert!(matches!(
            parse_pom(b"<other><x>1</x></other>"),
            Err(PomError::NotAProject)
        ));
        assert!(matches!(
            parse_pom(b"<!DOCTYPE project [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><project/>"),
            Err(PomError::Xml(XmlError::Doctype))
        ));
        let deep = format!("{}{}", "<a>".repeat(100), "</a>".repeat(100));
        assert!(matches!(
            parse_pom(deep.as_bytes()),
            Err(PomError::Xml(XmlError::TooComplex))
        ));
    }

    fn known(path: &str, seconds: i64) -> Known {
        Known {
            path: path.to_string(),
            updated_at: DateTime::from_timestamp(1_700_000_000 + seconds, 0).unwrap(),
        }
    }

    #[test]
    fn artifact_metadata_lists_only_versions_that_have_a_pom() {
        let files = [
            known("org/acme/widget/1.0/widget-1.0.pom", 0),
            known("org/acme/widget/1.0/widget-1.0.jar", 1),
            known("org/acme/widget/1.10/widget-1.10.pom", 2),
            known("org/acme/widget/1.9/widget-1.9.pom", 3),
            known("org/acme/widget/2.0-SNAPSHOT/widget-2.0-SNAPSHOT.pom", 4),
            // A half-done version: the jar arrived and the POM did not.
            known("org/acme/widget/3.0/widget-3.0.jar", 5),
            // Another artifact with a similar prefix.
            known("org/acme/widget-extra/9.0/widget-extra-9.0.pom", 6),
        ];
        let xml = artifact_metadata("org.acme", "widget", &files).expect("metadata");
        assert!(xml.contains("<latest>2.0-SNAPSHOT</latest>"), "{xml}");
        assert!(xml.contains("<release>1.10</release>"), "{xml}");
        let versions: Vec<&str> = xml
            .lines()
            .filter_map(|line| line.trim().strip_prefix("<version>"))
            .filter_map(|line| line.strip_suffix("</version>"))
            .collect();
        assert_eq!(versions, ["1.0", "1.9", "1.10", "2.0-SNAPSHOT"]);
        assert!(
            xml.contains("<lastUpdated>20231114221324</lastUpdated>"),
            "{xml}"
        );
        // And the result is an XML that the service itself knows how to read.
        let declared = parse_metadata(xml.as_bytes()).expect("well formed");
        assert_eq!(declared.artifact_id.as_deref(), Some("widget"));
        assert!(artifact_metadata("org.acme", "ghost", &files).is_none());
    }

    #[test]
    fn snapshot_metadata_points_to_the_newest_complete_build() {
        let base = "org/acme/widget/1.0-SNAPSHOT/widget-1.0";
        let files = [
            known(&format!("{base}-20240101.120000-1.pom"), 0),
            known(&format!("{base}-20240101.120000-1.jar"), 1),
            known(&format!("{base}-20240102.080000-2.pom"), 2),
            known(&format!("{base}-20240102.080000-2.jar"), 3),
            known(&format!("{base}-20240102.080000-2-sources.jar"), 4),
            // A half-done build: without a POM it does not count.
            known(&format!("{base}-20240103.090000-3.jar"), 5),
            known(&format!("{base}-20240102.080000-2.jar.sha1"), 6),
        ];
        let xml = snapshot_metadata("org.acme", "widget", "1.0-SNAPSHOT", &files).expect("xml");
        assert!(
            xml.contains("<timestamp>20240102.080000</timestamp>"),
            "{xml}"
        );
        assert!(xml.contains("<buildNumber>2</buildNumber>"), "{xml}");
        assert!(
            xml.contains("<value>1.0-20240102.080000-2</value>"),
            "{xml}"
        );
        assert!(!xml.contains("20240103"), "{xml}");
        assert!(xml.contains("<classifier>sources</classifier>"), "{xml}");
        assert!(snapshot_metadata("org.acme", "widget", "2.0-SNAPSHOT", &files).is_none());
    }

    #[test]
    fn uploaded_metadata_must_agree_with_its_path() {
        let level = MetadataLevel::Artifact {
            group_id: "org.acme".to_string(),
            artifact_id: "widget".to_string(),
        };
        let good = parse_metadata(
            b"<metadata><groupId>org.acme</groupId><artifactId>widget</artifactId></metadata>",
        )
        .unwrap();
        assert!(metadata_matches(&level, &good));
        let other = parse_metadata(
            b"<metadata><groupId>org.acme</groupId><artifactId>other</artifactId></metadata>",
        )
        .unwrap();
        assert!(!metadata_matches(&level, &other));
        assert_eq!(parse_metadata(b"<metadata"), Err(XmlError::Malformed));
        assert_eq!(parse_metadata(b"<project/>"), Err(XmlError::Malformed));
    }

    #[test]
    fn a_checksum_is_read_as_its_first_word() {
        let sha1 = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
        assert_eq!(
            declared_checksum(format!("{sha1}  widget.jar\n").as_bytes(), Algorithm::Sha1),
            Some(sha1.to_string())
        );
        assert_eq!(
            declared_checksum(sha1.to_ascii_uppercase().as_bytes(), Algorithm::Sha1),
            Some(sha1.to_string())
        );
        assert_eq!(declared_checksum(b"zz", Algorithm::Sha1), None);
        assert_eq!(declared_checksum(sha1.as_bytes(), Algorithm::Md5), None);
    }

    #[test]
    fn the_configuration_defaults_and_refuses_what_it_does_not_know() {
        let none = serde_json::json!({});
        assert_eq!(MavenConfig::of_settings(&none), Ok(MavenConfig::default()));
        let release = serde_json::json!({"maven": {"version_policy": "release"}});
        let config = MavenConfig::of_settings(&release).unwrap();
        assert!(config.admits(false) && !config.admits(true));
        assert!(!config.release_redeploy);
        for bad in [
            serde_json::json!({"maven": {"version_policy": "weekly"}}),
            serde_json::json!({"maven": {"surprise": true}}),
            serde_json::json!({"maven": 3}),
        ] {
            assert!(MavenConfig::of_settings(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_xml_with_huge_element_names_is_refused_before_it_is_materialized() {
        // Chain of long names and thousands of leaves: without a cap, each leaf
        // would carry its full path and memory would multiply.
        let name = "n".repeat(10_000);
        let mut xml = String::new();
        for _ in 0..30 {
            xml.push_str(&format!("<{name}>"));
        }
        xml.push_str(&"<a/>".repeat(1_000));
        for _ in 0..30 {
            xml.push_str(&format!("</{name}>"));
        }
        assert!(matches!(
            parse_pom(xml.as_bytes()),
            Err(PomError::Xml(XmlError::TooComplex))
        ));
    }

    #[test]
    fn many_leaves_with_long_paths_hit_the_materialization_budget() {
        // Valid names (64 bytes) at 30 levels: ~2 KB per path.
        let name = "n".repeat(MAX_XML_NAME);
        let mut xml = String::new();
        for _ in 0..30 {
            xml.push_str(&format!("<{name}>"));
        }
        xml.push_str(&"<a/>".repeat(MAX_XML_ELEMENTS - 40));
        for _ in 0..30 {
            xml.push_str(&format!("</{name}>"));
        }
        assert!(matches!(
            parse_pom(xml.as_bytes()),
            Err(PomError::Xml(XmlError::TooComplex))
        ));
    }

    #[test]
    fn a_group_with_dots_in_a_directory_is_not_a_second_spelling_of_the_group() {
        assert!(parse_path("com.acme/lib/1.0/lib-1.0.pom").is_err());
        assert!(parse_path("com/acme/lib/1.0/lib-1.0.pom").is_ok());
        assert!(parse_path("com.acme/lib/maven-metadata.xml").is_err());
        assert!(parse_path("com.acme/lib/1.0-SNAPSHOT/maven-metadata.xml").is_err());
        // The artifact can indeed contain dots.
        assert!(parse_path("com/acme/my.lib/1.0/my.lib-1.0.pom").is_ok());
    }

    #[test]
    fn sorting_versions_never_panics_with_a_non_transitive_order() {
        // `1.0-alpha < 1 < 1-sp < 1.0-alpha`: with more than twenty elements the
        // standard `sort_by` can abort on an ordering that is not total.
        let mut owned: Vec<String> = vec!["1.0-alpha".into(), "1".into(), "1-sp".into()];
        for n in 0..60 {
            owned.push(format!("{n}.0-alpha"));
            owned.push(format!("{n}"));
            owned.push(format!("{n}-sp"));
        }
        let mut versions: Vec<&String> = owned.iter().collect();
        sort_versions(&mut versions);
        assert_eq!(versions.len(), owned.len());
    }

    fn upstream_metadata(group: &str, artifact: &str, versions: &[&str], updated: &str) -> String {
        let listed: String = versions
            .iter()
            .map(|version| format!("<version>{version}</version>"))
            .collect();
        format!(
            "<metadata><groupId>{group}</groupId><artifactId>{artifact}</artifactId>\
             <versioning><versions>{listed}</versions>\
             <lastUpdated>{updated}</lastUpdated></versioning></metadata>"
        )
    }

    #[test]
    fn upstream_metadata_contributes_its_versions_and_date() {
        let xml = upstream_metadata("org.acme", "widget", &["1.0", "1.1"], "20240101120000");
        let found = declared_contribution(xml.as_bytes(), "org.acme", "widget").expect("parses");
        assert_eq!(found.versions, vec!["1.0", "1.1"]);
        assert_eq!(found.last_updated.as_deref(), Some("20240101120000"));
    }

    #[test]
    fn upstream_metadata_of_another_artifact_contributes_nothing() {
        let xml = upstream_metadata("org.acme", "other", &["9.9"], "20240101120000");
        let found = declared_contribution(xml.as_bytes(), "org.acme", "widget").expect("parses");
        assert_eq!(found, Contribution::default());
    }

    #[test]
    fn upstream_versions_with_an_invalid_shape_are_dropped() {
        let xml = upstream_metadata(
            "org.acme",
            "widget",
            &["1.0", "../evil", "2.0"],
            "not-a-date",
        );
        let found = declared_contribution(xml.as_bytes(), "org.acme", "widget").expect("parses");
        assert_eq!(found.versions, vec!["1.0", "2.0"]);
        assert_eq!(found.last_updated, None);
    }

    #[test]
    fn only_metadata_of_these_coordinates_without_versions_is_served_verbatim() {
        let plugins = "<metadata><plugins><plugin><prefix>x</prefix></plugin></plugins></metadata>";
        assert!(lists_no_versions(plugins.as_bytes(), "org.acme", "widget"));
        // With versions (even if discarded) or of another artifact: no.
        let hostile = upstream_metadata("org.acme", "widget", &["../evil"], "20240101000000");
        assert!(!lists_no_versions(hostile.as_bytes(), "org.acme", "widget"));
        let foreign = upstream_metadata("org.evil", "other", &[], "20240101000000");
        assert!(!lists_no_versions(foreign.as_bytes(), "org.acme", "widget"));
        assert!(!lists_no_versions(
            b"<metadata><unclosed>",
            "org.acme",
            "widget"
        ));
    }

    #[test]
    fn the_version_cap_keeps_the_newest_whatever_the_order_of_the_members() {
        let many: Vec<String> = (0..MAX_MERGED_VERSIONS).map(|n| format!("1.{n}")).collect();
        let big = Contribution {
            versions: many,
            last_updated: None,
        };
        let newer = Contribution {
            versions: vec!["9.0".to_string()],
            last_updated: None,
        };
        let one = merged_metadata("org.acme", "widget", &[big.clone(), newer.clone()]);
        let other = merged_metadata("org.acme", "widget", &[newer, big]);
        assert_eq!(one, other);
        let xml = one.expect("merged");
        assert!(xml.contains("<version>9.0</version>"));
        assert!(!xml.contains("<version>1.0</version>"));
    }

    #[test]
    fn upstream_metadata_with_a_doctype_is_refused() {
        let xml = "<!DOCTYPE metadata [<!ENTITY x \"y\">]><metadata/>";
        assert_eq!(
            declared_contribution(xml.as_bytes(), "org.acme", "widget"),
            Err(XmlError::Doctype)
        );
    }

    #[test]
    fn merged_metadata_unions_versions_and_does_not_depend_on_the_order() {
        let first = Contribution {
            versions: vec!["1.0".into(), "1.2".into()],
            last_updated: Some("20240101000000".into()),
        };
        let second = Contribution {
            versions: vec!["1.1".into(), "1.2".into(), "2.0-SNAPSHOT".into()],
            last_updated: Some("20250101000000".into()),
        };
        let one = merged_metadata("org.acme", "widget", &[first.clone(), second.clone()])
            .expect("there are versions");
        let other = merged_metadata("org.acme", "widget", &[second, first]).expect("versions");
        assert_eq!(one, other);
        assert!(one.contains("<latest>2.0-SNAPSHOT</latest>"));
        assert!(one.contains("<release>1.2</release>"));
        assert!(one.contains("<lastUpdated>20250101000000</lastUpdated>"));
        let listed: Vec<&str> = one.matches("<version>").collect();
        assert_eq!(listed.len(), 4, "a repeated version is listed once");
    }

    #[test]
    fn merged_metadata_without_versions_is_nothing() {
        assert_eq!(merged_metadata("org.acme", "widget", &[]), None);
        assert_eq!(
            merged_metadata("org.acme", "widget", &[Contribution::default()]),
            None
        );
    }

    #[test]
    fn merged_metadata_of_one_hosted_repository_is_the_derived_one() {
        let files = vec![Known {
            path: "org/acme/widget/1.0/widget-1.0.pom".to_string(),
            updated_at: Utc::now(),
        }];
        let derived = artifact_metadata("org.acme", "widget", &files).expect("metadata");
        let contribution = hosted_contribution("org.acme", "widget", &files);
        assert_eq!(
            merged_metadata("org.acme", "widget", &[contribution]),
            Some(derived)
        );
    }
}
