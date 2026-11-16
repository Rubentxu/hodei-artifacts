//! La parte pura del formato npm: nombres, rutas, documentos de
//! publicacion y packuments.
//!
//! Nada aqui toca disco, red ni catalogo: son funciones sobre cadenas y
//! JSON, y por eso se prueban sin infraestructura. El servicio las usa para
//! decidir que quiere el cliente (`route`), si un documento de publicacion
//! es admisible (`PublishDocument`) y como queda el packument despues de
//! cada operacion.
//!
//! # El packument es una instantanea, no una vista
//!
//! A diferencia de `maven-metadata.xml`, un packument lleva datos que no se
//! pueden derivar de los ficheros (el `package.json` de cada version, los
//! dist-tags, las deprecaciones). Por eso un hosted guarda **cada estado**
//! del packument como contenido direccionado por su digest y mueve el
//! puntero de la ruta: publicar, mover un dist-tag o deprecar crean una
//! instantanea nueva y no reescriben los bytes de las anteriores.
//!
//! # Layout
//!
//! La ruta de un asset es la del protocolo, la misma en hosted y en proxy:
//!
//! ```text
//! {nombre}                      el packument
//! {nombre}/-/{base}-{version}.tgz   el tarball
//! ```
//!
//! donde `nombre` puede ser `@alcance/base`.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

/// La longitud maxima de un nombre de paquete (completo, con alcance).
pub const MAX_NAME_LEN: usize = 214;

/// La longitud maxima de un dist-tag.
pub const MAX_TAG_LEN: usize = 64;

/// El dist-tag que npm resuelve cuando no se pide otro.
pub const LATEST: &str = "latest";

/// El sufijo de las coordenadas de las instantaneas del packument.
///
/// `#` no puede aparecer en un nombre npm, asi que las coordenadas de las
/// instantaneas nunca chocan con las de un tarball.
pub const PACKUMENT_COORDINATES_SUFFIX: &str = "#packument";

/// Los nombres que npm reserva aunque cumplan el resto de reglas.
const RESERVED: &[&str] = &["node_modules", "favicon.ico"];

/// Por que un nombre, una ruta o un documento no se admite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NpmError {
    /// El nombre del paquete no cumple las reglas de npm.
    InvalidName(&'static str),
    /// La ruta no es ninguna de las que habla el protocolo.
    InvalidPath(&'static str),
    /// El documento no se entiende o no es admisible.
    InvalidDocument(&'static str),
    /// El dist-tag no es admisible.
    InvalidTag(&'static str),
    /// La version ya esta publicada y una version no se reescribe.
    VersionExists(String),
    /// Lo pedido no existe en el packument.
    UnknownVersion(String),
    /// El documento intenta quitar algo: eso es un unpublish.
    Unpublish,
}

impl NpmError {
    /// El motivo, tal y como se le dice al cliente.
    pub fn message(&self) -> String {
        match self {
            Self::InvalidName(reason)
            | Self::InvalidPath(reason)
            | Self::InvalidDocument(reason)
            | Self::InvalidTag(reason) => (*reason).to_string(),
            Self::VersionExists(version) => {
                format!("You cannot publish over the previously published versions: {version}.")
            }
            Self::UnknownVersion(version) => format!("the version {version} does not exist"),
            Self::Unpublish => {
                "unpublishing is not allowed in this registry; deprecate the version instead"
                    .to_string()
            }
        }
    }
}

/// Un nombre de paquete, ya validado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageName {
    full: String,
    /// El alcance con `@`, si lo tiene.
    scope: Option<String>,
    base: String,
}

impl PackageName {
    /// Lee un nombre. `strict` aplica las reglas de un paquete **nuevo**
    /// (solo minusculas); un proxy admite lo que el upstream ya publico,
    /// que incluye nombres antiguos con mayusculas.
    pub fn parse(name: &str, strict: bool) -> Result<Self, NpmError> {
        if name.is_empty() {
            return Err(NpmError::InvalidName("the package name is empty"));
        }
        if name.len() > MAX_NAME_LEN {
            return Err(NpmError::InvalidName("the package name is too long"));
        }
        let (scope, base) = match name.strip_prefix('@') {
            Some(rest) => {
                let Some((scope, base)) = rest.split_once('/') else {
                    return Err(NpmError::InvalidName(
                        "a scoped package name is @scope/name",
                    ));
                };
                (Some(format!("@{scope}")), base)
            }
            None => (None, name),
        };
        let part_ok = |part: &str| {
            !part.is_empty()
                && !part.starts_with('.')
                && !part.starts_with('_')
                && part.bytes().all(|byte| {
                    byte.is_ascii_digit()
                        || byte.is_ascii_lowercase()
                        || (!strict && byte.is_ascii_uppercase())
                        || matches!(byte, b'-' | b'.' | b'_' | b'~')
                })
        };
        if let Some(scope) = &scope
            && !part_ok(&scope[1..])
        {
            return Err(NpmError::InvalidName("the scope is not a valid name"));
        }
        if !part_ok(base) {
            return Err(NpmError::InvalidName(
                "the package name has characters that npm does not allow",
            ));
        }
        if RESERVED.contains(&base) {
            return Err(NpmError::InvalidName("the package name is reserved"));
        }
        Ok(Self {
            full: name.to_string(),
            scope,
            base: base.to_string(),
        })
    }

    /// El nombre completo, con alcance.
    pub fn full(&self) -> &str {
        &self.full
    }

    /// El alcance con `@`, si lo tiene.
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    /// El nombre sin alcance.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// El nombre del fichero del tarball de una version.
    pub fn tarball_file(&self, version: &str) -> String {
        format!("{}-{version}.tgz", self.base)
    }

    /// La ruta del tarball de una version dentro del repositorio.
    pub fn tarball_path(&self, version: &str) -> String {
        format!("{}/-/{}", self.full, self.tarball_file(version))
    }
}

/// Lo que el cliente pide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /-/ping`.
    Ping,
    /// `GET /-/whoami`.
    Whoami,
    /// El packument de un paquete.
    Packument { name: PackageName },
    /// El tarball de un paquete.
    Tarball { name: PackageName, file: String },
    /// Todos los dist-tags de un paquete.
    DistTags { name: PackageName },
    /// Un dist-tag de un paquete.
    DistTag { name: PackageName, tag: String },
    /// `DELETE` de un paquete entero (`/{nombre}/-rev/{rev}`).
    UnpublishPackage { name: PackageName },
    /// `DELETE` de un tarball (`/{nombre}/-/{fichero}/-rev/{rev}`).
    UnpublishTarball { name: PackageName, file: String },
}

impl Route {
    /// La ruta del asset que respalda la peticion, si la peticion lee
    /// contenido del repositorio.
    pub fn asset_path(&self) -> Option<String> {
        match self {
            Self::Packument { name } | Self::DistTags { name } | Self::DistTag { name, .. } => {
                Some(name.full().to_string())
            }
            Self::Tarball { name, file } => Some(format!("{}/-/{file}", name.full())),
            _ => None,
        }
    }
}

/// Decodifica un segmento: solo `%2f` (la barra de un nombre con alcance) y
/// `%40` (la arroba) tienen sentido en un nombre npm.
fn decode_segment(segment: &str) -> Result<String, NpmError> {
    let mut out = String::with_capacity(segment.len());
    let bytes = segment.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            let escape = segment
                .get(index + 1..index + 3)
                .map(str::to_ascii_lowercase);
            match escape.as_deref() {
                Some("2f") => out.push('/'),
                Some("40") => out.push('@'),
                _ => {
                    return Err(NpmError::InvalidPath(
                        "the path has an escape npm never uses",
                    ));
                }
            }
            index += 3;
        } else {
            out.push(byte as char);
            index += 1;
        }
    }
    Ok(out)
}

fn path_char_ok(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, '-' | '.' | '_' | '~' | '@' | '%' | '+' | ':')
}

/// Un nombre de fichero de tarball: lo que el upstream publica, sin
/// atravesar directorios.
fn tarball_file_ok(file: &str) -> bool {
    file.len() <= 255
        && file.ends_with(".tgz")
        && !file.starts_with('.')
        && file.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+')
        })
}

/// Lee la ruta que sigue a `/npm/{repositorio}/`, **sin decodificar**.
pub fn route(rest: &str) -> Result<Route, NpmError> {
    if rest.is_empty() || rest.len() > 1024 {
        return Err(NpmError::InvalidPath("the path is empty or too long"));
    }
    if !rest
        .chars()
        .all(|character| path_char_ok(character) || character == '/')
    {
        return Err(NpmError::InvalidPath(
            "the path has characters npm never uses",
        ));
    }
    let raw: Vec<&str> = rest.split('/').collect();
    if raw.iter().any(|segment| segment.is_empty()) {
        return Err(NpmError::InvalidPath("the path has an empty segment"));
    }
    let segments: Vec<String> = raw
        .iter()
        .map(|segment| decode_segment(segment))
        .collect::<Result<_, _>>()?;
    // Un segmento puede traer una barra (el `%2f` del alcance) solo si es
    // la del nombre: `@alcance/base`.
    if segments
        .iter()
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(NpmError::InvalidPath("the path has a dot segment"));
    }

    if segments[0] == "-" {
        return match segments.as_slice() {
            [_, ping] if ping == "ping" => Ok(Route::Ping),
            [_, whoami] if whoami == "whoami" => Ok(Route::Whoami),
            [_, package, name, tags] if package == "package" && tags == "dist-tags" => {
                Ok(Route::DistTags {
                    name: PackageName::parse(name, false)?,
                })
            }
            [_, package, name, tags, tag] if package == "package" && tags == "dist-tags" => {
                Ok(Route::DistTag {
                    name: PackageName::parse(name, false)?,
                    tag: valid_tag(tag)?.to_string(),
                })
            }
            _ => Err(NpmError::InvalidPath("this npm endpoint is not served")),
        };
    }

    // El nombre: `@alcance%2fbase` llega en un segmento, `@alcance/base` en
    // dos; los dos son el mismo paquete.
    let (name_text, tail): (String, &[String]) =
        if segments[0].starts_with('@') && !segments[0].contains('/') {
            match segments.get(1) {
                Some(base) => (format!("{}/{}", segments[0], base), &segments[2..]),
                None => {
                    return Err(NpmError::InvalidName(
                        "a scoped package name is @scope/name",
                    ));
                }
            }
        } else {
            (segments[0].clone(), &segments[1..])
        };
    let name = PackageName::parse(&name_text, false)?;
    match tail {
        [] => Ok(Route::Packument { name }),
        [dash, file] if dash == "-" => {
            if !tarball_file_ok(file) {
                return Err(NpmError::InvalidPath("the tarball name is not valid"));
            }
            Ok(Route::Tarball {
                name,
                file: file.clone(),
            })
        }
        [rev, _] if rev == "-rev" => Ok(Route::UnpublishPackage { name }),
        [dash, file, rev, _] if dash == "-" && rev == "-rev" => {
            if !tarball_file_ok(file) {
                return Err(NpmError::InvalidPath("the tarball name is not valid"));
            }
            Ok(Route::UnpublishTarball {
                name,
                file: file.clone(),
            })
        }
        _ => Err(NpmError::InvalidPath("this npm endpoint is not served")),
    }
}

/// Lee una version semver. npm exige una version completa y valida.
pub fn valid_version(version: &str) -> Result<&str, NpmError> {
    if version.len() > 256 || semver::Version::parse(version).is_err() {
        return Err(NpmError::InvalidDocument(
            "the version is not a valid semver",
        ));
    }
    Ok(version)
}

/// Un dist-tag: un nombre corto que no es una version ni un rango.
pub fn valid_tag(tag: &str) -> Result<&str, NpmError> {
    if tag.is_empty() || tag.len() > MAX_TAG_LEN {
        return Err(NpmError::InvalidTag("the dist-tag is empty or too long"));
    }
    if !tag
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
    {
        return Err(NpmError::InvalidTag(
            "the dist-tag has characters npm never uses",
        ));
    }
    // Un tag que se lee como version o como rango haria ambiguo
    // `npm install pkg@x`: npm lo prohibe.
    if semver::Version::parse(tag).is_ok() || semver::VersionReq::parse(tag).is_ok() {
        return Err(NpmError::InvalidTag(
            "a dist-tag cannot be a version or a range",
        ));
    }
    Ok(tag)
}

/// Un adjunto del documento de publicacion, ya decodificado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// El nombre del fichero.
    pub file: String,
    /// El contenido: el tarball.
    pub bytes: Vec<u8>,
}

/// La peticion de un `PUT /{nombre}`.
///
/// Es una **publicacion** si trae el tarball y una **actualizacion de
/// metadatos** (deprecar) si no lo trae. Cualquier otra cosa es un error.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// Una version nueva con su tarball.
    Publish(Box<NewVersion>),
    /// Cambios de `deprecated` sobre versiones que ya existen.
    Metadata(Box<Document>),
}

/// La version que se publica.
#[derive(Debug, Clone, PartialEq)]
pub struct NewVersion {
    /// El nombre del paquete.
    pub name: PackageName,
    /// La version.
    pub version: String,
    /// El `package.json` que el cliente declara, sin tocar.
    pub manifest: Map<String, Value>,
    /// Los dist-tags que el cliente pide para esta version.
    pub tags: Vec<String>,
    /// El tarball.
    pub attachment: Attachment,
    /// Los campos del documento que describen el paquete.
    pub document: Document,
}

/// El documento tal y como llega, ya con forma.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// El nombre del paquete.
    pub name: PackageName,
    /// El objeto entero.
    pub value: Map<String, Value>,
}

/// Lee el cuerpo de un `PUT /{nombre}`.
///
/// `expected` es el nombre de la ruta: el documento no puede hablar de otro
/// paquete. `decode` convierte el base64 del adjunto (el servicio decide el
/// codec; aqui no hay dependencias de hash ni de base64).
pub fn parse_update(
    expected: &PackageName,
    body: &[u8],
    decode: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Update, NpmError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| NpmError::InvalidDocument("the body is not a JSON document"))?;
    let Value::Object(object) = value else {
        return Err(NpmError::InvalidDocument("the body is not a JSON object"));
    };
    let declared = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or(NpmError::InvalidDocument("the document has no name"))?;
    if declared != expected.full() {
        return Err(NpmError::InvalidDocument(
            "the document is for another package than the path",
        ));
    }
    let document = Document {
        name: expected.clone(),
        value: object,
    };
    let attachments = document
        .value
        .get("_attachments")
        .and_then(Value::as_object)
        .filter(|attachments| !attachments.is_empty());
    let Some(attachments) = attachments else {
        return Ok(Update::Metadata(Box::new(document)));
    };
    if attachments.len() != 1 {
        return Err(NpmError::InvalidDocument(
            "a publication carries exactly one tarball",
        ));
    }
    let versions = document
        .value
        .get("versions")
        .and_then(Value::as_object)
        .ok_or(NpmError::InvalidDocument("the document has no versions"))?;
    if versions.len() != 1 {
        return Err(NpmError::InvalidDocument(
            "a publication carries exactly one version",
        ));
    }
    let (version, manifest) = versions.iter().next().expect("one version");
    valid_version(version)?;
    let Value::Object(manifest) = manifest else {
        return Err(NpmError::InvalidDocument("the version is not an object"));
    };
    if manifest.get("name").and_then(Value::as_str) != Some(expected.full())
        || manifest.get("version").and_then(Value::as_str) != Some(version.as_str())
    {
        return Err(NpmError::InvalidDocument(
            "the version does not say the name and version it is published under",
        ));
    }
    let (file, attachment) = attachments.iter().next().expect("one attachment");
    // `libnpmpublish` llama al adjunto `{nombre completo}-{version}.tgz`
    // (`@acme/widget-1.0.0.tgz` si lleva alcance); el registro lo guarda como
    // `{base}-{version}.tgz`. Se aceptan las dos formas y se guarda la segunda.
    let basename = expected.tarball_file(version);
    if *file != basename && *file != format!("{}-{version}.tgz", expected.full()) {
        return Err(NpmError::InvalidDocument(
            "the tarball is not called {name}-{version}.tgz",
        ));
    }
    let file = &basename;
    let data = attachment
        .get("data")
        .and_then(Value::as_str)
        .ok_or(NpmError::InvalidDocument("the tarball has no data"))?;
    let bytes = decode(data).ok_or(NpmError::InvalidDocument("the tarball is not base64"))?;
    if bytes.is_empty() {
        return Err(NpmError::InvalidDocument("the tarball is empty"));
    }
    if let Some(length) = attachment.get("length").and_then(Value::as_u64)
        && length != bytes.len() as u64
    {
        return Err(NpmError::InvalidDocument(
            "the tarball is not as long as it says",
        ));
    }
    let tags = match document.value.get("dist-tags").and_then(Value::as_object) {
        Some(tags) => {
            let mut chosen = Vec::new();
            for (tag, target) in tags {
                valid_tag(tag)?;
                if target.as_str() != Some(version.as_str()) {
                    return Err(NpmError::InvalidTag(
                        "a publication only tags the version it publishes",
                    ));
                }
                chosen.push(tag.clone());
            }
            if chosen.is_empty() {
                vec![LATEST.to_string()]
            } else {
                chosen
            }
        }
        None => vec![LATEST.to_string()],
    };
    Ok(Update::Publish(Box::new(NewVersion {
        name: expected.clone(),
        version: version.clone(),
        manifest: manifest.clone(),
        tags,
        attachment: Attachment {
            file: file.clone(),
            bytes,
        },
        document,
    })))
}

/// Lo que el servicio sabe del tarball al publicar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TarballFacts {
    /// `sha1` en hexadecimal: el `shasum` historico de npm.
    pub shasum: String,
    /// `sha512-{base64}`: el `integrity` de npm.
    pub integrity: String,
}

/// Comprueba lo que el cliente declara de su tarball contra lo calculado.
pub fn check_declared_dist(
    manifest: &Map<String, Value>,
    facts: &TarballFacts,
) -> Result<(), NpmError> {
    let Some(dist) = manifest.get("dist").and_then(Value::as_object) else {
        return Ok(());
    };
    if let Some(shasum) = dist.get("shasum").and_then(Value::as_str)
        && !shasum.eq_ignore_ascii_case(&facts.shasum)
    {
        return Err(NpmError::InvalidDocument(
            "the shasum does not match the tarball",
        ));
    }
    if let Some(integrity) = dist.get("integrity").and_then(Value::as_str)
        && integrity != facts.integrity
    {
        return Err(NpmError::InvalidDocument(
            "the integrity does not match the tarball",
        ));
    }
    Ok(())
}

/// Quien publico, cuando, y con que correlacion: el origen de la version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// El sujeto verificado que publica.
    pub publisher: String,
    /// El instante, en RFC 3339.
    pub at: String,
    /// La correlacion de la peticion.
    pub correlation: String,
}

fn versions_of(packument: &mut Map<String, Value>) -> &mut Map<String, Value> {
    object_entry(packument, "versions")
}

fn object_entry<'a>(
    packument: &'a mut Map<String, Value>,
    key: &str,
) -> &'a mut Map<String, Value> {
    let slot = packument
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !slot.is_object() {
        *slot = Value::Object(Map::new());
    }
    slot.as_object_mut().expect("an object")
}

/// El packument de un paquete que todavia no tiene versiones.
pub fn empty_packument(name: &PackageName, now: &str) -> Map<String, Value> {
    let mut packument = Map::new();
    packument.insert("_id".into(), Value::String(name.full().to_string()));
    packument.insert("name".into(), Value::String(name.full().to_string()));
    packument.insert("dist-tags".into(), Value::Object(Map::new()));
    packument.insert("versions".into(), Value::Object(Map::new()));
    let mut time = Map::new();
    time.insert("created".into(), Value::String(now.to_string()));
    packument.insert("time".into(), Value::Object(time));
    packument
}

/// Lee el packument guardado.
pub fn parse_packument(bytes: &[u8]) -> Result<Map<String, Value>, NpmError> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(object)) => Ok(object),
        _ => Err(NpmError::InvalidDocument(
            "the packument is not a JSON object",
        )),
    }
}

/// Si el packument ya tiene la version.
pub fn has_version(packument: &Map<String, Value>, version: &str) -> bool {
    packument
        .get("versions")
        .and_then(Value::as_object)
        .is_some_and(|versions| versions.contains_key(version))
}

/// Anade una version publicada al packument.
///
/// Una version **no se reescribe**: si ya esta, es un error aunque el
/// contenido sea el mismo, que es lo que npm espera de un registro.
pub fn add_version(
    packument: &mut Map<String, Value>,
    new: &NewVersion,
    facts: &TarballFacts,
    origin: &Origin,
) -> Result<(), NpmError> {
    if has_version(packument, &new.version) {
        return Err(NpmError::VersionExists(new.version.clone()));
    }
    let mut manifest = new.manifest.clone();
    let mut dist = Map::new();
    dist.insert("shasum".into(), Value::String(facts.shasum.clone()));
    dist.insert("integrity".into(), Value::String(facts.integrity.clone()));
    // La URL absoluta depende de quien pregunta: se pone al servir.
    dist.insert(
        "tarball".into(),
        Value::String(new.name.tarball_path(&new.version)),
    );
    manifest.insert("dist".into(), Value::Object(dist));
    manifest.insert(
        "_id".into(),
        Value::String(format!("{}@{}", new.name.full(), new.version)),
    );
    // El origen lo pone el servicio y no el cliente: lo que el cliente
    // diga de si mismo no es una procedencia.
    let mut hodei = Map::new();
    hodei.insert(
        "publishedBy".into(),
        Value::String(origin.publisher.clone()),
    );
    hodei.insert("publishedAt".into(), Value::String(origin.at.clone()));
    hodei.insert(
        "correlation".into(),
        Value::String(origin.correlation.clone()),
    );
    manifest.insert("_hodei".into(), Value::Object(hodei));
    versions_of(packument).insert(new.version.clone(), Value::Object(manifest));

    let tags = object_entry(packument, "dist-tags");
    for tag in &new.tags {
        tags.insert(tag.clone(), Value::String(new.version.clone()));
    }
    // La descripcion del paquete es la de lo ultimo que se llama `latest`.
    if new.tags.iter().any(|tag| tag == LATEST) {
        for field in [
            "description",
            "readme",
            "keywords",
            "license",
            "homepage",
            "repository",
            "bugs",
            "author",
        ] {
            if let Some(value) = new.document.value.get(field) {
                packument.insert(field.to_string(), value.clone());
            }
        }
    }
    let time = object_entry(packument, "time");
    time.insert("modified".into(), Value::String(origin.at.clone()));
    time.insert(new.version.clone(), Value::String(origin.at.clone()));
    Ok(())
}

/// Mueve (o crea) un dist-tag.
///
/// El destino tiene que ser una version que existe: un tag que apunta a
/// nada rompe `npm install pkg@tag`.
pub fn set_dist_tag(
    packument: &mut Map<String, Value>,
    tag: &str,
    version: &str,
    at: &str,
) -> Result<bool, NpmError> {
    valid_tag(tag)?;
    if !has_version(packument, version) {
        return Err(NpmError::UnknownVersion(version.to_string()));
    }
    let tags = object_entry(packument, "dist-tags");
    if tags.get(tag).and_then(Value::as_str) == Some(version) {
        return Ok(false);
    }
    tags.insert(tag.to_string(), Value::String(version.to_string()));
    object_entry(packument, "time").insert("modified".into(), Value::String(at.to_string()));
    Ok(true)
}

/// Quita un dist-tag. `latest` no se quita: es lo que resuelve
/// `npm install pkg`.
pub fn remove_dist_tag(
    packument: &mut Map<String, Value>,
    tag: &str,
    at: &str,
) -> Result<bool, NpmError> {
    valid_tag(tag)?;
    if tag == LATEST {
        return Err(NpmError::InvalidTag(
            "the latest dist-tag cannot be removed",
        ));
    }
    let tags = object_entry(packument, "dist-tags");
    if tags.remove(tag).is_none() {
        return Ok(false);
    }
    object_entry(packument, "time").insert("modified".into(), Value::String(at.to_string()));
    Ok(true)
}

/// Aplica a un packument lo que dice un documento sin tarball.
///
/// Solo se admite cambiar `deprecated` de versiones que ya existen. Quitar
/// una version del documento es un unpublish, y anadir una sin tarball no
/// tiene bytes que servir: los dos se rechazan.
pub fn apply_metadata(
    packument: &mut Map<String, Value>,
    document: &Document,
    at: &str,
) -> Result<bool, NpmError> {
    let incoming = document
        .value
        .get("versions")
        .and_then(Value::as_object)
        .ok_or(NpmError::InvalidDocument("the document has no versions"))?;
    let known: BTreeSet<String> = packument
        .get("versions")
        .and_then(Value::as_object)
        .map(|versions| versions.keys().cloned().collect())
        .unwrap_or_default();
    if known.iter().any(|version| !incoming.contains_key(version)) {
        return Err(NpmError::Unpublish);
    }
    if incoming.keys().any(|version| !known.contains(version)) {
        return Err(NpmError::InvalidDocument(
            "a version cannot be added without its tarball",
        ));
    }
    let mut changed = false;
    for (version, entry) in incoming {
        let message = entry
            .get("deprecated")
            .and_then(Value::as_str)
            .unwrap_or("");
        let current = packument
            .get("versions")
            .and_then(Value::as_object)
            .and_then(|versions| versions.get(version))
            .and_then(|entry| entry.get("deprecated"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if message == current {
            continue;
        }
        if message.len() > 4096 {
            return Err(NpmError::InvalidDocument(
                "the deprecation message is too long",
            ));
        }
        let Some(Value::Object(target)) = versions_of(packument).get_mut(version) else {
            continue;
        };
        if message.is_empty() {
            target.remove("deprecated");
        } else {
            target.insert("deprecated".into(), Value::String(message.to_string()));
        }
        changed = true;
    }
    if changed {
        object_entry(packument, "time").insert("modified".into(), Value::String(at.to_string()));
    }
    Ok(changed)
}

/// El packument tal y como se sirve: cada `dist.tarball` apunta a **este**
/// repositorio, y no al upstream ni a la ruta relativa que se guarda.
///
/// `base` es `{esquema}://{host}/npm/{repositorio}` sin barra final y `name`
/// el nombre que pidio el cliente: nunca el del documento, que en un proxy lo
/// escribe el upstream y no es de fiar para construir una ruta.
pub fn rewrite_tarballs(packument: &mut Map<String, Value>, name: &str, base: &str) {
    let Some(versions) = packument.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for entry in versions.values_mut() {
        let Some(dist) = entry.get_mut("dist").and_then(Value::as_object_mut) else {
            continue;
        };
        let file = dist
            .get("tarball")
            .and_then(Value::as_str)
            .and_then(|url| url.rsplit('/').next())
            .filter(|file| tarball_file_ok(file));
        if let Some(file) = file {
            let url = format!("{base}/{name}/-/{file}");
            dist.insert("tarball".into(), Value::String(url));
        }
    }
}

/// Los dist-tags de un packument.
pub fn dist_tags(packument: &Map<String, Value>) -> Value {
    packument
        .get("dist-tags")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()))
}

/// Si la ruta de un asset es la de un tarball: el contenido inmutable de
/// un repositorio npm. Todo lo demas es un packument, que se republica.
pub fn is_tarball_path(asset_path: &str) -> bool {
    let mut segments = asset_path.rsplit('/');
    let (Some(file), Some(dash)) = (segments.next(), segments.next()) else {
        return false;
    };
    dash == "-" && file.ends_with(".tgz")
}

/// La configuracion de un repositorio npm.
///
/// Vive bajo la clave `npm` de los ajustes del repositorio:
///
/// ```json
/// { "npm": { "scopes": ["@acme"] } }
/// ```
///
/// Sin `scopes`, un hosted admite cualquier paquete. Con `scopes`, solo los
/// que cuelgan de uno de esos alcances: es la forma npm de separar quien
/// puede publicar que (un registro y un token por alcance).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpmConfig {
    /// Los alcances que el repositorio admite, con su arroba.
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl NpmConfig {
    /// La configuracion de unos ajustes de repositorio.
    ///
    /// Sin clave `npm`, la de por defecto; una clave que no se entiende es un
    /// error, nunca la de por defecto.
    pub fn of_settings(settings: &Value) -> Result<Self, String> {
        let Some(value) = settings.get("npm") else {
            return Ok(Self::default());
        };
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|error| format!("the npm settings cannot be read: {error}"))?;
        for scope in &config.scopes {
            let body = scope.strip_prefix('@').unwrap_or("");
            let valid = !body.is_empty()
                && body.len() <= MAX_NAME_LEN
                && body.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'-' | b'.' | b'_' | b'~')
                });
            if !valid {
                return Err(format!(
                    "the npm scope {scope:?} is not a valid scope (use @name)"
                ));
            }
        }
        Ok(config)
    }

    /// Si el repositorio admite publicar ese paquete.
    pub fn admits(&self, name: &PackageName) -> bool {
        if self.scopes.is_empty() {
            return true;
        }
        match name.scope() {
            Some(scope) => self
                .scopes
                .iter()
                .any(|allowed| allowed.trim_start_matches('@') == scope.trim_start_matches('@')),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn name(text: &str) -> PackageName {
        PackageName::parse(text, true).expect("a valid name")
    }

    fn b64(text: &str) -> Option<Vec<u8>> {
        // Un codec de pruebas: el servicio inyecta el de verdad.
        Some(text.as_bytes().to_vec())
    }

    #[test]
    fn names_follow_the_npm_rules() {
        assert_eq!(name("lodash").full(), "lodash");
        let scoped = name("@acme/widget");
        assert_eq!(scoped.scope(), Some("@acme"));
        assert_eq!(scoped.base(), "widget");
        assert_eq!(
            scoped.tarball_path("1.0.0"),
            "@acme/widget/-/widget-1.0.0.tgz"
        );
        for bad in [
            "",
            "UPPER",
            ".hidden",
            "_hidden",
            "has space",
            "a/b",
            "@acme",
            "@/x",
            "@acme/",
            "node_modules",
            "a%20b",
        ] {
            assert!(
                PackageName::parse(bad, true).is_err(),
                "{bad:?} must be refused"
            );
        }
        // Un proxy admite lo que el upstream ya publico: nombres antiguos.
        assert!(PackageName::parse("JSONStream", false).is_ok());
        assert!(PackageName::parse(&"a".repeat(215), false).is_err());
    }

    #[test]
    fn routes_read_every_form_the_npm_cli_sends() {
        assert_eq!(route("-/ping").unwrap(), Route::Ping);
        assert_eq!(route("-/whoami").unwrap(), Route::Whoami);
        let Route::Packument { name } = route("@acme%2fwidget").unwrap() else {
            panic!("a packument");
        };
        assert_eq!(name.full(), "@acme/widget");
        let Route::Packument { name } = route("@acme/widget").unwrap() else {
            panic!("a packument");
        };
        assert_eq!(name.full(), "@acme/widget");
        let Route::Tarball { name, file } = route("@acme/widget/-/widget-1.0.0.tgz").unwrap()
        else {
            panic!("a tarball");
        };
        assert_eq!(
            (name.full(), file.as_str()),
            ("@acme/widget", "widget-1.0.0.tgz")
        );
        assert!(matches!(
            route("-/package/@acme%2fwidget/dist-tags").unwrap(),
            Route::DistTags { .. }
        ));
        let Route::DistTag { tag, .. } = route("-/package/lodash/dist-tags/beta").unwrap() else {
            panic!("a dist-tag");
        };
        assert_eq!(tag, "beta");
        assert!(matches!(
            route("lodash/-rev/3-abc").unwrap(),
            Route::UnpublishPackage { .. }
        ));
        assert!(matches!(
            route("lodash/-/lodash-1.0.0.tgz/-rev/3-abc").unwrap(),
            Route::UnpublishTarball { .. }
        ));
    }

    #[test]
    fn routes_refuse_traversal_escapes_and_unknown_endpoints() {
        for bad in [
            "",
            "../x",
            "a/../b",
            "a//b",
            "lodash/%2e%2e",
            "lodash%00",
            "lodash\\x",
            "lodash/-/../x.tgz",
            "lodash/-/x.txt",
            "-/user/org.couchdb.user:x",
            "-/package/lodash/dist-tags/1.0.0",
            "lodash/other",
        ] {
            assert!(route(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn dist_tags_cannot_look_like_versions() {
        assert!(valid_tag("beta").is_ok());
        assert!(valid_tag("next-2").is_ok());
        for bad in ["", "1.0.0", "^1.0", "has space", "a/b", &"x".repeat(65)] {
            assert!(valid_tag(bad).is_err(), "{bad:?}");
        }
    }

    fn publish_doc(version: &str, tarball: &str) -> Vec<u8> {
        let file = format!("widget-{version}.tgz");
        serde_json::to_vec(&json!({
            "_id": "widget",
            "name": "widget",
            "description": "a widget",
            "dist-tags": {"latest": version},
            "versions": {version: {"name": "widget", "version": version, "description": "a widget"}},
            "_attachments": {file: {"content_type": "application/octet-stream", "data": tarball, "length": tarball.len()}},
        }))
        .unwrap()
    }

    fn facts() -> TarballFacts {
        TarballFacts {
            shasum: "ab".repeat(20),
            integrity: "sha512-AAAA".into(),
        }
    }

    fn origin() -> Origin {
        Origin {
            publisher: "alice".into(),
            at: "2026-01-01T00:00:00Z".into(),
            correlation: "c-1".into(),
        }
    }

    #[test]
    fn a_publication_needs_one_version_one_tarball_and_matching_names() {
        let widget = name("widget");
        let Update::Publish(new) =
            parse_update(&widget, &publish_doc("1.0.0", "tarball"), &b64).unwrap()
        else {
            panic!("a publication");
        };
        assert_eq!(new.version, "1.0.0");
        assert_eq!(new.tags, vec!["latest".to_string()]);
        assert_eq!(new.attachment.bytes, b"tarball");

        // Otro paquete que el de la ruta.
        assert!(parse_update(&name("other"), &publish_doc("1.0.0", "t"), &b64).is_err());
        // Un adjunto que no se llama como la version.
        let mut doc: Value = serde_json::from_slice(&publish_doc("1.0.0", "t")).unwrap();
        doc["_attachments"] = json!({"widget-9.9.9.tgz": {"data": "t"}});
        assert!(parse_update(&widget, &serde_json::to_vec(&doc).unwrap(), &b64).is_err());
        // Una version que no es semver.
        assert!(parse_update(&widget, &publish_doc("latest", "t"), &b64).is_err());
        // Un tag que apunta a otra version.
        let mut doc: Value = serde_json::from_slice(&publish_doc("1.0.0", "t")).unwrap();
        doc["dist-tags"] = json!({"latest": "0.0.1"});
        assert!(parse_update(&widget, &serde_json::to_vec(&doc).unwrap(), &b64).is_err());
        // Un adjunto vacio y un cuerpo que no es JSON.
        assert!(parse_update(&widget, &publish_doc("1.0.0", ""), &b64).is_err());
        assert!(parse_update(&widget, b"not json", &b64).is_err());
    }

    #[test]
    fn a_scoped_publication_names_its_tarball_as_libnpmpublish_does() {
        let scoped = name("@acme/widget");
        let document = |file: &str| {
            serde_json::to_vec(&json!({
                "name": "@acme/widget",
                "versions": {"1.0.0": {"name": "@acme/widget", "version": "1.0.0"}},
                "_attachments": {file: {"data": "tarball"}},
            }))
            .unwrap()
        };
        // El cliente real manda el nombre completo; el registro guarda la base.
        for file in ["@acme/widget-1.0.0.tgz", "widget-1.0.0.tgz"] {
            let Update::Publish(new) = parse_update(&scoped, &document(file), &b64).unwrap() else {
                panic!("a publication");
            };
            assert_eq!(new.attachment.file, "widget-1.0.0.tgz");
        }
        assert!(parse_update(&scoped, &document("@acme/widget-9.9.9.tgz"), &b64).is_err());
    }

    #[test]
    fn the_declared_integrity_has_to_be_the_computed_one() {
        let mut manifest = Map::new();
        assert!(check_declared_dist(&manifest, &facts()).is_ok());
        manifest.insert(
            "dist".into(),
            json!({"shasum": "ab".repeat(20), "integrity": "sha512-AAAA"}),
        );
        assert!(check_declared_dist(&manifest, &facts()).is_ok());
        manifest.insert("dist".into(), json!({"integrity": "sha512-BBBB"}));
        assert!(check_declared_dist(&manifest, &facts()).is_err());
        manifest.insert("dist".into(), json!({"shasum": "00"}));
        assert!(check_declared_dist(&manifest, &facts()).is_err());
    }

    fn published(version: &str) -> (Map<String, Value>, NewVersion) {
        let widget = name("widget");
        let Update::Publish(new) =
            parse_update(&widget, &publish_doc(version, "tarball"), &b64).unwrap()
        else {
            panic!("a publication");
        };
        (empty_packument(&widget, "2026-01-01T00:00:00Z"), *new)
    }

    #[test]
    fn publishing_adds_the_version_with_our_integrity_and_origin() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        let entry = &packument["versions"]["1.0.0"];
        assert_eq!(entry["dist"]["integrity"], "sha512-AAAA");
        assert_eq!(entry["dist"]["tarball"], "widget/-/widget-1.0.0.tgz");
        assert_eq!(entry["_hodei"]["publishedBy"], "alice");
        assert_eq!(packument["dist-tags"]["latest"], "1.0.0");
        assert_eq!(packument["description"], "a widget");
        assert_eq!(packument["time"]["1.0.0"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn a_published_version_is_never_overwritten() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        let again = add_version(&mut packument, &new, &facts(), &origin());
        assert_eq!(again, Err(NpmError::VersionExists("1.0.0".into())));
        assert!(again.unwrap_err().message().contains("cannot publish over"));
    }

    #[test]
    fn dist_tags_move_only_to_versions_that_exist_and_latest_stays() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        assert!(set_dist_tag(&mut packument, "beta", "1.0.0", "t").unwrap());
        assert!(!set_dist_tag(&mut packument, "beta", "1.0.0", "t").unwrap());
        assert_eq!(
            set_dist_tag(&mut packument, "beta", "9.9.9", "t"),
            Err(NpmError::UnknownVersion("9.9.9".into()))
        );
        assert!(set_dist_tag(&mut packument, "1.0.0", "1.0.0", "t").is_err());
        assert!(remove_dist_tag(&mut packument, "beta", "t").unwrap());
        assert!(!remove_dist_tag(&mut packument, "beta", "t").unwrap());
        assert!(remove_dist_tag(&mut packument, "latest", "t").is_err());
    }

    fn metadata(versions: Value) -> Document {
        Document {
            name: name("widget"),
            value: json!({"name": "widget", "versions": versions})
                .as_object()
                .unwrap()
                .clone(),
        }
    }

    #[test]
    fn deprecating_sets_and_clears_a_message_without_touching_the_rest() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        let deprecate = metadata(json!({"1.0.0": {"deprecated": "use 2.x"}}));
        assert!(apply_metadata(&mut packument, &deprecate, "t").unwrap());
        assert_eq!(packument["versions"]["1.0.0"]["deprecated"], "use 2.x");
        assert_eq!(
            packument["versions"]["1.0.0"]["dist"]["integrity"],
            "sha512-AAAA"
        );
        assert!(!apply_metadata(&mut packument, &deprecate, "t").unwrap());
        let clear = metadata(json!({"1.0.0": {"deprecated": ""}}));
        assert!(apply_metadata(&mut packument, &clear, "t").unwrap());
        assert!(packument["versions"]["1.0.0"].get("deprecated").is_none());
    }

    #[test]
    fn a_document_that_drops_a_version_is_an_unpublish_and_is_refused() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        assert_eq!(
            apply_metadata(&mut packument, &metadata(json!({})), "t"),
            Err(NpmError::Unpublish)
        );
        // Anadir una version sin tarball tampoco se admite.
        let extra = metadata(json!({"1.0.0": {}, "2.0.0": {}}));
        assert!(matches!(
            apply_metadata(&mut packument, &extra, "t"),
            Err(NpmError::InvalidDocument(_))
        ));
        assert!(has_version(&packument, "1.0.0"));
    }

    #[test]
    fn serving_points_every_tarball_at_this_repository() {
        let (mut packument, new) = published("1.0.0");
        add_version(&mut packument, &new, &facts(), &origin()).unwrap();
        rewrite_tarballs(
            &mut packument,
            "widget",
            "http://127.0.0.1:8080/npm/npm-all",
        );
        assert_eq!(
            packument["versions"]["1.0.0"]["dist"]["tarball"],
            "http://127.0.0.1:8080/npm/npm-all/widget/-/widget-1.0.0.tgz"
        );
        // Un packument de un upstream: su URL se sustituye por la nuestra.
        let mut upstream = json!({
            "name": "@acme/lib",
            "versions": {"2.0.0": {"dist": {"tarball": "https://registry.npmjs.org/@acme/lib/-/lib-2.0.0.tgz"}}}
        })
        .as_object()
        .unwrap()
        .clone();
        rewrite_tarballs(&mut upstream, "@acme/lib", "http://hodei/npm/proxy");
        // El nombre del documento no decide la ruta.
        let mut hostile = json!({
            "name": "a/../../otro",
            "versions": {"2.0.0": {"dist": {"tarball": "https://x/y/-/lib-2.0.0.tgz"}}}
        })
        .as_object()
        .unwrap()
        .clone();
        rewrite_tarballs(&mut hostile, "@acme/lib", "http://hodei/npm/proxy");
        assert_eq!(
            hostile["versions"]["2.0.0"]["dist"]["tarball"],
            "http://hodei/npm/proxy/@acme/lib/-/lib-2.0.0.tgz"
        );
        assert_eq!(
            upstream["versions"]["2.0.0"]["dist"]["tarball"],
            "http://hodei/npm/proxy/@acme/lib/-/lib-2.0.0.tgz"
        );
    }

    #[test]
    fn only_tarballs_are_immutable_content() {
        assert!(is_tarball_path("lodash/-/lodash-4.17.21.tgz"));
        assert!(is_tarball_path("@acme/lib/-/lib-1.0.0.tgz"));
        assert!(!is_tarball_path("lodash"));
        assert!(!is_tarball_path("@acme/lib"));
        assert!(!is_tarball_path("lodash/-/lodash-4.17.21.json"));
    }

    #[test]
    fn the_scope_policy_restricts_what_a_repository_admits() {
        use serde_json::json;
        let open = NpmConfig::of_settings(&json!({})).unwrap();
        let scoped = NpmConfig::of_settings(&json!({"npm": {"scopes": ["@acme"]}})).unwrap();
        let acme = PackageName::parse("@acme/tool", true).unwrap();
        let other = PackageName::parse("@other/tool", true).unwrap();
        let plain = PackageName::parse("left-pad", true).unwrap();
        assert!(open.admits(&plain) && open.admits(&other));
        assert!(scoped.admits(&acme));
        assert!(!scoped.admits(&other));
        assert!(!scoped.admits(&plain));
    }

    #[test]
    fn a_malformed_npm_setting_is_an_error_not_the_default() {
        use serde_json::json;
        for settings in [
            json!({"npm": {"scopes": ["acme"]}}),
            json!({"npm": {"scopes": ["@"]}}),
            json!({"npm": {"surprise": true}}),
            json!({"npm": 3}),
        ] {
            assert!(NpmConfig::of_settings(&settings).is_err(), "{settings}");
        }
    }
}
