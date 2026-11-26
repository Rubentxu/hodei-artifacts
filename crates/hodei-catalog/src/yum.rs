//! The grammar and documents of the RPM format (yum/dnf), pure and
//! effect-free: here an `.rpm` is opened, its header is read, and the
//! `primary` entry and the `repomd` are rendered. None of this touches the store or the
//! network: the handler decides when those bytes go out or come in.
//!
//! The contract is `createrepo_c`'s, which is what `dnf` expects to
//! read. A malformed index is not a nuance: dnf discards the
//! repository and the real client stops seeing the package.

use thiserror::Error;

/// Format failures: everything that smells of a broken rpm or unusable
/// metadata, stated with a name and a reason.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum YumError {
    /// The file is not an `.rpm`: missing magic numbers, a header that
    /// is not where it should be or an index that runs outside the file.
    #[error("the file is not a valid rpm container: {0}")]
    BadContainer(&'static str),
    /// A mandatory header field is missing: without it there is no package.
    #[error("the rpm header lacks the required field {0}")]
    MissingField(&'static str),
    /// The identifier does not fit in a safe pool path.
    #[error("the package identity is not filesystem safe: {0}")]
    BadIdentity(String),
}

/// What an `.rpm` declares about itself in its header: the fields
/// dnf needs to choose and to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpmMetadata {
    /// Package name.
    pub name: String,
    /// Epoch, if the spec declares it. Without epoch, dnf takes it as 0.
    pub epoch: Option<u32>,
    /// Upstream version.
    pub version: String,
    /// Packaging release (`1.fc40`).
    pub release: String,
    /// Binary architecture: `x86_64`, `aarch64`, `noarch`...
    pub architecture: String,
    /// One-line summary.
    pub summary: String,
    /// Long description, with its paragraphs.
    pub description: String,
    /// Declared license (SPDX in modern distros).
    pub license: Option<String>,
    /// Installed size: the header's `SIZE`, the one dnf shows when searching.
    pub installed_size: u64,
    /// Build time, in Unix seconds: it is the date that
    /// `createrepo` puts in `<time>` and therefore belongs to the file, not to the clock.
    pub build_time: u32,
}

// The tag numbers are those of rpm's `rpmtag.h`, pinned and
// stable since the nineties: the reader is guided by the tag, not by the
// type field, just as the suite's own tools do.
const TAG_NAME: i32 = 1000;
const TAG_VERSION: i32 = 1001;
const TAG_RELEASE: i32 = 1002;
const TAG_EPOCH: i32 = 1003;
const TAG_SUMMARY: i32 = 1004;
const TAG_DESCRIPTION: i32 = 1005;
const TAG_BUILDTIME: i32 = 1006;
const TAG_SIZE: i32 = 1009;
const TAG_LICENSE: i32 = 1014;
const TAG_ARCH: i32 = 1022;

/// A header index entry: tag, type, offset within the store and count.
struct IndexEntry {
    tag: i32,
    offset: u32,
}

/// The entries and the store of a header (`0x8e 0xad 0xe8`) at the
/// given position, with the end of the store so the
/// next header can be aligned.
fn read_header(bytes: &[u8], at: usize) -> Result<(Vec<IndexEntry>, usize, usize), YumError> {
    let complaint = |reason| Err(YumError::BadContainer(reason));
    if at + 16 > bytes.len() {
        return complaint("the header begins past the end of the file");
    }
    if bytes[at] != 0x8e || bytes[at + 1] != 0xad || bytes[at + 2] != 0xe8 {
        return complaint("the header magic is missing");
    }
    let count = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().expect("four bytes")) as usize;
    let hsize =
        u32::from_be_bytes(bytes[at + 12..at + 16].try_into().expect("four bytes")) as usize;
    let index_end = at
        .checked_add(16 + count * 16)
        .filter(|end| *end <= bytes.len())
        .ok_or(YumError::BadContainer(
            "the index runs past the end of the file",
        ))?;
    let store = index_end;
    let store_end = store
        .checked_add(hsize)
        .filter(|end| *end <= bytes.len())
        .ok_or(YumError::BadContainer(
            "the store runs past the end of the file",
        ))?;
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let p = at + 16 + i * 16;
        entries.push(IndexEntry {
            tag: i32::from_be_bytes(bytes[p..p + 4].try_into().expect("four bytes")),
            offset: u32::from_be_bytes(bytes[p + 8..p + 12].try_into().expect("four bytes")),
        });
    }
    Ok((entries, store, store_end))
}

/// The string the tag points at in the store, or nothing if the tag is missing.
fn entry_string(bytes: &[u8], entries: &[IndexEntry], store: usize, tag: i32) -> Option<String> {
    let entry = entries.iter().find(|entry| entry.tag == tag)?;
    let start = store.checked_add(entry.offset as usize)?;
    let text = bytes.get(start..)?;
    let end = text.iter().position(|byte| *byte == 0)?;
    Some(String::from_utf8_lossy(&text[..end]).into_owned())
}

/// The u32 the tag points at in the store.
fn entry_u32(bytes: &[u8], entries: &[IndexEntry], store: usize, tag: i32) -> Option<u32> {
    let entry = entries.iter().find(|entry| entry.tag == tag)?;
    let start = store.checked_add(entry.offset as usize)?;
    let four = bytes.get(start..start + 4)?;
    Some(u32::from_be_bytes(four.try_into().expect("four bytes")))
}

/// Opens an `.rpm`: the 96-byte lead, the signature header (padded to
/// 8 afterwards) and the main header, from which the fields come. Only
/// the main one is needed to index: the payload signatures are
/// a matter for dnf's gpgcheck, not for the pool grammar.
pub fn parse_rpm(bytes: &[u8]) -> Result<RpmMetadata, YumError> {
    if bytes.len() < 96 {
        return Err(YumError::BadContainer("the file is shorter than the lead"));
    }
    if &bytes[0..4] != b"\xed\xab\xee\xdb" {
        return Err(YumError::BadContainer("the lead magic is missing"));
    }
    // Signature header: it may be empty, but its structure must
    // be opened to know where the main one starts.
    let (_, _, signature_end) = read_header(bytes, 96)?;
    let main_at = signature_end + (8 - signature_end % 8) % 8;
    let (entries, store, _) = read_header(bytes, main_at)?;

    let field = |tag: i32, name| -> Result<String, YumError> {
        entry_string(bytes, &entries, store, tag).ok_or(YumError::MissingField(name))
    };
    let name = field(TAG_NAME, "NAME")?;
    let version = field(TAG_VERSION, "VERSION")?;
    let release = field(TAG_RELEASE, "RELEASE")?;
    let architecture = field(TAG_ARCH, "ARCH")?;
    let summary = field(TAG_SUMMARY, "SUMMARY")?;
    let description = field(TAG_DESCRIPTION, "DESCRIPTION")?;
    let installed_size =
        entry_u32(bytes, &entries, store, TAG_SIZE).ok_or(YumError::MissingField("SIZE"))? as u64;
    let build_time = entry_u32(bytes, &entries, store, TAG_BUILDTIME)
        .ok_or(YumError::MissingField("BUILDTIME"))?;
    Ok(RpmMetadata {
        name,
        epoch: entry_u32(bytes, &entries, store, TAG_EPOCH),
        version,
        release,
        architecture,
        summary,
        description,
        license: entry_string(bytes, &entries, store, TAG_LICENSE),
        installed_size,
        build_time,
    })
}

/// The file name the community expects: `{name}-{epoch}:{ver}-{rel}.{arch}.rpm`,
/// with the epoch only when it exists. It is the one `dnf` will see in `location`.
pub fn rpm_filename(metadata: &RpmMetadata) -> String {
    let epoch = metadata
        .epoch
        .map(|epoch| format!("{epoch}:"))
        .unwrap_or_default();
    format!(
        "{}-{}{}-{}.{}.rpm",
        metadata.name, epoch, metadata.version, metadata.release, metadata.architecture
    )
}

/// The pool path where the binary lives. Any
/// identifier that could escape the repository root or that is not
/// transportable over HTTP is rejected.
pub fn pool_path_for(metadata: &RpmMetadata) -> Result<String, YumError> {
    let candidate = rpm_filename(metadata);
    let safe = candidate.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-' | b'_' | b'~' | b':')
    });
    if !safe || candidate.contains("..") || metadata.name.is_empty() {
        return Err(YumError::BadIdentity(candidate));
    }
    Ok(format!("pool/{candidate}"))
}

/// The suffix with which a pool binary stores its index entry:
/// the `.rpm` bytes and its `primary` block are two immutable
/// assets, and the entry is what is read to assemble the index.
pub fn record_path_for(pool_path: &str) -> String {
    format!("{pool_path}.record")
}

/// Escapes text for an XML element: dnf's grammar does not admit
/// bare `&` or `<` in summary, description or license.
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Renders the `<package>` entry of a binary: the header fields plus
/// the index ones that only the repository knows (path, size, checksum).
/// It is the `createrepo_c` fragment that dnf turns into candidates,
/// stored as an immutable record next to the `.rpm` it describes.
pub fn package_record(metadata: &RpmMetadata, pool_path: &str, size: u64, sha256: &str) -> String {
    let epoch = metadata
        .epoch
        .map(|epoch| format!(" epoch=\"{epoch}\""))
        .unwrap_or_default();
    let license = metadata
        .license
        .as_deref()
        .map(|license| format!("    <rpm:license>{}</rpm:license>\n", xml_escape(license)))
        .unwrap_or_default();
    format!(
        concat!(
            "<package type=\"rpm\">\n",
            "  <name>{}</name>\n",
            "  <arch>{}</arch>\n",
            "  <version{} ver=\"{}\" rel=\"{}\"/>\n",
            "  <checksum type=\"sha256\" pkgid=\"YES\">{}</checksum>\n",
            "  <summary>{}</summary>\n",
            "  <description>{}</description>\n",
            "  <time file=\"{}\" build=\"{}\"/>\n",
            "  <size package=\"{}\" installed=\"{}\" archive=\"{}\"/>\n",
            "  <location href=\"{}\"/>\n",
            "  <format>\n{}  </format>\n",
            "</package>\n",
        ),
        xml_escape(&metadata.name),
        metadata.architecture,
        epoch,
        xml_escape(&metadata.version),
        xml_escape(&metadata.release),
        sha256,
        xml_escape(&metadata.summary),
        xml_escape(&metadata.description),
        metadata.build_time,
        metadata.build_time,
        size,
        metadata.installed_size,
        metadata.installed_size,
        pool_path,
        license,
    )
}

/// The complete `primary.xml` document: the common header and the entries
/// in the order they arrive (catalog path order), so that
/// two reads of the same state give the same bytes.
pub fn primary_document(records: &[&str]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <metadata xmlns=\"http://linux.duke.edu/metadata/common\" \
xmlns:rpm=\"http://linux.duke.edu/metadata/rpm\" \
packages=\"{count}\">\n",
    )
    .replace("{count}", &records.len().to_string());
    for record in records {
        out.push_str(record);
    }
    out.push_str("</metadata>\n");
    out
}

/// The entry of an index file inside `repomd.xml`.
pub struct RepodataEntry<'a> {
    /// The logical type: today, `primary`.
    pub kind: &'static str,
    /// The relative path under which it is served.
    pub location: &'a str,
    /// SHA256 of the file as served (gzip included).
    pub checksum: &'a str,
    /// Size of the file as served.
    pub size: u64,
    /// SHA256 of the uncompressed text.
    pub open_checksum: &'a str,
    /// Size of the uncompressed text.
    pub open_size: u64,
    /// The moment of the state that generated it, in Unix seconds.
    pub timestamp: u64,
}

/// The `repomd.xml` document: the list of indexes with their exact checksum.
/// It is what `dnf` downloads first and what is signed in
/// `repomd.xml.asc`, so every byte counts.
pub fn repomd_document(revision: u64, entries: &[RepodataEntry]) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <repomd xmlns=\"http://linux.duke.edu/metadata/repo\" \
xmlns:rpm=\"http://linux.duke.edu/metadata/rpm\">\n\
         <revision>{revision}</revision>\n"
    );
    for entry in entries {
        out.push_str(&format!(
            concat!(
                "  <data type=\"{kind}\">\n",
                "    <checksum type=\"sha256\">{checksum}</checksum>\n",
                "    <open-checksum type=\"sha256\">{open_checksum}</open-checksum>\n",
                "    <location href=\"{location}\"/>\n",
                "    <timestamp>{timestamp}</timestamp>\n",
                "    <size>{size}</size>\n",
                "    <open-size>{open_size}</open-size>\n",
                "  </data>\n",
            ),
            kind = entry.kind,
            checksum = entry.checksum,
            open_checksum = entry.open_checksum,
            location = entry.location,
            timestamp = entry.timestamp,
            size = entry.size,
            open_size = entry.open_size,
        ));
    }
    out.push_str("</repomd>\n");
    out
}

/// The name under which `primary.xml.gz` is served: content
/// addressed by its own SHA256, as in createrepo. This way the
/// file is immutable and cacheable forever.
pub fn primary_location(sha256: &str) -> String {
    format!("repodata/{sha256}-primary.xml.gz")
}

/// The routes the yum surface knows how to serve, already interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The binary itself, at `pool/{name}-{version}-{release}.{arch}.rpm`.
    Pool {
        /// The binary's path inside the repository.
        path: String,
    },
    /// The repository's `repomd.xml`.
    Repomd,
    /// The detached signature of `repomd.xml`: `repomd.xml.asc`.
    RepomdSignature,
    /// The `primary` index by its content name.
    Primary {
        /// The SHA256 hex with which the client requested it.
        checksum: String,
    },
    /// The hub's public key in armor, dnf's `gpgkey`.
    Key,
}

/// Normalizes the percent escapes that the client itself adds to the primary's
/// href: `dnf5` (libcurl) escapes reserved characters when building
/// the URL, so `:` arrives as `%3a` and `+` as `%2b`. Only `%XX`
/// sequences with valid hex are replaced; a stray `%` or one with invalid hex
/// is left as is and falls outside the charset in path validation.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    if !bytes.contains(&b'%') {
        return text.to_string();
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            out.push(hi << 4 | lo);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    // A sequence that does not decode to valid UTF-8 is not a path: the
    // raw one is returned so that charset validation rejects it.
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl Route {
    /// Interprets the rest of `/yum/{repository}/...`, raw.
    pub fn parse(rest: &str) -> Result<Route, YumError> {
        if rest == "key.asc" {
            return Ok(Route::Key);
        }
        if let Some(path) = rest.strip_prefix("pool/") {
            // The grammar charset is the security boundary: the
            // path is normalized first (`percent_decode`) and validated
            // afterwards, so that a hostile escape (`%2e%2e`, `%2f`) does not
            // open the door to anything the raw form did not already open.
            let path = percent_decode(path);
            if !path.ends_with(".rpm")
                || path.is_empty()
                || path.contains("..")
                || !path.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'.' | b'/' | b'+' | b'-' | b'_' | b'~' | b':')
                })
            {
                return Err(YumError::BadIdentity(rest.to_string()));
            }
            return Ok(Route::Pool {
                path: format!("pool/{path}"),
            });
        }
        if let Some(name) = rest.strip_prefix("repodata/") {
            match name {
                "repomd.xml" => return Ok(Route::Repomd),
                "repomd.xml.asc" => return Ok(Route::RepomdSignature),
                _ => {
                    // `{sha256}-primary.xml.gz`: only hex, hyphens and the
                    // suffix; any other name is an impossible path.
                    if let Some(hex) = name.strip_suffix("-primary.xml.gz")
                        && hex.len() == 64
                        && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                    {
                        return Ok(Route::Primary {
                            checksum: hex.to_lowercase(),
                        });
                    }
                    return Err(YumError::BadIdentity(rest.to_string()));
                }
            }
        }
        Err(YumError::BadIdentity(rest.to_string()))
    }
}

/// The metadata names that are served with the checksum in the name.
///
/// The `createrepo` layout addresses metadata by its own
/// SHA256: `repodata/{sha256}-primary.xml.gz`. That name is a promise
/// about the bytes served, and therefore it can be checked without
/// asking anyone.
const SHA256_HEX_LENGTH: usize = 64;

/// The routes the yum surface serves in **proxy** mode.
///
/// A hosted repository imposes its layout: the `primary` is called
/// whatever and the pool is `pool/`. A proxy cannot impose it: the
/// upstream decides what its metadata is called and the client requests it by the
/// name the `repomd.xml` declares. That is why there are **two** grammars and
/// not one: the hosted one remains strict (a hosted repository cannot
/// serve `Packages/...`), and the proxy's accepts what the
/// upstream publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamRoute {
    /// The upstream's public key, in armor: the one the client uses
    /// to verify the upstream's signature.
    Key,
    /// The upstream's `repomd.xml`, as is.
    Repomd,
    /// The detached signature of the upstream's `repomd.xml`.
    RepomdSignature,
    /// An upstream metadata file or package, by its path.
    Asset {
        /// The path under which it is requested from the upstream.
        path: String,
    },
}

impl UpstreamRoute {
    /// Interprets the rest of `/yum/{repository}/...` in proxy mode.
    ///
    /// The security rule is the hosted pool's: no `..`, no leading
    /// slash, no double slashes and with a bounded character set.
    /// What does not comply does not exist for that proxy.
    pub fn parse(rest: &str) -> Result<Self, YumError> {
        match rest {
            "key.asc" => return Ok(Self::Key),
            "repodata/repomd.xml" => return Ok(Self::Repomd),
            "repodata/repomd.xml.asc" => return Ok(Self::RepomdSignature),
            _ => {}
        }
        // The upstream decides the layout: its `repodata` may have more
        // indexes than the `primary`, and its pool may be `Packages/`.
        let known = rest.starts_with("repodata/")
            || rest.starts_with("Packages/")
            || (rest.starts_with("pool/") && rest.ends_with(".rpm"));
        if !known || !is_safe_upstream_path(rest) {
            return Err(YumError::BadIdentity(rest.to_string()));
        }
        Ok(Self::Asset {
            path: rest.to_string(),
        })
    }
}

/// Whether a relative path is safe to request from an upstream.
///
/// It is the same rule as a hosted repository's pool: no
/// dot segments, no leading slash, no double slashes, no trailing
/// slash and with a bounded character set.
fn is_safe_upstream_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains("//")
        && !path.contains("..")
        && path.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'/' | b'+' | b'-' | b'_' | b'~' | b':')
        })
}

/// The checksum that a path's **name** declares, if it declares one.
///
/// A `createrepo` metadata name is its own SHA256, and therefore
/// the name is a promise about the bytes. A proxy that serves bytes
/// that do not comply is serving corrupt metadata as if
/// it were good.
pub fn checksum_of_name(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (hex, rest) = name.split_once('-')?;
    if hex.len() != SHA256_HEX_LENGTH
        || rest.is_empty()
        || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(hex.to_lowercase())
}

/// A metadata file declared by an upstream's `repomd.xml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredMetadata {
    /// The path under which it is served.
    pub location: String,
    /// The SHA256 the `repomd` declares.
    pub checksum: String,
}

/// The metadata files that a `repomd.xml` declares, with their checksum.
///
/// The document comes from an upstream: it is read as it comes, and what is
/// sought is the `location` and the `checksum` of each `<data>`. A `repomd`
/// that is not understood declares nothing, and what it does not declare is not
/// served.
pub fn declared_metadata(repomd: &str) -> Vec<DeclaredMetadata> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = repomd[cursor..].find("<data") {
        let start = cursor + offset;
        let Some(end) = repomd[start..].find("</data>") else {
            break;
        };
        let inside = &repomd[start..start + end];
        cursor = start + end + "</data>".len();
        let Some((head, checksum)) = element_of(inside, "checksum") else {
            continue;
        };
        if !head.contains("type=\"sha256\"") || checksum.len() != SHA256_HEX_LENGTH {
            continue;
        }
        if !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let Some(location) = tag_of(inside, "location") else {
            continue;
        };
        let Some(href) = attribute_of(&location, "href") else {
            continue;
        };
        out.push(DeclaredMetadata {
            location: href,
            checksum: checksum.to_lowercase(),
        });
    }
    out
}

/// The checksum that a `repomd.xml` declares for a path, if it declares one.
pub fn declared_checksum(repomd: &str, location: &str) -> Option<String> {
    declared_metadata(repomd)
        .into_iter()
        .find(|entry| entry.location == location)
        .map(|entry| entry.checksum)
}

/// Routes served by the yum surface in `RepositoryMode::Group`.
///
/// A group exposes Hodei's own distribution key (`key.asc`), its
/// aggregated and signed `repomd.xml` + `repomd.xml.asc`, the content-
/// addressed `primary.xml.gz`, and `.rpm` package downloads resolved
/// across its members (`pool/...` or `Packages/...`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupRoute {
    /// Hodei's distribution public key in ASCII armor (`key.asc`).
    Key,
    /// Aggregated `repomd.xml`.
    Repomd,
    /// Detached OpenPGP signature over the aggregated `repomd.xml`.
    RepomdSignature,
    /// Content-addressed aggregated `primary.xml.gz`.
    Primary {
        /// Expected SHA-256 hex digest of the gzipped primary document.
        checksum: String,
    },
    /// Package download path resolved from the winning group member.
    Package {
        /// Relative package path (`pool/...` or `Packages/...`).
        path: String,
    },
}

impl GroupRoute {
    /// Parses a relative path under `/yum/{group}/...`.
    pub fn parse(rest: &str) -> Result<Self, YumError> {
        match Route::parse(rest) {
            Ok(Route::Key) => return Ok(Self::Key),
            Ok(Route::Repomd) => return Ok(Self::Repomd),
            Ok(Route::RepomdSignature) => return Ok(Self::RepomdSignature),
            Ok(Route::Primary { checksum }) => return Ok(Self::Primary { checksum }),
            Ok(Route::Pool { path }) => return Ok(Self::Package { path }),
            Err(_) => {}
        }
        if rest.starts_with("Packages/") && rest.ends_with(".rpm") && is_safe_upstream_path(rest) {
            return Ok(Self::Package {
                path: rest.to_string(),
            });
        }
        Err(YumError::BadIdentity(rest.to_string()))
    }
}

/// Primary metadata entry extracted from a `repomd.xml` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredPrimary {
    /// Relative location (`href`) of the primary metadata file.
    pub location: String,
    /// Expected SHA-256 hex digest of the compressed file.
    pub checksum: String,
    /// Expected SHA-256 hex digest of the uncompressed XML, if declared.
    pub open_checksum: Option<String>,
    /// Unix timestamp declared on the primary entry, if any.
    pub timestamp: Option<u64>,
}

/// Extracts the `<data type="primary">` entry from a `repomd.xml` document.
pub fn declared_primary(repomd: &str) -> Option<DeclaredPrimary> {
    let mut cursor = 0usize;
    while let Some(offset) = repomd[cursor..].find("<data") {
        let start = cursor + offset;
        let head_end = repomd[start..].find('>')? + start + 1;
        let end = repomd[start..].find("</data>")? + start;
        cursor = end + "</data>".len();
        let data_head = &repomd[start..head_end];
        if attribute_of(data_head, "type").as_deref() != Some("primary") {
            continue;
        }
        let inside = &repomd[head_end..end];
        let (checksum_head, checksum) = element_of(inside, "checksum")?;
        if attribute_of(&checksum_head, "type").as_deref() != Some("sha256")
            || checksum.len() != SHA256_HEX_LENGTH
            || !checksum.bytes().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        let location = tag_of(inside, "location")?;
        let href = attribute_of(&location, "href")?;
        let open_checksum = element_of(inside, "open-checksum").and_then(|(head, val)| {
            if attribute_of(&head, "type").as_deref() == Some("sha256")
                && val.len() == SHA256_HEX_LENGTH
                && val.bytes().all(|b| b.is_ascii_hexdigit())
            {
                Some(val.to_lowercase())
            } else {
                None
            }
        });
        let timestamp =
            element_of(inside, "timestamp").and_then(|(_, val)| val.parse::<u64>().ok());
        return Some(DeclaredPrimary {
            location: href,
            checksum: checksum.to_lowercase(),
            open_checksum,
            timestamp,
        });
    }
    None
}

/// Canonical RPM NEVRA identity: Name, Epoch, Version, Release, Arch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Nevra {
    /// Package name.
    pub name: String,
    /// Package epoch (defaults to 0 when omitted in XML).
    pub epoch: u32,
    /// Upstream version string.
    pub version: String,
    /// Package release string.
    pub release: String,
    /// Target architecture (`x86_64`, `noarch`, etc.).
    pub arch: String,
}

/// A `<package>` record extracted from `primary.xml` or a `.record` asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    /// Parsed NEVRA identity.
    pub nevra: Nevra,
    /// Relative download path (`<location href="..."/>`).
    pub location: String,
    /// Complete `<package ...>...</package>` XML element.
    pub xml: String,
}

/// Parses `<package ...>...</package>` blocks from a `primary.xml` document
/// or a standalone `.record` fragment.
pub fn parse_package_entries(primary_xml: &str) -> Result<Vec<PackageEntry>, YumError> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = primary_xml[cursor..].find("<package") {
        let start = cursor + offset;
        let after_open = &primary_xml[start + "<package".len()..];
        let next_char = after_open.chars().next();
        if !matches!(next_char, Some(' ') | Some('>')) {
            cursor = start + "<package".len();
            continue;
        }
        let Some(rel_end) = primary_xml[start..].find("</package>") else {
            return Err(YumError::BadContainer(
                "unterminated <package> element in primary metadata",
            ));
        };
        let end = start + rel_end + "</package>".len();
        let block = &primary_xml[start..end];
        cursor = end;

        let (_, name) = element_of(block, "name").ok_or(YumError::MissingField("NAME"))?;
        let (_, arch) = element_of(block, "arch").ok_or(YumError::MissingField("ARCH"))?;
        let version_tag = tag_of(block, "version").ok_or(YumError::MissingField("VERSION"))?;
        let version = attribute_of(&version_tag, "ver").ok_or(YumError::MissingField("VERSION"))?;
        let release = attribute_of(&version_tag, "rel").ok_or(YumError::MissingField("RELEASE"))?;
        let epoch = attribute_of(&version_tag, "epoch")
            .and_then(|e| e.parse::<u32>().ok())
            .unwrap_or(0);
        let location_tag = tag_of(block, "location").ok_or(YumError::MissingField("LOCATION"))?;
        let location =
            attribute_of(&location_tag, "href").ok_or(YumError::MissingField("LOCATION"))?;

        let mut xml = block.to_string();
        if !xml.ends_with('\n') {
            xml.push('\n');
        }
        out.push(PackageEntry {
            nevra: Nevra {
                name,
                epoch,
                version,
                release,
                arch,
            },
            location,
            xml,
        });
    }
    Ok(out)
}

/// Merges package entries from group members in precedence order (index 0 wins).
///
/// NEVRA collisions are resolved deterministically by keeping the entry from
/// the highest-precedence member, and the returned list is sorted by NEVRA
/// so identical member states always produce byte-identical `primary.xml`.
pub fn merge_package_entries(members: &[Vec<PackageEntry>]) -> Vec<PackageEntry> {
    let mut selected = std::collections::BTreeMap::new();
    for member_entries in members {
        for entry in member_entries {
            selected
                .entry(entry.nevra.clone())
                .or_insert_with(|| entry.clone());
        }
    }
    selected.into_values().collect()
}

/// The header of an element and its text: `<checksum type="sha256">abc`.
fn element_of(text: &str, name: &str) -> Option<(String, String)> {
    let open = format!("<{name}");
    let start = text.find(&open)?;
    let after = &text[start..];
    let head_end = after.find('>')? + 1;
    let close = format!("</{name}>");
    let end = after.find(&close)?;
    Some((
        after[..head_end].to_string(),
        after[head_end..end].trim().to_string(),
    ))
}

/// The opening tag of an element: `<location href="..."/>`.
fn tag_of(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let start = text.find(&open)?;
    let after = &text[start..];
    let head_end = after.find('>')? + 1;
    Some(after[..head_end].to_string())
}

/// The value of an attribute: `href="repodata/abc-primary.xml.gz"`.
fn attribute_of(text: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = text.find(&needle)? + needle.len();
    let end = text[start..].find('"')? + start;
    Some(text[start..end].to_string())
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) enum HeaderValue<'a> {
    Str(&'a str),
    U32(u32),
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct TestHeaderField<'a> {
    pub tag: i32,
    pub value: HeaderValue<'a>,
}

/// Builds a test `.rpm`: lead, empty signature header and main
/// header with the requested fields. It is not installable (the payload is
/// empty), but our reader and the pool grammar see it as a real
/// one: the header structure is identical.
#[cfg(test)]
pub(crate) fn build_test_rpm(fields: &[TestHeaderField]) -> Vec<u8> {
    let mut lead = vec![0u8; 96];
    lead[0..4].copy_from_slice(b"\xed\xab\xee\xdb");
    lead[4] = 3; // format version
    lead[6..8].copy_from_slice(&1u16.to_be_bytes()); // binary rpm

    // Signature: legitimate empty header (unsigned packages abound).
    let mut signature = vec![0x8e, 0xad, 0xe8, 1];
    signature.extend_from_slice(&[0u8; 4]); // reserved
    signature.extend_from_slice(&0u32.to_be_bytes()); // count
    signature.extend_from_slice(&0u32.to_be_bytes()); // store size

    let mut store: Vec<u8> = Vec::new();
    let mut index: Vec<u8> = Vec::new();
    for field in fields {
        let (offset, entry_size) = match &field.value {
            HeaderValue::Str(text) => {
                let offset = store.len() as u32;
                store.extend_from_slice(text.as_bytes());
                store.push(0);
                (offset, 0u32)
            }
            HeaderValue::U32(number) => {
                // Aligned to 4, as the store specification requires.
                let pad = (4 - store.len() % 4) % 4;
                store.extend(std::iter::repeat_n(0, pad));
                let offset = store.len() as u32;
                store.extend_from_slice(&number.to_be_bytes());
                (offset, 1u32)
            }
        };
        let (kind, count): (u32, u32) = match &field.value {
            HeaderValue::Str(_) => (6, 1u32),
            HeaderValue::U32(_) => (4, entry_size),
        };
        index.extend_from_slice(&field.tag.to_be_bytes());
        index.extend_from_slice(&kind.to_be_bytes());
        index.extend_from_slice(&offset.to_be_bytes());
        index.extend_from_slice(&count.to_be_bytes());
    }
    let mut main = vec![0x8e, 0xad, 0xe8, 1];
    main.extend_from_slice(&[0u8; 4]);
    main.extend_from_slice(&(fields.len() as u32).to_be_bytes());
    main.extend_from_slice(&(store.len() as u32).to_be_bytes());
    main.extend_from_slice(&index);
    main.extend_from_slice(&store);

    let mut rpm = lead;
    rpm.extend(signature);
    let pad = (8 - rpm.len() % 8) % 8;
    rpm.extend(std::iter::repeat_n(0, pad));
    rpm.extend(main);
    rpm
}

/// The fields every real package carries, as rpm-build renders them.
#[cfg(test)]
pub(crate) fn rpm_fields<'a>(
    name: &'a str,
    version: &'a str,
    release: &'a str,
    arch: &'a str,
) -> Vec<TestHeaderField<'a>> {
    let field = |tag, value| TestHeaderField { tag, value };
    vec![
        field(TAG_NAME, HeaderValue::Str(name)),
        field(TAG_VERSION, HeaderValue::Str(version)),
        field(TAG_RELEASE, HeaderValue::Str(release)),
        field(TAG_SUMMARY, HeaderValue::Str("A tiny hello package")),
        field(TAG_DESCRIPTION, HeaderValue::Str("Installed through dnf.")),
        field(TAG_BUILDTIME, HeaderValue::U32(1_700_000_000)),
        field(TAG_SIZE, HeaderValue::U32(4096)),
        field(TAG_ARCH, HeaderValue::Str(arch)),
        field(TAG_LICENSE, HeaderValue::Str("MIT")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello_rpm() -> Vec<u8> {
        build_test_rpm(&rpm_fields("hello-hodei", "1.0.0", "1.fc40", "x86_64"))
    }

    #[test]
    fn an_rpm_yields_its_header_fields() {
        let meta = parse_rpm(&hello_rpm()).expect("the header parses");
        assert_eq!(meta.name, "hello-hodei");
        assert_eq!(meta.version, "1.0.0");
        assert_eq!(meta.release, "1.fc40");
        assert_eq!(meta.architecture, "x86_64");
        assert_eq!(meta.epoch, None);
        assert_eq!(meta.license.as_deref(), Some("MIT"));
        assert_eq!(meta.installed_size, 4096);
        assert_eq!(meta.build_time, 1_700_000_000);
    }

    #[test]
    fn an_epoch_declared_in_the_header_survives_to_the_filename() {
        let mut fields = rpm_fields("hello-hodei", "1.0.0", "1.fc40", "x86_64");
        fields.push(TestHeaderField {
            tag: TAG_EPOCH,
            value: HeaderValue::U32(2),
        });
        let meta = parse_rpm(&build_test_rpm(&fields)).expect("the header parses");
        assert_eq!(meta.epoch, Some(2));
        assert_eq!(rpm_filename(&meta), "hello-hodei-2:1.0.0-1.fc40.x86_64.rpm");
    }

    #[test]
    fn a_file_without_the_lead_is_not_an_rpm() {
        for bytes in [
            b"not an rpm at all".to_vec(),
            vec![0u8; 96],
            hello_rpm()[..80].to_vec(),
        ] {
            assert!(matches!(
                parse_rpm(&bytes).unwrap_err(),
                YumError::BadContainer(_)
            ));
        }
    }

    #[test]
    fn a_header_without_name_is_not_a_package() {
        let fields: Vec<TestHeaderField> = rpm_fields("hello-hodei", "1.0.0", "1.fc40", "x86_64")
            .into_iter()
            .filter(|field| field.tag != TAG_NAME)
            .collect();
        assert_eq!(
            parse_rpm(&build_test_rpm(&fields)).unwrap_err(),
            YumError::MissingField("NAME")
        );
    }

    #[test]
    fn the_record_is_what_createrepo_documents() {
        let meta = parse_rpm(&hello_rpm()).expect("the header parses");
        let sha = "ab".repeat(32);
        let record = package_record(
            &meta,
            "pool/hello-hodei-1.0.0-1.fc40.x86_64.rpm",
            8192,
            &sha,
        );
        assert!(record.starts_with("<package type=\"rpm\">\n  <name>hello-hodei</name>"));
        assert!(record.contains("<checksum type=\"sha256\" pkgid=\"YES\">"));
        assert!(record.contains("<size package=\"8192\" installed=\"4096\" archive=\"4096\"/>"));
        assert!(record.contains("<location href=\"pool/hello-hodei-1.0.0-1.fc40.x86_64.rpm\"/>"));
        assert!(record.contains("<rpm:license>MIT</rpm:license>"));
        assert!(record.ends_with("</package>\n"));
    }

    #[test]
    fn the_record_escapes_what_is_not_xml() {
        let mut fields = rpm_fields("hello-hodei", "1.0.0", "1.fc40", "x86_64");
        fields[3] = TestHeaderField {
            tag: TAG_SUMMARY,
            value: HeaderValue::Str("Greeting <b>bold</b> & more"),
        };
        let meta = parse_rpm(&build_test_rpm(&fields)).expect("the header parses");
        let record = package_record(&meta, "pool/x.rpm", 1, &"0".repeat(64));
        assert!(record.contains("&lt;b&gt;bold&lt;/b&gt; &amp; more"));
    }

    #[test]
    fn the_primary_document_counts_its_packages() {
        let document = primary_document(&["<package type=\"rpm\">\n</package>\n", "otra"]);
        assert!(document.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<metadata xmlns=\"http://linux.duke.edu/metadata/common\" xmlns:rpm=\"http://linux.duke.edu/metadata/rpm\" packages=\"2\">"));
        assert!(document.ends_with("otra</metadata>\n"));
    }

    #[test]
    fn the_repomd_pins_the_served_bytes() {
        let entry = RepodataEntry {
            kind: "primary",
            location: "repodata/cafe-primary.xml.gz",
            checksum: "cafe",
            size: 123,
            open_checksum: "foca",
            open_size: 456,
            timestamp: 1_700_000_000,
        };
        let document = repomd_document(1_700_000_000, &[entry]);
        assert!(document.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<repomd xmlns=\"http://linux.duke.edu/metadata/repo\""));
        assert!(document.contains("<revision>1700000000</revision>"));
        assert!(document.contains("<data type=\"primary\">"));
        assert!(document.contains("<checksum type=\"sha256\">cafe</checksum>"));
        assert!(document.contains("<location href=\"repodata/cafe-primary.xml.gz\"/>"));
        assert!(document.contains("<size>123</size>"));
        assert!(document.contains("<open-size>456</open-size>"));
    }

    #[test]
    fn the_pool_path_stays_inside_the_repository() {
        let meta = parse_rpm(&hello_rpm()).expect("the header parses");
        assert_eq!(
            pool_path_for(&meta).unwrap(),
            "pool/hello-hodei-1.0.0-1.fc40.x86_64.rpm"
        );
        let mut fields = rpm_fields("..x", "1.0", "1.fc40", "x86_64");
        fields[1] = TestHeaderField {
            tag: TAG_VERSION,
            value: HeaderValue::Str("1/../0"),
        };
        let escaped = parse_rpm(&build_test_rpm(&fields)).expect("the header parses");
        assert!(pool_path_for(&escaped).is_err());
    }

    #[test]
    fn the_routes_are_the_yum_urls() {
        assert_eq!(Route::parse("repodata/repomd.xml").unwrap(), Route::Repomd);
        assert_eq!(
            Route::parse("repodata/repomd.xml.asc").unwrap(),
            Route::RepomdSignature
        );
        assert_eq!(
            Route::parse(&format!("repodata/{}-primary.xml.gz", "ab".repeat(32))).unwrap(),
            Route::Primary {
                checksum: "ab".repeat(32)
            }
        );
        assert_eq!(Route::parse("key.asc").unwrap(), Route::Key);
        assert_eq!(
            Route::parse("pool/hello-hodei-1.0.0-1.fc40.x86_64.rpm").unwrap(),
            Route::Pool {
                path: "pool/hello-hodei-1.0.0-1.fc40.x86_64.rpm".into()
            }
        );
        // dnf5 escapes the href's reserved characters when requesting the package: the
        // same path arrives with `%3a` (epoch) and `%2b` (plus) and must be
        // interpreted as the catalog's raw path.
        assert_eq!(
            Route::parse("pool/hello-tilde-1%3a1.0.0~rc1%2bdfsg-1.x86_64.rpm").unwrap(),
            Route::Pool {
                path: "pool/hello-tilde-1:1.0.0~rc1+dfsg-1.x86_64.rpm".into()
            }
        );
    }

    #[test]
    fn percent_escapes_never_widen_the_path() {
        // An escape that would produce `..` falls just like the raw form: the
        // normalization opens no door.
        assert!(Route::parse("pool/%2e%2e/etc/passwd.rpm").is_err());
        assert!(Route::parse("pool/main/%2e%2e/secret.rpm").is_err());
        // Stray `%`, invalid hex or incomplete sequence: outside the
        // charset, never a path.
        assert!(Route::parse("pool/a%zzb.rpm").is_err());
        assert!(Route::parse("pool/a%2.rpm").is_err());
        assert!(Route::parse("pool/a%b.rpm").is_err());
        // Double percent: a single pass, `%252e` leaves `%` in the path and
        // the charset rejects it.
        assert!(Route::parse("pool/a%252eb.rpm").is_err());
        // The invariant: what the escaped form accepts is exactly what
        // the decoded raw form would accept, no more and no less.
        assert_eq!(
            Route::parse("pool/a%2fb.rpm").unwrap(),
            Route::parse("pool/a/b.rpm").unwrap()
        );
        assert_eq!(
            Route::parse("pool/aa%41b.rpm").unwrap(),
            Route::Pool {
                path: "pool/aaAb.rpm".into()
            }
        );
    }

    #[test]
    fn a_route_cannot_escape_or_be_nonsense() {
        assert!(Route::parse("pool/../etc/passwd").is_err());
        assert!(Route::parse("pool/main/x/no-extension").is_err());
        assert!(Route::parse("repodata/nope.xml").is_err());
        assert!(Route::parse("repodata/zz-primary.xml.gz").is_err());
        assert!(Route::parse("index.html").is_err());
    }

    #[test]
    fn the_record_hangs_from_the_binary_it_describes() {
        assert_eq!(
            record_path_for("pool/hello-1.0-1.x86_64.rpm"),
            "pool/hello-1.0-1.x86_64.rpm.record"
        );
    }

    #[test]
    fn the_upstream_grammar_accepts_what_the_upstream_publishes() {
        let sha = "ab".repeat(32);
        assert_eq!(UpstreamRoute::parse("key.asc").unwrap(), UpstreamRoute::Key);
        assert_eq!(
            UpstreamRoute::parse("repodata/repomd.xml").unwrap(),
            UpstreamRoute::Repomd
        );
        assert_eq!(
            UpstreamRoute::parse("repodata/repomd.xml.asc").unwrap(),
            UpstreamRoute::RepomdSignature
        );
        for path in [
            format!("repodata/{sha}-primary.xml.gz"),
            format!("repodata/{sha}-filelists.xml.gz"),
            "repodata/primary.xml.gz".to_string(),
            "Packages/hello-1.0-1.x86_64.rpm".to_string(),
            "pool/hello-1.0-1.x86_64.rpm".to_string(),
        ] {
            assert_eq!(
                UpstreamRoute::parse(&path).unwrap(),
                UpstreamRoute::Asset { path: path.clone() },
                "the upstream publishes {path}"
            );
        }
    }

    #[test]
    fn the_upstream_grammar_refuses_what_cannot_be_asked() {
        for path in [
            "repodata/../etc/passwd",
            "repodata//x.xml.gz",
            "/repodata/repomd.xml",
            "Packages/",
            "pool/hello.txt",
            "otra/cosa",
            "index.html",
        ] {
            assert!(
                UpstreamRoute::parse(path).is_err(),
                "the upstream grammar accepted {path}"
            );
        }
    }

    #[test]
    fn the_name_of_a_metadata_declares_its_checksum() {
        let sha = "ab".repeat(32);
        assert_eq!(
            checksum_of_name(&format!("repodata/{sha}-primary.xml.gz")).as_deref(),
            Some(sha.as_str())
        );
        assert_eq!(
            checksum_of_name(&format!("repodata/{sha}-filelists.xml.gz")).as_deref(),
            Some(sha.as_str())
        );
        assert_eq!(checksum_of_name("repodata/primary.xml.gz"), None);
        assert_eq!(checksum_of_name("repodata/repomd.xml"), None);
        assert_eq!(checksum_of_name("Packages/hello-1.0-1.x86_64.rpm"), None);
    }

    #[test]
    fn the_repomd_declares_the_metadata_it_pins() {
        let sha = "ab".repeat(32);
        let entry = RepodataEntry {
            kind: "primary",
            location: &format!("repodata/{sha}-primary.xml.gz"),
            checksum: &sha,
            size: 123,
            open_checksum: &"cd".repeat(32),
            open_size: 456,
            timestamp: 1_700_000_000,
        };
        let document = repomd_document(1_700_000_000, &[entry]);
        assert_eq!(document.matches("<data ").count(), 1);
        let declared = declared_metadata(&document);
        assert_eq!(declared.len(), 1);
        assert_eq!(
            declared[0].location,
            format!("repodata/{sha}-primary.xml.gz")
        );
        assert_eq!(declared[0].checksum, sha);
        assert_eq!(
            declared_checksum(&document, &format!("repodata/{sha}-primary.xml.gz")).as_deref(),
            Some(sha.as_str())
        );
        assert_eq!(
            declared_checksum(&document, "repodata/primary.xml.gz"),
            None
        );
    }

    #[test]
    fn group_route_accepts_pool_and_packages_and_rejects_traversal() {
        assert_eq!(GroupRoute::parse("key.asc"), Ok(GroupRoute::Key));
        assert_eq!(
            GroupRoute::parse("repodata/repomd.xml"),
            Ok(GroupRoute::Repomd)
        );
        assert_eq!(
            GroupRoute::parse("repodata/repomd.xml.asc"),
            Ok(GroupRoute::RepomdSignature)
        );
        assert_eq!(
            GroupRoute::parse("pool/hello-1.0-1.x86_64.rpm"),
            Ok(GroupRoute::Package {
                path: "pool/hello-1.0-1.x86_64.rpm".to_string()
            })
        );
        assert_eq!(
            GroupRoute::parse("Packages/hello-1.0-1.fc40.x86_64.rpm"),
            Ok(GroupRoute::Package {
                path: "Packages/hello-1.0-1.fc40.x86_64.rpm".to_string()
            })
        );
        assert!(GroupRoute::parse("Packages/../etc/passwd.rpm").is_err());
        assert!(GroupRoute::parse("repodata/other.xml").is_err());
    }

    #[test]
    fn declared_primary_extracts_primary_entry_and_open_checksum() {
        let sha = "ab".repeat(32);
        let open_sha = "cd".repeat(32);
        let entry = RepodataEntry {
            kind: "primary",
            location: &format!("repodata/{sha}-primary.xml.gz"),
            checksum: &sha,
            size: 123,
            open_checksum: &open_sha,
            open_size: 456,
            timestamp: 1_700_000_000,
        };
        let document = repomd_document(1_700_000_000, &[entry]);
        let primary = declared_primary(&document).expect("primary entry");
        assert_eq!(primary.location, format!("repodata/{sha}-primary.xml.gz"));
        assert_eq!(primary.checksum, sha);
        assert_eq!(primary.open_checksum, Some(open_sha));
        assert_eq!(primary.timestamp, Some(1_700_000_000));
    }

    #[test]
    fn merge_package_entries_deduplicates_nevra_by_member_precedence() {
        let first_xml = "<package type=\"rpm\">\n  <name>hello</name>\n  <arch>x86_64</arch>\n  <version ver=\"1.0\" rel=\"1\"/>\n  <checksum type=\"sha256\" pkgid=\"YES\">1111</checksum>\n  <location href=\"pool/hello-1.0-1.x86_64.rpm\"/>\n</package>\n";
        let second_xml = "<package type=\"rpm\">\n  <name>hello</name>\n  <arch>x86_64</arch>\n  <version epoch=\"0\" ver=\"1.0\" rel=\"1\"/>\n  <checksum type=\"sha256\" pkgid=\"YES\">2222</checksum>\n  <location href=\"Packages/hello-1.0-1.x86_64.rpm\"/>\n</package>\n<package type=\"rpm\">\n  <name>alpha</name>\n  <arch>noarch</arch>\n  <version ver=\"2.0\" rel=\"1\"/>\n  <checksum type=\"sha256\" pkgid=\"YES\">3333</checksum>\n  <location href=\"Packages/alpha-2.0-1.noarch.rpm\"/>\n</package>\n";
        let first = parse_package_entries(first_xml).expect("first entries");
        let second = parse_package_entries(second_xml).expect("second entries");
        let merged = merge_package_entries(&[first, second]);
        assert_eq!(merged.len(), 2);
        // Sorted by NEVRA: alpha comes before hello.
        assert_eq!(merged[0].nevra.name, "alpha");
        assert_eq!(merged[1].nevra.name, "hello");
        // First member wins NEVRA collision (even when omitted epoch == epoch="0").
        assert_eq!(merged[1].location, "pool/hello-1.0-1.x86_64.rpm");
    }
}
