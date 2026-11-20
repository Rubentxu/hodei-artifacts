//! La gramatica del **module registry protocol v1** de Terraform/OpenTofu.
//!
//! Un registry de modulos privado habla tres lecturas y una escritura
//! propia del hub, todos colgando de la superficie de un repositorio
//! **hosted** del formato Terraform:
//!
//! 1. Service discovery: el hub responde en `/.well-known/terraform.json`
//!    `{"modules.v1": "<base del protocolo>"}`. El cliente pide siempre
//!    `https://{host}/.well-known/terraform.json` (el esquema lo fija
//!    `terraform-svchost/disco`), y un host sin TLS declara el servicio
//!    directamente en su config de CLI (`host` + `services`), que es el
//!    `ForceHostServices` del disco.
//! 2. `GET {base}/{namespace}/{name}/{system}/versions`: el indice de
//!    versiones, paginable; el cliente solo lee `modules[0].versions[]`.
//! 3. `GET {base}/{namespace}/{name}/{system}/{version}/download`: una
//!    respuesta **sin cuerpo** cuyo `X-Terraform-Get` es una fuente
//!    go-getter. Si el valor empieza por `/`, `./` o `../` el cliente lo
//!    resuelve contra la URL que pidió, y la descarga del artefacto la
//!    hace go-getter, que **no** lleva las credenciales del registry: el
//!    artefacto se autentica con netrc (Basic) o con el mismo Bearer.
//! 4. `POST publish/{...}/{version}`: la publicacion la define el hub
//!    (el CLI no sabe publicar modulos): un tarball completo por version,
//!    inmutable, con el sha256 calculado al commit en el recibo.
//!
//! Aqui vive solo la gramatica: que rutas existen y que segmentos son
//! legitimamente un namespace, un nombre, un sistema, una version o un
//! nombre de fichero. Como en el mirror, una ruta llega **sin decodificar**
//! y un segmento con escape por porcentaje, separador de Windows, vacio o
//! `..` es un intento de traversal, y se rechaza antes de tocar el
//! catalogo.

use serde_json::{Value, json};

/// Lo que puede salir mal al leer una ruta del registry de modulos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleRegistryError {
    /// La ruta no pertenece al protocolo del registry.
    InvalidPath(String),
    /// El namespace, el nombre o el sistema no son validos.
    InvalidCoordinates(String),
    /// La version no es semver estricto.
    InvalidVersion(String),
}

impl ModuleRegistryError {
    /// El mensaje que explica el rechazo.
    pub fn message(&self) -> String {
        match self {
            Self::InvalidPath(path) => {
                format!("the path `{path}` is not a module registry protocol path")
            }
            Self::InvalidCoordinates(name) => {
                format!("the module coordinates `{name}` are not valid")
            }
            Self::InvalidVersion(version) => {
                format!("the version `{version}` is not strict semver")
            }
        }
    }

    /// Si el rechazo es de la ruta y no de quien la pide: una ruta que el
    /// protocolo no conoce es un recurso que no existe.
    pub fn is_path_error(&self) -> bool {
        matches!(self, Self::InvalidPath(_))
    }
}

impl std::fmt::Display for ModuleRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ModuleRegistryError {}

/// Los prefijos que este protocolo reclama sobre la superficie de un
/// repositorio Terraform. Un repositorio **hosted** habla registry y un
/// **proxy** habla mirror: la modalidad decide el dialecto, y el prefijo
/// solo separa las rutas del registry de las del mirror.
pub const REGISTRY_PREFIXES: [&str; 3] = ["v1/modules/", "publish/", "artifacts/"];

/// Si el resto de la ruta de un repositorio es una ruta del registry de
/// modulos y no del mirror de providers.
pub fn is_registry_path(rest: &str) -> bool {
    REGISTRY_PREFIXES
        .iter()
        .any(|prefix| rest.starts_with(prefix))
}

/// La direccion de un modulo dentro del registry: `namespace/name/system`.
///
/// El cliente la construye a partir de la fuente `host/namespace/name/
/// system` y pide por los tres segmentos finales; el hub los guarda tal
/// cual publica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRef {
    /// El namespace del modulo.
    pub namespace: String,
    /// El nombre del modulo.
    pub name: String,
    /// El sistema destino (`local`, `aws`, `kubernetes`, ...).
    pub system: String,
}

impl ModuleRef {
    /// La fuente que el cliente ve, sin el host: la que el documento de
    /// versiones declara como `source`.
    pub fn source(&self) -> String {
        format!("{}/{}/{}", self.namespace, self.name, self.system)
    }

    /// El nombre del paquete en el catalogo.
    pub fn package_name(&self) -> String {
        self.source()
    }

    /// El camino del artefacto de una version en el CAS: un tarball
    /// versionado e inmutable.
    pub fn artifact_path(&self, version: &str) -> String {
        format!(
            "artifacts/{}/{}/{}/{}.tar.gz",
            self.namespace, self.name, self.system, version
        )
    }
}

/// Una ruta del registry de modulos, ya validada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// El indice de versiones de un modulo.
    Versions {
        /// La direccion del modulo.
        reference: ModuleRef,
    },
    /// La descarga de una version: la respuesta lleva el `X-Terraform-Get`.
    Download {
        /// La direccion del modulo.
        reference: ModuleRef,
        /// La version pedida.
        version: String,
    },
    /// El tarball de una version.
    Artifact {
        /// La direccion del modulo.
        reference: ModuleRef,
        /// La version pedida.
        version: String,
    },
    /// La publicacion del hub: un tarball completo para una version nueva.
    Publish {
        /// La direccion del modulo.
        reference: ModuleRef,
        /// La version que se publica.
        version: String,
    },
}

impl Route {
    /// La ruta que dice su propio nombre.
    ///
    /// La ruta llega **sin decodificar**: las reglas del protocolo son las
    /// unicas que interpretan un `%`, y ahi no hay nada que interpretar.
    /// Un segmento vacio, un `..`, una barra invertida o un escape por
    /// porcentaje son un traversal, no una ruta.
    pub fn parse(rest: &str) -> Result<Self, ModuleRegistryError> {
        if rest.contains('%') || rest.contains('\\') {
            return Err(ModuleRegistryError::InvalidPath(rest.to_string()));
        }
        let segments: Vec<&str> = rest.split('/').collect();
        if segments.iter().any(|segment| segment.is_empty()) {
            return Err(ModuleRegistryError::InvalidPath(rest.to_string()));
        }
        match segments.as_slice() {
            ["v1", "modules", namespace, name, system, "versions"] => {
                let reference = the_coordinates(namespace, name, system)?;
                Ok(Self::Versions { reference })
            }
            [
                "v1",
                "modules",
                namespace,
                name,
                system,
                version,
                "download",
            ] => {
                let reference = the_coordinates(namespace, name, system)?;
                valid_version(version)?;
                Ok(Self::Download {
                    reference,
                    version: version.to_string(),
                })
            }
            ["publish", namespace, name, system, version] => {
                let reference = the_coordinates(namespace, name, system)?;
                valid_version(version)?;
                Ok(Self::Publish {
                    reference,
                    version: version.to_string(),
                })
            }
            ["artifacts", namespace, name, system, file] => {
                let reference = the_coordinates(namespace, name, system)?;
                let Some(version) = file.strip_suffix(".tar.gz") else {
                    return Err(ModuleRegistryError::InvalidPath(rest.to_string()));
                };
                valid_version(version)?;
                Ok(Self::Artifact {
                    reference,
                    version: version.to_string(),
                })
            }
            _ => Err(ModuleRegistryError::InvalidPath(rest.to_string())),
        }
    }

    /// La version que la ruta nombra, si la nombra.
    pub fn version(&self) -> Option<&str> {
        match self {
            Self::Versions { .. } => None,
            Self::Download { version, .. }
            | Self::Artifact { version, .. }
            | Self::Publish { version, .. } => Some(version),
        }
    }
}

/// Un namespace, un nombre o un sistema: minusculas, digitos, guiones y
/// guiones bajos, sin borde de guion. Las mismas reglas del mirror, que
/// son las que el cliente usa para sus fuentes.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.ends_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn the_coordinates(
    namespace: &str,
    name: &str,
    system: &str,
) -> Result<ModuleRef, ModuleRegistryError> {
    if !valid_name(namespace) || !valid_name(name) || !valid_name(system) {
        return Err(ModuleRegistryError::InvalidCoordinates(format!(
            "{namespace}/{name}/{system}"
        )));
    }
    Ok(ModuleRef {
        namespace: namespace.to_string(),
        name: name.to_string(),
        system: system.to_string(),
    })
}

/// La version: semver estricto, sin `v` delante y sin espacios. Las
/// mismas reglas del mirror de providers.
fn valid_version(version: &str) -> Result<(), ModuleRegistryError> {
    let core = |text: &str| {
        let numbers: Vec<&str> = text.split('.').collect();
        numbers.len() == 3
            && numbers
                .iter()
                .all(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
    };
    let identifiers = |text: &str| {
        text.split('.')
            .all(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    };
    let (release, _) = match version.split_once(['-', '+']) {
        Some((release, rest)) => (release, rest),
        None => (version, ""),
    };
    let ok = core(release)
        && match version.split_once('+') {
            Some((_, build)) => identifiers(build),
            None => true,
        }
        && match version.split_once('-') {
            Some((_, rest)) => {
                let pre = rest.split('+').next().unwrap_or_default();
                identifiers(pre)
            }
            None => true,
        };
    if ok {
        Ok(())
    } else {
        Err(ModuleRegistryError::InvalidVersion(version.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Documentos
// ---------------------------------------------------------------------------

/// El documento de service discovery. Esta release **no declara
/// `providers.v1`**: el registry de providers de origen no se implementa
/// (AC3 no implementado), y un servicio que no se sabe servir no se
/// anuncia.
pub fn discovery_document(module_base: &str) -> Value {
    json!({ "modules.v1": module_base })
}

/// El documento de versiones de un modulo. El cliente lee
/// `modules[0].versions[].version` y ordena por semver; `root` va vacio
/// porque las dependencias de un modulo de registry las declara su
/// configuracion, no el indice.
pub fn versions_document(reference: &ModuleRef, versions: &[&str]) -> Value {
    let versions: Vec<Value> = versions
        .iter()
        .map(|version| {
            json!({
                "version": version,
                "root": { "providers": [], "dependencies": [] },
                "submodules": [],
            })
        })
        .collect();
    json!({
        "meta": {
            "limit": 100,
            "current_offset": 0,
            "next_offset": null,
            "prev_offset": null,
        },
        "modules": [
            {
                "source": reference.source(),
                "versions": versions,
            }
        ],
    })
}

/// La fuente go-getter de la descarga de una version: el camino del
/// artefacto en esta superficie, **relativo** (el cliente lo resuelve
/// contra la URL que pidio), con el checksum que go-getter verifica al
/// bajar. El `.tar.gz` del final es lo que hace que go-getter trate la
/// descarga como fichero + descompresion, que es la unica forma en que
/// verifica checksums (`checksum cannot be specified for directory
/// download` si el camino no acaba en un archivo conocido).
pub fn download_location(
    repository_key: &str,
    reference: &ModuleRef,
    version: &str,
    sha256: &str,
) -> String {
    format!(
        "/terraform/{repository_key}/{}?checksum=sha256:{sha256}",
        reference.artifact_path(version)
    )
}

/// El recibo de una publicacion: la identidad de la version y el sha256
/// que el hub calculo al commit, el mismo que luego declara la descarga.
pub fn publish_receipt(
    reference: &ModuleRef,
    version: &str,
    sha256: &str,
    size_bytes: u64,
) -> Value {
    json!({
        "namespace": reference.namespace,
        "name": reference.name,
        "system": reference.system,
        "version": version,
        "sha256": sha256,
        "size_bytes": size_bytes,
        "path": reference.artifact_path(version),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> ModuleRef {
        ModuleRef {
            namespace: "hodei".to_string(),
            name: "demo".to_string(),
            system: "local".to_string(),
        }
    }

    #[test]
    fn the_protocol_paths_parse() {
        assert_eq!(
            Route::parse("v1/modules/hodei/demo/local/versions"),
            Ok(Route::Versions {
                reference: reference()
            })
        );
        assert_eq!(
            Route::parse("v1/modules/hodei/demo/local/1.0.0/download"),
            Ok(Route::Download {
                reference: reference(),
                version: "1.0.0".to_string(),
            })
        );
        assert_eq!(
            Route::parse("artifacts/hodei/demo/local/1.0.0.tar.gz"),
            Ok(Route::Artifact {
                reference: reference(),
                version: "1.0.0".to_string(),
            })
        );
        assert_eq!(
            Route::parse("publish/hodei/demo/local/1.0.0"),
            Ok(Route::Publish {
                reference: reference(),
                version: "1.0.0".to_string(),
            })
        );
    }

    #[test]
    fn the_registry_prefixes_are_ours() {
        assert!(is_registry_path("v1/modules/hodei/demo/local/versions"));
        assert!(is_registry_path("publish/hodei/demo/local/1.0.0"));
        assert!(is_registry_path("artifacts/hodei/demo/local/1.0.0.tar.gz"));
        assert!(!is_registry_path("index.json"));
        assert!(!is_registry_path(
            "registry.terraform.io/hashicorp/random/index.json"
        ));
    }

    #[test]
    fn a_traversal_is_not_a_path() {
        for hostile in [
            "v1/modules/hodei/../demo/local/versions",
            "v1/modules//demo/local/versions",
            "v1/modules/hodei/demo/local/1.0.0/../download",
            "artifacts/hodei/demo/local/..%2f1.0.0.tar.gz",
            "publish/hodei\\demo/local/1.0.0",
            "v1/modules/hodei/demo/local/1.0.0/download/",
            "artifacts/hodei/demo/local/1.0.0.zip",
            "",
            "v1/modules/hodei/demo/local",
        ] {
            let _ = Route::parse(hostile).expect_err("a hostile path is refused");
        }
    }

    #[test]
    fn a_bad_coordinate_or_version_is_refused() {
        assert!(Route::parse("v1/modules/Hodei/demo/local/versions").is_err());
        assert!(Route::parse("v1/modules/-hodei/demo/local/versions").is_err());
        assert!(Route::parse("v1/modules/hodei/demo/local/v1/download").is_err());
        assert!(Route::parse("v1/modules/hodei/demo/local/1.0/download").is_err());
        assert!(Route::parse("publish/hodei/demo/local/1.0.0-.0").is_err());
    }

    #[test]
    fn a_prerelease_version_is_a_version() {
        assert!(Route::parse("publish/hodei/demo/local/1.0.0-rc.1").is_ok());
    }

    #[test]
    fn the_documents_say_what_the_client_reads() {
        let discovery = discovery_document("/terraform/modules-hosted/v1/modules/");
        assert_eq!(
            discovery["modules.v1"],
            "/terraform/modules-hosted/v1/modules/"
        );
        assert!(
            discovery.get("providers.v1").is_none(),
            "AC3: no providers declared"
        );

        let versions = versions_document(&reference(), &["1.0.0", "2.0.0"]);
        assert_eq!(versions["modules"][0]["source"], "hodei/demo/local");
        assert_eq!(versions["modules"][0]["versions"][0]["version"], "1.0.0");

        let location = download_location("modules-hosted", &reference(), "1.0.0", "abc");
        assert_eq!(
            location,
            "/terraform/modules-hosted/artifacts/hodei/demo/local/1.0.0.tar.gz?checksum=sha256:abc"
        );

        let receipt = publish_receipt(&reference(), "1.0.0", "abc", 10);
        assert_eq!(receipt["sha256"], "abc");
        assert_eq!(receipt["path"], "artifacts/hodei/demo/local/1.0.0.tar.gz");
    }

    #[test]
    fn the_artifact_path_is_versioned_and_flat() {
        let reference = reference();
        assert_eq!(
            reference.artifact_path("1.0.0"),
            "artifacts/hodei/demo/local/1.0.0.tar.gz"
        );
        assert_eq!(reference.package_name(), "hodei/demo/local");
    }
}
