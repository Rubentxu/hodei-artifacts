//! La gramatica y los documentos del formato RPM (yum/dnf), puros y
//! sin efectos: aqui se abre un `.rpm`, se lee su header, se pintan la
//! ficha `primary` y el `repomd`. Nada de esto toca el almacen ni la
//! red: el handler decide cuando esos bytes salen o entran.
//!
//! El contrato es el de `createrepo_c`, que es lo que `dnf` espera
//! leer. Un indice mal formado no es un matiz: dnf descarta el
//! repositorio y el cliente real deja de ver el paquete.

use thiserror::Error;

/// Los fallos del formato: todo lo que huele a rpm roto o a metadata
/// inservible, dicho con nombre y razon.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum YumError {
    /// El fichero no es un `.rpm`: magicos que faltan, un header que
    /// no esta donde debe o un indice que se sale del fichero.
    #[error("the file is not a valid rpm container: {0}")]
    BadContainer(&'static str),
    /// Falta un campo obligatorio del header: sin el no hay paquete.
    #[error("the rpm header lacks the required field {0}")]
    MissingField(&'static str),
    /// El identificador no cabe en una ruta segura del pool.
    #[error("the package identity is not filesystem safe: {0}")]
    BadIdentity(String),
}

/// Lo que un `.rpm` declara de si mismo en su header: los campos que
/// dnf necesita para elegir y para instalar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpmMetadata {
    /// Nombre del paquete.
    pub name: String,
    /// Epoch, si el spec la declara. Sin epoch, dnf la toma como 0.
    pub epoch: Option<u32>,
    /// Version upstream.
    pub version: String,
    /// Release del empaquetado (`1.fc40`).
    pub release: String,
    /// Arquitectura del binario: `x86_64`, `aarch64`, `noarch`...
    pub architecture: String,
    /// Resumen de una linea.
    pub summary: String,
    /// Descripcion larga, con sus parrafos.
    pub description: String,
    /// Licencia declarada (SPDX en las distros modernas).
    pub license: Option<String>,
    /// Tamano instalado: `SIZE` del header, el que pinta dnf al buscar.
    pub installed_size: u64,
    /// Momento de construccion, en segundos Unix: es la fecha que
    /// `createrepo` pone en `<time>` y por eso es del fichero, no del reloj.
    pub build_time: u32,
}

// Los numeros de tag son los de `rpmtag.h` de rpm, anclados y
// estables desde los noventa: el reader se guia por el tag, no por el
// campo tipo, igual que hacen las herramientas de la propia suite.
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

/// Un indice del header: tag, tipo, offset dentro del store y count.
struct IndexEntry {
    tag: i32,
    offset: u32,
}

/// Los entries y el store de un header (`0x8e 0xad 0xe8`) en la
/// posicion dada, con el final del store para poder alinear el
/// siguiente header.
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

/// La cadena que el tag senala en el store, o nada si el tag no esta.
fn entry_string(bytes: &[u8], entries: &[IndexEntry], store: usize, tag: i32) -> Option<String> {
    let entry = entries.iter().find(|entry| entry.tag == tag)?;
    let start = store.checked_add(entry.offset as usize)?;
    let text = bytes.get(start..)?;
    let end = text.iter().position(|byte| *byte == 0)?;
    Some(String::from_utf8_lossy(&text[..end]).into_owned())
}

/// El u32 que el tag senala en el store.
fn entry_u32(bytes: &[u8], entries: &[IndexEntry], store: usize, tag: i32) -> Option<u32> {
    let entry = entries.iter().find(|entry| entry.tag == tag)?;
    let start = store.checked_add(entry.offset as usize)?;
    let four = bytes.get(start..start + 4)?;
    Some(u32::from_be_bytes(four.try_into().expect("four bytes")))
}

/// Abre un `.rpm`: el lead de 96 bytes, el header de firma (padding a
/// 8 despues) y el header principal, del que salen los campos. Solo
/// hace falta el principal para indexar: las firmas del payload son
/// materia del gpgcheck de dnf, no de la gramatica del pool.
pub fn parse_rpm(bytes: &[u8]) -> Result<RpmMetadata, YumError> {
    if bytes.len() < 96 {
        return Err(YumError::BadContainer("the file is shorter than the lead"));
    }
    if &bytes[0..4] != b"\xed\xab\xee\xdb" {
        return Err(YumError::BadContainer("the lead magic is missing"));
    }
    // Header de firma: puede estar vacio, pero su estructura debe
    // abrirse para saber donde empieza el principal.
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

/// El nombre de fichero que la comunidad espera: `{name}-{epoch}:{ver}-{rel}.{arch}.rpm`,
/// con el epoch solo cuando existe. Es el que `dnf` vera en `location`.
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

/// La ruta del pool donde vive el binario. Se rechaza cualquier
/// identificador que pueda escapar de la raiz del repositorio o que no
/// sea transportable por HTTP.
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

/// El sufijo con el que un binario del pool guarda su ficha de indice:
/// los bytes del `.rpm` y su bloque `primary` son dos assets
/// inmutables, y la ficha es lo que se lee para ensamblar el indice.
pub fn record_path_for(pool_path: &str) -> String {
    format!("{pool_path}.record")
}

/// Escapa el texto para un elemento XML: la gramatica de dnf no admite
/// `&` ni `<` sueltos en resumen, descripcion o licencia.
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

/// Pinta la ficha `<package>` de un binario: los campos del header mas
/// los de indice que solo conoce el repositorio (ruta, tamano, suma).
/// Es el fragmento de `createrepo_c` que dnf convierte en candidatos,
/// guardado como registro inmutable junto al `.rpm` que describe.
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

/// El documento `primary.xml` completo: la cabecera comun y las fichas
/// en el orden en que llegan (orden de ruta del catalogo), para que
/// dos lecturas del mismo estado den los mismos bytes.
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

/// La entrada de un fichero de indice dentro de `repomd.xml`.
pub struct RepodataEntry<'a> {
    /// El tipo logico: hoy, `primary`.
    pub kind: &'static str,
    /// La ruta relativa por la que se sirve.
    pub location: &'a str,
    /// SHA256 del fichero tal como se sirve (gzip incluido).
    pub checksum: &'a str,
    /// Tamano del fichero tal como se sirve.
    pub size: u64,
    /// SHA256 del texto sin comprimir.
    pub open_checksum: &'a str,
    /// Tamano del texto sin comprimir.
    pub open_size: u64,
    /// El momento del estado que lo genero, en segundos Unix.
    pub timestamp: u64,
}

/// El documento `repomd.xml`: la lista de indices con su suma exacta.
/// Es lo que `dnf` descarga primero y lo que va firmado en
/// `repomd.xml.asc`, asi que cada byte cuenta.
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

/// El nombre con el que se sirve el `primary.xml.gz`: contenido
/// direccionado por su propia SHA256, igual que en createrepo. Asi el
/// fichero es inmutable y cacheable por siempre.
pub fn primary_location(sha256: &str) -> String {
    format!("repodata/{sha256}-primary.xml.gz")
}

/// Las rutas que la superficie yum sabe servir, ya interpretadas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// El binario en si, en `pool/{name}-{version}-{release}.{arch}.rpm`.
    Pool {
        /// La ruta del binario dentro del repositorio.
        path: String,
    },
    /// El `repomd.xml` del repositorio.
    Repomd,
    /// La firma separada del `repomd.xml`: `repomd.xml.asc`.
    RepomdSignature,
    /// El indice `primary` por su nombre de contenido.
    Primary {
        /// El hexagono SHA256 con el que el cliente lo pidio.
        checksum: String,
    },
    /// La clave publica del hub en armored, el `gpgkey` de dnf.
    Key,
}

impl Route {
    /// Interpreta el resto de `/yum/{repositorio}/...`, crudo.
    pub fn parse(rest: &str) -> Result<Route, YumError> {
        if rest == "key.asc" {
            return Ok(Route::Key);
        }
        if let Some(path) = rest.strip_prefix("pool/") {
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
                    // `{sha256}-primary.xml.gz`: solo hex, guiones y el
                    // sufijo; cualquier otro nombre es ruta imposible.
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

/// Construye un `.rpm` de prueba: lead, header de firma vacio y header
/// principal con los campos pedidos. No es instalable (el payload es
/// vacio), pero nuestro lector y la gramatica del pool lo ven como uno
/// de verdad: la estructura de cabeceras es identica.
#[cfg(test)]
pub(crate) fn build_test_rpm(fields: &[TestHeaderField]) -> Vec<u8> {
    let mut lead = vec![0u8; 96];
    lead[0..4].copy_from_slice(b"\xed\xab\xee\xdb");
    lead[4] = 3; // version del formato
    lead[6..8].copy_from_slice(&1u16.to_be_bytes()); // rpm binario

    // Firma: header vacio legitimo (los paquetes sin firma abundan).
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
                // Alineado a 4, como exige la especificacion del store.
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

/// Los campos que todo paquete real lleva, como los pinta rpm-build.
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
}
