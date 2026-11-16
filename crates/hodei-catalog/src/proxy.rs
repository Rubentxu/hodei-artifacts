//! El upstream de un repositorio proxy y el recibo de lo que se bajo de el.
//!
//! Un repositorio `Proxy` no guarda lo que alguien publica: resuelve contra
//! un **upstream declarado** y conserva lo que baja. Quien pide no elige a
//! donde va el servidor: pide una ruta **relativa**, y el destino sale de la
//! configuracion del repositorio, que esta versionada.
//!
//! Este modulo es puro: describe y valida la configuracion, y define el
//! recibo y su libro. Lo que toca la red (resolver nombres, conectar,
//! seguir redirecciones) vive en el servicio, que es quien conoce la
//! politica de salida de la maquina.
//!
//! # La configuracion
//!
//! Vive en `config_settings.proxy.upstream`:
//!
//! ```json
//! { "proxy": { "upstream": {
//!     "id": "central",
//!     "url": "https://repo.example.com/releases/",
//!     "scopes": ["acme/"],
//!     "credential_ref": "env:CENTRAL_TOKEN",
//!     "redirects": { "max": 3, "cross_origin": false },
//!     "max_bytes": 104857600,
//!     "require_checksum": false
//! } } }
//! ```
//!
//! - `scopes` son prefijos de ruta permitidos: lo que no cae en un scope no
//!   se pide al upstream. `"/"` es todo el upstream. No hay scopes por
//!   defecto: sin scopes la configuracion no es valida.
//! - `credential_ref` es una **referencia** (`env:NOMBRE`), nunca el
//!   secreto. El secreto se lee en el momento de la peticion y solo viaja
//!   al origen del upstream.
//! - `redirects` dice cuantas redirecciones se siguen y si se admiten
//!   hacia otro origen. Por defecto, tres y solo al mismo origen.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::{CatalogError, RepositoryConfig};

/// El tope de redirecciones que una configuracion puede pedir.
pub const MAX_REDIRECTS_CEILING: u8 = 10;

/// Las redirecciones que se siguen si la configuracion no dice otra cosa.
pub const DEFAULT_REDIRECTS: u8 = 3;

/// Por que una configuracion de upstream no es valida.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the upstream configuration is not valid: {0}")]
pub struct UpstreamConfigError(pub String);

fn invalid(reason: impl Into<String>) -> UpstreamConfigError {
    UpstreamConfigError(reason.into())
}

/// La referencia a una credencial.
///
/// Es el **nombre** de donde sale el secreto, no el secreto. Una
/// configuracion versionada y auditada no puede llevar un token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef(String);

impl CredentialRef {
    /// Lee una referencia `env:NOMBRE`.
    ///
    /// Solo se admite lo que es inequivocamente una referencia: cualquier
    /// otra cosa podria ser un secreto escrito donde no debe, y se rechaza
    /// sin repetirla en el error.
    pub fn parse(raw: &str) -> Result<Self, UpstreamConfigError> {
        let Some(name) = raw.strip_prefix("env:") else {
            return Err(invalid(
                "credential_ref must be a reference of the form env:NAME",
            ));
        };
        let well_formed = !name.is_empty()
            && name.len() <= 128
            && name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            && !name.as_bytes()[0].is_ascii_digit();
        if !well_formed {
            return Err(invalid(
                "credential_ref must be a reference of the form env:NAME",
            ));
        }
        Ok(Self(raw.to_string()))
    }

    /// La referencia, tal y como se escribio.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// El nombre de la variable de entorno que guarda el secreto.
    pub fn env_name(&self) -> &str {
        self.0.strip_prefix("env:").unwrap_or_default()
    }
}

/// El upstream de un repositorio proxy, ya validado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamConfig {
    /// El identificador estable del upstream. Va en los recibos.
    pub id: String,
    /// La URL base. Termina en `/`, sin credenciales, consulta ni fragmento.
    pub base: Url,
    /// Los prefijos de ruta que se pueden pedir. Nunca vacio.
    pub scopes: Vec<String>,
    /// De donde sale la credencial, si el upstream pide una.
    pub credential: Option<CredentialRef>,
    /// Cuantas redirecciones se siguen como maximo.
    pub max_redirects: u8,
    /// Si se admite una redireccion hacia otro origen.
    pub cross_origin_redirects: bool,
    /// El tope de bytes de una respuesta, si la configuracion lo estrecha.
    pub max_bytes: Option<u64>,
    /// Si el upstream tiene que declarar el checksum del contenido.
    ///
    /// Con `true`, una respuesta que no lo declara se rechaza: el contenido
    /// no se puede presentar como verificado contra el upstream.
    pub require_checksum: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUpstream {
    id: String,
    url: String,
    scopes: Vec<String>,
    #[serde(default)]
    credential_ref: Option<String>,
    #[serde(default)]
    redirects: Option<RawRedirects>,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    require_checksum: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRedirects {
    #[serde(default)]
    max: Option<u8>,
    #[serde(default)]
    cross_origin: bool,
}

impl UpstreamConfig {
    /// El upstream que declara la configuracion de un repositorio.
    ///
    /// `Ok(None)` es "no declara ninguno"; una declaracion que existe y no
    /// se entiende es un error, nunca un valor por defecto: un ajuste de
    /// salida a la red ilegible no puede degradar a "permitido".
    pub fn of(config: &RepositoryConfig) -> Result<Option<Self>, UpstreamConfigError> {
        let Some(proxy) = config.config_settings.get("proxy") else {
            return Ok(None);
        };
        // Una clave que no se conoce (`upstraem`, `cache`) no se ignora: un
        // ajuste que parece estar puesto y no se aplica es peor que un error.
        if let Some(object) = proxy.as_object()
            && let Some(unknown) = object.keys().find(|key| key.as_str() != "upstream")
        {
            return Err(invalid(format!(
                "the proxy settings do not know the key {unknown:?}"
            )));
        }
        let Some(upstream) = proxy.get("upstream") else {
            return Err(invalid("the proxy settings do not declare an upstream"));
        };
        Self::parse(upstream).map(Some)
    }

    /// Valida un upstream escrito como JSON.
    pub fn parse(value: &Value) -> Result<Self, UpstreamConfigError> {
        let raw: RawUpstream = serde_json::from_value(value.clone())
            .map_err(|error| invalid(format!("it cannot be read: {error}")))?;

        let id_ok = !raw.id.is_empty()
            && raw.id.len() <= 64
            && raw.id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            });
        if !id_ok {
            return Err(invalid("id must be 1-64 characters of [a-z0-9_-]"));
        }

        let mut base = Url::parse(&raw.url).map_err(|_| invalid("url is not an absolute URL"))?;
        if !matches!(base.scheme(), "http" | "https") {
            return Err(invalid("url must use http or https"));
        }
        if base.host().is_none() {
            return Err(invalid("url has no host"));
        }
        // Una URL con credenciales las pondria en cada traza que la nombre.
        if !base.username().is_empty() || base.password().is_some() {
            return Err(invalid("url must not carry credentials"));
        }
        if base.query().is_some() || base.fragment().is_some() {
            return Err(invalid("url must not carry a query or a fragment"));
        }
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }

        if raw.scopes.is_empty() {
            return Err(invalid("scopes must declare at least one prefix"));
        }
        let mut scopes = Vec::with_capacity(raw.scopes.len());
        for scope in &raw.scopes {
            scopes.push(Self::scope(scope)?);
        }

        let credential = raw
            .credential_ref
            .as_deref()
            .map(CredentialRef::parse)
            .transpose()?;

        let (max_redirects, cross_origin_redirects) = match raw.redirects {
            Some(redirects) => (
                redirects.max.unwrap_or(DEFAULT_REDIRECTS),
                redirects.cross_origin,
            ),
            None => (DEFAULT_REDIRECTS, false),
        };
        if max_redirects > MAX_REDIRECTS_CEILING {
            return Err(invalid(format!(
                "redirects.max cannot be more than {MAX_REDIRECTS_CEILING}"
            )));
        }
        if raw.max_bytes == Some(0) {
            return Err(invalid("max_bytes cannot be zero"));
        }

        Ok(Self {
            id: raw.id,
            base,
            scopes,
            credential,
            max_redirects,
            cross_origin_redirects,
            max_bytes: raw.max_bytes,
            require_checksum: raw.require_checksum,
        })
    }

    fn scope(raw: &str) -> Result<String, UpstreamConfigError> {
        if raw == "/" {
            return Ok(String::new());
        }
        let bad = raw.is_empty()
            // El prefijo es de segmento: sin la barra final, `acme` dejaria
            // pasar `acme-evil/`.
            || !raw.ends_with('/')
            || raw.starts_with('/')
            || raw.contains('\\')
            || raw.contains('?')
            || raw.contains('#')
            || raw.chars().any(char::is_control)
            || raw.split('/').any(|segment| segment == ".." || segment == ".");
        if bad {
            return Err(invalid(
                "a scope is a relative prefix without dot segments, or /",
            ));
        }
        Ok(raw.to_string())
    }

    /// Si una ruta relativa cae dentro de algun scope.
    pub fn in_scope(&self, asset_path: &str) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope.is_empty() || asset_path.starts_with(scope.as_str()))
    }

    /// La URL del upstream para una ruta relativa.
    ///
    /// La ruta se **añade** a la base segmento a segmento y codificada: no
    /// puede cambiar el host, el esquema ni salir de la base. `None` si la
    /// ruta no esta en un scope o no es una ruta relativa limpia.
    pub fn target(&self, asset_path: &str) -> Option<Url> {
        if asset_path.is_empty() || !self.in_scope(asset_path) {
            return None;
        }
        let mut url = self.base.clone();
        {
            let mut segments = url.path_segments_mut().ok()?;
            // La base termina en `/`: el ultimo segmento vacio se quita
            // para que la ruta no genere un `//`.
            segments.pop_if_empty();
            for segment in asset_path.split('/') {
                if segment.is_empty() || segment == "." || segment == ".." {
                    return None;
                }
                segments.push(segment);
            }
        }
        Some(url)
    }

    /// El origen del upstream: esquema, host y puerto, sin ruta.
    pub fn origin(&self) -> String {
        origin_of(&self.base)
    }
}

/// El origen de una URL (`esquema://host:puerto`), sin ruta ni credenciales.
pub fn origin_of(url: &Url) -> String {
    format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.port_or_known_default().unwrap_or_default()
    )
}

/// Con que se comprobo el contenido que bajo del upstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChecksumEvidence {
    /// El upstream declaro el checksum y coincide con lo recibido.
    Declared {
        /// La cabecera que lo declaro.
        header: String,
    },
    /// El upstream no declaro nada: el digest es el que se calculo aqui.
    ///
    /// Es identidad del contenido, no verificacion contra el upstream, y el
    /// recibo lo dice.
    ComputedOnly,
}

/// El recibo de una resolucion: de donde vino el contenido y con que se
/// comprobo.
///
/// Se escribe cuando el contenido se ha recibido **entero** y ha pasado la
/// comprobacion, y antes de publicarlo en el catalogo. Un fetch que falla
/// no deja recibo: no hay nada que afirmar.
///
/// No lleva credenciales ni la consulta de la URL: es lo que se enseña.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceReceipt {
    /// El identificador del recibo.
    pub id: String,
    /// La cuenta propietaria del repositorio.
    pub account: String,
    /// El repositorio proxy.
    pub repository: String,
    /// La ruta que se resolvio.
    pub asset_path: String,
    /// El upstream que respondio.
    pub upstream_id: String,
    /// La URL de donde vino el contenido, tras las redirecciones, sin
    /// credenciales ni consulta.
    pub source_url: String,
    /// La version de la configuracion con la que se resolvio.
    pub config_version: u64,
    /// El digest del contenido recibido.
    pub digest: ContentDigest,
    /// Los bytes recibidos.
    pub size_bytes: u64,
    /// Con que se comprobo.
    pub checksum: ChecksumEvidence,
    /// El estado HTTP de la respuesta que se acepto.
    pub status: u16,
    /// Las redirecciones que se siguieron.
    pub redirects: u8,
    /// La etiqueta de entidad que declaro el upstream, si declaro alguna.
    pub etag: Option<String>,
    /// Quien pidio el contenido.
    pub actor: String,
    /// La correlacion de la peticion.
    pub correlation_id: String,
    /// Cuando se recibio.
    pub fetched_at: DateTime<Utc>,
}

/// El libro de los recibos de procedencia.
#[async_trait]
pub trait ProvenanceLedger: Send + Sync {
    /// Guarda un recibo. Repetir el mismo `id` lo deja como estaba.
    async fn record(&self, receipt: &ProvenanceReceipt) -> Result<(), CatalogError>;

    /// Los recibos de una ruta, el mas reciente primero.
    async fn receipts_of(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<Vec<ProvenanceReceipt>, CatalogError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DigestScope;
    use serde_json::json;

    fn upstream(extra: Value) -> Value {
        let mut base = json!({
            "id": "central",
            "url": "https://repo.example.com/releases",
            "scopes": ["acme/"],
        });
        if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
            for (key, value) in extra {
                base.insert(key.clone(), value.clone());
            }
        }
        base
    }

    #[test]
    fn a_complete_configuration_is_read_with_its_defaults() {
        let config = UpstreamConfig::parse(&upstream(json!({}))).expect("valid");
        assert_eq!(config.id, "central");
        // La base siempre termina en `/`.
        assert_eq!(config.base.as_str(), "https://repo.example.com/releases/");
        assert_eq!(config.max_redirects, DEFAULT_REDIRECTS);
        assert!(!config.cross_origin_redirects);
        assert!(config.credential.is_none());
        assert!(!config.require_checksum);
    }

    #[test]
    fn what_cannot_be_read_is_refused_instead_of_defaulted() {
        let cases = [
            json!({"url": "ftp://repo.example.com/"}),
            json!({"url": "https://user:secret@repo.example.com/"}),
            json!({"url": "https://repo.example.com/?token=1"}),
            json!({"url": "https://repo.example.com/#x"}),
            json!({"url": "not a url"}),
            json!({"scopes": []}),
            json!({"scopes": ["../x/"]}),
            json!({"scopes": ["/abs/"]}),
            json!({"scopes": [""]}),
            json!({"scopes": ["acme"]}),
            json!({"id": "Has Spaces"}),
            json!({"credential_ref": "hunter2"}),
            json!({"credential_ref": "env:lower"}),
            json!({"credential_ref": "env:"}),
            json!({"redirects": {"max": 200}}),
            json!({"max_bytes": 0}),
            // Un campo que no se conoce es un campo que se escribio mal.
            json!({"allow_private": true}),
        ];
        for case in cases {
            assert!(
                UpstreamConfig::parse(&upstream(case.clone())).is_err(),
                "{case} should be refused"
            );
        }
    }

    #[test]
    fn a_secret_written_in_the_reference_is_not_echoed_in_the_error() {
        let error = UpstreamConfig::parse(&upstream(json!({"credential_ref": "s3cr3t-token"})))
            .expect_err("not a reference");
        assert!(!error.to_string().contains("s3cr3t-token"));
    }

    #[test]
    fn a_path_is_appended_to_the_base_and_cannot_leave_it() {
        let config = UpstreamConfig::parse(&upstream(json!({"scopes": ["/"]}))).expect("valid");
        assert_eq!(
            config.target("acme/lib 1.jar").expect("target").as_str(),
            "https://repo.example.com/releases/acme/lib%201.jar"
        );
        // Ningun intento de cambiar el destino produce una URL.
        for hostile in ["../etc/passwd", "a/../../b", "a//b", "", "a/./b"] {
            assert!(config.target(hostile).is_none(), "{hostile:?}");
        }
        // Una ruta con apariencia de URL es solo un segmento codificado.
        let url = config.target("http:%2F%2Fevil.test/x").expect("segment");
        assert_eq!(url.host_str(), Some("repo.example.com"));
    }

    #[test]
    fn a_path_outside_the_scopes_has_no_target() {
        let config = UpstreamConfig::parse(&upstream(json!({}))).expect("valid");
        assert!(config.target("acme/lib.jar").is_some());
        assert!(config.target("other/lib.jar").is_none());
        // El prefijo es de segmento: `acme-evil/` no cae en `acme/`.
        assert!(config.target("acme-evil/lib.jar").is_none());
    }

    #[test]
    fn the_settings_of_a_repository_without_proxy_are_not_an_upstream() {
        let config = RepositoryConfig::new(DigestScope::Global);
        assert_eq!(UpstreamConfig::of(&config), Ok(None));

        let mut broken = RepositoryConfig::new(DigestScope::Global);
        broken.config_settings = json!({"proxy": {}});
        assert!(UpstreamConfig::of(&broken).is_err());
    }

    #[test]
    fn a_key_the_proxy_settings_do_not_know_is_an_error_not_an_ignored_setting() {
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings =
            json!({"proxy": {"upstream": upstream(json!({})), "upstraem": {}}});
        assert!(UpstreamConfig::of(&config).is_err());
    }

    #[test]
    fn the_origin_has_no_path_and_no_credentials() {
        let url = Url::parse("https://repo.example.com/a/b?x=1").unwrap();
        assert_eq!(origin_of(&url), "https://repo.example.com:443");
    }
}
