//! La parte pura del formato Maven 2: rutas, versiones, POM y metadatos.
//!
//! Nada aqui toca disco, red ni catalogo: son funciones sobre cadenas y
//! bytes, y por eso se prueban sin infraestructura. El servicio las usa
//! para decidir si una ruta es Maven, si una version es release o
//! snapshot, si un POM dice lo mismo que la ruta donde se sube, y para
//! **derivar** `maven-metadata.xml` de lo que el catalogo ya sabe.
//!
//! # Por que los metadatos se derivan y no se guardan
//!
//! `maven-metadata.xml` es una vista de lo que hay en el repositorio. Si
//! se guardara, habria dos fuentes de verdad (los ficheros y el XML) y
//! dos despliegues concurrentes se pisarian al reescribirlo. Derivandolo
//! en cada lectura, el XML siempre dice lo que hay, un fallo a mitad de
//! un despliegue no deja un XML inconsistente, y no hay nada que reparar.
//!
//! # Layout
//!
//! ```text
//! {grupo/con/barras}/{artifactId}/{version}/{artifactId}-{version}[-{clasificador}].{ext}
//! {grupo/con/barras}/{artifactId}/maven-metadata.xml
//! {grupo/con/barras}/{artifactId}/{version}-SNAPSHOT/maven-metadata.xml
//! cualquiera de los anteriores + .md5 | .sha1 | .sha256 | .sha512
//! ```

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

/// El nombre del fichero de metadatos.
pub const METADATA_FILE: &str = "maven-metadata.xml";

/// El sufijo de una version snapshot.
pub const SNAPSHOT_SUFFIX: &str = "-SNAPSHOT";

/// Los bytes maximos de un POM o de unos metadatos que el servicio lee en
/// memoria.
pub const MAX_XML_BYTES: usize = 1024 * 1024;

/// Los bytes maximos de un fichero de checksum.
pub const MAX_CHECKSUM_BYTES: usize = 1024;

/// La longitud maxima de una version.
const MAX_VERSION_LEN: usize = 128;

/// La profundidad maxima de elementos XML que se acepta.
const MAX_XML_DEPTH: usize = 32;

/// Los elementos XML maximos de un documento.
const MAX_XML_ELEMENTS: usize = 20_000;

/// Longitud maxima del nombre de un elemento XML.
const MAX_XML_NAME: usize = 64;

/// Bytes maximos de rutas materializadas al leer un XML.
const MAX_XML_PATH_BYTES: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Rutas
// ---------------------------------------------------------------------------

/// Por que una ruta no es una ruta Maven valida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// No tiene la forma `grupo/artefacto/version/fichero`.
    Layout,
    /// Un identificador de grupo o artefacto no es valido.
    Identifier,
    /// La version no es una version valida.
    Version,
    /// El nombre del fichero no corresponde a su artefacto y version.
    FileName,
}

impl PathError {
    /// Lo que se le dice al cliente.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Layout => "the path is not a Maven 2 repository path",
            Self::Identifier => "the group or artifact identifier is not valid",
            Self::Version => "the version is not valid",
            Self::FileName => "the file name does not match the artifact and version of its path",
        }
    }
}

/// Como se escribe la version de un fichero respecto a su directorio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionKind {
    /// Una version fija (`1.2.3`).
    Release,
    /// El nombre flotante de un snapshot (`1.0-SNAPSHOT`).
    Snapshot,
    /// Un snapshot con marca de tiempo (`1.0-20240101.120000-3`).
    Timestamped {
        /// `yyyyMMdd.HHmmss`.
        stamp: String,
        /// El numero de construccion.
        build: u32,
    },
}

/// Un fichero de un artefacto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenFile {
    /// El `groupId` (con puntos).
    pub group_id: String,
    /// El `artifactId`.
    pub artifact_id: String,
    /// La version del **directorio** (`1.0-SNAPSHOT`, nunca la marca).
    pub version: String,
    /// Como se escribe la version dentro del nombre del fichero.
    pub kind: VersionKind,
    /// El clasificador, si lo hay.
    pub classifier: Option<String>,
    /// La extension (`jar`, `pom`, `tar.gz`...).
    pub extension: String,
}

impl MavenFile {
    /// Si es el POM del artefacto (sin clasificador).
    pub fn is_pom(&self) -> bool {
        self.extension == "pom" && self.classifier.is_none()
    }

    /// Si la version del directorio es un snapshot.
    pub fn is_snapshot(&self) -> bool {
        self.version.ends_with(SNAPSHOT_SUFFIX)
    }
}

/// Que unos metadatos describen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataLevel {
    /// Las versiones de un artefacto.
    Artifact {
        /// El `groupId`.
        group_id: String,
        /// El `artifactId`.
        artifact_id: String,
    },
    /// Las construcciones de un snapshot.
    Version {
        /// El `groupId`.
        group_id: String,
        /// El `artifactId`.
        artifact_id: String,
        /// La version snapshot.
        version: String,
    },
}

/// El algoritmo de un checksum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// MD5, 32 caracteres hexadecimales.
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

    /// Los caracteres hexadecimales del resumen.
    pub fn hex_len(self) -> usize {
        match self {
            Self::Md5 => 32,
            Self::Sha1 => 40,
            Self::Sha256 => 64,
            Self::Sha512 => 128,
        }
    }
}

/// De que es el checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// De un fichero del artefacto.
    File(MavenFile),
    /// De unos metadatos.
    Metadata(MetadataLevel),
}

/// Lo que nombra una ruta de un repositorio Maven 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Un fichero del artefacto (POM, JAR, fuentes, modulo...).
    File(MavenFile),
    /// `maven-metadata.xml`.
    Metadata(MetadataLevel),
    /// El checksum de un fichero o de unos metadatos.
    Checksum {
        /// El algoritmo.
        algorithm: Algorithm,
        /// De que es.
        subject: Subject,
        /// La ruta de aquello de lo que es checksum.
        subject_path: String,
    },
}

/// Si una version es valida.
///
/// Una version se escribe en rutas y en ficheros: solo letras, cifras y
/// `. _ - +`, sin empezar ni acabar por separador, y sin ser `.` o `..`.
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

/// Un segmento de grupo: un identificador sin puntos, porque el grupo
/// `com.acme` es el directorio `com/acme` y no puede haber dos formas de
/// escribir las mismas coordenadas.
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

/// Analiza una ruta de un repositorio Maven 2.
pub fn parse_path(path: &str) -> Result<Target, PathError> {
    for (suffix, algorithm) in Algorithm::ALL {
        if let Some(subject_path) = path.strip_suffix(suffix) {
            // Un checksum de un checksum no existe.
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
    // grupo (>=1) + artefacto + version + fichero
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
    // La extension empieza por letra: `a-1.0.1.jar` en el directorio `1.0`
    // no es el fichero de la version `1.0`.
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
        // grupo (>=1) + artefacto + version snapshot
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

/// Separa de un nombre de fichero la version y lo que queda detras.
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
// Versiones
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

/// El orden de un calificador respecto a la version publicada (`5`).
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

/// El relleno de una version mas corta: `0` frente a un numero, la version
/// publicada frente a un calificador.
fn non_number<'a>(other: &Item, zero: &'a Item, release: &'a Item) -> &'a Item {
    match other {
        Item::Number(_) => zero,
        Item::Qualifier(_) => release,
    }
}

/// Compara dos versiones con el orden de Maven: `1.0-SNAPSHOT < 1.0 < 1.0.1`
/// y `1.9 < 1.10`.
pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let (a, b) = (items(left), items(right));
    for index in 0..a.len().max(b.len()) {
        let ordering = compare_items(a.get(index), b.get(index));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    // Iguales para Maven, distintas como texto: el desempate es estable.
    left.cmp(right)
}

// ---------------------------------------------------------------------------
// Configuracion
// ---------------------------------------------------------------------------

/// Que versiones admite un repositorio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionPolicy {
    /// Releases y snapshots.
    #[default]
    Mixed,
    /// Solo versiones fijas.
    Release,
    /// Solo snapshots.
    Snapshot,
}

/// La configuracion de un repositorio Maven 2.
///
/// Vive bajo la clave `maven` de los ajustes del repositorio:
///
/// ```json
/// { "maven": { "version_policy": "release", "release_redeploy": false } }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MavenConfig {
    /// Que versiones admite. Por defecto, todas.
    #[serde(default)]
    pub version_policy: VersionPolicy,
    /// Si una release puede cambiar de contenido. Por defecto, no.
    #[serde(default)]
    pub release_redeploy: bool,
}

impl MavenConfig {
    /// La configuracion de unos ajustes de repositorio.
    ///
    /// Sin clave `maven`, la de por defecto; una clave que no se entiende
    /// es un error, nunca la de por defecto.
    pub fn of_settings(settings: &Value) -> Result<Self, String> {
        match settings.get("maven") {
            None => Ok(Self::default()),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|error| format!("the maven settings cannot be read: {error}")),
        }
    }

    /// Si el repositorio admite versiones de ese tipo.
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

/// Por que un XML no se puede leer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlError {
    /// No esta bien formado.
    Malformed,
    /// Lleva una declaracion `DOCTYPE`, que no se acepta.
    Doctype,
    /// Es demasiado profundo o demasiado grande.
    TooComplex,
}

/// Un elemento con texto: su ruta (`project/parent/groupId`) y su texto.
type Leaf = (String, String);

/// Lee un XML y devuelve el texto de cada elemento, por ruta.
///
/// No expande entidades externas ni acepta `DOCTYPE`: un POM es un dato que
/// llega de fuera.
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

/// Donde acaba una etiqueta de apertura, respetando las comillas.
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

/// Las coordenadas que declara un POM, con la herencia del padre aplicada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PomCoordinates {
    /// El `groupId`.
    pub group_id: String,
    /// El `artifactId`.
    pub artifact_id: String,
    /// La `version`.
    pub version: String,
}

/// Por que un POM no sirve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PomError {
    /// No es XML valido, o es demasiado grande.
    Xml(XmlError),
    /// No es un `project`.
    NotAProject,
    /// Faltan coordenadas.
    Incomplete,
}

impl PomError {
    /// Lo que se le dice al cliente.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Xml(_) => "the POM is not a valid XML document",
            Self::NotAProject => "the POM is not a Maven project",
            Self::Incomplete => "the POM does not declare its group, artifact and version",
        }
    }
}

/// Las coordenadas de un POM.
///
/// El `groupId` y la `version` se heredan del padre cuando el proyecto no
/// los declara, como hace Maven.
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
// Metadatos
// ---------------------------------------------------------------------------

/// Los metadatos que un cliente sube, leidos para comprobarlos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredMetadata {
    /// El `groupId`, si lo declara.
    pub group_id: Option<String>,
    /// El `artifactId`, si lo declara.
    pub artifact_id: Option<String>,
    /// La `version`, si la declara.
    pub version: Option<String>,
}

/// Lee unos metadatos subidos por un cliente.
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

/// Si lo que declara un cliente en sus metadatos cuadra con su ruta.
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

/// Un fichero conocido del catalogo: la ruta y cuando se publico.
#[derive(Debug, Clone)]
pub struct Known {
    /// La ruta dentro del repositorio.
    pub path: String,
    /// Cuando se publico.
    pub updated_at: DateTime<Utc>,
}

fn stamp(moment: DateTime<Utc>) -> String {
    moment.format("%Y%m%d%H%M%S").to_string()
}

/// Los metadatos de un artefacto, derivados de sus ficheros.
///
/// Una version aparece cuando existe su **POM**: el POM es lo ultimo que un
/// despliegue completo deja, y una version con jar y sin POM no se puede
/// resolver. Devuelve `None` si no hay ninguna version.
pub fn artifact_metadata(group_id: &str, artifact_id: &str, files: &[Known]) -> Option<String> {
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
    if versions.is_empty() {
        return None;
    }
    let mut ordered: Vec<&String> = versions.keys().collect();
    sort_versions(&mut ordered);
    let latest = ordered.last().copied()?;
    let release = ordered
        .iter()
        .rev()
        .find(|version| !version.ends_with(SNAPSHOT_SUFFIX));
    let last = versions.values().max().copied()?;

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
    xml.push_str(&format!("    <lastUpdated>{}</lastUpdated>\n", stamp(last)));
    xml.push_str("  </versioning>\n</metadata>\n");
    Some(xml)
}

/// Ordena versiones sin entrar nunca en panico.
///
/// La comparacion de Maven no es transitiva en todos los casos
/// (`1.0-alpha < 1 < 1-sp < 1.0-alpha`) y `sort_by` de la libreria estandar
/// puede abortar ante un orden que no es total. La insercion binaria acepta
/// cualquier comparador y es estable; el coste lineal de los desplazamientos
/// es despreciable frente a los miles de versiones que cabe en un artefacto.
fn sort_versions(versions: &mut Vec<&String>) {
    let mut sorted: Vec<&String> = Vec::with_capacity(versions.len());
    for version in versions.drain(..) {
        let at =
            sorted.partition_point(|other| compare_versions(other, version) != Ordering::Greater);
        sorted.insert(at, version);
    }
    *versions = sorted;
}

/// La clave de una construccion: clasificador y extension.
type LatestKey = (Option<String>, String);

/// Lo que se sabe de la construccion mas reciente: timestamp, numero y fecha.
type LatestBuild = (String, u32, DateTime<Utc>);

/// Los metadatos de un snapshot, derivados de sus ficheros con marca.
///
/// Una construccion cuenta cuando tiene su POM; el `snapshot` es la mas
/// reciente de las que cuentan y `snapshotVersions` lleva, por cada par
/// clasificador y extension, su construccion mas reciente. Devuelve `None`
/// si no hay ninguna construccion con marca.
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

    // Por cada (clasificador, extension), la construccion mas reciente que
    // tiene POM.
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

/// El checksum que se sirve o se compara: hexadecimal en minusculas.
///
/// Un cliente puede escribir el resumen seguido del nombre del fichero o
/// con salto de linea; solo cuenta la primera palabra.
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
        // Un checksum de un checksum no existe.
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
            // Una version a medias: el jar llego y el POM no.
            known("org/acme/widget/3.0/widget-3.0.jar", 5),
            // Otro artefacto con un prefijo parecido.
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
        // Y el resultado es un XML que el propio servicio sabe leer.
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
            // Una construccion a medias: sin POM no cuenta.
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
        // Cadena de nombres largos y miles de hojas: sin tope, cada hoja
        // llevaria su ruta completa y la memoria se multiplicaria.
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
        // Nombres validos (64 bytes) a 30 niveles: ~2 KB por ruta.
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
        // El artefacto si puede llevar puntos.
        assert!(parse_path("com/acme/my.lib/1.0/my.lib-1.0.pom").is_ok());
    }

    #[test]
    fn sorting_versions_never_panics_with_a_non_transitive_order() {
        // `1.0-alpha < 1 < 1-sp < 1.0-alpha`: con mas de veinte elementos el
        // `sort_by` estandar puede abortar ante un orden que no es total.
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
}
