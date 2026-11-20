//! La gramatica del **network mirror protocol** de Terraform/OpenTofu.
//!
//! Un mirror de providers habla cuatro documentos, todos colgando de la
//! base que la configuracion del cliente declara:
//!
//! 1. `index.json`: el indice raiz, con los providers que el mirror
//!    ofrece y sus versiones.
//! 2. `{hostname}/{namespace}/{type}/index.json`: el indice de un
//!    provider, con sus versiones, plataformas y versiones de protocolo.
//! 3. `{hostname}/{namespace}/{type}/{version}.json`: el documento de
//!    version, con el nombre del zip de cada plataforma.
//! 4. `{hostname}/{namespace}/{type}/{filename}`: el zip del provider.
//!
//! Aqui vive solo la gramatica: que rutas existen y que segmentos son
//! legitimamente un hostname, un namespace, un provider, una version o un
//! nombre de fichero. La lectura del `terraform` escapa las rutas el solo
//! y no hay documento que este cliente reescriba, asi que cada segmento se
//! valida estricto: un `..`, un separador de Windows, un escape por
//! porcentaje o un segmento vacio no son una ruta, son un intento de
//! traversal, y se rechazan antes de tocar el catalogo o el upstream.

/// Lo que puede salir mal al leer una ruta del mirror.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerraformError {
    /// La ruta no pertenece al protocolo del mirror.
    InvalidPath(String),
    /// El hostname no es un nombre de registry legitimo.
    InvalidHostname(String),
    /// El namespace o el tipo de provider no son validos.
    InvalidProvider(String),
    /// La version no es semver estricto.
    InvalidVersion(String),
    /// El nombre del fichero no es un zip legitimo.
    InvalidFilename(String),
}

impl TerraformError {
    /// El mensaje que explica el rechazo.
    pub fn message(&self) -> String {
        match self {
            Self::InvalidPath(path) => {
                format!("the path `{path}` is not a network mirror protocol path")
            }
            Self::InvalidHostname(host) => {
                format!("the hostname `{host}` is not a valid provider registry host")
            }
            Self::InvalidProvider(name) => {
                format!("the provider namespace or type `{name}` is not valid")
            }
            Self::InvalidVersion(version) => {
                format!("the version `{version}` is not strict semver")
            }
            Self::InvalidFilename(name) => {
                format!("the filename `{name}` is not a provider archive name")
            }
        }
    }

    /// Si el rechazo es de la ruta y no de quien la pide: una ruta que el
    /// protocolo no conoce es un recurso que no existe.
    pub fn is_path_error(&self) -> bool {
        matches!(self, Self::InvalidPath(_))
    }
}

impl std::fmt::Display for TerraformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for TerraformError {}

/// Una ruta del mirror, ya validada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// El indice raiz del mirror.
    RootIndex,
    /// El indice de un provider.
    ProviderIndex {
        /// El hostname del registry de origen.
        hostname: String,
        /// El namespace del provider.
        namespace: String,
        /// El tipo del provider.
        provider: String,
    },
    /// El documento de una version.
    VersionDoc {
        /// El hostname del registry de origen.
        hostname: String,
        /// El namespace del provider.
        namespace: String,
        /// El tipo del provider.
        provider: String,
        /// La version, semver estricto.
        version: String,
    },
    /// El zip de una version para una plataforma.
    ProviderFile {
        /// El hostname del registry de origen.
        hostname: String,
        /// El namespace del provider.
        namespace: String,
        /// El tipo del provider.
        provider: String,
        /// El nombre del fichero, tal y como el upstream lo declara.
        filename: String,
    },
}

impl Route {
    /// La ruta que dice su propio nombre: el camino de asset que este
    /// mirror guarda y sirve, identico al que el protocolo usa.
    pub fn asset_path(&self) -> String {
        match self {
            Self::RootIndex => "index.json".to_string(),
            Self::ProviderIndex {
                hostname,
                namespace,
                provider,
            } => format!("{hostname}/{namespace}/{provider}/index.json"),
            Self::VersionDoc {
                hostname,
                namespace,
                provider,
                version,
            } => format!("{hostname}/{namespace}/{provider}/{version}.json"),
            Self::ProviderFile {
                hostname,
                namespace,
                provider,
                filename,
            } => format!("{hostname}/{namespace}/{provider}/{filename}"),
        }
    }

    /// Las coordenadas del provider de la ruta, si las tiene.
    pub fn provider_coordinates(&self) -> Option<(&str, &str, &str)> {
        match self {
            Self::RootIndex => None,
            Self::ProviderIndex {
                hostname,
                namespace,
                provider,
            }
            | Self::VersionDoc {
                hostname,
                namespace,
                provider,
                ..
            }
            | Self::ProviderFile {
                hostname,
                namespace,
                provider,
                ..
            } => Some((hostname.as_str(), namespace.as_str(), provider.as_str())),
        }
    }

    /// La ruta que dice su propio nombre.
    ///
    /// La ruta llega **sin decodificar**: las reglas del protocolo son las
    /// unicas que interpretan un `%`, y ahi no hay nada que interpretar.
    /// Un segmento vacio, un `..`, una barra invertida o un escape por
    /// porcentaje son un traversal, no una ruta.
    pub fn parse(rest: &str) -> Result<Self, TerraformError> {
        if rest.contains('%') || rest.contains('\\') {
            return Err(TerraformError::InvalidPath(rest.to_string()));
        }
        let segments: Vec<&str> = rest.split('/').collect();
        if segments.iter().any(|segment| segment.is_empty()) {
            return Err(TerraformError::InvalidPath(rest.to_string()));
        }
        match segments.as_slice() {
            ["index.json"] => Ok(Self::RootIndex),
            [hostname, namespace, provider, "index.json"] => {
                the_provider(hostname, namespace, provider)?;
                Ok(Self::ProviderIndex {
                    hostname: hostname.to_string(),
                    namespace: namespace.to_string(),
                    provider: provider.to_string(),
                })
            }
            [hostname, namespace, provider, version] => {
                the_provider(hostname, namespace, provider)?;
                if let Some(version) = version.strip_suffix(".json") {
                    valid_version(version)?;
                    Ok(Self::VersionDoc {
                        hostname: hostname.to_string(),
                        namespace: namespace.to_string(),
                        provider: provider.to_string(),
                        version: version.to_string(),
                    })
                } else {
                    valid_filename(version)?;
                    Ok(Self::ProviderFile {
                        hostname: hostname.to_string(),
                        namespace: namespace.to_string(),
                        provider: provider.to_string(),
                        filename: version.to_string(),
                    })
                }
            }
            _ => Err(TerraformError::InvalidPath(rest.to_string())),
        }
    }
}

/// El hostname del registry: etiquetas minusculas separadas por puntos,
/// cada una sin guiones en el borde y sin estar vacia. Un `..` no pasa:
/// la etiqueta que lo contiene no cumple la forma.
fn valid_hostname(hostname: &str) -> Result<(), TerraformError> {
    let ok = !hostname.is_empty()
        && hostname.len() <= 253
        && hostname.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.starts_with(|c: char| c.is_ascii_digit() || c.is_ascii_lowercase())
                && label.ends_with(|c: char| c.is_ascii_digit() || c.is_ascii_lowercase())
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
    if ok {
        Ok(())
    } else {
        Err(TerraformError::InvalidHostname(hostname.to_string()))
    }
}

/// Un namespace o un tipo de provider: minusculas, digitos, guiones y
/// guiones bajos, sin borde de guion.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.ends_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn the_provider(hostname: &str, namespace: &str, provider: &str) -> Result<(), TerraformError> {
    valid_hostname(hostname)?;
    if !valid_name(namespace) || !valid_name(provider) {
        return Err(TerraformError::InvalidProvider(format!(
            "{namespace}/{provider}"
        )));
    }
    Ok(())
}

/// La version: semver estricto, sin `v` delante y sin espacios. El
/// `terraform` pide versiones exactas, y una version que no es semver no
/// es una version que el protocolo declare.
fn valid_version(version: &str) -> Result<(), TerraformError> {
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
        Err(TerraformError::InvalidVersion(version.to_string()))
    }
}

/// El nombre del fichero: el que el documento de version declara para la
/// plataforma. Un zip de provider es un nombre plano, sin separadores ni
/// escapes, que termina en `.zip`.
fn valid_filename(filename: &str) -> Result<(), TerraformError> {
    let ok = filename.len() <= 255
        && filename.ends_with(".zip")
        && filename.len() > 4
        && filename.strip_suffix(".zip").is_some_and(|base| {
            !base.is_empty()
                && base
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                && !base.contains("..")
        });
    if ok {
        Ok(())
    } else {
        Err(TerraformError::InvalidFilename(filename.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_index_is_the_index() {
        assert_eq!(
            Route::parse("index.json"),
            Ok(Route::RootIndex),
            "the root index parses"
        );
        assert!(Route::parse("").unwrap_err().is_path_error());
    }

    #[test]
    fn the_provider_documents_parse() {
        assert_eq!(
            Route::parse("registry.terraform.io/hashicorp/random/index.json"),
            Ok(Route::ProviderIndex {
                hostname: "registry.terraform.io".to_string(),
                namespace: "hashicorp".to_string(),
                provider: "random".to_string(),
            })
        );
        assert_eq!(
            Route::parse("registry.terraform.io/hashicorp/random/3.4.3.json"),
            Ok(Route::VersionDoc {
                hostname: "registry.terraform.io".to_string(),
                namespace: "hashicorp".to_string(),
                provider: "random".to_string(),
                version: "3.4.3".to_string(),
            })
        );
        assert_eq!(
            Route::parse(
                "registry.terraform.io/hashicorp/random/terraform-provider-random_3.4.3_linux_amd64.zip"
            ),
            Ok(Route::ProviderFile {
                hostname: "registry.terraform.io".to_string(),
                namespace: "hashicorp".to_string(),
                provider: "random".to_string(),
                filename: "terraform-provider-random_3.4.3_linux_amd64.zip".to_string(),
            })
        );
    }

    #[test]
    fn a_traversal_is_not_a_path() {
        for hostile in [
            "index.json/../other.json",
            "registry.terraform.io/../etc/passwd",
            "registry.terraform.io/hashicorp/../random/index.json",
            "registry.terraform.io//random/index.json",
            "registry.terraform.io/hashicorp/random/",
            "",
            "a%2e%2e/index.json",
            "registry.terraform.io\\hashicorp\\random\\index.json",
            ".hidden/index.json",
            "index.json.",
        ] {
            // The refusal matters more than the reason: any error of the
            // grammar is a refusal, and none of these paths reaches the
            // catalog or the upstream.
            let _ = Route::parse(hostile).expect_err("a hostile path is refused");
        }
    }

    #[test]
    fn a_bad_hostname_or_name_is_refused() {
        assert!(Route::parse("..//x/index.json").is_err());
        assert!(Route::parse("bad_host.com/ns/type/index.json").is_err());
        assert!(Route::parse("registry.terraform.io/NS/type/index.json").is_err());
        assert!(Route::parse("registry.terraform.io/-ns/type/index.json").is_err());
        assert!(Route::parse("registry.terraform.io/ns/-/index.json").is_err());
        assert!(Route::parse("registry.terraform.io/ns/t..ype/index.json").is_err());
    }

    #[test]
    fn a_bad_version_is_refused() {
        assert!(Route::parse("registry.terraform.io/hashicorp/random/v1.json").is_err());
        assert!(Route::parse("registry.terraform.io/hashicorp/random/1.0.json").is_err());
        assert!(Route::parse("registry.terraform.io/hashicorp/random/1..0.json").is_err());
        assert!(Route::parse("registry.terraform.io/hashicorp/random/1.0.0 .json").is_err());
        assert!(Route::parse("registry.terraform.io/hashicorp/random/1.0.0-rc.1.json").is_ok());
    }

    #[test]
    fn a_bad_filename_is_refused() {
        assert!(Route::parse("registry.terraform.io/hashicorp/random/provider.tar.gz").is_err());
        assert!(Route::parse("registry.terraform.io/hashicorp/random/.zip").is_err());
        assert!(Route::parse("registry.terraform.io/hashicorp/random/evi..l.zip").is_err());
        assert!(Route::parse("registry.terraform.io/hashicorp/random/x.zip/x").is_err());
    }

    #[test]
    fn the_asset_path_is_the_protocol_path() {
        let route = Route::parse("registry.terraform.io/hashicorp/random/3.4.3.json").expect("ok");
        assert_eq!(
            route.asset_path(),
            "registry.terraform.io/hashicorp/random/3.4.3.json"
        );
        let route = Route::parse("index.json").expect("ok");
        assert_eq!(route.asset_path(), "index.json");
    }
}
