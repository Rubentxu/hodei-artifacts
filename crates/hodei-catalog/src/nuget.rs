//! The **NuGet v3** grammar: identities, versions and documents.
//!
//! It is the pure part of the protocol: whatever needs effects (the
//! store, the proxy, authorization) lives in the handler. Here we
//! decide what a valid path is, which two versions are the same
//! version and what each document says, without touching a single comma of the
//! outside world.
//!
//! NuGet is **case-insensitive** by contract: the identifier
//! and the version travel in URLs always normalized (lowercase), and the
//! canonical package address is `flatcontainer/{id}/{version}`.
//! Requesting the path without normalizing names nothing: it is a 404, not an
//! alias.

use serde_json::{Value, json};

/// What the protocol does not accept, with the reason for the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NugetError {
    /// The path does not belong to the NuGet v3 grammar.
    InvalidPath(String),
    /// The package identifier is not valid.
    InvalidId(String),
    /// The version is not a NuGet version.
    InvalidVersion(String),
    /// The package body is not a usable nupkg.
    InvalidPackage(String),
}

impl NugetError {
    /// The message the client sees.
    pub fn message(&self) -> String {
        match self {
            Self::InvalidPath(what) => {
                format!("the path is not part of the NuGet v3 protocol: {what}")
            }
            Self::InvalidId(what) => format!("not a valid package id: {what}"),
            Self::InvalidVersion(what) => format!("not a valid NuGet version: {what}"),
            Self::InvalidPackage(what) => {
                format!("the uploaded package is not usable: {what}")
            }
        }
    }

    /// Whether the failure is a grammar one: the client named nothing, which is
    /// different from naming something wrong.
    pub fn is_path_error(&self) -> bool {
        matches!(self, Self::InvalidPath(_))
    }
}

/// A package identifier, already normalized: lowercase.
pub fn normalize_id(id: &str) -> String {
    id.to_ascii_lowercase()
}

/// Whether the identifier is one NuGet admits: letters, digits,
/// dot and hyphen, starting and ending with an alphanumeric.
pub fn valid_id(id: &str) -> Result<(), NugetError> {
    let bad = |why: &str| NugetError::InvalidId(format!("{id}: {why}"));
    if id.is_empty() || id.len() > 128 {
        return Err(bad("the id is empty or too long"));
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(bad("only letters, digits, dots and dashes are allowed"));
    }
    let first = id.as_bytes()[0];
    let last = id.as_bytes()[id.len() - 1];
    if !(first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric()) {
        return Err(bad("the id must start and end with a letter or a digit"));
    }
    Ok(())
}

/// The protocol path of a version: the package identity in the
/// flat container. `registrations/{id}` and the version index
/// live around it.
pub fn package_asset_path(id: &str, version: &str) -> String {
    format!("flatcontainer/{id}/{version}/{id}.nupkg")
}

// ---------------------------------------------------------------------------
// NuGet versions
// ---------------------------------------------------------------------------

/// A NuGet version: one to four numbers and, optionally, a prerelease
/// label. The build label (`+...`) is not identity:
/// two packages with the same version and a different label are the same
/// version, and the protocol discards it when normalizing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NuGetVersion {
    numbers: [u64; 4],
    pre: Vec<String>,
}

impl NuGetVersion {
    /// The number at the requested position.
    pub fn number(&self, index: usize) -> u64 {
        self.numbers[index]
    }

    /// Whether it is a prerelease: the version without a label comes first.
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    /// The string the protocol uses to name the version:
    /// at least three parts, the fourth only if it carries something, and no
    /// build label.
    pub fn normalized(&self) -> String {
        let mut core = if self.numbers[3] != 0 {
            format!(
                "{}.{}.{}.{}",
                self.numbers[0], self.numbers[1], self.numbers[2], self.numbers[3]
            )
        } else {
            format!(
                "{}.{}.{}",
                self.numbers[0], self.numbers[1], self.numbers[2]
            )
        };
        if !self.pre.is_empty() {
            core.push('-');
            core.push_str(&self.pre.join("."));
        }
        core
    }

    /// NuGet's ordering: numbers first, then the label
    /// (no label beats having a label, and between labels
    /// numeric segments are compared as numbers and the others as
    /// text, in lowercase).
    pub fn compare(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        for index in 0..4 {
            match self.numbers[index].cmp(&other.numbers[index]) {
                Ordering::Equal => {}
                other_order => return other_order,
            }
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (left, right) in self.pre.iter().zip(other.pre.iter()) {
                    let order = compare_pre(left, right);
                    if order != Ordering::Equal {
                        return order;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        }
    }
}

/// Two prerelease label segments: numeric ones are compared
/// as numbers (and come before textual ones), the rest as text
/// in lowercase.
fn compare_pre(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let left_number = left.parse::<u64>().ok();
    let right_number = right.parse::<u64>().ok();
    match (left_number, right_number) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()),
    }
}

/// The version parser: `major[.minor[.patch[.revision]]][-prerelease][+build]`.
pub fn parse_version(text: &str) -> Option<NuGetVersion> {
    let (core, _build) = text.split_once('+').unwrap_or((text, ""));
    let (core, pre) = match core.find('-') {
        Some(at) => (&core[..at], Some(core[at + 1..].to_string())),
        None => (core, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut numbers = [0u64; 4];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        numbers[index] = part.parse().ok()?;
    }
    let pre = match pre {
        None => Vec::new(),
        Some(label) => {
            if label.is_empty()
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
            {
                return None;
            }
            label
                .to_ascii_lowercase()
                .split('.')
                .map(str::to_string)
                .collect()
        }
    };
    Some(NuGetVersion { numbers, pre })
}

/// The normalized version exactly as the protocol writes it: the
/// flat container path asks for it this way.
pub fn normalize_version(text: &str) -> Option<String> {
    parse_version(text).map(|version| version.normalized())
}

// ---------------------------------------------------------------------------
// The package: nupkg and nuspec
// ---------------------------------------------------------------------------

/// What the server needs to know about an uploaded `.nupkg`: who it is
/// and which version it declares inside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageIdentity {
    /// The normalized package identifier.
    pub id: String,
    /// The normalized version declared by the nuspec.
    pub version: String,
}

/// The simplest XML identifier that works here: inside the `metadata`
/// of the nuspec, `<id>` and `<version>` are plain-text elements without
/// attributes or nesting. Looking for the closest closing tag from the start is
/// enough and does not drag an XML parser into the pure core.
fn tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(xml[start..end].trim())
}

/// The nuspec inside the zip: the exact name is `{id}.nuspec`, but
/// here the only `.nuspec` every package carries is enough.
fn nuspec_of(zip: &[u8]) -> Result<String, NugetError> {
    use std::io::Read;
    let reader = std::io::Cursor::new(zip);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|why| NugetError::InvalidPackage(format!("not a readable zip: {why}")))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|why| NugetError::InvalidPackage(format!("bad entry: {why}")))?;
        if !entry.name().to_ascii_lowercase().ends_with(".nuspec") {
            continue;
        }
        let mut xml = String::new();
        entry
            .read_to_string(&mut xml)
            .map_err(|why| NugetError::InvalidPackage(format!("unreadable nuspec: {why}")))?;
        return Ok(xml);
    }
    Err(NugetError::InvalidPackage(
        "the package contains no .nuspec".to_string(),
    ))
}

/// The identity declared by the package interior. It is the authority
/// on publication: the URL does not override what the nuspec says.
pub fn package_identity(zip: &[u8]) -> Result<PackageIdentity, NugetError> {
    let xml = nuspec_of(zip)?;
    let id = tag_text(&xml, "id")
        .filter(|text| !text.is_empty())
        .ok_or_else(|| NugetError::InvalidPackage("the nuspec has no id".to_string()))?
        .to_string();
    valid_id(&id)?;
    let version = tag_text(&xml, "version")
        .filter(|text| !text.is_empty())
        .ok_or_else(|| NugetError::InvalidPackage("the nuspec has no version".to_string()))?;
    let version = normalize_version(version).ok_or_else(|| {
        NugetError::InvalidPackage(format!("the nuspec version is not valid: {version}"))
    })?;
    Ok(PackageIdentity {
        id: normalize_id(&id),
        version,
    })
}

/// The nupkg inside the `multipart/form-data` that the real client uses
/// to publish: `dotnet nuget push` sends the package as a part
/// of the form, not as a raw body. **Only** the zip is returned,
/// never the envelope: identity and digest are measured on the nupkg.
pub fn multipart_package(body: &[u8]) -> Result<Vec<u8>, NugetError> {
    let no_package =
        || NugetError::InvalidPackage("the upload is a form without a nupkg file part".to_string());
    let find = |needle: &[u8], from: usize| -> Option<usize> {
        if needle.is_empty() || from >= body.len() {
            return None;
        }
        body[from..]
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|position| position + from)
    };
    if !body.starts_with(b"--") {
        return Err(no_package());
    }
    let line_end = find(b"\r\n", 2).ok_or_else(no_package)?;
    let boundary = &body[2..line_end];
    if boundary.is_empty() || boundary.len() > 70 {
        return Err(no_package());
    }
    let mut delimiter = b"\r\n--".to_vec();
    delimiter.extend_from_slice(boundary);

    let mut cursor = line_end + 2;
    let mut fallback: Option<Vec<u8>> = None;
    loop {
        let next = find(&delimiter, cursor);
        let segment = match next {
            Some(index) => &body[cursor..index],
            None => &body[cursor..],
        };
        if let Some(separator) = segment
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| position + 4)
        {
            let headers = segment[..separator].to_ascii_lowercase();
            let payload = segment[separator..].to_vec();
            let mentions = |needle: &[u8]| headers.windows(needle.len()).any(|w| w == needle);
            if mentions(b"filename=") {
                if mentions(b".nupkg") {
                    return Ok(payload);
                }
                fallback.get_or_insert(payload);
            }
        }
        match next {
            None => break,
            Some(index) => {
                let after = index + delimiter.len();
                if body.get(after..after + 2) == Some(b"--") {
                    break;
                }
                cursor = find(b"\r\n", after).map_or(body.len(), |end| end + 2);
                if cursor >= body.len() {
                    break;
                }
            }
        }
    }
    fallback.ok_or_else(no_package)
}

// ---------------------------------------------------------------------------
// Protocol documents
// ---------------------------------------------------------------------------

/// The `index.json` of the flat container: the v3 contract defines it as an
/// array of **strings** (`{"versions": ["1.0.0"]}`), not of objects: the
/// NuGet client deserializes `versions` as `List<string>` and an object
/// tastes like broken JSON to it. The latest version goes last. The identities are already
/// normalized.
pub fn version_index(versions: &[String]) -> Value {
    json!({
        "count": versions.len(),
        "versions": versions,
    })
}

/// Single registration leaf item for `(id, version)`.
pub fn registration_item(id: &str, version: &str) -> Value {
    json!({
        "@id": format!("packages/{id}/{version}"),
        "packageContent": format!("flatcontainer/{id}/{version}/{id}.nupkg"),
        "catalogEntry": {
            "id": id,
            "version": version,
            "packageStatus": "validated",
        },
    })
}

/// Composes a registration leaf document from pre-resolved per-version items.
pub fn registration_document(id: &str, latest_version: &str, items: Vec<Value>) -> Value {
    json!({
        "@id": format!("packages/{id}/index.json"),
        "@type": ["PackageRegistrationLeaf", "catalog:entry"],
        "version": latest_version,
        "votes": 0,
        "packageStatus": "validated",
        "items": items,
    })
}

/// The registration leaf of a package: the `items` per version with their
/// flatcontainer address. The whole catalog (registration index and
/// leaf with full dependencies) is future scope; this leaf is
/// what the client needs to resolve id and version.
pub fn registration_leaf(id: &str, versions: &[String]) -> Value {
    let items = versions
        .iter()
        .map(|version| registration_item(id, version))
        .collect::<Vec<_>>();
    let latest = versions.last().map(String::as_str).unwrap_or_default();
    registration_document(id, latest, items)
}

/// Extracts a registration leaf item for `(id, version)` from an upstream
/// registration document (supporting both flat `items` and paged `items[].items[]`),
/// rewriting `@id` and `packageContent` to relative group paths so upstream
/// origins are never leaked or mixed.
pub fn extract_registration_item(doc: &Value, id: &str, version: &str) -> Option<Value> {
    let target_version = normalize_version(version)?;
    let items = doc.get("items")?.as_array()?;
    for entry in items {
        if let Some(found) = match_registration_entry(entry, id, &target_version) {
            return Some(found);
        }
        if let Some(page_items) = entry.get("items").and_then(|v| v.as_array()) {
            for sub in page_items {
                if let Some(found) = match_registration_entry(sub, id, &target_version) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn match_registration_entry(entry: &Value, id: &str, target_version: &str) -> Option<Value> {
    let catalog_entry = entry.get("catalogEntry")?;
    let entry_version = catalog_entry.get("version")?.as_str()?;
    if normalize_version(entry_version).as_deref() != Some(target_version) {
        return None;
    }
    let mut cloned = entry.clone();
    if let Some(obj) = cloned.as_object_mut() {
        obj.insert(
            "@id".to_string(),
            Value::String(format!("packages/{id}/{target_version}")),
        );
        obj.insert(
            "packageContent".to_string(),
            Value::String(format!(
                "flatcontainer/{id}/{target_version}/{id}.{target_version}.nupkg"
            )),
        );
    }
    Some(cloned)
}

/// Resource base URLs discovered from an upstream NuGet v3 service index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIndexResources {
    /// Base URL for `PackageBaseAddress/3.0.0` (flat container).
    pub flat_container: String,
    /// Base URL for `RegistrationsBaseUrl`, if advertised.
    pub registrations: Option<String>,
}

/// Extracts `PackageBaseAddress` and `RegistrationsBaseUrl` from a NuGet v3
/// service index JSON document.
pub fn service_index_resources(doc: &Value) -> Option<ServiceIndexResources> {
    let resources = doc.get("resources")?.as_array()?;
    let mut flat_container: Option<String> = None;
    let mut registrations: Option<String> = None;
    for resource in resources {
        let Some(id_url) = resource.get("@id").and_then(Value::as_str) else {
            continue;
        };
        let types: Vec<&str> = match resource.get("@type") {
            Some(Value::String(s)) => vec![s.as_str()],
            Some(Value::Array(arr)) => arr.iter().filter_map(Value::as_str).collect(),
            _ => continue,
        };
        for ty in types {
            if ty == "PackageBaseAddress/3.0.0"
                || (flat_container.is_none() && ty.starts_with("PackageBaseAddress"))
            {
                flat_container = Some(id_url.to_string());
            }
            if ty.starts_with("RegistrationsBaseUrl") {
                // Prefer plain JSON endpoints (without `-gz-`) when multiple exist.
                let is_gz = id_url.contains("-gz-");
                if registrations.is_none()
                    || (!is_gz && registrations.as_ref().is_some_and(|u| u.contains("-gz-")))
                {
                    registrations = Some(id_url.to_string());
                }
            }
        }
    }
    Some(ServiceIndexResources {
        flat_container: flat_container?,
        registrations,
    })
}

/// What the flat container path asks for, if it asks for anything:
/// `flatcontainer/{id}/{version}/{id}.nupkg` or `{id}/index.json`.
/// Everything is lowercase; unnormalized uppercase names nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlatRequest {
    /// The package's version list.
    Versions {
        /// The package, in lowercase.
        id: String,
    },
    /// A specific file of a version: the nupkg and companions
    /// such as the loose `.nuspec` files some clients request.
    File {
        /// The package, in lowercase.
        id: String,
        /// The version, as the path names it.
        version: String,
        /// The file name inside the version.
        file: String,
    },
}

/// The flat path parser: separates identity from file without touching
/// the store.
pub fn parse_flat(path: &str) -> Result<FlatRequest, NugetError> {
    let rest = path
        .strip_prefix("flatcontainer/")
        .ok_or_else(|| NugetError::InvalidPath(path.to_string()))?;
    let mut parts = rest.split('/');
    let raw_id = parts.next().unwrap_or_default();
    if raw_id.is_empty() || raw_id.to_ascii_lowercase() != raw_id {
        return Err(NugetError::InvalidPath(format!(
            "the flat container address must be lowercase: {raw_id}"
        )));
    }
    valid_id(raw_id)?;
    match (parts.next(), parts.next(), parts.next()) {
        (Some("index.json"), None, None) => Ok(FlatRequest::Versions {
            id: raw_id.to_string(),
        }),
        (Some(raw_version), Some(file), None) => {
            if file.to_ascii_lowercase() != file {
                return Err(NugetError::InvalidPath(format!(
                    "the flat container address must be lowercase: {file}"
                )));
            }
            let version = if raw_version.to_ascii_lowercase() == raw_version {
                raw_version.to_string()
            } else {
                return Err(NugetError::InvalidPath(format!(
                    "the flat container address must be lowercase: {raw_version}"
                )));
            };
            Ok(FlatRequest::File {
                id: raw_id.to_string(),
                version,
                file: file.to_string(),
            })
        }
        _ => Err(NugetError::InvalidPath(path.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Paths and service index
// ---------------------------------------------------------------------------

/// What a `/nuget/{repositorio}/...` path asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NuGetRoute {
    /// The service index: what is in this feed and where.
    ServiceIndex,
    /// Something from the flat container: the version list or a file.
    Flat(FlatRequest),
    /// The package's registration leaf.
    Registration {
        /// The package, in lowercase.
        id: String,
    },
    /// Publishing a package through the Publish endpoint. The
    /// identity in the URL is optional because the real client does
    /// `PUT {publish}/` with the nupkg inside a multipart form
    /// and **without** id or version in the path: the nuspec rules.
    Publish {
        /// The requested package, normalized, if the URL promises it.
        id: Option<String>,
        /// The requested version, normalized, if the URL carries it.
        version: Option<String>,
    },
}

impl NuGetRoute {
    /// The package the path points to, if it points to one: it is what
    /// the audit records.
    pub fn package(&self) -> Option<&str> {
        match self {
            Self::ServiceIndex => None,
            Self::Flat(FlatRequest::Versions { id }) => Some(id),
            Self::Flat(FlatRequest::File { id, .. }) => Some(id),
            Self::Registration { id } => Some(id),
            Self::Publish { id, .. } => id.as_deref(),
        }
    }
}

/// The path parser: the index, the flat container, the registrations and the
/// publish endpoint. The path arrives undecoded and raw.
pub fn parse_route(rest: &str) -> Result<NuGetRoute, NugetError> {
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() || rest == "v3/index.json" || rest == "index.json" {
        return Ok(NuGetRoute::ServiceIndex);
    }
    if rest.starts_with("flatcontainer/") {
        return Ok(NuGetRoute::Flat(parse_flat(rest)?));
    }
    if let Some(registration) = rest.strip_prefix("registrations/") {
        let id = registration
            .strip_suffix("/index.json")
            .ok_or_else(|| NugetError::InvalidPath(rest.to_string()))?;
        if id.to_ascii_lowercase() != id {
            return Err(NugetError::InvalidPath(format!(
                "the registrations address must be lowercase: {id}"
            )));
        }
        valid_id(id)?;
        return Ok(NuGetRoute::Registration { id: id.to_string() });
    }
    if let Some(publish) = rest.strip_prefix("package/") {
        // The real client publishes against the empty address: the nuspec
        // is the only identity in play.
        if publish.trim_end_matches('/').is_empty() {
            return Ok(NuGetRoute::Publish {
                id: None,
                version: None,
            });
        }
        let (id, version) = publish
            .split_once('/')
            .ok_or_else(|| NugetError::InvalidPath(rest.to_string()))?;
        if version.contains('/') || version.is_empty() {
            return Err(NugetError::InvalidPath(rest.to_string()));
        }
        valid_id(id)?;
        let version = normalize_version(version)
            .ok_or_else(|| NugetError::InvalidVersion(version.to_string()))?;
        return Ok(NuGetRoute::Publish {
            id: Some(normalize_id(id)),
            version: Some(version),
        });
    }
    Err(NugetError::InvalidPath(rest.to_string()))
}

/// The feed's service index. The addresses it declares are
/// absolute, as the client sees them: the flat container is the
/// operation that resolves versions, the registrations the one that lists them
/// for search, and Publish the one that writes.
pub fn service_index(base: &str) -> Value {
    json!({
        "@context": { "@vocab": "http://schema.nuget.org/services#" },
        "version": "3.0.0",
        "resources": [
            {
                "@id": format!("{base}/v3/index.json"),
                "@type": "ServiceIndex/3.0.0",
            },
            {
                "@id": format!("{base}/flatcontainer"),
                "@type": "PackageBaseAddress/3.0.0",
                "comment": "Base URL of the flat container",
            },
            {
                "@id": format!("{base}/registrations"),
                "@type": "RegistrationsBaseUrl/3.6.0",
                "comment": "Base URL of package registrations",
            },
            {
                "@id": format!("{base}/package"),
                "@type": "PackagePublish/2.0.0",
                "comment": "Push with PUT {id}/{version} or a multipart form, and the X-NuGet-ApiKey header",
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> NuGetVersion {
        parse_version(text).expect("a valid version")
    }

    #[test]
    fn missing_parts_read_as_zero() {
        assert_eq!(version("1").numbers, [1, 0, 0, 0]);
        assert_eq!(version("1.2").numbers, [1, 2, 0, 0]);
        assert_eq!(version("1.2.3.4").numbers, [1, 2, 3, 4]);
    }

    #[test]
    fn normalization_keeps_at_least_three_parts_and_drops_the_fourth_when_empty() {
        assert_eq!(version("1.0").normalized(), "1.0.0");
        assert_eq!(version("1.0.0").normalized(), "1.0.0");
        assert_eq!(version("1.0.0.0").normalized(), "1.0.0");
        assert_eq!(version("1.0.0.1").normalized(), "1.0.0.1");
    }

    #[test]
    fn leading_zeros_are_not_identity() {
        assert_eq!(version("01.2.03").normalized(), "1.2.3");
    }

    #[test]
    fn the_build_label_is_not_part_of_the_identity() {
        assert_eq!(version("1.2.3+build.9").normalized(), "1.2.3");
        assert_eq!(
            version("1.2.3+build.9"),
            version("1.2.3+build.10"),
            "the same version is the same version"
        );
    }

    #[test]
    fn prerelease_labels_are_lowercase_identity() {
        assert_eq!(version("1.0.0-Beta.2").normalized(), "1.0.0-beta.2");
        assert_eq!(version("1.0.0-alpha.01").normalized(), "1.0.0-alpha.01");
    }

    #[test]
    fn ordering_follows_the_nuget_rules() {
        use std::cmp::Ordering;
        assert_eq!(version("1.0.0").compare(&version("1.0.1")), Ordering::Less);
        assert_eq!(version("2").compare(&version("1.9.9")), Ordering::Greater);
        assert_eq!(
            version("1.0.0.2").compare(&version("1.0.1")),
            Ordering::Less
        );
        // no label beats having a label
        assert_eq!(
            version("1.0.0").compare(&version("1.0.0-beta")),
            Ordering::Greater
        );
        assert_eq!(
            version("1.0.0-beta").compare(&version("1.0.0-beta.1")),
            Ordering::Less
        );
        // numeric label segments come before textual ones
        assert_eq!(
            version("1.0.0-alpha").compare(&version("1.0.0-1")),
            Ordering::Greater
        );
        assert_eq!(
            version("1.0.0-beta.2").compare(&version("1.0.0-beta.10")),
            Ordering::Less
        );
        assert_eq!(
            version("1.0.0-beta").compare(&version("1.0.0-BETA")),
            Ordering::Equal
        );
    }

    #[test]
    fn nonsense_versions_are_rejected() {
        for text in [
            "",
            "1..2",
            "a.b.c",
            "1.2.3.4.5",
            "1.2.3-",
            "1.2.-3",
            "-1.0.0",
        ] {
            assert!(parse_version(text).is_none(), "{text} is not a version");
        }
    }

    #[test]
    fn ids_are_lowercased_and_shaped() {
        assert_eq!(normalize_id("Newtonsoft.Json"), "newtonsoft.json");
        assert!(valid_id("Newtonsoft.Json").is_ok());
        assert!(valid_id("a-b-c").is_ok());
        for bad in [
            "",
            ".hidden",
            "trailing.",
            "space id",
            "under_score",
            &"a".repeat(129),
        ] {
            assert!(valid_id(bad).is_err(), "{bad:?} is not an id");
        }
    }

    #[test]
    fn the_flat_address_is_the_canonical_one() {
        assert_eq!(
            package_asset_path("newtonsoft.json", "13.0.1"),
            "flatcontainer/newtonsoft.json/13.0.1/newtonsoft.json.nupkg"
        );
    }

    #[test]
    fn flat_paths_parse_only_when_normalised() {
        assert_eq!(
            parse_flat("flatcontainer/sample.lib/index.json").unwrap(),
            FlatRequest::Versions {
                id: "sample.lib".to_string()
            }
        );
        assert_eq!(
            parse_flat("flatcontainer/sample.lib/13.0.1/sample.lib.nupkg").unwrap(),
            FlatRequest::File {
                id: "sample.lib".to_string(),
                version: "13.0.1".to_string(),
                file: "sample.lib.nupkg".to_string(),
            }
        );
        // unnormalized uppercase names nothing: grammar, not identity
        assert!(
            parse_flat("flatcontainer/Sample.Lib/13.0.1/index.json")
                .unwrap_err()
                .is_path_error()
        );
        assert!(
            parse_flat("flatcontainer/sample.lib/13.0.1/Sample.Lib.NUPKG")
                .unwrap_err()
                .is_path_error()
        );
        assert!(
            parse_flat("flatcontainer/sample.lib/13.0.1")
                .unwrap_err()
                .is_path_error()
        );
        assert!(parse_flat("others/sample.lib").unwrap_err().is_path_error());
        // an id with a slash is not an id: the grammar rejects it first
        assert!(parse_flat("flatcontainer/acr/id/13.0.1/index.json").is_err());
    }

    #[test]
    fn the_index_lists_versions_in_order() {
        let versions = ["1.0.0".to_string(), "1.1.0".to_string()];
        let index = version_index(&versions);
        assert_eq!(index["count"], 2);
        assert_eq!(index["versions"][1], "1.1.0");
    }

    #[test]
    fn the_registration_leaf_points_at_the_flat_container() {
        let versions = ["1.0.0".to_string(), "1.1.0".to_string()];
        let leaf = registration_leaf("acr/id", &versions);
        assert_eq!(leaf["version"], "1.1.0");
        assert_eq!(
            leaf["items"][0]["packageContent"],
            "flatcontainer/acr/id/1.0.0/acr/id.nupkg"
        );
    }

    /// A toy zip with the nuspec inside, compressed or not.
    fn nupkg(entries: &[(&str, &str)], deflate: bool) -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = if deflate {
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
        } else {
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
        };
        for (name, body) in entries {
            writer.start_file(*name, options).expect("the entry opens");
            writer.write_all(body.as_bytes()).expect("written");
        }
        writer.finish().expect("the zip closes").into_inner()
    }

    fn nuspec(id: &str, version: &str) -> String {
        format!(
            r#"<?xml version="1.0"?>
<package xmlns="http://schemas.microsoft.com/packaging/2013/05/nuspec.xsd">
  <metadata>
    <id>{id}</id>
    <version>{version}</version>
    <authors>a</authors>
  </metadata>
</package>"#
        )
    }

    #[test]
    fn the_nuspec_inside_the_package_is_the_authority() {
        let zip = nupkg(
            &[("Sample.nuspec", &nuspec("Sample.Lib", "01.2.0+build"))],
            true,
        );
        let identity = package_identity(&zip).expect("a package");
        assert_eq!(
            identity,
            PackageIdentity {
                id: "sample.lib".to_string(),
                version: "1.2.0".to_string(),
            }
        );
    }

    #[test]
    fn packages_that_are_not_packages_are_rejected() {
        assert!(
            package_identity(&nupkg(&[("readme.md", "hi")], false))
                .unwrap_err()
                .message()
                .contains("no .nuspec")
        );
        assert!(package_identity(b"not a zip at all").is_err());
        let zip = nupkg(&[("a.nuspec", &nuspec("bad id!", "1.0.0"))], false);
        assert!(package_identity(&zip).is_err());
        let zip = nupkg(&[("a.nuspec", &nuspec("fine", "nope"))], false);
        assert!(package_identity(&zip).is_err());
    }

    #[test]
    fn routes_are_recognised_by_their_grammar() {
        assert_eq!(parse_route("").unwrap(), NuGetRoute::ServiceIndex);
        assert_eq!(
            parse_route("v3/index.json").unwrap(),
            NuGetRoute::ServiceIndex
        );
        assert_eq!(
            parse_route("flatcontainer/sample.lib/index.json").unwrap(),
            NuGetRoute::Flat(FlatRequest::Versions {
                id: "sample.lib".to_string()
            })
        );
        assert_eq!(
            parse_route("registrations/sample.lib/index.json").unwrap(),
            NuGetRoute::Registration {
                id: "sample.lib".to_string()
            }
        );
        // The push names it however it wants: the path is normalized, not rejected.
        assert_eq!(
            parse_route("package/Sample.Lib/01.2.0+meta").unwrap(),
            NuGetRoute::Publish {
                id: Some("sample.lib".to_string()),
                version: Some("1.2.0".to_string()),
            }
        );
        // The real client names nothing in the URL: the bare publish
        // address is also a valid push path.
        assert_eq!(
            parse_route("package/").unwrap(),
            NuGetRoute::Publish {
                id: None,
                version: None,
            }
        );
        assert!(parse_route("registrations/Sample.Lib/index.json").is_err());
        assert!(parse_route("package/sample.lib").is_err());
        assert!(parse_route("package/sample.lib/1.0.0/extra").is_err());
        assert!(parse_route("package/sample.lib/notaversion").is_err());
        assert!(parse_route("service/query?q=x").is_err());
    }

    #[test]
    fn the_form_upload_hides_the_nupkg_behind_the_envelope() {
        let zip = nupkg(&[("Sample.nuspec", &nuspec("Sample.Form", "1.0.0"))], false);
        let mut body = "--7ca4\r\nContent-Disposition: form-data; name=\"description\"\r\n\r\nignored\r\n\
             --7ca4\r\nContent-Disposition: form-data; name=\"package\"; filename=\"sample.form.1.0.0.nupkg\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            .as_bytes()
            .to_vec();
        body.extend(zip.clone());
        body.extend(b"\r\n--7ca4--\r\n".iter().copied());
        assert_eq!(multipart_package(&body).expect("the nupkg part"), zip);
        // A body that is not a form is left alone: the raw zip is
        // recognized by its header, not by guessing.
        assert!(multipart_package(&zip).is_err());
        assert!(
            multipart_package(
                b"--7ca4\r\nContent-Disposition: form-data; name=\"notes\"\r\n\r\nhi\r\n--7ca4--\r\n"
            )
            .is_err()
        );
    }

    #[test]
    fn the_service_index_declares_the_operations_the_client_needs() {
        let index = service_index("http://127.0.0.1:9/nuget/feed");
        let types: Vec<&str> = index["resources"]
            .as_array()
            .expect("resources")
            .iter()
            .map(|resource| resource["@type"].as_str().expect("a type"))
            .collect();
        assert!(types.contains(&"PackageBaseAddress/3.0.0"));
        assert!(types.contains(&"RegistrationsBaseUrl/3.6.0"));
        assert!(types.contains(&"PackagePublish/2.0.0"));
        assert_eq!(
            index["resources"][1]["@id"],
            "http://127.0.0.1:9/nuget/feed/flatcontainer"
        );
    }
}
