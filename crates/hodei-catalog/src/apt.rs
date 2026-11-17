//! La gramatica y los documentos del formato Debian (APT), puros y sin
//! efectos: aqui se abre un `.deb`, se lee su `control`, se pintan los
//! indices `Packages` y `Release`. Nada de esto toca el almacen ni la
//! red: el handler decide cuando esos bytes salen o entran.
//!
//! El contrato es el de `dpkg-scanpackages` y `apt-ftparchive`, que es
//! lo que `apt-get` espera leer. Un indice mal formado no es un matiz:
//! apt lo descarta y el cliente real deja de ver el paquete.

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
        block.extend(std::iter::repeat(0).take(pad));
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
    let mut out = String::new();
    out.push_str("Origin: Hodei\n");
    out.push_str(&format!("Suite: {dist}\n"));
    out.push_str(&format!("Codename: {dist}\n"));
    out.push_str(&format!("Date: {date}\n"));
    out.push_str(&format!("Architectures: {}\n", ARCHITECTURES.join(" ")));
    out.push_str("Components: main\nSHA256:\n");
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

/// Las rutas que la superficie apt sabe servir, ya interpretadas. La
/// componente es siempre `main`: es la que declara `Release` y la que
/// pintan los registros; una distribucion con mas componentes es
/// materia del proxy, no del hosted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// El binario en si, en `pool/main/{package}/{archivo}.deb`. La
    /// ruta completa es lo que se valida contra el control: el pool no
    /// admite archivos que no declaren ser lo que dicen sus bytes.
    Pool {
        /// La ruta del binario dentro del repositorio.
        path: String,
    },
    /// El indice de una arquitectura de una distribucion.
    Packages {
        /// La distribucion: `stable`, `trixie`...
        dist: String,
        /// La arquitectura del indice pedido.
        architecture: String,
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

impl Route {
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
            .ok_or(AptError::BadIdentity(rest.to_string()))?;
        if dist.is_empty()
            || dist.contains("..")
            || dist.contains('/')
            || !dist
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(AptError::BadIdentity(dist.to_string()));
        }
        match tail {
            "Release" => Ok(Route::Release {
                dist: dist.to_string(),
            }),
            "Release.gpg" => Ok(Route::ReleaseSignature {
                dist: dist.to_string(),
            }),
            _ => {
                // apt siempre pide el indice con su componente:
                // `main/binary-{arch}/Packages`. La variante sin
                // componente tambien se acepta: es la forma monolítica
                // que alguna herramienta usa y el hosted solo tiene
                // `main`. Otra componente es una ruta imposible.
                let index = tail
                    .strip_prefix("main/")
                    .unwrap_or(tail)
                    .strip_prefix("binary-")
                    .and_then(|arch| arch.strip_suffix("/Packages"))
                    .ok_or_else(|| AptError::BadIdentity(tail.to_string()))?;
                if index.is_empty()
                    || !index
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                {
                    return Err(AptError::BadIdentity(index.to_string()));
                }
                Ok(Route::Packages {
                    dist: dist.to_string(),
                    architecture: index.to_string(),
                })
            }
        }
    }

    /// La fecha `Date:` de `Release` en el formato que apt parsea con
    /// strptime: RFC 1123 con la zona dicha en letras, no en numero.
    pub fn release_date(moment: &chrono::DateTime<chrono::Utc>) -> String {
        moment.format("%a, %d %b %Y %H:%M:%S UTC").to_string()
    }
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
                architecture: "amd64".into()
            }
        );
        // La forma que apt siempre usa: con la componente delante.
        assert_eq!(
            Route::parse("dists/stable/main/binary-arm64/Packages").unwrap(),
            Route::Packages {
                dist: "stable".into(),
                architecture: "arm64".into()
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
        // No hay componente que valga, ni path traversal, ni rutas sueltas.
        assert!(Route::parse("dists/stable/other/Packages").is_err());
        assert!(Route::parse("pool/../etc/passwd").is_err());
        assert!(Route::parse("pool/main/x/no-extension").is_err());
        assert!(Route::parse("dists/st..able/Release").is_err());
        assert!(Route::parse("dists/").is_err());
        assert!(Route::parse("index.html").is_err());
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
}
