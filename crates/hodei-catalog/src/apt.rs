//! La gramatica y los documentos del formato Debian (APT), puros y sin
//! efectos: aqui se abre un `.deb`, se lee su `control`, se pintan los
//! indices `Packages` y `Release`. Nada de esto toca el almacen ni la
//! red: el handler decide cuando esos bytes salen o entran.
//!
//! El contrato es el de `dpkg-scanpackages` y `apt-ftparchive`, que es
//! lo que `apt-get` espera leer. Un indice mal formado no es un matiz:
//! apt lo descarta y el cliente real deja de ver el paquete.

use crate::model::RepositoryConfig;
use crate::proxy::UpstreamConfigError;
use serde::Deserialize;
use sha2::Digest as _;
use std::collections::BTreeMap;
use std::io::Read as _;
use thiserror::Error;

/// Los fallos del formato: todo lo que huea a archive roto o a control
/// inservible, dicho con nombre y razon.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AptError {
    /// El contenedor `ar` no es un `.deb`: magicos que faltan, un
    /// tamano que no es numero o un miembro que se sale del archive.
    #[error("the archive is not a valid deb container: {0}")]
    BadArchive(&'static str),
    /// No hay miembro `control` en el archive: sin el no hay paquete.
    #[error("the control member is missing or unreadable")]
    NoControl,
    /// Falta un campo obligatorio del `control`: dpkg no lo admite.
    #[error("the control file lacks the required field {0}")]
    MissingField(&'static str),
    /// El identificador no cabe en una ruta segura del pool.
    #[error("the package identity is not filesystem safe: {0}")]
    BadIdentity(String),
    /// Upstream Release or Packages metadata is malformed or expired.
    #[error("invalid apt metadata: {0}")]
    InvalidMetadata(String),
}

/// Lo que un `.deb` declara de si mismo: los campos del `control` que
/// APT necesita para elegir y para instalar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebControl {
    /// Nombre del paquete, minusculo por convencion de dpkg.
    pub package: String,
    /// Version debian completa, con epoch y `+dfsg` si los hay.
    pub version: String,
    /// Arquitectura del binario: `amd64`, `arm64`, `all`...
    pub architecture: String,
    /// Clausula `Depends` tal cual la entiende dpkg.
    pub depends: Option<String>,
    /// Responsable declarado del paquete.
    pub maintainer: Option<String>,
    /// Descripcion completa: resumen en la primera linea y parrafos
    /// de detalle a continuacion, con sus espacios iniciales.
    pub description: String,
    /// Seccion del archive.
    pub section: Option<String>,
    /// Prioridad del paquete.
    pub priority: Option<String>,
}

/// El miembro de un contenedor `ar`: nombre y bytes, nada mas.
struct ArMember {
    name: String,
    data: Vec<u8>,
}

/// Abre un contenedor `ar` (el envoltorio de todo `.deb`) y devuelve
/// sus miembros por nombre. El formato son 8 bytes de magicos y luego
/// cabeceras de 60: nombre(16) mtime(12) uid(6) gid(6) modo(8)
/// tamano(10) "`\n"(2), con el cuerpo padding a par.
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

/// Deshace `gzip`: el `GzDecoder` de flate2 lee el formato completo
/// (magicos, cabecera con los campos opcionales de GNU, cuerpo y CRC),
/// asi que aqui solo se comprueba que el miembro sea gzip de verdad.
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

/// Recorre un tar ustar de 512 en 512 y devuelve el contenido del
/// primer miembro cuyo nombre coincida exactamente.
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
        // El tamanho esta en octal ASCII, 12 bytes con campo de sobra.
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

/// Lee el `control` de un `.deb`: la secuencia de miembros `ar`, el
/// `control.tar` (según lo comprima dpkg: plano, gzip o xz) y los
/// campos de cabecera.
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

/// Analiza el texto de un `control`: campos simples y la descripción
/// multilínea. Las líneas que empiezan en blanco o `.` Continúan el
/// campo anterior, que es como dpkg las junta.
fn parse_control(text: &str) -> Result<DebControl, AptError> {
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut last = String::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') || line == "." {
            let entry = fields.get_mut(&last).ok_or(AptError::NoControl)?;
            // La linea de continuacion se conserva tal cual: el espacio
            // inicial es parte del parrafo, no escape de formato.
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

/// Como dpkg comprime el `control.tar` de un `.deb` de prueba: sin
/// comprimir, gzip o el xz que trae por defecto desde hace décadas.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum ControlCompression {
    Plain,
    Gz,
    Xz,
}

/// Construye un `.deb` de prueba: un contenedor `ar` con un
/// `debian-binary`, un `control.tar` con el control dado y el
/// comprimido pedidos, y un `data.tar.gz` vacio. Solo para pruebas;
/// no es un paquete instalable, pero apt y nuestro lector lo parsean
/// igual.
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
            // Las cabeceras `ar` miden 60 exactos; el nombre largo se
            // recorta porque en las pruebas los nombres son cortos.
            header.truncate(60);
        }
        header.into_bytes()
    }
    fn tar_block(name: &str, data: &[u8]) -> Vec<u8> {
        let mut block = vec![0u8; 512];
        block[0..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{:07o}\0", data.len());
        block[124..132].copy_from_slice(size.as_bytes());
        block[156] = b'0'; // tipo fichero regular
        block.extend_from_slice(data);
        let pad = (512 - data.len() % 512) % 512;
        block.extend(std::iter::repeat_n(0, pad));
        block
    }

    let mut control_tar = tar_block("./control", control_text.as_bytes());
    control_tar.extend(vec![0u8; 1024]); // dos bloques de cero de cierre
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

/// Pinta el registro `Packages` de un binario: los campos del control
/// en el orden que respeta dpkg, mas los de indice que solo conoce el
/// repositorio (tamanho, suma, path). Es el formato de `apt-ftparchive
/// packages`, clave para que el gestor encuentre el paquete y lo
/// descargue.
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
    // El campo de cabecera del bloque descriptivo; multilineas aparte.
    record.push_str(&format!("Description: {}\n", control.description));
    record.push('\n');
    record
}

/// La cabecera `Release` del dist: el origen, la suite y la codename
/// (igual que el dist pedido: apt avisa si no coinciden), la componente
/// y los ficheros de indice con su suma y su tamanho, tal y como los
/// verifica `apt`. Solo SHA256: es lo que `apt-ftparchive` emite por
/// defecto hoy y lo unico que apt exige. Los indices ya estan
/// renderizados; aqui solo se pintan las filas.
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

/// Las arquitecturas que se indexan por defecto: el bucle de APT y su
/// empaquetado, que es lo que corre en esta plataforma.
pub const ARCHITECTURES: &[&str] = &["amd64", "arm64"];

/// La fila de un indice dentro de `Release`: suma, tamanho y nombre,
/// con la sangria de un espacio que el formato exige.
fn line_for(name: &str, bytes: &[u8]) -> String {
    let sum = format!("{:x}", sha2::Sha256::digest(bytes));
    format!("{sum} {} {name}", bytes.len())
}

/// La ruta del pool donde vive un binario: es la que se pinta en
/// `Filename:` y la que apt recorrera para descargarlo. Se rechaza
/// cualquier identificador que pueda escapar de la raiz del
/// repositorio o que no sea transportable por HTTP.
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

/// El sufijo con el que un binario del pool guarda su registro de
/// indice: los bytes del `.deb` y su ficha `Packages` son dos assets
/// inmutables, y la ficha es lo que se lee para ensamblar el indice.
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

/// Las rutas que la superficie apt sabe servir, ya interpretadas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// El binario en si, en `pool/.../{archivo}.deb`.
    Pool {
        /// La ruta del binario dentro del repositorio.
        path: String,
    },
    /// El indice de una arquitectura de una distribucion.
    Packages {
        /// La distribucion: `stable`, `bookworm`, `trixie`...
        dist: String,
        /// The component (`main`, `contrib`, ...), or `None` for flat/monolithic layout.
        component: Option<String>,
        /// La arquitectura del indice pedido.
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
    /// La cabecera firmada de una distribucion.
    Release {
        /// La distribucion de la cabecera.
        dist: String,
    },
    /// La firma separada de esa cabecera: `Release.gpg`.
    ReleaseSignature {
        /// La distribucion firmada.
        dist: String,
    },
    /// La clave publica del hub en armored, el `signed-by` de apt.
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

    /// Interpreta el resto de `/apt/{repositorio}/...`, crudo: apt no
    /// escapa nombres, y un `%` aqui es un nombre imposible.
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
        // Todo lo demas cuelga de dists/{dist}/. El nombre de la
        // distribucion es un identificador: sin barras ni puntos.
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

    /// La fecha `Date:` de `Release` en el formato que apt parsea con
    /// strptime: RFC 1123 con la zona dicha en letras, no en numero.
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

    /// El dpkg real comprime el `control.tar` en gzip o en xz: si el
    /// lector no abre esos formatos, un `.deb` construido por la distro
    /// no se puede publicar.
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
        // La descripcion es el campo completo con sus continuaciones.
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
        // El bloque termina en la linea en blanco: apt separa registros.
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
        // Suite y Codename tienen que decir el dist pedido: si no,
        // apt suelta un `Conflicting distribution` en cada update.
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
        // Las versiones debianas legitimas con `+` y `~` pasan.
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
        // La forma que apt siempre usa: con la componente delante.
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
}
