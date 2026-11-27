//! The grammar and documents of the Debian (APT) format, pure and free of
//! effects: this is where a `.deb` is opened, its `control` is read, and the
//! `Packages` and `Release` indexes are rendered. None of this touches the store or the
//! network: the handler decides when those bytes go out or come in.
//!
//! The contract is that of `dpkg-scanpackages` and `apt-ftparchive`, which is
//! what `apt-get` expects to read. A malformed index is not a nuance:
//! apt discards it and the real client stops seeing the package.

use crate::model::RepositoryConfig;
use crate::proxy::UpstreamConfigError;
use serde::Deserialize;
use sha2::Digest as _;
use std::collections::BTreeMap;
use std::io::Read as _;
use thiserror::Error;

/// The format failures: everything that smells of a broken archive or an unusable
/// control, stated with a name and a reason.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AptError {
    /// The `ar` container is not a `.deb`: missing magic bytes, a
    /// size that is not a number or a member that goes beyond the archive.
    #[error("the archive is not a valid deb container: {0}")]
    BadArchive(&'static str),
    /// There is no `control` member in the archive: without it there is no package.
    #[error("the control member is missing or unreadable")]
    NoControl,
    /// A mandatory `control` field is missing: dpkg does not accept it.
    #[error("the control file lacks the required field {0}")]
    MissingField(&'static str),
    /// The identifier does not fit in a safe pool path.
    #[error("the package identity is not filesystem safe: {0}")]
    BadIdentity(String),
    /// Upstream Release or Packages metadata is malformed or expired.
    #[error("invalid apt metadata: {0}")]
    InvalidMetadata(String),
}

/// What a `.deb` declares about itself: the `control` fields that
/// APT needs to choose and to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebControl {
    /// Package name, lowercase by dpkg convention.
    pub package: String,
    /// Full Debian version, with epoch and `+dfsg` if present.
    pub version: String,
    /// Binary architecture: `amd64`, `arm64`, `all`...
    pub architecture: String,
    /// `Depends` clause exactly as dpkg understands it.
    pub depends: Option<String>,
    /// Declared maintainer of the package.
    pub maintainer: Option<String>,
    /// Full description: summary on the first line and detail paragraphs
    /// following, with their leading spaces.
    pub description: String,
    /// Archive section.
    pub section: Option<String>,
    /// Package priority.
    pub priority: Option<String>,
}

/// A member of an `ar` container: name and bytes, nothing more.
struct ArMember {
    name: String,
    data: Vec<u8>,
}

/// Opens an `ar` container (the wrapper of every `.deb`) and returns
/// its members by name. The format is 8 bytes of magic and then
/// 60-byte headers: name(16) mtime(12) uid(6) gid(6) mode(8)
/// size(10) "`\n"(2), with the body padded to even length.
fn ar_members(bytes: &[u8]) -> Result<Vec<ArMember>, AptError> {
    if bytes.len() < 8 || &bytes[..8] != b"!<arch>\n" {
        return Err(AptError::BadArchive("missing the !<arch> magic"));
    }
    let mut members = Vec::new();
    let mut cursor = 8usize;
    while cursor + 60 <= bytes.len() {
        let header = &bytes[cursor..cursor + 60];
        if header.iter().all(|byte| *byte == b'\n') || header.starts_with(b"\n") {
            break;
        }
        let field = |range: std::ops::Range<usize>| {
            String::from_utf8_lossy(&header[range]).trim().to_string()
        };
        let name = field(0..16);
        let size: usize = field(48..58)
            .parse()
            .map_err(|_| AptError::BadArchive("a member size is not a number"))?;
        cursor += 60;
        let end = cursor
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or(AptError::BadArchive(
                "a member runs past the end of the archive",
            ))?;
        let data = bytes[cursor..end].to_vec();
        cursor = end + (size % 2);
        members.push(ArMember {
            name: name.trim_end_matches('/').to_string(),
            data,
        });
    }
    Ok(members)
}

/// Undoes `gzip`: flate2's `GzDecoder` reads the full format
/// (magic, header with GNU's optional fields, body and CRC),
/// so here it is only checked that the member really is gzip.
fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, AptError> {
    if bytes.len() < 2 || bytes[0] != 0x1f || bytes[1] != 0x8b {
        return Err(AptError::BadArchive("the data member is not gzip"));
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|_| AptError::BadArchive("the gzip body does not inflate"))?;
    Ok(out)
}

/// Walks a ustar tar in 512-byte steps and returns the content of the
/// first member whose name matches exactly.
fn tar_member(tar: &[u8], wanted: &str) -> Option<Vec<u8>> {
    let mut cursor = 0usize;
    while cursor + 512 <= tar.len() {
        let header = &tar[cursor..cursor + 512];
        if header.iter().all(|byte| *byte == 0) {
            return None;
        }
        let name = String::from_utf8_lossy(&header[0..100])
            .trim_end_matches('\0')
            .to_string();
        // The size is in ASCII octal, 12 bytes with spare field.
        let size_field = &header[124..136];
        let size_str = String::from_utf8_lossy(size_field)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        let size = usize::from_str_radix(&size_str, 8).unwrap_or(0);
        if name == wanted {
            let end = cursor + 512 + size;
            if end > tar.len() {
                return None;
            }
            return Some(tar[cursor + 512..end].to_vec());
        }
        cursor += 512 + size.div_ceil(512) * 512;
    }
    None
}

/// Reads the `control` of a `.deb`: the sequence of `ar` members, the
/// `control.tar` (depending on how dpkg compresses it: plain, gzip or xz) and the
/// header fields.
pub fn read_deb_control(deb: &[u8]) -> Result<DebControl, AptError> {
    let members = ar_members(deb)?;
    let data_member = members
        .iter()
        .find(|member| member.name.starts_with("control.tar"))
        .ok_or(AptError::BadArchive("no control.tar member"))?;
    let tar = if data_member.name.ends_with(".gz") {
        gunzip(&data_member.data)?
    } else if data_member.name.ends_with(".xz") {
        let mut out = Vec::new();
        xz2::read::XzDecoder::new(&data_member.data[..])
            .read_to_end(&mut out)
            .map_err(|_| AptError::BadArchive("the xz body does not decompress"))?;
        out
    } else {
        data_member.data.clone()
    };
    let raw = tar_member(&tar, "./control")
        .or_else(|| tar_member(&tar, "control"))
        .ok_or(AptError::NoControl)?;
    parse_control(&String::from_utf8_lossy(&raw))
}

/// Parses the text of a `control`: simple fields and the multiline
/// description. Lines that start with a blank or `.` continue the
/// previous field, which is how dpkg joins them.
fn parse_control(text: &str) -> Result<DebControl, AptError> {
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut last = String::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') || line == "." {
            let entry = fields.get_mut(&last).ok_or(AptError::NoControl)?;
            // The continuation line is kept as is: the leading space
            // is part of the paragraph, not a formatting escape.
            entry.push('\n');
            entry.push_str(line);
        } else if let Some((key, value)) = line.split_once(':') {
            last = key.trim().to_ascii_lowercase();
            fields.insert(last.clone(), value.trim().to_string());
        }
    }
    let mut take = |name: &str| fields.remove(name).filter(|value| !value.is_empty());
    let package = take("package").ok_or(AptError::MissingField("Package"))?;
    let version = take("version").ok_or(AptError::MissingField("Version"))?;
    let architecture = take("architecture").ok_or(AptError::MissingField("Architecture"))?;
    let description = take("description").ok_or(AptError::MissingField("Description"))?;
    let depends = take("depends");
    let maintainer = take("maintainer");
    let section = take("section");
    let priority = take("priority");
    Ok(DebControl {
        package,
        version,
        architecture,
        depends,
        maintainer,
        description,
        section,
        priority,
    })
}

/// How dpkg compresses the `control.tar` of a test `.deb`: uncompressed,
/// gzip or the xz it has used by default for decades.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum ControlCompression {
    Plain,
    Gz,
    Xz,
}

/// Builds a test `.deb`: an `ar` container with a
/// `debian-binary`, a `control.tar` with the given control and the
/// requested compression, and an empty `data.tar.gz`. For tests only;
/// it is not an installable package, but apt and our reader parse it
/// all the same.
#[cfg(test)]
pub(crate) fn build_test_deb(control_text: &str) -> Vec<u8> {
    build_test_deb_compressed(control_text, ControlCompression::Plain)
}

#[cfg(test)]
pub(crate) fn build_test_deb_compressed(
    control_text: &str,
    compression: ControlCompression,
) -> Vec<u8> {
    fn ar_header(name: &str, size: usize) -> Vec<u8> {
        let mut header = format!("{name:<16}0           0     0     644      {size:<10}`\n");
        if header.len() != 60 {
            // The `ar` headers are exactly 60 bytes; the long name is
            // truncated because in the tests the names are short.
            header.truncate(60);
        }
        header.into_bytes()
    }
    fn tar_block(name: &str, data: &[u8]) -> Vec<u8> {
        let mut block = vec![0u8; 512];
        block[0..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{:07o}\0", data.len());
        block[124..132].copy_from_slice(size.as_bytes());
        block[156] = b'0'; // regular file type
        block.extend_from_slice(data);
        let pad = (512 - data.len() % 512) % 512;
        block.extend(std::iter::repeat_n(0, pad));
        block
    }

    let mut control_tar = tar_block("./control", control_text.as_bytes());
    control_tar.extend(vec![0u8; 1024]); // two zero blocks as terminator
    let mut data_tar = vec![0u8; 1024];
    {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&data_tar).unwrap();
        data_tar = encoder.finish().unwrap();
    }
    let (control_name, control_body) = match compression {
        ControlCompression::Plain => ("control.tar", control_tar.clone()),
        ControlCompression::Gz => {
            use flate2::Compression;
            use flate2::write::GzEncoder;
            use std::io::Write;
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&control_tar).unwrap();
            ("control.tar.gz", encoder.finish().unwrap())
        }
        ControlCompression::Xz => {
            use std::io::Write;
            use xz2::write::XzEncoder;
            let mut encoder = XzEncoder::new(Vec::new(), 6);
            encoder.write_all(&control_tar).unwrap();
            ("control.tar.xz", encoder.finish().unwrap())
        }
    };

    let mut deb = Vec::new();
    deb.extend_from_slice(b"!<arch>\n");
    for (name, body) in [
        ("debian-binary", b"2.0\n".to_vec()),
        (control_name, control_body),
        ("data.tar.gz", data_tar),
    ] {
        deb.extend_from_slice(&ar_header(name, body.len()));
        deb.extend_from_slice(&body);
        if body.len() % 2 == 1 {
            deb.push(b'\n');
        }
    }
    deb
}

/// Renders the `Packages` record of a binary: the control fields
/// in the order dpkg respects, plus the index fields that only the
/// repository knows (size, sum, path). It is the format of `apt-ftparchive
/// packages`, key for the manager to find the package and
/// download it.
pub fn packages_record(control: &DebControl, pool_path: &str, size: u64, sha256: &str) -> String {
    fn push_opt(record: &mut String, name: &str, value: Option<&str>) {
        if let Some(value) = value {
            record.push_str(name);
            record.push_str(": ");
            record.push_str(value);
            record.push('\n');
        }
    }
    let mut record = String::new();
    record.push_str(&format!("Package: {}\n", control.package));
    record.push_str(&format!(
        "Version: {}\nArchitecture: {}\n",
        control.version, control.architecture
    ));
    push_opt(&mut record, "Section", control.section.as_deref());
    push_opt(&mut record, "Priority", control.priority.as_deref());
    push_opt(&mut record, "Maintainer", control.maintainer.as_deref());
    push_opt(&mut record, "Depends", control.depends.as_deref());
    record.push_str(&format!("Filename: {pool_path}\n"));
    record.push_str(&format!("Size: {size}\n"));
    record.push_str(&format!("SHA256: {sha256}\n"));
    // The header field of the descriptive block; multilines handled separately.
    record.push_str(&format!("Description: {}\n", control.description));
    record.push('\n');
    record
}

/// The `Release` header of the dist: the origin, the suite and the codename
/// (same as the requested dist: apt warns if they do not match), the component
/// and the index files with their sum and size, exactly as
/// `apt` verifies them. SHA256 only: it is what `apt-ftparchive` emits by
/// default today and the only thing apt requires. The indexes are already
/// rendered; here only the rows are drawn.
pub fn release_header(dist: &str, date: &str, indexes: &[(&str, &[u8])]) -> String {
    release_header_with_options(dist, date, indexes, false)
}

/// Renders a Debian `Release` header with `Acquire-By-Hash: yes` enabled so
/// APT clients fetch immutable content-addressed index snapshots.
pub fn release_header_by_hash(dist: &str, date: &str, indexes: &[(&str, &[u8])]) -> String {
    release_header_with_options(dist, date, indexes, true)
}

fn release_header_with_options(
    dist: &str,
    date: &str,
    indexes: &[(&str, &[u8])],
    acquire_by_hash: bool,
) -> String {
    let mut out = String::new();
    out.push_str("Origin: Hodei\n");
    out.push_str(&format!("Suite: {dist}\n"));
    out.push_str(&format!("Codename: {dist}\n"));
    out.push_str(&format!("Date: {date}\n"));
    out.push_str(&format!("Architectures: {}\n", ARCHITECTURES.join(" ")));
    out.push_str("Components: main\n");
    if acquire_by_hash {
        out.push_str("Acquire-By-Hash: yes\n");
    }
    out.push_str("SHA256:\n");
    for (name, bytes) in indexes {
        out.push_str(&format!(" {}\n", line_for(name, bytes)));
    }
    out
}

/// The architectures indexed by default: the APT loop and its
/// packaging, which is what runs on this platform.
pub const ARCHITECTURES: &[&str] = &["amd64", "arm64"];

/// The row of an index inside `Release`: sum, size and name,
/// with the one-space indentation that the format requires.
fn line_for(name: &str, bytes: &[u8]) -> String {
    let sum = format!("{:x}", sha2::Sha256::digest(bytes));
    format!("{sum} {} {name}", bytes.len())
}

/// The pool path where a binary lives: it is the one rendered in
/// `Filename:` and the one apt will walk to download it. Any
/// identifier that could escape the root of the
/// repository or that is not transportable over HTTP is rejected.
pub fn pool_path_for(package: &str, version: &str, architecture: &str) -> Result<String, AptError> {
    let candidate = format!("{package}_{version}_{architecture}.deb");
    let safe = candidate.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-' | b'_' | b'~' | b':')
    });
    if !safe || candidate.contains("..") || package.is_empty() {
        return Err(AptError::BadIdentity(candidate));
    }
    Ok(format!("pool/main/{package}/{candidate}"))
}

/// The suffix with which a pool binary stores its index
/// record: the bytes of the `.deb` and its `Packages` record are two immutable
/// assets, and the record is what is read to assemble the index.
pub fn record_path_for(pool_path: &str) -> String {
    format!("{pool_path}.record")
}

/// Compression format of a Debian `Packages` index file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackagesCompression {
    /// Uncompressed `Packages`.
    Plain,
    /// Gzip-compressed `Packages.gz`.
    Gz,
    /// XZ-compressed `Packages.xz`.
    Xz,
}

impl PackagesCompression {
    /// File extension for this compression format.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Gz => ".gz",
            Self::Xz => ".xz",
        }
    }

    /// HTTP Content-Type for this compression format.
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Plain => "text/plain",
            Self::Gz => "application/gzip",
            Self::Xz => "application/x-xz",
        }
    }
}

/// The paths the apt surface knows how to serve, already interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The binary itself, at `pool/.../{archivo}.deb`.
    Pool {
        /// The path of the binary within the repository.
        path: String,
    },
    /// The index of an architecture of a distribution.
    Packages {
        /// The distribution: `stable`, `bookworm`, `trixie`...
        dist: String,
        /// The component (`main`, `contrib`, ...), or `None` for flat/monolithic layout.
        component: Option<String>,
        /// The architecture of the requested index.
        architecture: String,
        /// Index compression (`Packages`, `Packages.gz`, `Packages.xz`).
        compression: PackagesCompression,
    },
    /// By-hash index lookup (`dists/{dist}/{component}/binary-{arch}/by-hash/SHA256/{hash}`).
    PackagesByHash {
        /// Distribution name.
        dist: String,
        /// Optional component name.
        component: Option<String>,
        /// Target architecture.
        architecture: String,
        /// Hash algorithm (e.g. `SHA256`).
        algorithm: String,
        /// Expected hex digest.
        hash: String,
    },
    /// Inline-signed `InRelease` file for a distribution.
    InRelease {
        /// Distribution name.
        dist: String,
    },
    /// The signed header of a distribution.
    Release {
        /// The distribution of the header.
        dist: String,
    },
    /// The detached signature of that header: `Release.gpg`.
    ReleaseSignature {
        /// The signed distribution.
        dist: String,
    },
    /// The hub's public key in armored form, the `signed-by` of apt.
    Key,
}

fn valid_apt_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.contains("..")
        && !segment.contains('/')
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn parse_packages_suffix(tail: &str) -> Option<(&str, PackagesCompression)> {
    if let Some(prefix) = tail.strip_suffix("/Packages") {
        Some((prefix, PackagesCompression::Plain))
    } else if let Some(prefix) = tail.strip_suffix("/Packages.gz") {
        Some((prefix, PackagesCompression::Gz))
    } else if let Some(prefix) = tail.strip_suffix("/Packages.xz") {
        Some((prefix, PackagesCompression::Xz))
    } else {
        None
    }
}

fn parse_component_and_arch(prefix: &str) -> Result<(Option<String>, String), AptError> {
    let (component, arch_part) = if let Some(arch) = prefix.strip_prefix("binary-") {
        (None, arch)
    } else if let Some((comp, rest)) = prefix.split_once('/') {
        if !valid_apt_segment(comp) {
            return Err(AptError::BadIdentity(comp.to_string()));
        }
        let arch = rest
            .strip_prefix("binary-")
            .ok_or_else(|| AptError::BadIdentity(prefix.to_string()))?;
        (Some(comp.to_string()), arch)
    } else {
        return Err(AptError::BadIdentity(prefix.to_string()));
    };
    if arch_part.is_empty()
        || !arch_part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(AptError::BadIdentity(arch_part.to_string()));
    }
    Ok((component, arch_part.to_string()))
}

impl Route {
    /// Canonical repository-relative asset path for this route.
    pub fn asset_path(&self) -> String {
        match self {
            Self::Key => "key.asc".to_string(),
            Self::Pool { path } => path.clone(),
            Self::InRelease { dist } => format!("dists/{dist}/InRelease"),
            Self::Release { dist } => format!("dists/{dist}/Release"),
            Self::ReleaseSignature { dist } => format!("dists/{dist}/Release.gpg"),
            Self::Packages {
                dist,
                component: Some(component),
                architecture,
                compression,
            } => format!(
                "dists/{dist}/{component}/binary-{architecture}/Packages{}",
                compression.extension()
            ),
            Self::Packages {
                dist,
                component: None,
                architecture,
                compression,
            } => format!(
                "dists/{dist}/binary-{architecture}/Packages{}",
                compression.extension()
            ),
            Self::PackagesByHash {
                dist,
                component: Some(component),
                architecture,
                algorithm,
                hash,
            } => {
                format!("dists/{dist}/{component}/binary-{architecture}/by-hash/{algorithm}/{hash}")
            }
            Self::PackagesByHash {
                dist,
                component: None,
                architecture,
                algorithm,
                hash,
            } => format!("dists/{dist}/binary-{architecture}/by-hash/{algorithm}/{hash}"),
        }
    }

    /// Interprets the rest of `/apt/{repositorio}/...`, raw: apt does not
    /// escape names, and a `%` here is an impossible name.
    pub fn parse(rest: &str) -> Result<Route, AptError> {
        if rest == "key.asc" {
            return Ok(Route::Key);
        }
        if let Some(path) = rest.strip_prefix("pool/") {
            if !path.ends_with(".deb")
                || path.is_empty()
                || path.contains("..")
                || !path.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'.' | b'/' | b'+' | b'-' | b'_' | b'~' | b':')
                })
            {
                return Err(AptError::BadIdentity(rest.to_string()));
            }
            return Ok(Route::Pool {
                path: format!("pool/{path}"),
            });
        }
        // Everything else hangs from dists/{dist}/. The name of the
        // distribution is an identifier: no slashes or dots.
        let dist_part = rest
            .strip_prefix("dists/")
            .ok_or_else(|| AptError::BadIdentity(rest.to_string()))?;
        let (dist, tail) = dist_part
            .split_once('/')
            .ok_or_else(|| AptError::BadIdentity(rest.to_string()))?;
        if !valid_apt_segment(dist) {
            return Err(AptError::BadIdentity(dist.to_string()));
        }
        match tail {
            "InRelease" => Ok(Route::InRelease {
                dist: dist.to_string(),
            }),
            "Release" => Ok(Route::Release {
                dist: dist.to_string(),
            }),
            "Release.gpg" => Ok(Route::ReleaseSignature {
                dist: dist.to_string(),
            }),
            _ => {
                if let Some((prefix, compression)) = parse_packages_suffix(tail) {
                    let (component, architecture) = parse_component_and_arch(prefix)?;
                    return Ok(Route::Packages {
                        dist: dist.to_string(),
                        component,
                        architecture,
                        compression,
                    });
                }
                if let Some((prefix, hash_tail)) = tail.split_once("/by-hash/") {
                    let (component, architecture) = parse_component_and_arch(prefix)?;
                    let (algorithm, hash) = hash_tail
                        .split_once('/')
                        .ok_or_else(|| AptError::BadIdentity(tail.to_string()))?;
                    if algorithm != "SHA256"
                        || hash.len() != 64
                        || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                    {
                        return Err(AptError::BadIdentity(tail.to_string()));
                    }
                    return Ok(Route::PackagesByHash {
                        dist: dist.to_string(),
                        component,
                        architecture,
                        algorithm: algorithm.to_string(),
                        hash: hash.to_string(),
                    });
                }
                Err(AptError::BadIdentity(tail.to_string()))
            }
        }
    }

    /// The `Date:` of `Release` in the format that apt parses with
    /// strptime: RFC 1123 with the zone spelled in letters, not in numbers.
    pub fn release_date(moment: &chrono::DateTime<chrono::Utc>) -> String {
        moment.format("%a, %d %b %Y %H:%M:%S UTC").to_string()
    }
}

/// Optional APT proxy scope configuration under `config_settings.apt`.
///
/// Restricts which distributions, components, and architectures this proxy
/// repository is allowed to serve. When a list is empty, that dimension is
/// unrestricted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AptProxyConfig {
    /// Allowed distributions (empty means unrestricted).
    pub distributions: Vec<String>,
    /// Allowed components (empty means unrestricted).
    pub components: Vec<String>,
    /// Allowed architectures (empty means unrestricted).
    pub architectures: Vec<String>,
    /// Operator-pinned OpenPGP public key used to authenticate proxy metadata
    /// before a group republishes its hashes.
    pub trusted_key_asc: Option<String>,
    /// The oldest `Date` (seconds before now) of a `Release` the proxy still
    /// serves. Upstream that stops publishing keeps a signed `Release` valid
    /// forever when it carries no `Valid-Until`; this bound is the operator's
    /// freeze detector. `None` means no age bound beyond `Valid-Until`.
    pub max_metadata_age_secs: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAptProxyConfig {
    #[serde(default)]
    distributions: Vec<String>,
    #[serde(default)]
    components: Vec<String>,
    #[serde(default)]
    architectures: Vec<String>,
    #[serde(default)]
    trusted_key_asc: Option<String>,
    #[serde(default)]
    max_metadata_age_secs: Option<u64>,
}

impl AptProxyConfig {
    /// Reads and validates `config_settings.apt` if present.
    pub fn of(config: &RepositoryConfig) -> Result<Self, UpstreamConfigError> {
        let Some(value) = config.config_settings.get("apt") else {
            return Ok(Self::default());
        };
        if value.is_null() {
            return Ok(Self::default());
        }
        let raw: RawAptProxyConfig = serde_json::from_value(value.clone())
            .map_err(|error| UpstreamConfigError(format!("invalid apt settings: {error}")))?;
        for item in raw
            .distributions
            .iter()
            .chain(raw.components.iter())
            .chain(raw.architectures.iter())
        {
            if !valid_apt_segment(item) {
                return Err(UpstreamConfigError(format!(
                    "invalid apt identifier: {item:?}"
                )));
            }
        }
        Ok(Self {
            distributions: raw.distributions,
            components: raw.components,
            architectures: raw.architectures,
            trusted_key_asc: raw.trusted_key_asc,
            max_metadata_age_secs: raw.max_metadata_age_secs,
        })
    }

    /// Returns true if `dist` is allowed by this configuration.
    pub fn allows_dist(&self, dist: &str) -> bool {
        self.distributions.is_empty() || self.distributions.iter().any(|d| d == dist)
    }

    /// Returns true if `component` is allowed by this configuration.
    pub fn allows_component(&self, component: Option<&str>) -> bool {
        if self.components.is_empty() {
            return true;
        }
        match component {
            Some(comp) => self.components.iter().any(|c| c == comp),
            None => false,
        }
    }

    /// Returns true if `arch` is allowed by this configuration.
    pub fn allows_architecture(&self, arch: &str) -> bool {
        self.architectures.is_empty() || self.architectures.iter().any(|a| a == arch)
    }

    /// Returns true if `pool_path` belongs to an allowed component.
    pub fn allows_pool_path(&self, pool_path: &str) -> bool {
        if self.components.is_empty() {
            return true;
        }
        let Some(rest) = pool_path.strip_prefix("pool/") else {
            return false;
        };
        let Some((component, _)) = rest.split_once('/') else {
            return false;
        };
        self.components.iter().any(|c| c == component)
    }
}

/// Parsed fields of an upstream `Release` or `InRelease` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRelease {
    /// `Suite` header value, if present.
    pub suite: Option<String>,
    /// `Codename` header value, if present.
    pub codename: Option<String>,
    /// `Date` timestamp, if present: when upstream produced the `Release`.
    pub date: Option<chrono::DateTime<chrono::Utc>>,
    /// `Valid-Until` timestamp, if present.
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
    /// Map from relative index path (e.g. `main/binary-amd64/Packages.xz`) to `(sha256_hex, size_bytes)`.
    pub sha256_entries: BTreeMap<String, (String, u64)>,
}

impl ParsedRelease {
    /// Returns true if `Valid-Until` is present and strictly earlier than `now`.
    pub fn is_expired(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.valid_until.is_some_and(|until| until < now)
    }
}

/// Extracts the plain `Release` body from `Release` or OpenPGP clearsigned `InRelease` bytes.
pub fn extract_release_body(bytes: &[u8], is_inrelease: bool) -> Result<String, AptError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AptError::InvalidMetadata("Release is not valid UTF-8".to_string()))?;
    if !is_inrelease {
        return Ok(text.to_string());
    }
    let mut lines = text.lines();
    let mut found_header = false;
    for line in &mut lines {
        if line.trim_end() == "-----BEGIN PGP SIGNED MESSAGE-----" {
            found_header = true;
            break;
        }
    }
    if !found_header {
        return Err(AptError::InvalidMetadata(
            "InRelease is missing the PGP signed message header".to_string(),
        ));
    }
    // Skip armor headers (e.g. `Hash: SHA256`) until the blank separator line.
    let mut found_blank = false;
    for line in &mut lines {
        if line.trim_end().is_empty() {
            found_blank = true;
            break;
        }
    }
    if !found_blank {
        return Err(AptError::InvalidMetadata(
            "InRelease is missing the header separator".to_string(),
        ));
    }
    let mut body = String::new();
    let mut found_sig = false;
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "-----BEGIN PGP SIGNATURE-----" {
            found_sig = true;
            break;
        }
        // RFC 4880 dash-escaping: `- ` at the start of a line escapes `-`.
        let unescaped = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        body.push_str(unescaped);
        body.push('\n');
    }
    if !found_sig {
        return Err(AptError::InvalidMetadata(
            "InRelease is missing the PGP signature block".to_string(),
        ));
    }
    Ok(body)
}

fn parse_apt_timestamp(raw: &str) -> Result<chrono::DateTime<chrono::Utc>, AptError> {
    let trimmed = raw.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(trimmed) {
        return Ok(dt.with_timezone(&chrono::Utc));
    }
    if let Some(without_utc) = trimmed.strip_suffix(" UTC")
        && let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(without_utc, "%a, %d %b %Y %H:%M:%S")
    {
        return Ok(naive.and_utc());
    }
    Err(AptError::InvalidMetadata(format!(
        "unparseable Release timestamp: {trimmed}"
    )))
}

/// Parses a `Release` document, extracting `Suite`, `Codename`, `Valid-Until`, and `SHA256:` entries.
pub fn parse_release(text: &str) -> Result<ParsedRelease, AptError> {
    let mut suite = None;
    let mut codename = None;
    let mut valid_until = None;
    let mut date = None;
    let mut sha256_entries = BTreeMap::new();
    let mut in_sha256 = false;

    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            if in_sha256 {
                let mut parts = line.split_whitespace();
                let (Some(digest), Some(size_str), Some(path)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                if parts.next().is_some() {
                    continue;
                }
                let size = size_str.parse::<u64>().map_err(|_| {
                    AptError::InvalidMetadata(format!("invalid size in Release SHA256: {size_str}"))
                })?;
                sha256_entries.insert(path.to_string(), (digest.to_ascii_lowercase(), size));
            }
            continue;
        }
        in_sha256 = false;
        if let Some((key, value)) = line.split_once(':') {
            let key_lower = key.trim().to_ascii_lowercase();
            let val = value.trim();
            match key_lower.as_str() {
                "suite" if !val.is_empty() => suite = Some(val.to_string()),
                "codename" if !val.is_empty() => codename = Some(val.to_string()),
                // An unreadable `Date` is not a parse error of the whole index:
                // it stays `None`, and only a proxy that pins a key (and so
                // needs the date to detect rollback) refuses it.
                "date" if !val.is_empty() => {
                    date = parse_apt_timestamp(val).ok();
                }
                "valid-until" if !val.is_empty() => {
                    valid_until = Some(parse_apt_timestamp(val)?);
                }
                "sha256" => {
                    in_sha256 = true;
                }
                _ => {}
            }
        }
    }

    if sha256_entries.is_empty() {
        return Err(AptError::InvalidMetadata(
            "Release contains no SHA256 entries".to_string(),
        ));
    }

    Ok(ParsedRelease {
        suite,
        codename,
        date,
        valid_until,
        sha256_entries,
    })
}

/// Decompresses a `Packages`, `Packages.gz`, or `Packages.xz` index payload.
pub fn decompress_packages(
    bytes: &[u8],
    compression: PackagesCompression,
) -> Result<Vec<u8>, AptError> {
    match compression {
        PackagesCompression::Plain => Ok(bytes.to_vec()),
        PackagesCompression::Gz => gunzip(bytes),
        PackagesCompression::Xz => {
            let mut out = Vec::new();
            xz2::read::XzDecoder::new(bytes)
                .read_to_end(&mut out)
                .map_err(|_| AptError::BadArchive("the xz Packages index does not decompress"))?;
            Ok(out)
        }
    }
}

/// Expected digest and size of a `.deb` entry in a `Packages` index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebIndexEntry {
    /// Expected lowercase hex SHA256 digest.
    pub sha256: String,
    /// Expected size in bytes.
    pub size: u64,
}

/// Finds the `SHA256` and `Size` for `pool_path` inside a decompressed `Packages` text.
pub fn find_deb_in_packages(packages_text: &str, pool_path: &str) -> Option<DebIndexEntry> {
    let mut filename: Option<&str> = None;
    let mut sha256: Option<String> = None;
    let mut size: Option<u64> = None;

    let check_stanza = |filename: Option<&str>, sha256: Option<String>, size: Option<u64>| {
        if filename == Some(pool_path)
            && let (Some(sha256), Some(size)) = (sha256, size)
        {
            return Some(DebIndexEntry { sha256, size });
        }
        None
    };

    for raw_line in packages_text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            if let Some(found) = check_stanza(filename, sha256.take(), size) {
                return Some(found);
            }
            filename = None;
            size = None;
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        if let Some(val) = line.strip_prefix("Filename:") {
            filename = Some(val.trim());
        } else if let Some(val) = line.strip_prefix("SHA256:") {
            sha256 = Some(val.trim().to_ascii_lowercase());
        } else if let Some(val) = line.strip_prefix("Size:") {
            size = val.trim().parse::<u64>().ok();
        }
    }
    check_stanza(filename, sha256, size)
}

/// A single package stanza parsed from a Debian `Packages` index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStanza {
    /// `Package` field value.
    pub package: String,
    /// `Version` field value.
    pub version: String,
    /// `Architecture` field value.
    pub architecture: String,
    /// `Filename` field value.
    pub filename: String,
    /// Normalized stanza text (no leading/trailing blank lines, ends with `\n\n`).
    pub raw: String,
}

/// Parses all package stanzas from a decompressed Debian `Packages` index.
pub fn parse_packages_stanzas(packages_text: &str) -> Result<Vec<PackageStanza>, AptError> {
    let normalized = packages_text.replace("\r\n", "\n");
    let mut out = Vec::new();
    for block in normalized.split("\n\n") {
        let trimmed = block.trim_matches('\n');
        if trimmed.trim().is_empty() {
            continue;
        }
        let mut package: Option<String> = None;
        let mut version: Option<String> = None;
        let mut architecture: Option<String> = None;
        let mut filename: Option<String> = None;
        for line in trimmed.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                continue;
            }
            if let Some(val) = line.strip_prefix("Package:") {
                if package.is_none() {
                    package = Some(val.trim().to_string());
                }
            } else if let Some(val) = line.strip_prefix("Version:") {
                if version.is_none() {
                    version = Some(val.trim().to_string());
                }
            } else if let Some(val) = line.strip_prefix("Architecture:") {
                if architecture.is_none() {
                    architecture = Some(val.trim().to_string());
                }
            } else if let Some(val) = line.strip_prefix("Filename:")
                && filename.is_none()
            {
                filename = Some(val.trim().to_string());
            }
        }
        let (Some(package), Some(version), Some(architecture), Some(filename)) =
            (package, version, architecture, filename)
        else {
            return Err(AptError::BadArchive(
                "a Packages stanza is missing Package, Version, Architecture, or Filename",
            ));
        };
        if package.is_empty()
            || version.is_empty()
            || architecture.is_empty()
            || filename.is_empty()
        {
            return Err(AptError::BadArchive(
                "a Packages stanza has an empty identity field",
            ));
        }
        out.push(PackageStanza {
            package,
            version,
            architecture,
            filename,
            raw: format!("{trimmed}\n\n"),
        });
    }
    Ok(out)
}

/// Composes a deterministic `Packages` index for `target_arch` from member
/// stanzas ordered by group precedence (first member wins on `(package, version, arch)`).
pub fn compose_packages_index<'a>(
    member_stanzas: impl IntoIterator<Item = &'a [PackageStanza]>,
    target_arch: &str,
) -> String {
    let mut winners: BTreeMap<(&str, &str, &str), &str> = BTreeMap::new();
    for stanzas in member_stanzas {
        for stanza in stanzas {
            if stanza.architecture != target_arch && stanza.architecture != "all" {
                continue;
            }
            winners
                .entry((
                    stanza.package.as_str(),
                    stanza.version.as_str(),
                    stanza.architecture.as_str(),
                ))
                .or_insert(stanza.raw.as_str());
        }
    }
    let mut out = String::new();
    for raw in winners.into_values() {
        out.push_str(raw);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_CONTROL: &str = concat!(
        "Package: hello-hodei\n",
        "Version: 1.0.0\n",
        "Architecture: amd64\n",
        "Depends: libc6 (>= 2.34)\n",
        "Maintainer: Hodei <hodei@example.local>\n",
        "Section: utils\n",
        "Priority: optional\n",
        "Description: A tiny hello package\n",
        " This is the long description line.\n",
        " .\n",
        " And the final one.\n",
    );

    fn full_deb() -> Vec<u8> {
        build_test_deb(FULL_CONTROL)
    }

    /// The real dpkg compresses `control.tar` in gzip or xz: if the
    /// reader does not open those formats, a `.deb` built by the distro
    /// cannot be published.
    #[test]
    fn a_compressed_control_tar_still_yields_its_control() {
        for compression in [ControlCompression::Gz, ControlCompression::Xz] {
            let deb = build_test_deb_compressed(FULL_CONTROL, compression);
            let control = read_deb_control(&deb)
                .unwrap_or_else(|error| panic!("the {compression:?} control must parse: {error}"));
            assert_eq!(control.package, "hello-hodei");
            assert_eq!(control.depends.as_deref(), Some("libc6 (>= 2.34)"));
        }
    }

    #[test]
    fn a_deb_container_loses_its_control() {
        let control = read_deb_control(&full_deb()).expect("the control parses");
        assert_eq!(control.package, "hello-hodei");
        assert_eq!(control.version, "1.0.0");
        assert_eq!(control.architecture, "amd64");
        assert_eq!(control.depends.as_deref(), Some("libc6 (>= 2.34)"));
        assert_eq!(control.section.as_deref(), Some("utils"));
        // The description is the complete field with its continuations.
        assert!(control.description.starts_with("A tiny hello package\n"));
        assert!(
            control
                .description
                .contains(" This is the long description line.")
        );
        assert!(control.description.ends_with(" And the final one."));
    }

    #[test]
    fn an_archive_without_the_magic_is_rejected() {
        let error = read_deb_control(b"not an archive at all").unwrap_err();
        assert!(matches!(error, AptError::BadArchive(_)));
    }

    #[test]
    fn a_truncated_member_is_rejected() {
        let mut deb = full_deb();
        deb.truncate(deb.len() / 2);
        let error = read_deb_control(&deb).unwrap_err();
        assert!(matches!(error, AptError::BadArchive(_)));
    }

    #[test]
    fn a_control_without_package_is_not_a_package() {
        let deb = build_test_deb("Version: 1.0.0\nArchitecture: amd64\nDescription: x\n");
        assert_eq!(
            read_deb_control(&deb).unwrap_err(),
            AptError::MissingField("Package")
        );
    }

    #[test]
    fn the_record_is_what_dpkg_documents() {
        let control = read_deb_control(&full_deb()).expect("the control parses");
        let sha = "ab".repeat(32);
        let record = packages_record(&control, "pool/main/h/hello.deb", 4096, &sha);
        assert!(record.starts_with("Package: hello-hodei\nVersion: 1.0.0\nArchitecture: amd64\n"));
        assert!(record.contains("Depends: libc6 (>= 2.34)\n"));
        assert!(record.contains("Filename: pool/main/h/hello.deb\nSize: 4096\n"));
        assert!(record.ends_with('\n'));
        // The block ends at the blank line: apt separates records.
        assert!(record.ends_with("\n\n"));
    }

    #[test]
    fn the_release_lists_the_indexes_with_sha256() {
        let index = b"Package: hello-hodei\n\n";
        let release = release_header(
            "stable",
            "Tue, 17 Nov 2026 18:00:00 UTC",
            &[("main/binary-amd64/Packages", index)],
        );
        // Suite and Codename must state the requested dist: otherwise,
        // apt emits a `Conflicting distribution` on every update.
        assert!(release.starts_with(
            "Origin: Hodei\nSuite: stable\nCodename: stable\nDate: Tue, 17 Nov 2026 18:00:00 UTC\n"
        ));
        assert!(release.contains("Components: main\nSHA256:\n"));
        let expected = format!("{:x}", sha2::Sha256::digest(index));
        assert!(release.contains(&format!(
            " {expected} {} main/binary-amd64/Packages\n",
            index.len()
        )));
    }

    #[test]
    fn the_pool_path_stays_inside_the_repository() {
        assert_eq!(
            pool_path_for("hello", "1.0.0", "amd64").unwrap(),
            "pool/main/hello/hello_1.0.0_amd64.deb"
        );
        assert!(pool_path_for("..x", "1.0", "amd64").is_err());
        assert!(pool_path_for("he/llo", "1.0", "amd64").is_err());
        assert!(pool_path_for("", "1.0", "amd64").is_err());
        // Legitimate Debian versions with `+` and `~` pass.
        assert!(pool_path_for("pkg", "1:2.3+dfsg~b1", "arm64").is_ok());
    }

    #[test]
    fn the_routes_are_the_apt_urls() {
        assert_eq!(
            Route::parse("dists/stable/binary-amd64/Packages").unwrap(),
            Route::Packages {
                dist: "stable".into(),
                component: None,
                architecture: "amd64".into(),
                compression: PackagesCompression::Plain,
            }
        );
        // The form that apt always uses: with the component first.
        assert_eq!(
            Route::parse("dists/stable/main/binary-arm64/Packages").unwrap(),
            Route::Packages {
                dist: "stable".into(),
                component: Some("main".into()),
                architecture: "arm64".into(),
                compression: PackagesCompression::Plain,
            }
        );
        assert_eq!(
            Route::parse("dists/bookworm/contrib/binary-amd64/Packages.xz").unwrap(),
            Route::Packages {
                dist: "bookworm".into(),
                component: Some("contrib".into()),
                architecture: "amd64".into(),
                compression: PackagesCompression::Xz,
            }
        );
        assert_eq!(
            Route::parse("dists/bookworm/InRelease").unwrap(),
            Route::InRelease {
                dist: "bookworm".into()
            }
        );
        assert_eq!(
            Route::parse("dists/stable/Release").unwrap(),
            Route::Release {
                dist: "stable".into()
            }
        );
        assert_eq!(
            Route::parse("dists/stable/Release.gpg").unwrap(),
            Route::ReleaseSignature {
                dist: "stable".into()
            }
        );
        assert_eq!(Route::parse("key.asc").unwrap(), Route::Key);
        assert_eq!(
            Route::parse("pool/main/hello/hello_1.0.0_amd64.deb").unwrap(),
            Route::Pool {
                path: "pool/main/hello/hello_1.0.0_amd64.deb".into()
            }
        );
    }

    #[test]
    fn a_route_cannot_escape_or_be_nonsense() {
        // Invalid component/arch paths, path traversal, or unknown routes.
        assert!(Route::parse("dists/stable/other/Packages").is_err());
        assert!(Route::parse("pool/../etc/passwd").is_err());
        assert!(Route::parse("pool/main/x/no-extension").is_err());
        assert!(Route::parse("dists/st..able/Release").is_err());
        assert!(Route::parse("dists/").is_err());
        assert!(Route::parse("index.html").is_err());
    }

    #[test]
    fn release_parsing_extracts_valid_until_and_sha256_entries() {
        let release = concat!(
            "Origin: Debian\n",
            "Suite: stable\n",
            "Codename: bookworm\n",
            "Date: Tue, 17 Nov 2026 18:00:00 UTC\n",
            "Valid-Until: Tue, 24 Nov 2026 18:00:00 UTC\n",
            "SHA256:\n",
            " e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 0 main/binary-amd64/Packages\n",
            " 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef 42 main/binary-amd64/Packages.xz\n",
        );
        let parsed = parse_release(release).unwrap();
        assert_eq!(parsed.suite.as_deref(), Some("stable"));
        assert_eq!(parsed.codename.as_deref(), Some("bookworm"));
        assert!(parsed.valid_until.is_some());
        assert_eq!(
            parsed.sha256_entries.get("main/binary-amd64/Packages.xz"),
            Some(&(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                42
            ))
        );
        let before = chrono::DateTime::parse_from_rfc3339("2026-11-20T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let after = chrono::DateTime::parse_from_rfc3339("2026-11-25T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(!parsed.is_expired(before));
        assert!(parsed.is_expired(after));
    }

    #[test]
    fn find_deb_in_packages_matches_filename_and_digest() {
        let packages = concat!(
            "Package: hello-hodei\n",
            "Version: 1.0.0\n",
            "Architecture: amd64\n",
            "Filename: pool/main/h/hello-hodei/hello-hodei_1.0.0_amd64.deb\n",
            "Size: 1234\n",
            "SHA256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "Description: Hello\n",
            "\n",
        );
        let found = find_deb_in_packages(
            packages,
            "pool/main/h/hello-hodei/hello-hodei_1.0.0_amd64.deb",
        )
        .unwrap();
        assert_eq!(
            found,
            DebIndexEntry {
                sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
                size: 1234
            }
        );
        assert!(find_deb_in_packages(packages, "pool/main/o/other/other_1.0_amd64.deb").is_none());
    }

    #[test]
    fn the_record_hangs_from_the_binary_it_describes() {
        assert_eq!(
            record_path_for("pool/main/hello/hello_1.0.0_amd64.deb"),
            "pool/main/hello/hello_1.0.0_amd64.deb.record"
        );
    }

    #[test]
    fn the_release_date_is_what_apt_strptimes() {
        let moment = chrono::DateTime::parse_from_rfc3339("2026-11-17T18:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            Route::release_date(&moment),
            "Tue, 17 Nov 2026 18:00:00 UTC"
        );
    }

    #[test]
    fn compose_packages_index_respects_precedence_and_is_deterministic() {
        let m1 = parse_packages_stanzas(concat!(
            "Package: zebra\n",
            "Version: 1.0.0\n",
            "Architecture: amd64\n",
            "Filename: pool/main/z/zebra_1.0.0_amd64.deb\n",
            "SHA256: 1111111111111111111111111111111111111111111111111111111111111111\n\n",
            "Package: shared\n",
            "Version: 2.0.0\n",
            "Architecture: amd64\n",
            "Filename: pool/main/s/shared_2.0.0_amd64.deb\n",
            "SHA256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\n",
        ))
        .unwrap();
        let m2 = parse_packages_stanzas(concat!(
            "Package: shared\n",
            "Version: 2.0.0\n",
            "Architecture: amd64\n",
            "Filename: pool/main/s/shared_2.0.0_amd64.deb\n",
            "SHA256: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\n",
            "Package: alpha\n",
            "Version: 1.0.0\n",
            "Architecture: all\n",
            "Filename: pool/main/a/alpha_1.0.0_all.deb\n",
            "SHA256: cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\n\n",
        ))
        .unwrap();

        let composed = compose_packages_index([m1.as_slice(), m2.as_slice()], "amd64");
        assert!(
            composed.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert!(
            !composed.contains("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
        assert!(composed.contains("Package: alpha"));
        assert!(composed.contains("Package: zebra"));
        let alpha_pos = composed.find("Package: alpha").unwrap();
        let shared_pos = composed.find("Package: shared").unwrap();
        let zebra_pos = composed.find("Package: zebra").unwrap();
        assert!(alpha_pos < shared_pos && shared_pos < zebra_pos);
    }

    #[test]
    fn release_date_is_parsed_and_an_unreadable_one_is_none() {
        let sha = "SHA256:\n 00 1 main/binary-amd64/Packages\n";
        let good = parse_release(&format!("Date: Tue, 17 Nov 2026 18:00:00 UTC\n{sha}")).unwrap();
        assert_eq!(
            good.date,
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-11-17T18:00:00Z")
                    .unwrap()
                    .into()
            )
        );
        let bad = parse_release(&format!("Date: not a date\n{sha}")).unwrap();
        assert_eq!(bad.date, None);
        assert_eq!(parse_release(sha).unwrap().date, None);
    }
}
