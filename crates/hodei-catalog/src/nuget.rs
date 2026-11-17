//! La gramatica **NuGet v3**: identidades, versiones y documentos.
//!
//! Es la parte pura del protocolo: lo que necesita efectos (el
//! almacen, el proxy, la autorizacion) vive en el handler. Aqui se
//! decide que es una ruta valida, que dos versiones son la misma
//! version y que dice cada documento, sin tocar una sola coma del
//! exterior.
//!
//! NuGet es **insensible a mayusculas** por contrato: el identificador
//! y la version viajan por URL siempre normalizados (minusculas), y la
//! direccion canonica del paquete es `flatcontainer/{id}/{version}`.
//! Pedir la ruta sin normalizar no nombra nada: es un 404, no un
//! alias.

use serde_json::{Value, json};

/// Lo que el protocolo no acepta, con el motivo para el cliente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NugetError {
    /// La ruta no pertenece a la gramatica NuGet v3.
    InvalidPath(String),
    /// El identificador de paquete no es valido.
    InvalidId(String),
    /// La version no es una version NuGet.
    InvalidVersion(String),
    /// El cuerpo del paquete no es un nupkg utilizable.
    InvalidPackage(String),
}

impl NugetError {
    /// El mensaje que ve el cliente.
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

    /// Si el fallo es de gramatica: el cliente no nombro nada, que es
    /// distinto de nombrar algo mal.
    pub fn is_path_error(&self) -> bool {
        matches!(self, Self::InvalidPath(_))
    }
}

/// El identificador de un paquete, ya normalizado: minusculas.
pub fn normalize_id(id: &str) -> String {
    id.to_ascii_lowercase()
}

/// Si el identificador es de los que NuGet admite: letras, digitos,
/// punto y guion, empezando y terminando en alfanumerico.
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

/// La ruta de protocolo de una version: la identidad del paquete en el
/// flat container. `registrations/{id}` y el indice de versiones
/// viven alrededor de ella.
pub fn package_asset_path(id: &str, version: &str) -> String {
    format!("flatcontainer/{id}/{version}/{id}.nupkg")
}

// ---------------------------------------------------------------------------
// Versiones NuGet
// ---------------------------------------------------------------------------

/// Una version NuGet: uno a cuatro numeros y, si acaso, una etiqueta
/// de previos. La etiqueta de compilacion (`+...`) no es identidad:
/// dos paquetes con la misma version y distinta etiqueta son la misma
/// version, y el protocolo la descarta al normalizar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NuGetVersion {
    numbers: [u64; 4],
    pre: Vec<String>,
}

impl NuGetVersion {
    /// El numero en la posicion pedida.
    pub fn number(&self, index: usize) -> u64 {
        self.numbers[index]
    }

    /// Si es un previos: la version sin etiqueta va primero.
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    /// La cadena con la que el protocolo nombra a la version:
    /// tres partes como minimo, la cuarta solo si trae algo, y sin
    /// etiqueta de compilacion.
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

    /// El orden de NuGet: primero los numeros, luego la etiqueta
    /// (sin etiqueta gana a con etiqueta, y entre etiquetas se
    /// comparan segmentos numericos como numeros y los demas como
    /// texto, en minusculas).
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

/// Dos segmentos de etiqueta de previos: los numericos se comparan
/// como numeros (y van antes que los textuales), el resto como texto
/// en minusculas.
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

/// El parser de versiones: `major[.minor[.patch[.revision]]][-prerelease][+build]`.
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

/// La version normalizada tal y como la escribe el protocolo: la ruta
/// del flat container la pide asi.
pub fn normalize_version(text: &str) -> Option<String> {
    parse_version(text).map(|version| version.normalized())
}

// ---------------------------------------------------------------------------
// El paquete: nupkg y nuspec
// ---------------------------------------------------------------------------

/// Lo que el servidor necesita saber de un `.nupkg` subido: quien es
/// y que version declara por dentro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageIdentity {
    /// El identificador normalizado del paquete.
    pub id: String,
    /// La version normalizada que declara el nuspec.
    pub version: String,
}

/// El identificador XML mas simple que vale aqui: dentro del `metadata`
/// del nuspec, `<id>` y `<version>` son elementos de texto plano sin
/// atributos ni anidados. Buscar el cierre mas cercano al inicio es
/// suficiente y no arrastra un parser XML al nucleo puro.
fn tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(xml[start..end].trim())
}

/// El nuspec dentro del zip: el nombre exacto es `{id}.nuspec`, pero
/// aqui basta con el unico `.nuspec` que todo paquete lleva.
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

/// La identidad que declara el interior del paquete. Es la autoridad
/// en la publicacion: la URL no manda sobre lo que dice el nuspec.
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

/// El nupkg dentro del `multipart/form-data` que el cliente real usa
/// para publicar: `dotnet nuget push` manda el paquete como un trozo
/// del formulario, no como cuerpo crudo. Se devuelve **solo** el zip,
/// nunca el sobre: la identidad y el digest se miden sobre el nupkg.
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
// Documentos de protocolo
// ---------------------------------------------------------------------------

/// El `index.json` del flat container: el contrato v3 lo define como un
/// array de **cadenas** (`{"versions": ["1.0.0"]}`), no de objetos: el
/// cliente NuGet deserializa `versions` como `List<string>` y un objeto
/// le sabe a JSON roto. La ultima version, al final. Las identidades ya
/// estan normalizadas.
pub fn version_index(versions: &[String]) -> Value {
    json!({
        "count": versions.len(),
        "versions": versions,
    })
}

/// La hoja de registro de un paquete: los `items` por version con su
/// direccion flatcontainer. El catalogo entero (registration index y
/// leaf con dependencias completas) es alcance futuro; esta hoja es
/// lo que el cliente necesita para resolver id y version.
pub fn registration_leaf(id: &str, versions: &[String]) -> Value {
    json!({
        "@id": format!("packages/{id}/index.json"),
        "@type": ["PackageRegistrationLeaf", "catalog:entry"],
        "version": versions.last().cloned().unwrap_or_default(),
        "votes": 0,
        "packageStatus": "validated",
        "items": versions
            .iter()
            .map(|version| {
                json!({
                    "@id": format!("packages/{id}/{version}"),
                    "packageContent": format!(
                        "flatcontainer/{id}/{version}/{id}.nupkg"
                    ),
                    "catalogEntry": {
                        "id": id,
                        "version": version,
                        "packageStatus": "validated",
                    },
                })
            })
            .collect::<Vec<_>>(),
    })
}

/// Que pide la ruta del flat container, si es que pide algo:
/// `flatcontainer/{id}/{version}/{id}.nupkg` o `{id}/index.json`.
/// Todo va minusculas; mayusculas sin normalizar no nombran nada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlatRequest {
    /// La lista de versiones del paquete.
    Versions {
        /// El paquete, en minusculas.
        id: String,
    },
    /// Un fichero concreto de una version: el nupkg y companeros
    /// como las `.nuspec` sueltas que algunos clientes piden.
    File {
        /// El paquete, en minusculas.
        id: String,
        /// La version, tal y como la nombra la ruta.
        version: String,
        /// El nombre del fichero dentro de la version.
        file: String,
    },
}

/// El parser de la ruta flat: separa identidad de fichero sin tocar
/// el almacen.
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
// Rutas y service index
// ---------------------------------------------------------------------------

/// Lo que pide una ruta de `/nuget/{repositorio}/...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NuGetRoute {
    /// El indice de servicios: que hay en este feed y donde.
    ServiceIndex,
    /// Algo del flat container: la lista de versiones o un fichero.
    Flat(FlatRequest),
    /// La hoja de registro del paquete.
    Registration {
        /// El paquete, en minusculas.
        id: String,
    },
    /// La publicacion de un paquete por el endpoint Publish. La
    /// identidad en la URL es opcional porque el cliente real hace
    /// `PUT {publish}/` con el nupkg dentro de un formulario multipart
    /// y **sin** id ni version en la ruta: el nuspec manda.
    Publish {
        /// El paquete pedido, normalizado, si la URL lo promete.
        id: Option<String>,
        /// La version pedida, normalizada, si la URL la lleva.
        version: Option<String>,
    },
}

impl NuGetRoute {
    /// El paquete al que apunta la ruta, si apunta a alguno: es lo que
    /// registra la auditoria.
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

/// El parser de rutas: el indice, el flat container, los registros y el
/// endpoint de publicacion. La ruta llega sin decodificar y en crudo.
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
        // El cliente real publica contra la direccion vacia: el nuspec
        // es la unica identidad en juego.
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

/// El indice de servicios del feed. Las direcciones que declara son
/// absolutas, tal y como el cliente las ve: el flat container es la
/// operacion que resuelve versiones, los registros la que las lista
/// para buscar, y el Publish la que escribe.
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
        // sin etiqueta gana a con etiqueta
        assert_eq!(
            version("1.0.0").compare(&version("1.0.0-beta")),
            Ordering::Greater
        );
        assert_eq!(
            version("1.0.0-beta").compare(&version("1.0.0-beta.1")),
            Ordering::Less
        );
        // los segmentos numericos de etiqueta van antes que los textuales
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
        // mayusculas sin normalizar no nombran nada: gramatica, no identidad
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
        // un id con barra no es un id: la gramatica lo rechaza antes
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

    /// Un zip de juguete con el nuspec dentro, comprimido o no.
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
        // El push nombra como quiere: la ruta se normaliza, no se rechaza.
        assert_eq!(
            parse_route("package/Sample.Lib/01.2.0+meta").unwrap(),
            NuGetRoute::Publish {
                id: Some("sample.lib".to_string()),
                version: Some("1.2.0".to_string()),
            }
        );
        // El cliente real no nombra nada en la URL: la direccion de
        // publicacion a secas tambien es una ruta de push valida.
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
        // Un cuerpo que no es formulario no se toca: el zip crudo se
        // reconoce por su cabecera, no por adivinar.
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
