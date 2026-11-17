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
    #[error("the archive is not a valid deb container: {0}")]
    BadArchive(&'static str),
    #[error("the control member is missing or unreadable")]
    NoControl,
    #[error("the control file lacks the required field {0}")]
    MissingField(&'static str),
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
            .ok_or(AptError::BadArchive("a member runs past the end of the archive"))?;
        let data = bytes[cursor..end].to_vec();
        cursor = end + (size % 2);
        members.push(ArMember {
            name: name.trim_end_matches('/').to_string(),
            data,
        });
    }
    Ok(members)
}

/// Deshace `gzip` sin dependencias nuevas: el header de 10 bytes, los
/// campos opcionales que GNU y las versiones de kernel meten ahí, y
/// luego el bloque `stored` o deflate del cuerpo.
fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, AptError> {
    if bytes.len() < 10 || bytes[0] != 0x1f || bytes[1] != 0x8b || bytes[2] != 8 {
        return Err(AptError::BadArchive("the data member is not gzip"));
    }
    let flags = bytes[3];
    let mut cursor = 10usize;
    if flags & 0x04 != 0 {
        let extra_len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        cursor += 2 + extra_len;
    }
    for flag in [0x08u8, 0x10] {
        if flags & flag != 0 {
            while bytes[cursor] != 0 {
                cursor += 1;
            }
            cursor += 1;
        }
    }
    if flags & 0x02 != 0 {
        cursor += 2;
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(&bytes[cursor..])
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
/// `control.tar` (con o sin `gzip`) y los campos de cabecera.
pub fn read_deb_control(deb: &[u8]) -> Result<DebControl, AptError> {
    let members = ar_members(deb)?;
    let data_member = members
        .iter()
        .find(|member| member.name.starts_with("control.tar"))
        .ok_or(AptError::BadArchive("no control.tar member"))?;
    let tar = if data_member.name.ends_with(".gz") {
        gunzip(&data_member.data)?
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

/// Construye un `.deb` de prueba: un contenedor `ar` con un
/// `debian-binary`, un `control.tar` sin comprimir con el control dado
/// y un `data.tar.gz` vacio. Solo para pruebas; no es un paquete
/// instalable, pero apt y nuestro lector lo parsean igual.
#[cfg(test)]
pub(crate) fn build_test_deb(control_text: &str) -> Vec<u8> {
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
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&data_tar).unwrap();
        data_tar = encoder.finish().unwrap();
    }

    let mut deb = Vec::new();
    deb.extend_from_slice(b"!<arch>\n");
    for (name, body) in [
        ("debian-binary", b"2.0\n".to_vec()),
        ("control.tar", control_tar),
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

/// La cabecera `Release` del dist: el origen, la componente y los
/// ficheros de indice con su suma y su tamanho, tal y como los
/// verifica `apt`. Solo SHA256: es lo que `apt-ftparchive` emite por
/// defecto hoy y lo unico que apt exige. Los indices ya estan
/// renderizados; aqui solo se pintan las filas.
pub fn release_header(date: &str, indexes: &[(&str, &[u8])]) -> String {
    let mut out = String::new();
    out.push_str("Origin: Hodei\n");
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
        assert!(control.description.contains(" This is the long description line."));
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
        let release = release_header("Tue, 17 Nov 2026 18:00:00 UTC", &[("main/binary-amd64/Packages", index)]);
        assert!(release.starts_with("Origin: Hodei\nDate: Tue, 17 Nov 2026 18:00:00 UTC\n"));
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
}


