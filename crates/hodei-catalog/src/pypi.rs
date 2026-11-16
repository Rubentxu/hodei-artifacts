//! La parte pura del formato PyPI: los nombres normalizados (PEP 503), las
//! distribuciones, el documento de un proyecto y las dos formas de
//! publicarlo (el HTML de PEP 503 y el JSON de PEP 691).
//!
//! Nada aqui toca disco, red ni catalogo: son funciones sobre cadenas y
//! JSON, y por eso se prueban sin infraestructura.
//!
//! # El documento de un proyecto
//!
//! El indice de un proyecto es lo que el cliente ve: los ficheros que hay,
//! con su hash, su tamano, la version de Python que necesitan y si estan
//! marcados como *yanked*. Es un **documento**, no una vista del catalogo:
//! se escribe cuando se publica y se vuelve a escribir cuando una version
//! se retira. Los bytes de las distribuciones viven aparte, en su propia
//! ruta, y no se tocan al reescribir el documento.
//!
//! # Los dos formatos
//!
//! - **HTML** (PEP 503): una pagina con un enlace por fichero. El hash va
//!   en el fragmento (`#sha256=...`) y la version de Python en
//!   `data-requires-python`.
//! - **JSON** (PEP 691): un documento con `meta`, `name` y `files`. El hash
//!   va en `hashes` y la version de Python en `requires-python`.
//!
//! Los dos dicen lo mismo: un fichero que aparece en el HTML aparece en el
//! JSON con el mismo hash y el mismo estado *yanked*. Lo que se negocia es
//! la representacion, no el contenido.
//!
//! # El HTML es nuestro, no del upstream
//!
//! Un indice que viene de otro se vuelve a pintar aqui: el HTML que sale
//! de este servicio lo escribe esta pagina, con los valores escapados. Un
//! upstream puede publicar lo que quiera en su HTML; lo que el cliente
//! recibe de aqui no lo puede inyectar.

use serde_json::{Map, Value};

/// El sufijo de las coordenadas del documento de un proyecto.
///
/// El `#` no puede aparecer en un nombre de proyecto, asi que las
/// coordenadas del documento nunca chocan con las de una distribucion.
pub const PROJECT_COORDINATES_SUFFIX: &str = "#project";

/// El tipo de contenido de la Simple API en JSON (PEP 691).
pub const JSON_CONTENT_TYPE: &str = "application/vnd.pypi.simple.v1+json";

/// El tipo de contenido de la Simple API en HTML (PEP 503).
pub const HTML_CONTENT_TYPE: &str = "application/vnd.pypi.simple.v1+html";

/// El tipo de contenido de la Simple API en JSON, en su version *latest*.
///
/// PEP 691 lo define como la misma representacion que la version `1.0`,
/// y un cliente puede pedirla en vez de la otra.
pub const LATEST_JSON_CONTENT_TYPE: &str = "application/vnd.pypi.simple.latest+json";

/// La version de la Simple API que se anuncia.
pub const API_VERSION: &str = "1.0";

/// La longitud maxima de un nombre de proyecto.
pub const MAX_PROJECT_LEN: usize = 255;

/// Por que un nombre, un fichero, una version o un documento no se admiten.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PypiError {
    /// El nombre del proyecto no cumple las reglas de PEP 503.
    #[error("the project name is not valid: {0}")]
    InvalidName(String),
    /// El nombre del fichero no es el de una distribucion.
    #[error("the file name is not a valid distribution: {0}")]
    InvalidFileName(String),
    /// La version no es una version de PEP 440.
    #[error("the version is not valid: {0}")]
    InvalidVersion(String),
    /// A la subida le falta un campo obligatorio.
    #[error("the upload is missing the field {0}")]
    MissingField(&'static str),
    /// A la subida le falta el fichero.
    #[error("the upload does not carry the distribution")]
    MissingFile,
    /// Lo que declara la subida no coincide con el fichero.
    #[error("the upload does not match the distribution: {0}")]
    Mismatch(String),
    /// El hash declarado no es el de los bytes recibidos.
    #[error("the digest of the upload does not match its content")]
    DigestMismatch,
    /// El documento no se entiende.
    #[error("the project document is not valid: {0}")]
    InvalidDocument(String),
    /// El fichero ya esta en el proyecto.
    #[error("the distribution is already published in this project")]
    FileExists,
    /// El proyecto no tiene ese fichero.
    #[error("the project does not have that distribution")]
    UnknownFile(String),
    /// La ruta no es ninguna de las que habla el protocolo.
    #[error("the path is not part of the PyPI surface: {0}")]
    InvalidPath(String),
}

impl PypiError {
    /// El motivo, tal y como se le dice al cliente.
    pub fn message(&self) -> String {
        self.to_string()
    }
}

/// Normaliza un nombre de proyecto (PEP 503).
///
/// Las mayusculas dejan de distinguir y cualquier racha de `-`, `_` o `.`
/// es un solo `-`: `Flask`, `flask`, `flask_login` y `flask.login` son el
/// mismo proyecto.
pub fn normalize(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    let mut separator = false;
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            // Una racha es un separador, y uno solo.
            if !separator {
                normalized.push('-');
            }
            separator = true;
            continue;
        }
        separator = false;
        normalized.extend(character.to_lowercase());
    }
    normalized
}

/// Un nombre de proyecto ya normalizado.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectName(String);

impl std::fmt::Display for ProjectName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl ProjectName {
    /// Lee un nombre y lo normaliza.
    ///
    /// El nombre normalizado es el que se usa en la ruta del indice; el
    /// cliente puede pedir el que quiera, porque todos son el mismo.
    pub fn parse(raw: &str) -> Result<Self, PypiError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(PypiError::InvalidName("the name is empty".into()));
        }
        if trimmed.len() > MAX_PROJECT_LEN {
            return Err(PypiError::InvalidName("the name is too long".into()));
        }
        if !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(PypiError::InvalidName(
                "the name has characters that PyPI does not allow".into(),
            ));
        }
        let normalized = normalize(trimmed);
        // El nombre de una distribucion empieza y acaba en alfanumerico:
        // es lo que dice el nombre canonico de los metadatos.
        if normalized.is_empty()
            || !normalized
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric())
            || !normalized
                .chars()
                .last()
                .is_some_and(|character| character.is_ascii_alphanumeric())
        {
            return Err(PypiError::InvalidName(
                "the name is empty once normalized".into(),
            ));
        }
        Ok(Self(normalized))
    }

    /// El nombre normalizado.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// De que tipo de distribucion se trata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionKind {
    /// Un *wheel*: un directorio zip con la version de Python en el nombre.
    Wheel,
    /// Un *sdist*: el tarball o zip con el codigo fuente.
    Sdist,
}

/// Una distribucion de un proyecto, con su nombre de fichero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distribution {
    /// El nombre del fichero, tal y como se publico.
    pub filename: String,
    /// El proyecto al que pertenece.
    pub project: ProjectName,
    /// La version que declara.
    pub version: String,
    /// De que tipo es.
    pub kind: DistributionKind,
}

impl Distribution {
    /// Lee el nombre de un fichero de distribucion.
    ///
    /// El nombre de un wheel es
    /// `{distribucion}-{version}(-{build})?-{python}-{abi}-{plataforma}.whl`
    /// y el de un sdist `{nombre}-{version}.tar.gz` (o `.zip`).
    ///
    /// Las partes tienen que ser las que son: un nombre que el `pip` no
    /// acepta no se anuncia, porque un enlace que el cliente no puede
    /// instalar es un enlace roto.
    pub fn parse(filename: &str) -> Result<Self, PypiError> {
        if filename.is_empty() || filename.len() > 255 {
            return Err(PypiError::InvalidFileName(filename.to_string()));
        }
        if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
            return Err(PypiError::InvalidFileName(filename.to_string()));
        }
        if let Some(stem) = filename.strip_suffix(".whl") {
            // Ni una parte mas ni una menos: el nombre de un wheel tiene
            // cinco partes, o seis con etiqueta de compilacion.
            let parts: Vec<&str> = stem.split('-').collect();
            let (version_text, tags) = match parts.len() {
                5 => (&parts[1], &parts[2..]),
                6 => {
                    if !valid_build(parts[2]) {
                        return Err(PypiError::InvalidFileName(filename.to_string()));
                    }
                    (&parts[1], &parts[3..])
                }
                _ => return Err(PypiError::InvalidFileName(filename.to_string())),
            };
            // Las tres etiquetas: version de Python, ABI y plataforma.
            if tags.len() != 3 || !tags.iter().all(|tag| valid_tag(tag)) {
                return Err(PypiError::InvalidFileName(filename.to_string()));
            }
            let version = Version::parse(version_text)
                .ok_or_else(|| PypiError::InvalidVersion(version_text.to_string()))?;
            let project = ProjectName::parse(parts[0])?;
            return Ok(Self {
                filename: filename.to_string(),
                project,
                version: version.canonical(),
                kind: DistributionKind::Wheel,
            });
        }
        for suffix in [".tar.gz", ".zip"] {
            let Some(stem) = filename.strip_suffix(suffix) else {
                continue;
            };
            let Some((name, version_text)) = stem.rsplit_once('-') else {
                return Err(PypiError::InvalidFileName(filename.to_string()));
            };
            let version = Version::parse(version_text)
                .ok_or_else(|| PypiError::InvalidVersion(version_text.to_string()))?;
            let project = ProjectName::parse(name)?;
            return Ok(Self {
                filename: filename.to_string(),
                project,
                version: version.canonical(),
                kind: DistributionKind::Sdist,
            });
        }
        Err(PypiError::InvalidFileName(filename.to_string()))
    }
}

/// La etiqueta de compilacion de un wheel: un numero y, detras, lo que
/// sea.
///
/// PEP 427 dice que tiene que empezar por un digito, y el `pip` tambien:
/// su regla es un digito y, detras, cualquier cosa que no sea un guion.
/// Por eso `1.5` y `1abc` son etiquetas de compilacion validas.
fn valid_build(build: &str) -> bool {
    let Some(first) = build.as_bytes().first() else {
        return false;
    };
    first.is_ascii_digit() && !build.contains('-')
}

/// Una etiqueta de un wheel: version de Python, ABI o plataforma.
///
/// Cada etiqueta es un conjunto de etiquetas sueltas separadas por un
/// punto: `py2.py3`, `cp39.cp310`, `manylinux_2_17_x86_64`.
///
/// Lo que se acepta es lo que acepta el `pip`: la etiqueta es una lista de
/// etiquetas sueltas separadas por puntos, y el `pip` no mira los
/// caracteres de cada una. Esta superficie tampoco los mira: lo que se
/// anuncia tiene que poder instalarse, no parecerse a lo que se parece.
fn valid_tag(tag: &str) -> bool {
    !tag.is_empty() && !tag.contains('-')
}

/// La clase de una *pre release*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PreKind {
    /// Una alfa.
    Alpha,
    /// Una beta.
    Beta,
    /// Una *release candidate*.
    Rc,
}

impl PreKind {
    /// La letra con la que se escribe.
    fn letter(self) -> &'static str {
        match self {
            Self::Alpha => "a",
            Self::Beta => "b",
            Self::Rc => "rc",
        }
    }
}

/// Una version de PEP 440.
///
/// La PEP define la gramatica y la forma **canonica**. Lo que se guarda y
/// lo que se compara es la forma canonica, porque un cliente puede escribir
/// la version como le plazca (`1.0-2`, `1.0.rc1`, `v1.0`) y el nombre del
/// fichero la lleva canonica (`1.0.post2`, `1.0rc1`, `1.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// La epoca.
    epoch: u64,
    /// La release: sus numeros.
    release: Vec<u64>,
    /// La pre release: su clase y su numero.
    pre: Option<(PreKind, u64)>,
    /// El post release.
    post: Option<u64>,
    /// La *development release*.
    dev: Option<u64>,
    /// La version local, ya normalizada.
    local: Option<String>,
}

impl Version {
    /// Lee una version de PEP 440.
    ///
    /// `None` si lo que hay no es una version de PEP 440.
    pub fn parse(raw: &str) -> Option<Self> {
        let text = raw.trim();
        if text.is_empty() {
            return None;
        }
        // El prefijo `v` es de la PEP: `v1.0` es `1.0`.
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        // La gramatica de la PEP no distingue mayusculas: `1.0RC1` es
        // `1.0rc1`. Las minusculas son ademas la forma canonica, asi que
        // se normaliza una vez y se trabaja siempre en minusculas.
        let lowered = text.to_ascii_lowercase();
        // La version local va al final y separated por un `+`.
        let (text, local) = match lowered.split_once('+') {
            Some((head, local)) => (head, Some(normalize_local(local)?)),
            None => (lowered.as_str(), None),
        };
        // La epoca: `1!2.0`.
        let (epoch, text) = match text.split_once('!') {
            Some((epoch, tail)) => {
                if epoch.is_empty() || !epoch.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                (epoch.parse().ok()?, tail)
            }
            None => (0, text),
        };
        // La release: `1`, `1.0`, `1.0.0`.
        let end = text
            .find(|character: char| !character.is_ascii_digit() && character != '.')
            .unwrap_or(text.len());
        let (mut release_text, mut rest) = text.split_at(end);
        // El punto que separa la release de su sufijo (`1.0.dev1`) no es
        // parte de la release: es el separador del sufijo.
        if let Some(stripped) = release_text.strip_suffix('.') {
            release_text = stripped;
            rest = &text[end - 1..];
        }
        let mut release = Vec::new();
        for part in release_text.split('.') {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            release.push(part.parse().ok()?);
        }
        // Los sufijos van en este orden y cada uno una vez: una *pre
        // release*, un *post release* y una *development release*.
        let mut pre = None;
        let mut post = None;
        let mut dev = None;
        if let Some((kind, number, tail)) = take_marker(rest, PRE_MARKERS) {
            pre = Some((kind, number));
            rest = tail;
        }
        if let Some((number, tail)) = take_implicit_post(rest) {
            post = Some(number);
            rest = tail;
        } else if let Some((number, tail)) = take_named_marker(rest, POST_MARKERS) {
            post = Some(number);
            rest = tail;
        }
        if let Some((number, tail)) = take_named_marker(rest, DEV_MARKERS) {
            dev = Some(number);
            rest = tail;
        }
        if !rest.is_empty() {
            return None;
        }
        Some(Self {
            epoch,
            release,
            pre,
            post,
            dev,
            local,
        })
    }

    /// La forma canonica de la version.
    ///
    /// Es la que lleva el nombre del fichero: `1.0-2` es `1.0.post2`, y
    /// `1.0+Ubuntu-1` es `1.0+ubuntu.1`.
    pub fn canonical(&self) -> String {
        let mut version = String::new();
        if self.epoch != 0 {
            version.push_str(&self.epoch.to_string());
            version.push('!');
        }
        let release: Vec<String> = self.release.iter().map(u64::to_string).collect();
        version.push_str(&release.join("."));
        if let Some((kind, number)) = self.pre {
            version.push_str(kind.letter());
            version.push_str(&number.to_string());
        }
        if let Some(post) = self.post {
            version.push_str(".post");
            version.push_str(&post.to_string());
        }
        if let Some(dev) = self.dev {
            version.push_str(".dev");
            version.push_str(&dev.to_string());
        }
        if let Some(local) = &self.local {
            version.push('+');
            version.push_str(local);
        }
        version
    }
}

impl Ord for Version {
    /// El orden de PEP 440.
    ///
    /// Una *pre release* va antes que su release, un *post release* va
    /// despues, una *development release* va antes que las dos, y la
    /// version local va despues de la misma version sin ella.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        self.epoch
            .cmp(&other.epoch)
            .then_with(|| {
                let length = self.release.len().max(other.release.len());
                for index in 0..length {
                    let left = self.release.get(index).copied().unwrap_or(0);
                    let right = other.release.get(index).copied().unwrap_or(0);
                    match left.cmp(&right) {
                        Ordering::Equal => {}
                        other => return other,
                    }
                }
                Ordering::Equal
            })
            .then_with(|| match (self.pre, other.pre) {
                (None, None) => Ordering::Equal,
                // Sin pre release es la mas nueva.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(&right),
            })
            .then_with(|| self.post.cmp(&other.post))
            .then_with(|| match (self.dev, other.dev) {
                (None, None) => Ordering::Equal,
                // Sin development release es la mas nueva.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(&right),
            })
            .then_with(|| match (&self.local, &other.local) {
                (None, None) => Ordering::Equal,
                // La version local es mas nueva que la misma sin ella.
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(left), Some(right)) => left.cmp(right),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Las marcas de una *pre release*, de la mas larga a la mas corta.
///
/// El orden importa: `alpha` antes que `a`, y `preview` antes que `pre`.
const PRE_MARKERS: &[(&str, PreKind)] = &[
    ("alpha", PreKind::Alpha),
    ("a", PreKind::Alpha),
    ("beta", PreKind::Beta),
    ("b", PreKind::Beta),
    ("preview", PreKind::Rc),
    ("pre", PreKind::Rc),
    ("c", PreKind::Rc),
    ("rc", PreKind::Rc),
];

/// Las marcas de un *post release*, de la mas larga a la mas corta.
const POST_MARKERS: &[&str] = &["post", "rev", "r"];

/// Las marcas de una *development release*.
const DEV_MARKERS: &[&str] = &["dev"];

/// La forma canonica de una version.
///
/// `None` si lo que hay no es una version de PEP 440.
pub fn canonical_version(raw: &str) -> Option<String> {
    Version::parse(raw).map(|version| version.canonical())
}

/// Si una version es una version de PEP 440.
///
/// No es el comparador (de eso se encarga el cliente), es el filtro: un
/// indice con una version que no existe no se sirve.
pub fn valid_version(version: &str) -> bool {
    canonical_version(version).is_some()
}

/// La parte local de una version, normalizada.
///
/// Los segmentos se separan por `.`, `-` o `_` y se escriben en
/// minusculas: `1.0+Ubuntu-1` es `1.0+ubuntu.1`.
///
/// Un segmento numerico se escribe sin ceros a la izquierda: `01` es `1` y
/// `00` es `0`. La PEP lo dice asi, y por eso `1.0+01` y `1.0+1` son la
/// misma version y el mismo fichero.
fn normalize_local(local: &str) -> Option<String> {
    if local.is_empty() {
        return None;
    }
    let mut segments = Vec::new();
    for part in local.split(['.', '-', '_']) {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return None;
        }
        let segment = part.to_ascii_lowercase();
        let segment = if segment.bytes().all(|byte| byte.is_ascii_digit()) {
            let trimmed = segment.trim_start_matches('0');
            if trimmed.is_empty() {
                "0".to_string()
            } else {
                trimmed.to_string()
            }
        } else {
            segment
        };
        segments.push(segment);
    }
    Some(segments.join("."))
}

/// Un numero al principio del texto.
fn take_number(text: &str) -> Option<(u64, &str)> {
    let end = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    text[..end]
        .parse()
        .ok()
        .map(|number| (number, &text[end..]))
}

/// El separador de una marca: `.`, `-` o `_`.
fn take_separator(text: &str) -> &str {
    match text.as_bytes().first() {
        Some(b'.' | b'-' | b'_') => &text[1..],
        _ => text,
    }
}

/// El numero que va detras de una marca, si lo hay.
///
/// Sin separador, el numero tiene que empezar por un digito: `rc1` lleva
/// un numero y `rcx` no lo lleva.
fn take_optional_number(text: &str) -> (u64, &str) {
    let separated = take_separator(text);
    if separated.len() != text.len() {
        return match take_number(separated) {
            Some((number, tail)) => (number, tail),
            None => (0, separated),
        };
    }
    match take_number(text) {
        Some((number, tail)) => (number, tail),
        None => (0, text),
    }
}

/// Una marca con su numero opcional.
///
/// El separador va delante de la marca y es opcional; el numero va detras
/// de un separador opcional. Si la marca no esta, no se consume nada: el
/// separador es de la marca que venga.
fn take_marker<'a>(
    text: &'a str,
    names: &[(&'static str, PreKind)],
) -> Option<(PreKind, u64, &'a str)> {
    let separated = take_separator(text);
    if separated.len() != text.len() {
        for (name, kind) in names {
            if let Some(tail) = separated.strip_prefix(name) {
                let (number, tail) = take_optional_number(tail);
                return Some((*kind, number, tail));
            }
        }
        return None;
    }
    for (name, kind) in names {
        if let Some(tail) = text.strip_prefix(name) {
            let (number, tail) = take_optional_number(tail);
            return Some((*kind, number, tail));
        }
    }
    None
}

/// Una marca que no tiene clase: la del *post release* y la de la
/// *development release*.
///
/// Es la misma regla que [`take_marker`], sin la clase: el *post release*
/// y la *development release* no se distinguen por su letra.
fn take_named_marker<'a>(text: &'a str, names: &[&'static str]) -> Option<(u64, &'a str)> {
    let separated = take_separator(text);
    if separated.len() != text.len() {
        for name in names {
            if let Some(tail) = separated.strip_prefix(name) {
                let (number, tail) = take_optional_number(tail);
                return Some((number, tail));
            }
        }
        return None;
    }
    for name in names {
        if let Some(tail) = text.strip_prefix(name) {
            let (number, tail) = take_optional_number(tail);
            return Some((number, tail));
        }
    }
    None
}

/// El *post release* implicito: `1.0-1`.
///
/// El guion va pegado y el numero tambien: `1.0-1` es un post release y
/// `1.0-r1` es otra cosa.
fn take_implicit_post(text: &str) -> Option<(u64, &str)> {
    let tail = text.strip_prefix('-')?;
    take_number(tail)
}

/// Un fichero de un proyecto, tal y como lo ve el indice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// El nombre del fichero.
    pub filename: String,
    /// El hash SHA-256, en hexadecimal.
    pub sha256: String,
    /// El tamano en bytes, si se sabe.
    pub size: Option<u64>,
    /// La version de Python que necesita, si la declara.
    pub requires_python: Option<String>,
    /// El motivo del *yank*, si la version esta retirada.
    ///
    /// `None` es que la version sigue viva; `Some("")` es que esta retirada
    /// sin motivo escrito.
    pub yanked: Option<String>,
    /// De donde sale el contenido cuando no es de este servicio.
    ///
    /// En un proxy es la URL del upstream; en un hosted no hay ninguna y el
    /// contenido esta en el catalogo.
    pub source: Option<String>,
}

impl FileEntry {
    /// El JSON de la entrada, en la forma del documento guardado.
    ///
    /// El documento guardado es un documento de la Simple API (PEP 691):
    /// el hash va en `hashes` y la URL del fichero en `url`.
    fn to_value(&self, url: &str) -> Value {
        let mut entry = Map::new();
        entry.insert("filename".into(), Value::String(self.filename.clone()));
        entry.insert("url".into(), Value::String(url.to_string()));
        let mut hashes = Map::new();
        hashes.insert("sha256".into(), Value::String(self.sha256.clone()));
        entry.insert("hashes".into(), Value::Object(hashes));
        if let Some(size) = self.size {
            entry.insert("size".into(), Value::from(size));
        }
        if let Some(requires_python) = &self.requires_python {
            entry.insert(
                "requires-python".into(),
                Value::String(requires_python.clone()),
            );
        }
        match &self.yanked {
            Some(reason) => {
                entry.insert("yanked".into(), Value::String(reason.clone()));
            }
            None => {
                entry.insert("yanked".into(), Value::Bool(false));
            }
        }
        Value::Object(entry)
    }

    /// Lee la entrada de un documento de la Simple API.
    fn from_value(value: &Value) -> Result<Self, PypiError> {
        let object = value
            .as_object()
            .ok_or_else(|| PypiError::InvalidDocument("a file entry is not an object".into()))?;
        let filename = object
            .get("filename")
            .and_then(Value::as_str)
            .ok_or_else(|| PypiError::InvalidDocument("a file entry has no name".into()))?
            .to_string();
        let sha256 = object
            .get("hashes")
            .and_then(|hashes| hashes.get("sha256"))
            .and_then(Value::as_str)
            .ok_or_else(|| PypiError::InvalidDocument("a file entry has no hash".into()))?
            .to_ascii_lowercase();
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(PypiError::InvalidDocument(
                "the hash of a file entry is not a SHA-256".into(),
            ));
        }
        let size = object.get("size").and_then(Value::as_u64);
        let requires_python = object
            .get("requires-python")
            .and_then(Value::as_str)
            .map(str::to_string);
        let yanked = match object.get("yanked") {
            Some(Value::String(reason)) => Some(reason.clone()),
            Some(Value::Bool(true)) => Some(String::new()),
            _ => None,
        };
        // La URL del documento es de donde sale el contenido cuando el
        // documento no es de este servicio.
        let source = object
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(Self {
            filename,
            sha256,
            size,
            requires_python,
            yanked,
            source,
        })
    }
}

/// El documento de un proyecto: lo que el indice sabe de el.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDocument {
    /// El nombre normalizado del proyecto.
    pub name: String,
    /// Los ficheros del proyecto.
    pub files: Vec<FileEntry>,
}

impl ProjectDocument {
    /// Un documento vacio.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            files: Vec::new(),
        }
    }

    /// Anade un fichero al documento.
    ///
    /// Un fichero con el mismo nombre no se anade dos veces: el documento
    /// describe lo que hay, no lo que se ha pedido.
    pub fn add(&mut self, entry: FileEntry) -> Result<(), PypiError> {
        if self
            .files
            .iter()
            .any(|file| file.filename == entry.filename)
        {
            return Err(PypiError::FileExists);
        }
        self.files.push(entry);
        Ok(())
    }

    /// El fichero de ese nombre, si esta.
    pub fn file(&self, filename: &str) -> Option<&FileEntry> {
        self.files.iter().find(|file| file.filename == filename)
    }

    /// Las versiones del proyecto, sin repetir y en orden.
    pub fn versions(&self) -> Vec<String> {
        let mut versions: Vec<String> = Vec::new();
        for file in &self.files {
            let Ok(distribution) = Distribution::parse(&file.filename) else {
                continue;
            };
            if !versions.contains(&distribution.version) {
                versions.push(distribution.version.clone());
            }
        }
        versions.sort_by(|left, right| compare_versions(left, right));
        versions
    }

    /// Retira (o devuelve a la vida) una version entera.
    ///
    /// El estado es de la *release*, no del fichero: todos los ficheros de
    /// una version quedan igual, como manda PEP 592.
    pub fn set_yanked(&mut self, version: &str, yanked: Option<String>) -> bool {
        let mut changed = false;
        for file in &mut self.files {
            let Ok(distribution) = Distribution::parse(&file.filename) else {
                continue;
            };
            if distribution.version != version {
                continue;
            }
            if file.yanked != yanked {
                file.yanked = yanked.clone();
                changed = true;
            }
        }
        changed
    }

    /// El JSON del documento, tal y como se guarda.
    ///
    /// El documento guardado es un documento de la Simple API (PEP 691):
    /// lo que el indice muestra y lo que el cliente descarga son la misma
    /// cosa. `files_base` es el prefijo donde viven los ficheros
    /// (`/pypi/repositorio/files`).
    pub fn to_value(&self, files_base: &str) -> Value {
        let mut document = Map::new();
        let mut meta = Map::new();
        meta.insert("api-version".into(), Value::String(API_VERSION.to_string()));
        document.insert("meta".into(), Value::Object(meta));
        document.insert("name".into(), Value::String(self.name.clone()));
        document.insert(
            "files".into(),
            Value::Array(
                self.files
                    .iter()
                    .map(|file| {
                        let url = format!(
                            "{}/{}/{}",
                            files_base.trim_end_matches('/'),
                            encode_segment(&self.name),
                            encode_segment(&file.filename)
                        );
                        file.to_value(&url)
                    })
                    .collect(),
            ),
        );
        Value::Object(document)
    }

    /// Lee el documento guardado.
    pub fn from_value(value: &Value) -> Result<Self, PypiError> {
        let object = value
            .as_object()
            .ok_or_else(|| PypiError::InvalidDocument("the document is not an object".into()))?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| PypiError::InvalidDocument("the document has no name".into()))?
            .to_string();
        let files = object
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| PypiError::InvalidDocument("the document has no files".into()))?;
        let mut document = Self::new(name);
        for file in files {
            document.add(FileEntry::from_value(file)?)?;
        }
        Ok(document)
    }
}

/// Lo que el cliente pide en la superficie PyPI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /simple/`: el indice de los proyectos.
    Root,
    /// `GET /simple/{proyecto}/`: la pagina de un proyecto.
    Project { project: ProjectName },
    /// `GET /files/{proyecto}/{fichero}`: la distribucion.
    File {
        project: ProjectName,
        filename: String,
    },
    /// `POST /`: la subida.
    Upload,
    /// `POST /yank`: retirar una version.
    Yank,
}

/// Lee el resto de la ruta de `/pypi/{repositorio}/{resto}`.
///
/// El proyecto va normalizado en la URL, pero el cliente puede pedirlo con
/// el nombre que quiera: `Flask` y `flask` son el mismo proyecto y se
/// contestan igual.
pub fn route(rest: &str) -> Result<Route, PypiError> {
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        return Ok(Route::Upload);
    }
    if rest == "simple" {
        return Ok(Route::Root);
    }
    if rest == "yank" {
        return Ok(Route::Yank);
    }
    if let Some(project) = rest.strip_prefix("simple/") {
        if project.is_empty() || project.contains('/') {
            return Err(PypiError::InvalidPath(rest.to_string()));
        }
        return Ok(Route::Project {
            project: ProjectName::parse(project)?,
        });
    }
    if let Some(path) = rest.strip_prefix("files/") {
        let Some((project, filename)) = path.split_once('/') else {
            return Err(PypiError::InvalidPath(rest.to_string()));
        };
        if project.is_empty() || filename.is_empty() || filename.contains('/') {
            return Err(PypiError::InvalidPath(rest.to_string()));
        }
        let filename = decode_segment(filename)?;
        if filename.is_empty() || filename.contains('/') || filename.contains("..") {
            return Err(PypiError::InvalidPath(rest.to_string()));
        }
        return Ok(Route::File {
            project: ProjectName::parse(project)?,
            filename,
        });
    }
    Err(PypiError::InvalidPath(rest.to_string()))
}

/// Decodifica un segmento de ruta.
///
/// Un `%2F` no es un separador: es un `/` dentro del nombre, y un nombre
/// de fichero no lo tiene.
pub fn decode_segment(segment: &str) -> Result<String, PypiError> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(PypiError::InvalidPath(segment.to_string()));
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                .map_err(|_| PypiError::InvalidPath(segment.to_string()))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| PypiError::InvalidPath(segment.to_string()))?;
            decoded.push(byte);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).map_err(|_| PypiError::InvalidPath(segment.to_string()))
}

/// Compara dos versiones de PEP 440 por su parte numerica.
///
/// El indice no decide que version es la mas nueva: el cliente lo hace con
/// su propio comparador. Aqui solo se ordena lo que se enseña, y de forma
/// que `1.10` vaya despues de `1.9`.
pub fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (Version::parse(left), Version::parse(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        // Una version que no es de PEP 440 se ordena como texto: no la
        // sabe nadie mas, y al menos el orden es estable.
        _ => left.cmp(right),
    }
}

/// Como se negocia la representacion del indice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Representation {
    /// HTML (PEP 503).
    Html,
    /// JSON (PEP 691).
    Json,
}

/// Elige la representacion que pide el cliente.
///
/// `pip` de hoy pide JSON primero; un cliente antiguo o un navegador piden
/// HTML. Sin cabecera `Accept` se responde HTML, que es lo que entiende
/// todo el mundo.
pub fn negotiate(accept: Option<&str>) -> Representation {
    let Some(accept) = accept else {
        return Representation::Html;
    };
    let mut best: Option<(f32, Representation)> = None;
    for part in accept.split(',') {
        let mut pieces = part.split(';');
        let media = pieces.next().unwrap_or("").trim().to_ascii_lowercase();
        let mut quality = 1.0f32;
        for parameter in pieces {
            let parameter = parameter.trim();
            if let Some(value) = parameter.strip_prefix("q=") {
                quality = value.parse().unwrap_or(0.0);
            }
        }
        let representation = if media == JSON_CONTENT_TYPE || media == LATEST_JSON_CONTENT_TYPE {
            Some(Representation::Json)
        } else if media == HTML_CONTENT_TYPE || media == "text/html" {
            Some(Representation::Html)
        } else {
            None
        };
        let Some(representation) = representation else {
            continue;
        };
        // Una calidad de cero es una negativa: el cliente dice que no
        // quiere esa representacion.
        if quality <= 0.0 {
            continue;
        }
        if best.is_none_or(|(current, _)| quality > current) {
            best = Some((quality, representation));
        }
    }
    best.map(|(_, representation)| representation)
        .unwrap_or(Representation::Html)
}

/// La URL de un fichero, tal y como la ve el cliente.
///
/// El `base` es la raiz de la superficie (`http://host/pypi/repositorio`).
pub fn file_url(base: &str, project: &str, filename: &str) -> String {
    format!(
        "{}/files/{}/{}",
        base.trim_end_matches('/'),
        encode_segment(project),
        encode_segment(filename),
    )
}

/// La URL de un fichero con su hash, que es como lo publica el indice.
pub fn file_url_with_hash(base: &str, project: &str, filename: &str, sha256: &str) -> String {
    format!(
        "{}/files/{}/{}#sha256={}",
        base.trim_end_matches('/'),
        encode_segment(project),
        encode_segment(filename),
        sha256
    )
}

/// Escapa un texto para meterlo en HTML.
pub fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Codifica un segmento de ruta.
fn encode_segment(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// El HTML de la pagina de un proyecto (PEP 503).
pub fn render_project_html(document: &ProjectDocument, base: &str) -> String {
    let mut page = String::new();
    page.push_str("<!DOCTYPE html>\n<html>\n  <head>\n");
    page.push_str(&format!(
        "    <meta name=\"pypi:repository-version\" content=\"{API_VERSION}\">\n"
    ));
    page.push_str(&format!(
        "    <title>Links for {}</title>\n",
        escape_html(&document.name)
    ));
    page.push_str("  </head>\n  <body>\n");
    page.push_str(&format!(
        "    <h1>Links for {}</h1>\n",
        escape_html(&document.name)
    ));
    for file in &document.files {
        let url = file_url_with_hash(base, &document.name, &file.filename, &file.sha256);
        page.push_str(&format!("    <a href=\"{}\"", escape_html(&url)));
        if let Some(requires_python) = &file.requires_python {
            page.push_str(&format!(
                " data-requires-python=\"{}\"",
                escape_html(requires_python)
            ));
        }
        if let Some(yanked) = &file.yanked {
            page.push_str(&format!(" data-yanked=\"{}\"", escape_html(yanked)));
        }
        page.push_str(&format!(">{}</a><br/>\n", escape_html(&file.filename)));
    }
    page.push_str("  </body>\n</html>\n");
    page
}

/// El JSON de la pagina de un proyecto (PEP 691).
pub fn render_project_json(document: &ProjectDocument, base: &str) -> Value {
    let mut files = Vec::new();
    for file in &document.files {
        let mut entry = Map::new();
        entry.insert("filename".into(), Value::String(file.filename.clone()));
        entry.insert(
            "url".into(),
            Value::String(file_url_with_hash(
                base,
                &document.name,
                &file.filename,
                &file.sha256,
            )),
        );
        let mut hashes = Map::new();
        hashes.insert("sha256".into(), Value::String(file.sha256.clone()));
        entry.insert("hashes".into(), Value::Object(hashes));
        if let Some(requires_python) = &file.requires_python {
            entry.insert(
                "requires-python".into(),
                Value::String(requires_python.clone()),
            );
        }
        // El estado *yanked* se dice siempre: `false` es que la version
        // sigue viva, y el motivo es que esta retirada.
        match &file.yanked {
            Some(yanked) => {
                entry.insert("yanked".into(), Value::String(yanked.clone()));
            }
            None => {
                entry.insert("yanked".into(), Value::Bool(false));
            }
        }
        if let Some(size) = file.size {
            entry.insert("size".into(), Value::from(size));
        }
        files.push(Value::Object(entry));
    }
    let mut page = Map::new();
    let mut meta = Map::new();
    meta.insert("api-version".into(), Value::String(API_VERSION.to_string()));
    page.insert("meta".into(), Value::Object(meta));
    page.insert("name".into(), Value::String(document.name.clone()));
    page.insert("files".into(), Value::Array(files));
    page.insert(
        "versions".into(),
        Value::Array(document.versions().into_iter().map(Value::String).collect()),
    );
    Value::Object(page)
}

/// El HTML del indice general (PEP 503).
pub fn render_root_html(projects: &[String], base: &str) -> String {
    let mut page = String::new();
    page.push_str("<!DOCTYPE html>\n<html>\n  <head>\n");
    page.push_str(&format!(
        "    <meta name=\"pypi:repository-version\" content=\"{API_VERSION}\">\n"
    ));
    page.push_str("    <title>Simple index</title>\n");
    page.push_str("  </head>\n  <body>\n");
    for project in projects {
        let url = format!(
            "{}/simple/{}/",
            base.trim_end_matches('/'),
            encode_segment(project)
        );
        page.push_str(&format!(
            "    <a href=\"{}\">{}</a><br/>\n",
            escape_html(&url),
            escape_html(project)
        ));
    }
    page.push_str("  </body>\n</html>\n");
    page
}

/// El JSON del indice general (PEP 691).
pub fn render_root_json(projects: &[String]) -> Value {
    let mut page = Map::new();
    let mut meta = Map::new();
    meta.insert("api-version".into(), Value::String(API_VERSION.to_string()));
    page.insert("meta".into(), Value::Object(meta));
    page.insert(
        "projects".into(),
        Value::Array(
            projects
                .iter()
                .map(|project| {
                    let mut entry = Map::new();
                    entry.insert("name".into(), Value::String(project.clone()));
                    Value::Object(entry)
                })
                .collect(),
        ),
    );
    Value::Object(page)
}

/// Lo que declara una subida.
///
/// Los campos son los que manda `twine`: el nombre, la version, el hash y
/// la version de Python que necesita el paquete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    /// El proyecto, ya normalizado.
    pub project: ProjectName,
    /// La version.
    pub version: String,
    /// El nombre del fichero.
    pub filename: String,
    /// El hash SHA-256 del contenido.
    pub sha256: String,
    /// La version de Python que necesita, si la declara.
    pub requires_python: Option<String>,
}

/// Lee los campos de una subida y los comprueba contra el fichero.
///
/// La comprobacion es contra los bytes, no contra lo que dice el cliente:
/// el hash declarado tiene que ser el de lo recibido, y el nombre y la
/// version tienen que ser los del nombre del fichero.
pub fn parse_upload(
    fields: &[(String, String)],
    filename: &str,
    content_sha256: &str,
) -> Result<Upload, PypiError> {
    let lookup = |name: &str| -> Option<&str> {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    };
    match lookup(":action") {
        Some("file_upload") => {}
        Some(other) => {
            return Err(PypiError::Mismatch(format!(
                "the action {other:?} is not an upload"
            )));
        }
        None => return Err(PypiError::MissingField(":action")),
    }
    match lookup("protocol_version") {
        Some("1") => {}
        Some(other) => {
            return Err(PypiError::Mismatch(format!(
                "the protocol version {other:?} is not supported"
            )));
        }
        None => return Err(PypiError::MissingField("protocol_version")),
    }
    let declared_name = lookup("name").ok_or(PypiError::MissingField("name"))?;
    let declared_version = lookup("version").ok_or(PypiError::MissingField("version"))?;
    let declared_sha256 =
        lookup("sha256_digest").ok_or(PypiError::MissingField("sha256_digest"))?;
    if !declared_sha256.eq_ignore_ascii_case(content_sha256) {
        return Err(PypiError::DigestMismatch);
    }
    let distribution = Distribution::parse(filename)?;
    let declared_project = ProjectName::parse(declared_name)?;
    if declared_project != distribution.project {
        return Err(PypiError::Mismatch(format!(
            "the name {declared_name:?} is not the project of the file"
        )));
    }
    // La version que declara el cliente y la del nombre del fichero se
    // comparan en su forma canonica: el cliente puede escribirla como le
    // plazca (`1.0-2`, `1.0.rc1`, `v1.0`) y el nombre del fichero la
    // lleva canonica (`1.0.post2`, `1.0rc1`, `1.0`).
    let declared_version = canonical_version(declared_version)
        .ok_or_else(|| PypiError::InvalidVersion(declared_version.to_string()))?;
    if declared_version != distribution.version {
        return Err(PypiError::Mismatch(format!(
            "the version {declared_version:?} is not the version of the file"
        )));
    }
    Ok(Upload {
        project: declared_project,
        version: declared_version,
        filename: filename.to_string(),
        sha256: declared_sha256.to_ascii_lowercase(),
        requires_python: lookup("requires_python")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project(name: &str) -> ProjectName {
        ProjectName::parse(name).expect("a valid name")
    }

    #[test]
    fn names_are_normalized_the_way_pep_503_says() {
        assert_eq!(normalize("Flask"), "flask");
        assert_eq!(normalize("flask_login"), "flask-login");
        assert_eq!(normalize("flask.login"), "flask-login");
        assert_eq!(normalize("Flask--Login__Now"), "flask-login-now");
        assert_eq!(project("Flask.Login").as_str(), "flask-login");
        assert_eq!(
            project("flask_login").as_str(),
            project("Flask.Login").as_str()
        );
        for bad in ["", "  ", "a b", "a/b", "a:b", "-lead", "trail-"] {
            assert!(
                ProjectName::parse(bad).is_err(),
                "{bad:?} should not be a name"
            );
        }
    }

    #[test]
    fn distributions_are_read_from_their_file_name() {
        let wheel = Distribution::parse("labpkg-1.0-py3-none-any.whl").expect("a wheel");
        assert_eq!(wheel.project.as_str(), "labpkg");
        assert_eq!(wheel.version, "1.0");
        assert_eq!(wheel.kind, DistributionKind::Wheel);

        let underscore = Distribution::parse("my_pkg-2.1.1-py3-none-any.whl").expect("a wheel");
        assert_eq!(underscore.project.as_str(), "my-pkg");

        let sdist = Distribution::parse("labpkg-1.0.tar.gz").expect("an sdist");
        assert_eq!(sdist.project.as_str(), "labpkg");
        assert_eq!(sdist.version, "1.0");
        assert_eq!(sdist.kind, DistributionKind::Sdist);

        let zip = Distribution::parse("labpkg-1.0.zip").expect("an sdist");
        assert_eq!(zip.kind, DistributionKind::Sdist);

        for bad in [
            "labpkg.whl",
            "labpkg-1.0-py3-none-any.tar.gz",
            "labpkg-not.a.version-py3-none-any.whl",
            "../evil-1.0-py3-none-any.whl",
            "labpkg-1.0-py3-none-any.whl.asc",
        ] {
            assert!(
                Distribution::parse(bad).is_err(),
                "{bad:?} should not be a distribution"
            );
        }
    }

    #[test]
    fn versions_follow_pep_440() {
        for good in [
            "1",
            "1.0",
            "1.0.0",
            "1.0.0.0",
            "2.1.3",
            "1.0a1",
            "1.0rc1",
            "1.0.dev1",
            "1.0.post1",
            "1.0-1",
            "1!2.0",
            "1.0+local.1",
            "1.0.0.post2.dev3",
        ] {
            assert!(valid_version(good), "{good:?} should be a version");
        }
        for bad in ["", "x", "1..0", "1.0.", ".1", "1.0+", "1.0+_", "1.0zz"] {
            assert!(!valid_version(bad), "{bad:?} should not be a version");
        }
    }

    #[test]
    fn the_document_keeps_one_entry_per_file() {
        let mut document = ProjectDocument::new("labpkg");
        let entry = FileEntry {
            filename: "labpkg-1.0-py3-none-any.whl".into(),
            sha256: "a".repeat(64),
            size: Some(10),
            requires_python: Some(">=3.9".into()),
            yanked: None,
            source: None,
        };
        document.add(entry.clone()).expect("the first file");
        assert_eq!(document.add(entry).unwrap_err(), PypiError::FileExists);
        assert_eq!(document.files.len(), 1);
        assert!(document.file("labpkg-1.0-py3-none-any.whl").is_some());
    }

    #[test]
    fn a_version_is_yanked_as_a_whole() {
        let mut document = ProjectDocument::new("labpkg");
        for filename in [
            "labpkg-1.0-py3-none-any.whl",
            "labpkg-1.0.tar.gz",
            "labpkg-2.0-py3-none-any.whl",
        ] {
            document
                .add(FileEntry {
                    filename: filename.into(),
                    sha256: "b".repeat(64),
                    size: None,
                    requires_python: None,
                    yanked: None,
                    source: None,
                })
                .expect("a file");
        }
        assert!(document.set_yanked("1.0", Some("broken".into())));
        assert!(!document.set_yanked("1.0", Some("broken".into())));
        assert_eq!(
            document.file("labpkg-1.0.tar.gz").unwrap().yanked,
            Some("broken".into())
        );
        assert_eq!(
            document.file("labpkg-2.0-py3-none-any.whl").unwrap().yanked,
            None
        );
    }

    #[test]
    fn the_json_and_the_html_say_the_same() {
        let mut document = ProjectDocument::new("labpkg");
        document
            .add(FileEntry {
                filename: "labpkg-1.0-py3-none-any.whl".into(),
                sha256: "c".repeat(64),
                size: Some(42),
                requires_python: Some(">=3.9".into()),
                yanked: Some("broken".into()),
                source: None,
            })
            .expect("a file");
        let base = "http://localhost:8080/pypi/repo";
        let html = render_project_html(&document, base);
        assert!(html.contains("data-requires-python=\"&gt;=3.9\""));
        assert!(html.contains("data-yanked=\"broken\""));
        assert!(html.contains(&format!("#sha256={}", "c".repeat(64))));
        let json = render_project_json(&document, base);
        let files = json.get("files").and_then(Value::as_array).expect("files");
        assert_eq!(files.len(), 1);
        let entry = &files[0];
        assert_eq!(
            entry
                .get("hashes")
                .and_then(|hashes| hashes.get("sha256"))
                .and_then(Value::as_str),
            Some("c".repeat(64).as_str())
        );
        assert_eq!(
            entry.get("requires-python").and_then(Value::as_str),
            Some(">=3.9")
        );
        assert_eq!(entry.get("yanked").and_then(Value::as_str), Some("broken"));
        assert_eq!(entry.get("size").and_then(Value::as_u64), Some(42));
        let url = entry.get("url").and_then(Value::as_str).expect("a url");
        assert!(url.starts_with("http://localhost:8080/pypi/repo/files/labpkg/"));
    }

    #[test]
    fn a_file_that_is_not_yanked_says_so() {
        let mut document = ProjectDocument::new("labpkg");
        document
            .add(FileEntry {
                filename: "labpkg-1.0-py3-none-any.whl".into(),
                sha256: "9".repeat(64),
                size: Some(11),
                requires_python: None,
                yanked: None,
                source: None,
            })
            .expect("a file");
        // El JSON dice que no esta retirada: el cliente decide con el.
        let json = render_project_json(&document, "http://localhost");
        let files = json.get("files").and_then(Value::as_array).expect("files");
        assert_eq!(files[0].get("yanked"), Some(&Value::Bool(false)));
        // Y el HTML no pone el atributo, que es lo mismo.
        let html = render_project_html(&document, "http://localhost");
        assert!(!html.contains("data-yanked"));
    }

    #[test]
    fn the_html_escapes_what_comes_from_the_client() {
        let mut document = ProjectDocument::new("labpkg");
        document
            .add(FileEntry {
                filename: "<script>alert(1)</script>".into(),
                sha256: "d".repeat(64),
                size: None,
                requires_python: Some("\"><script>".into()),
                yanked: None,
                source: None,
            })
            .expect("a file");
        let html = render_project_html(&document, "http://localhost");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn the_representation_is_negotiated() {
        assert_eq!(negotiate(None), Representation::Html);
        assert_eq!(
            negotiate(Some("application/vnd.pypi.simple.v1+json")),
            Representation::Json
        );
        assert_eq!(
            negotiate(Some(
                "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1, text/html;q=0.01"
            )),
            Representation::Json
        );
        assert_eq!(negotiate(Some("text/html")), Representation::Html);
        assert_eq!(
            negotiate(Some(
                "application/vnd.pypi.simple.v1+html;q=0.9, application/vnd.pypi.simple.v1+json;q=0.2"
            )),
            Representation::Html
        );
        assert_eq!(negotiate(Some("application/json")), Representation::Html);
        // La version `latest` del JSON es la misma representacion.
        assert_eq!(
            negotiate(Some("application/vnd.pypi.simple.latest+json")),
            Representation::Json
        );
        // Una calidad de cero es una negativa.
        assert_eq!(
            negotiate(Some("application/vnd.pypi.simple.v1+json;q=0")),
            Representation::Html
        );
    }

    #[test]
    fn a_version_is_canonicalized_the_way_pep_440_says() {
        for (raw, canonical) in [
            ("1.0", "1.0"),
            ("1.0-2", "1.0.post2"),
            ("1.0-rc1", "1.0rc1"),
            ("1.0.rc1", "1.0rc1"),
            ("1.0_rc1", "1.0rc1"),
            ("1.0.alpha1", "1.0a1"),
            ("1.0.beta1", "1.0b1"),
            ("1.0.pre1", "1.0rc1"),
            ("1.0.c1", "1.0rc1"),
            ("v1.0", "1.0"),
            ("1.0.post", "1.0.post0"),
            ("1.0-r2", "1.0.post2"),
            ("1.2post2", "1.2.post2"),
            ("1.0+ubuntu-1", "1.0+ubuntu.1"),
            ("01.0", "1.0"),
            ("1.0.dev", "1.0.dev0"),
            // La gramatica de la PEP no distingue mayusculas.
            ("1.0RC1", "1.0rc1"),
            ("1.0A1", "1.0a1"),
            ("1.0B1", "1.0b1"),
            ("1.0POST1", "1.0.post1"),
            ("1.0DEV1", "1.0.dev1"),
            ("1.0+ABC", "1.0+abc"),
            // Y un segmento numerico de la version local no lleva ceros.
            ("1.0+01", "1.0+1"),
            ("1.0+abc.01", "1.0+abc.1"),
            ("1.0+00", "1.0+0"),
        ] {
            assert_eq!(
                canonical_version(raw).as_deref(),
                Some(canonical),
                "{raw:?}"
            );
        }
        for bad in [
            "1.0-",
            "1.0.dev1.post1",
            "1.0.dev1.dev2",
            "1.0.post1.post2",
            "1.0-1-2",
            "1.0+_x",
            "1!",
        ] {
            assert!(
                canonical_version(bad).is_none(),
                "{bad:?} should not be a version"
            );
        }
    }

    #[test]
    fn a_version_is_compared_in_its_canonical_form() {
        // El cliente escribe la version como le plazca; el nombre del
        // fichero la lleva canonica.
        let sha = "e".repeat(64);
        let mut fields = vec![
            (":action".to_string(), "file_upload".to_string()),
            ("protocol_version".to_string(), "1".to_string()),
            ("name".to_string(), "labpkg".to_string()),
            ("version".to_string(), "1.0-2".to_string()),
            ("sha256_digest".to_string(), sha.clone()),
        ];
        let upload =
            parse_upload(&fields, "labpkg-1.0.post2.tar.gz", &sha).expect("a valid upload");
        assert_eq!(upload.version, "1.0.post2");
        // Y una version que no es la del fichero se rechaza.
        fields[3].1 = "1.0-3".to_string();
        assert!(matches!(
            parse_upload(&fields, "labpkg-1.0.post2.tar.gz", &sha),
            Err(PypiError::Mismatch(_))
        ));
    }

    #[test]
    fn a_wheel_that_the_client_would_reject_is_not_a_distribution() {
        // Las partes de un wheel son las que son: ni una mas ni una menos.
        assert!(Distribution::parse("labpkg-1.0-py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-1-py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-py3-none-any-extra.whl").is_err());
        assert!(Distribution::parse("labpkg-1.0-1-2-py3-none-any.whl").is_err());
        assert!(Distribution::parse("labpkg-1.0-x-py3-none-any.whl").is_err());
        assert!(Distribution::parse("labpkg-1.0-py3-none.whl").is_err());
    }

    #[test]
    fn a_wheel_that_the_client_would_install_is_a_distribution() {
        // La regla es la del `pip`: la etiqueta de compilacion empieza por
        // un digito y detras va lo que sea, y las tres etiquetas son las
        // que son. Un nombre que el `pip` instala no se rechaza aqui.
        assert!(Distribution::parse("labpkg-1.0-1.5-py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-1abc-py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-py3-none-any+mips.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-py2.py3-none-any.whl").is_ok());
        assert!(Distribution::parse("labpkg-1.0-cp39-cp39-manylinux_2_17_x86_64.whl").is_ok());
    }

    #[test]
    fn an_upload_is_checked_against_its_content() {
        let sha = "e".repeat(64);
        let fields = vec![
            (":action".to_string(), "file_upload".to_string()),
            ("protocol_version".to_string(), "1".to_string()),
            ("name".to_string(), "Lab.Pkg".to_string()),
            ("version".to_string(), "1.0".to_string()),
            ("sha256_digest".to_string(), sha.clone()),
            ("requires_python".to_string(), ">=3.9".to_string()),
        ];
        let upload =
            parse_upload(&fields, "lab_pkg-1.0-py3-none-any.whl", &sha).expect("a valid upload");
        assert_eq!(upload.project.as_str(), "lab-pkg");
        assert_eq!(upload.version, "1.0");
        assert_eq!(upload.requires_python.as_deref(), Some(">=3.9"));

        let corrupted = parse_upload(&fields, "lab_pkg-1.0-py3-none-any.whl", &"f".repeat(64));
        assert_eq!(corrupted.unwrap_err(), PypiError::DigestMismatch);

        let mut other_name = fields.clone();
        other_name[2].1 = "otro".to_string();
        assert!(matches!(
            parse_upload(&other_name, "lab_pkg-1.0-py3-none-any.whl", &sha),
            Err(PypiError::Mismatch(_))
        ));

        let mut other_version = fields.clone();
        other_version[3].1 = "2.0".to_string();
        assert!(matches!(
            parse_upload(&other_version, "lab_pkg-1.0-py3-none-any.whl", &sha),
            Err(PypiError::Mismatch(_))
        ));

        let without_action: Vec<(String, String)> = fields
            .iter()
            .filter(|(field, _)| field != ":action")
            .cloned()
            .collect();
        assert_eq!(
            parse_upload(&without_action, "lab_pkg-1.0-py3-none-any.whl", &sha).unwrap_err(),
            PypiError::MissingField(":action")
        );
    }

    #[test]
    fn the_document_survives_a_round_trip() {
        let mut document = ProjectDocument::new("labpkg");
        document
            .add(FileEntry {
                filename: "labpkg-1.0-py3-none-any.whl".into(),
                sha256: "1".repeat(64),
                size: Some(7),
                requires_python: None,
                yanked: Some(String::new()),
                source: None,
            })
            .expect("a file");
        let value = document.to_value("/pypi/repo/files");
        let read = ProjectDocument::from_value(&value).expect("the document");
        assert_eq!(read.name, document.name);
        assert_eq!(read.files.len(), 1);
        assert_eq!(read.files[0].filename, document.files[0].filename);
        assert_eq!(read.files[0].sha256, document.files[0].sha256);
        assert_eq!(read.files[0].size, document.files[0].size);
        assert_eq!(read.files[0].yanked, Some(String::new()));
        // La URL guardada es la de la superficie, no la de un upstream.
        assert_eq!(
            read.files[0].source.as_deref(),
            Some("/pypi/repo/files/labpkg/labpkg-1.0-py3-none-any.whl")
        );
    }

    #[test]
    fn a_document_of_the_upstream_is_read_the_same_way() {
        let upstream = json!({
            "meta": {"api-version": "1.0"},
            "name": "labpkg",
            "files": [{
                "filename": "labpkg-1.0-py3-none-any.whl",
                "url": "https://files.example.com/packages/ab/cd/labpkg-1.0-py3-none-any.whl",
                "hashes": {"sha256": "2".repeat(64)},
                "requires-python": ">=3.9",
                "yanked": "broken",
                "size": 99
            }]
        });
        let document = ProjectDocument::from_value(&upstream).expect("the document");
        assert_eq!(document.name, "labpkg");
        let file = &document.files[0];
        assert_eq!(file.sha256, "2".repeat(64));
        assert_eq!(file.size, Some(99));
        assert_eq!(file.requires_python.as_deref(), Some(">=3.9"));
        assert_eq!(file.yanked.as_deref(), Some("broken"));
        assert_eq!(
            file.source.as_deref(),
            Some("https://files.example.com/packages/ab/cd/labpkg-1.0-py3-none-any.whl")
        );
    }

    #[test]
    fn the_routes_of_the_surface_are_read() {
        assert_eq!(route("").unwrap(), Route::Upload);
        assert_eq!(route("simple").unwrap(), Route::Root);
        assert_eq!(route("simple/").unwrap(), Route::Root);
        assert_eq!(route("yank").unwrap(), Route::Yank);
        assert_eq!(
            route("simple/Flask.Login/").unwrap(),
            Route::Project {
                project: project("flask-login")
            }
        );
        assert_eq!(
            route("files/labpkg/labpkg-1.0%2Blocal-py3-none-any.whl").unwrap(),
            Route::File {
                project: project("labpkg"),
                filename: "labpkg-1.0+local-py3-none-any.whl".to_string()
            }
        );
        for bad in [
            "nope",
            "files/labpkg",
            "files/labpkg/a/b",
            "files/labpkg/..",
        ] {
            assert!(route(bad).is_err(), "{bad:?} should not be a route");
        }
    }

    #[test]
    fn a_document_that_is_not_ours_is_refused() {
        assert!(ProjectDocument::from_value(&json!({})).is_err());
        assert!(ProjectDocument::from_value(&json!({"name": "x", "files": [{}]})).is_err());
        assert!(
            ProjectDocument::from_value(&json!({
                "name": "x",
                "files": [{"filename": "a.whl", "sha256": "zz"}]
            }))
            .is_err()
        );
    }
}
