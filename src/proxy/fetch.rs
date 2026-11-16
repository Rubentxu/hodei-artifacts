//! El cliente que sale a buscar contenido a un upstream.
//!
//! Hace una sola cosa: abrir una respuesta `200` de un upstream permitido,
//! siguiendo las redirecciones **a mano**, para decidir cada salto con la
//! politica de salida y no con la que trae el cliente HTTP. Lo que entrega
//! no es contenido verificado: es una respuesta abierta y un
//! [`BodyGuard`] que comprueba, trozo a trozo y al final, que lo recibido
//! es entero, no excede el presupuesto y coincide con lo que el upstream
//! declaro.
//!
//! Decisiones que conviene tener a la vista:
//!
//! - **Sin proxies del entorno ni redirecciones automaticas.** El cliente
//!   se construye con `no_proxy` y `Policy::none()`: ni `HTTP_PROXY` ni un
//!   `Location` deciden a donde se va.
//! - **Resolucion fijada.** El host se resuelve aqui, se valida y la
//!   conexion se hace a esas mismas direcciones (`resolve_to_addrs`). El
//!   TLS sigue verificando el nombre del host, no la IP.
//! - **Cliente por salto.** Cada salto tiene su propia resolucion fijada,
//!   asi que cada salto tiene su propio cliente y ninguna conexion
//!   reutilizada puede llevar a un destino que no se valido.
//! - **La credencial no sale de su origen.** Solo se envia al origen del
//!   upstream configurado, nunca a otro al que una redireccion lleve.
//! - **Sin compresion.** Se pide `identity`: un upstream que comprime de
//!   todos modos no se acepta, porque el digest y la longitud son de los
//!   bytes tal y como estan en disco.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hodei_catalog::{ChecksumEvidence, CredentialRef, UpstreamConfig, origin_of};
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, HeaderValue, LOCATION,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::{Host, Url};

use super::egress::{EgressDenied, EgressPolicy, HostResolver, SystemResolver};

/// La cabecera donde el upstream declara el checksum del contenido.
pub const CHECKSUM_HEADER: &str = "x-checksum-sha256";

/// Un secreto: se puede usar y no se puede mostrar.
///
/// No implementa `Display`, y su `Debug` lo oculta: un secreto que se
/// puede formatear acaba en un log.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Envuelve un secreto.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// De donde salen los secretos.
pub trait SecretSource: Send + Sync {
    /// El secreto de una referencia, si existe.
    fn get(&self, reference: &CredentialRef) -> Option<Secret>;
}

/// Los secretos del entorno del proceso que el servicio ha autorizado.
///
/// La configuracion de un repositorio solo **nombra** una variable; que esa
/// variable pueda salir del proceso lo decide el operador del servicio con
/// una lista de nombres. Sin lista no se lee ninguna: un repositorio no
/// puede apuntar su `credential_ref` a un secreto que no es de proxy (la
/// base de datos, el token de otra cuenta) para enviarlo a un upstream.
#[derive(Debug, Default, Clone)]
pub struct EnvSecrets {
    allowed: std::collections::BTreeSet<String>,
}

impl EnvSecrets {
    /// Solo estas variables pueden usarse como credencial.
    pub fn allowing<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed: names.into_iter().map(Into::into).collect(),
        }
    }
}

impl SecretSource for EnvSecrets {
    fn get(&self, reference: &CredentialRef) -> Option<Secret> {
        if !self.allowed.contains(reference.env_name()) {
            return None;
        }
        std::env::var(reference.env_name())
            .ok()
            .filter(|value| !value.is_empty())
            .map(Secret::new)
    }
}

/// Los limites de servicio de una salida.
#[derive(Debug, Clone, Copy)]
pub struct FetchLimits {
    /// Lo que se espera para conectar y para resolver.
    pub connect_timeout: Duration,
    /// Lo que dura una peticion entera, cuerpo incluido.
    pub total_timeout: Duration,
    /// El techo de bytes de una respuesta. Un repositorio puede estrecharlo,
    /// no ensancharlo.
    pub max_bytes: u64,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            total_timeout: Duration::from_secs(300),
            max_bytes: 1 << 30,
        }
    }
}

/// Por que no se pudo traer un contenido.
///
/// Ningun caso lleva la URL ni la respuesta del upstream: el cliente que
/// pide el contenido no tiene por que ver la red interna del servicio.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// La politica de salida no deja ir ahi.
    #[error("egress denied: {0}")]
    Egress(#[from] EgressDenied),
    /// La ruta no esta entre los alcances del upstream.
    #[error("the path is outside the scopes of the upstream")]
    OutOfScope,
    /// El upstream no tiene ese contenido.
    #[error("the upstream does not have that content")]
    NotFound,
    /// El upstream respondio con un estado que no es un contenido.
    #[error("the upstream answered with status {0}")]
    Status(u16),
    /// La redireccion no esta permitida.
    #[error("redirect refused: {0}")]
    Redirect(&'static str),
    /// El upstream no respondio a tiempo.
    #[error("the upstream timed out")]
    Timeout,
    /// No se pudo hablar con el upstream (red, TLS, protocolo).
    #[error("the upstream could not be reached")]
    Unreachable,
    /// La respuesta no es utilizable.
    #[error("the response is not usable: {0}")]
    BadResponse(&'static str),
    /// La respuesta termino antes de lo que decia.
    #[error("the response was cut short")]
    Truncated,
    /// La respuesta es mas grande de lo que se admite.
    #[error("the content is larger than allowed")]
    TooLarge,
    /// El checksum que declaro el upstream no es el del contenido.
    #[error("the checksum declared by the upstream does not match the content")]
    ChecksumMismatch,
    /// El repositorio exige checksum y el upstream no lo declaro.
    #[error("the upstream did not declare a checksum and the repository requires one")]
    ChecksumMissing,
    /// La credencial configurada no esta disponible.
    #[error("the credential of the upstream is not available")]
    Credential,
    /// Hay demasiadas salidas a la vez.
    #[error("too many fetches are in flight")]
    Busy,
    /// El servicio no sale a la red: se pidio una salida en modo offline.
    #[error("the service is offline and does not reach any upstream")]
    Offline,
}

/// Los validadores con que se pregunta al upstream si un contenido que ya se
/// tiene sigue valiendo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validators {
    /// Se envia como `If-None-Match`.
    pub etag: Option<String>,
    /// Se envia como `If-Modified-Since`.
    pub last_modified: Option<String>,
}

impl Validators {
    /// Si no hay ninguno que enviar.
    pub fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }
}

/// Lo que contesta el upstream a una peticion que puede ser condicional.
#[derive(Debug)]
pub enum Fetched {
    /// El upstream entrega el contenido (`200`).
    Content(Box<Opened>),
    /// El upstream confirma que el contenido que se tiene sigue valiendo
    /// (`304`): no hay cuerpo.
    NotModified(NotModified),
}

/// La confirmacion de un `304`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotModified {
    /// De donde vino la confirmacion, tras las redirecciones, sin consulta.
    pub source_url: String,
    /// Las redirecciones que se siguieron.
    pub redirects: u8,
    /// El `ETag` que declaro el `304`, si declaro alguno.
    pub etag: Option<String>,
    /// El `Last-Modified` que declaro el `304`, si declaro alguno.
    pub last_modified: Option<String>,
}

/// El cliente de salida del servicio.
#[derive(Clone)]
pub struct ProxyFetcher {
    policy: Arc<EgressPolicy>,
    resolver: Arc<dyn HostResolver>,
    secrets: Arc<dyn SecretSource>,
    limits: FetchLimits,
    permits: Arc<Semaphore>,
    offline: bool,
}

impl std::fmt::Debug for ProxyFetcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyFetcher")
            .field("policy", &self.policy)
            .field("limits", &self.limits)
            .field("offline", &self.offline)
            .finish_non_exhaustive()
    }
}

impl ProxyFetcher {
    /// Un cliente con el resolutor y los secretos del sistema.
    pub fn new(policy: EgressPolicy, limits: FetchLimits, max_concurrent: usize) -> Self {
        Self {
            policy: Arc::new(policy),
            resolver: Arc::new(SystemResolver),
            secrets: Arc::new(EnvSecrets::default()),
            limits,
            permits: Arc::new(Semaphore::new(max_concurrent.max(1))),
            offline: false,
        }
    }

    /// Un cliente que no sale a la red: toda peticion falla con
    /// [`FetchError::Offline`] antes de resolver un nombre.
    ///
    /// Es la defensa de ultima linea del modo offline: aunque la decision de
    /// cache se equivocase, ninguna ruta de codigo llega a un upstream.
    #[must_use]
    pub fn with_offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    /// Si el cliente esta en modo offline.
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// Con otro resolutor, para probar el rebind.
    #[must_use]
    pub fn with_resolver(mut self, resolver: Arc<dyn HostResolver>) -> Self {
        self.resolver = resolver;
        self
    }

    /// Con estas variables de entorno como unicas credenciales posibles.
    #[must_use]
    pub fn with_allowed_credentials<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.secrets = Arc::new(EnvSecrets::allowing(names));
        self
    }

    /// Con otra fuente de secretos.
    #[must_use]
    pub fn with_secrets(mut self, secrets: Arc<dyn SecretSource>) -> Self {
        self.secrets = secrets;
        self
    }

    /// Abre la respuesta del contenido de `asset_path`.
    ///
    /// Solo devuelve una respuesta `200` de un destino permitido en todos
    /// sus saltos. Lo que devuelve **no esta verificado**: quien lo lee
    /// pasa los trozos por el [`BodyGuard`].
    pub async fn open(
        &self,
        upstream: &UpstreamConfig,
        asset_path: &str,
    ) -> Result<Opened, FetchError> {
        match self.open_conditional(upstream, asset_path, None).await? {
            Fetched::Content(opened) => Ok(*opened),
            // Sin validadores no hay nada que confirmar: un `304` no
            // solicitado es una respuesta que no es un contenido.
            Fetched::NotModified(_) => Err(FetchError::Status(304)),
        }
    }

    /// Como [`Self::open`], pero preguntando al upstream si el contenido que
    /// ya se tiene sigue valiendo.
    ///
    /// Los validadores se envian **solo al origen del upstream**, no a un
    /// origen al que una redireccion lleve. Un `304` sin validadores
    /// enviados no es una respuesta valida.
    pub async fn open_conditional(
        &self,
        upstream: &UpstreamConfig,
        asset_path: &str,
        validators: Option<&Validators>,
    ) -> Result<Fetched, FetchError> {
        if self.offline {
            return Err(FetchError::Offline);
        }
        let validators = validators.filter(|validators| !validators.is_empty());
        // Lo que no cae en un scope no consume ni un permiso.
        let mut url = upstream.target(asset_path).ok_or(FetchError::OutOfScope)?;
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| FetchError::Busy)?;
        // El tope de tiempo es de la resolucion entera, no de cada salto:
        // una cadena de redirecciones lentas no retiene el permiso mas.
        let deadline = tokio::time::Instant::now() + self.limits.total_timeout;
        let budget = upstream
            .max_bytes
            .map_or(self.limits.max_bytes, |own| own.min(self.limits.max_bytes));
        let upstream_origin = upstream.origin();
        let mut redirects: u8 = 0;

        loop {
            let response = self
                .hop(upstream, &upstream_origin, &url, deadline, validators)
                .await?;
            let status = response.status().as_u16();
            match status {
                200 => {
                    return Self::accept(response, url, redirects, budget, permit)
                        .map(|opened| Fetched::Content(Box::new(opened)));
                }
                // Un `304` solo vale de quien recibio los validadores: otro
                // origen al que llevo un redirect no sabe de que copia habla.
                304 if validators.is_some() && origin_of(&url) == upstream_origin => {
                    return Ok(Fetched::NotModified(NotModified {
                        source_url: format!("{}{}", origin_of(&url), url.path()),
                        redirects,
                        etag: header_text(&response, reqwest::header::ETAG, 200),
                        last_modified: header_text(&response, reqwest::header::LAST_MODIFIED, 100),
                    }));
                }
                301 | 302 | 303 | 307 | 308 => {
                    redirects += 1;
                    if redirects > upstream.max_redirects {
                        return Err(FetchError::Redirect("too many redirects"));
                    }
                    let next = Self::redirect_target(&response, &url, upstream)?;
                    url = next;
                }
                404 | 410 => return Err(FetchError::NotFound),
                other => return Err(FetchError::Status(other)),
            }
        }
    }

    /// Una peticion a un destino ya admitido por la politica.
    async fn hop(
        &self,
        upstream: &UpstreamConfig,
        upstream_origin: &str,
        url: &Url,
        deadline: tokio::time::Instant,
        validators: Option<&Validators>,
    ) -> Result<reqwest::Response, FetchError> {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(FetchError::Timeout);
        }
        let entry = self.policy.check_url(url)?;
        let port = url
            .port_or_known_default()
            .ok_or(EgressDenied::PortNotAllowed)?;

        // Un nombre se resuelve aqui, se valida y se fija. Una IP literal
        // ya se valido en `check_url`.
        let pinned: Option<(String, Vec<SocketAddr>)> = match url.host() {
            Some(Host::Domain(domain)) => {
                let addresses = tokio::time::timeout(
                    self.limits.connect_timeout,
                    self.resolver.resolve(domain, port),
                )
                .await
                .map_err(|_| FetchError::Timeout)?
                .map_err(|_| EgressDenied::Unresolved)?;
                EgressPolicy::validate_addresses(entry, &addresses)?;
                Some((
                    domain.to_string(),
                    addresses
                        .into_iter()
                        .map(|ip| SocketAddr::new(ip, port))
                        .collect(),
                ))
            }
            _ => None,
        };

        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(self.limits.connect_timeout)
            .timeout(remaining)
            .pool_max_idle_per_host(0)
            .user_agent(concat!("hodei-artifacts-proxy/", env!("CARGO_PKG_VERSION")));
        if let Some((domain, addresses)) = &pinned {
            builder = builder.resolve_to_addrs(domain, addresses);
        }
        let client = builder.build().map_err(|_| FetchError::Unreachable)?;

        let mut request = client
            .get(url.clone())
            .header(ACCEPT, "application/octet-stream, */*;q=0.8")
            .header(ACCEPT_ENCODING, "identity");
        // La credencial es del upstream: solo viaja a su origen.
        if let Some(credential) = &upstream.credential
            && origin_of(url) == upstream_origin
        {
            let secret = self.secrets.get(credential).ok_or(FetchError::Credential)?;
            let mut value = HeaderValue::from_str(&format!("Bearer {}", secret.expose()))
                .map_err(|_| FetchError::Credential)?;
            value.set_sensitive(true);
            request = request.header(AUTHORIZATION, value);
        }
        // Los validadores tambien son del upstream: un origen al que lleva
        // una redireccion no tiene por que ver cual es el estado de la
        // copia que se tiene.
        if let Some(validators) = validators
            && origin_of(url) == upstream_origin
        {
            if let Some(etag) = validators.etag.as_deref()
                && let Ok(value) = HeaderValue::from_str(etag)
            {
                request = request.header(reqwest::header::IF_NONE_MATCH, value);
            }
            if let Some(since) = validators.last_modified.as_deref()
                && let Ok(value) = HeaderValue::from_str(since)
            {
                request = request.header(reqwest::header::IF_MODIFIED_SINCE, value);
            }
        }

        request.send().await.map_err(|error| {
            let timed_out = error.is_timeout();
            // El texto del error de reqwest lleva la URL: se quita antes de
            // dejarlo en el rastro.
            tracing::warn!(
                target: "hodei_artifacts::proxy",
                host = url.host_str().unwrap_or(""),
                error = %error.without_url(),
                "the upstream could not be reached"
            );
            if timed_out {
                FetchError::Timeout
            } else {
                FetchError::Unreachable
            }
        })
    }

    /// A donde lleva una redireccion, si esta permitido ir.
    fn redirect_target(
        response: &reqwest::Response,
        from: &Url,
        upstream: &UpstreamConfig,
    ) -> Result<Url, FetchError> {
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(FetchError::Redirect("the redirect has no usable location"))?;
        let mut next = from
            .join(location)
            .map_err(|_| FetchError::Redirect("the redirect location is not a url"))?;
        next.set_fragment(None);
        if from.scheme() == "https" && next.scheme() != "https" {
            return Err(FetchError::Redirect("a redirect cannot downgrade to http"));
        }
        if !upstream.cross_origin_redirects && origin_of(&next) != origin_of(from) {
            return Err(FetchError::Redirect(
                "a redirect to another origin is not allowed",
            ));
        }
        // El destino se juzga con la politica de salida al empezar el
        // salto siguiente; aqui solo se descarta lo que no es ni una URL
        // que se pueda juzgar.
        Ok(next)
    }

    /// Acepta una respuesta `200`, si sus cabeceras son utilizables.
    fn accept(
        response: reqwest::Response,
        final_url: Url,
        redirects: u8,
        budget: u64,
        permit: OwnedSemaphorePermit,
    ) -> Result<Opened, FetchError> {
        let headers = response.headers();
        if let Some(encoding) = headers.get(CONTENT_ENCODING)
            && !encoding.as_bytes().eq_ignore_ascii_case(b"identity")
        {
            return Err(FetchError::BadResponse(
                "the upstream compressed a response that was asked uncompressed",
            ));
        }
        let content_length = response.content_length();
        if let Some(length) = content_length
            && length > budget
        {
            return Err(FetchError::TooLarge);
        }
        let declared = match headers.get(CHECKSUM_HEADER) {
            None => None,
            Some(value) => Some(value.to_str().ok().and_then(parse_declared_sha256).ok_or(
                FetchError::BadResponse("the checksum declared by the upstream is not a sha256"),
            )?),
        };
        let etag = header_text(&response, reqwest::header::ETAG, 200);
        let last_modified = header_text(&response, reqwest::header::LAST_MODIFIED, 100);
        Ok(Opened {
            response,
            final_url,
            redirects,
            content_length,
            declared,
            etag,
            last_modified,
            budget,
            _permit: permit,
        })
    }
}

/// Una respuesta `200` abierta, sin leer.
pub struct Opened {
    response: reqwest::Response,
    final_url: Url,
    redirects: u8,
    content_length: Option<u64>,
    declared: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
    budget: u64,
    _permit: OwnedSemaphorePermit,
}

/// El texto de una cabecera, si es texto visible y no es desmedida.
fn header_text(
    response: &reqwest::Response,
    name: reqwest::header::HeaderName,
    max_len: usize,
) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= max_len)
        .map(str::to_string)
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opened")
            .field("redirects", &self.redirects)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

impl Opened {
    /// El guardian de los bytes que van a llegar.
    pub fn guard(&self, require_checksum: bool) -> BodyGuard {
        BodyGuard {
            count: 0,
            budget: self.budget,
            expected_length: self.content_length,
            declared: self.declared.clone(),
            require_checksum,
        }
    }

    /// El siguiente trozo, o `None` al terminar.
    pub async fn chunk(&mut self) -> Result<Option<bytes::Bytes>, FetchError> {
        self.response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                FetchError::Timeout
            } else {
                // Una conexion que se corta a mitad de un cuerpo es una
                // respuesta truncada: no se sabe cuanto faltaba.
                FetchError::Truncated
            }
        })
    }

    /// De donde vino el contenido: origen y ruta, sin credenciales ni
    /// consulta.
    pub fn source_url(&self) -> String {
        format!("{}{}", origin_of(&self.final_url), self.final_url.path())
    }

    /// Las redirecciones que se siguieron.
    pub fn redirects(&self) -> u8 {
        self.redirects
    }

    /// La etiqueta de entidad que declaro el upstream.
    pub fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// El `Last-Modified` que declaro el upstream.
    pub fn last_modified(&self) -> Option<&str> {
        self.last_modified.as_deref()
    }
}

/// Lo que se comprobo de un contenido recibido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// Los bytes recibidos.
    pub size_bytes: u64,
    /// Con que se comprobo.
    pub evidence: ChecksumEvidence,
}

/// Comprueba el contenido que llega.
///
/// No calcula el digest: lo calcula la sesion de escritura que ya lo hace
/// mientras pasan los trozos, y aqui se le pregunta. Dos calculos del mismo
/// contenido son dos oportunidades de discrepar.
#[derive(Debug, Clone)]
pub struct BodyGuard {
    count: u64,
    budget: u64,
    expected_length: Option<u64>,
    declared: Option<String>,
    require_checksum: bool,
}

impl BodyGuard {
    /// Cuenta un trozo. Falla en cuanto se pasa del presupuesto.
    pub fn push(&mut self, chunk_len: usize) -> Result<(), FetchError> {
        self.count = self.count.saturating_add(chunk_len as u64);
        if self.count > self.budget {
            return Err(FetchError::TooLarge);
        }
        if let Some(expected) = self.expected_length
            && self.count > expected
        {
            // Mas bytes de los que anuncio: el upstream no sabe lo que
            // envia.
            return Err(FetchError::BadResponse(
                "the upstream sent more bytes than it announced",
            ));
        }
        Ok(())
    }

    /// Cierra la comprobacion con el digest (hex, minusculas) de lo que
    /// llego.
    pub fn finish(self, computed_hex: &str) -> Result<Verified, FetchError> {
        if let Some(expected) = self.expected_length
            && self.count != expected
        {
            return Err(FetchError::Truncated);
        }
        let evidence = match self.declared {
            Some(declared) => {
                if !declared.eq_ignore_ascii_case(computed_hex) {
                    return Err(FetchError::ChecksumMismatch);
                }
                ChecksumEvidence::Declared {
                    header: CHECKSUM_HEADER.to_string(),
                }
            }
            None if self.require_checksum => return Err(FetchError::ChecksumMissing),
            None => ChecksumEvidence::ComputedOnly,
        };
        Ok(Verified {
            size_bytes: self.count,
            evidence,
        })
    }
}

/// El sha256 que declara una cabecera: `<hex>` o `sha256:<hex>`.
///
/// Devuelve el hexadecimal en minusculas, o `None` si no es un sha256.
pub fn parse_declared_sha256(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let value = match raw.split_once(':') {
        Some((algorithm, value)) if algorithm.eq_ignore_ascii_case("sha256") => value,
        Some(_) => return None,
        None => raw,
    };
    (value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae";

    fn guard(budget: u64, length: Option<u64>, declared: Option<&str>, require: bool) -> BodyGuard {
        BodyGuard {
            count: 0,
            budget,
            expected_length: length,
            declared: declared.map(str::to_string),
            require_checksum: require,
        }
    }

    #[test]
    fn a_secret_is_never_formatted() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(***)");
    }

    #[test]
    fn the_declared_checksum_is_read_with_or_without_prefix() {
        assert_eq!(parse_declared_sha256(HEX).as_deref(), Some(HEX));
        assert_eq!(
            parse_declared_sha256(&format!("SHA256:{}", HEX.to_uppercase())).as_deref(),
            Some(HEX)
        );
        assert!(parse_declared_sha256("sha1:abcd").is_none());
        assert!(parse_declared_sha256("zz").is_none());
        assert!(parse_declared_sha256(&HEX[..63]).is_none());
        assert!(parse_declared_sha256("").is_none());
    }

    #[test]
    fn a_body_over_the_budget_is_refused_as_it_arrives() {
        let mut guard = guard(10, None, None, false);
        assert!(guard.push(6).is_ok());
        assert_eq!(guard.push(5), Err(FetchError::TooLarge));
    }

    #[test]
    fn more_bytes_than_announced_are_refused() {
        let mut guard = guard(100, Some(5), None, false);
        assert!(guard.push(5).is_ok());
        assert!(matches!(guard.push(1), Err(FetchError::BadResponse(_))));
    }

    #[test]
    fn a_truncated_body_is_not_a_verified_one() {
        let mut guard = guard(100, Some(10), Some(HEX), false);
        guard.push(4).expect("push");
        assert_eq!(guard.finish(HEX), Err(FetchError::Truncated));
    }

    #[test]
    fn a_wrong_declared_checksum_is_refused() {
        let mut guard = guard(100, Some(3), Some(HEX), false);
        guard.push(3).expect("push");
        assert_eq!(
            guard.finish(&"0".repeat(64)),
            Err(FetchError::ChecksumMismatch)
        );
    }

    #[test]
    fn a_matching_declared_checksum_is_recorded_as_declared() {
        let mut guard = guard(100, Some(3), Some(HEX), true);
        guard.push(3).expect("push");
        let verified = guard.finish(HEX).expect("verified");
        assert_eq!(verified.size_bytes, 3);
        assert_eq!(
            verified.evidence,
            ChecksumEvidence::Declared {
                header: CHECKSUM_HEADER.to_string()
            }
        );
    }

    #[test]
    fn a_missing_checksum_is_refused_only_when_required() {
        let mut required = guard(100, None, None, true);
        required.push(3).expect("push");
        assert_eq!(required.finish(HEX), Err(FetchError::ChecksumMissing));

        let mut optional = guard(100, None, None, false);
        optional.push(3).expect("push");
        let verified = optional.finish(HEX).expect("computed only");
        assert_eq!(verified.evidence, ChecksumEvidence::ComputedOnly);
    }

    /// Un resolutor que contesta lo que el test quiere, por nombre.
    struct Scripted(std::collections::HashMap<&'static str, Vec<std::net::IpAddr>>);

    #[async_trait::async_trait]
    impl crate::proxy::egress::HostResolver for Scripted {
        async fn resolve(&self, host: &str, _port: u16) -> std::io::Result<Vec<std::net::IpAddr>> {
            self.0
                .get(host)
                .cloned()
                .ok_or_else(|| std::io::Error::other("unknown name"))
        }
    }

    /// Un servidor que contesta con una respuesta fija a cada peticion.
    async fn serve_always(answer: String) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let answer = answer.clone();
                tokio::spawn(async move {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer).await;
                    let _ = stream.write_all(answer.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        port
    }

    fn fetcher_with(
        allowed: Vec<crate::proxy::egress::AllowedHost>,
        names: Vec<(&'static str, Vec<std::net::IpAddr>)>,
    ) -> ProxyFetcher {
        ProxyFetcher::new(
            crate::proxy::egress::EgressPolicy::new(allowed).expect("policy"),
            FetchLimits::default(),
            2,
        )
        .with_resolver(Arc::new(Scripted(names.into_iter().collect())))
    }

    fn entry(host: &str, port: Option<u16>, private: bool) -> crate::proxy::egress::AllowedHost {
        crate::proxy::egress::AllowedHost {
            host: host.to_string(),
            port,
            allow_private: private,
            allow_http: true,
        }
    }

    fn upstream(url: &str, extra: serde_json::Value) -> UpstreamConfig {
        let mut config = serde_json::json!({ "id": "lab", "url": url, "scopes": ["/"] });
        for (key, value) in extra.as_object().expect("an object") {
            config[key] = value.clone();
        }
        UpstreamConfig::parse(&config).expect("an upstream")
    }

    #[tokio::test]
    async fn the_connection_goes_to_the_address_that_was_validated() {
        let port = serve_always(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".to_string(),
        )
        .await;
        // `pinned.test` no existe en el DNS del sistema: solo se alcanza si
        // la conexion usa la direccion que el resolutor dio y la politica
        // valido -- no una segunda resolucion.
        let fetcher = fetcher_with(
            vec![entry("pinned.test", Some(port), true)],
            vec![("pinned.test", vec!["127.0.0.1".parse().unwrap()])],
        );
        let upstream = upstream(
            &format!("http://pinned.test:{port}/"),
            serde_json::json!({}),
        );
        let mut opened = fetcher.open(&upstream, "a.bin").await.expect("opened");
        let chunk = opened.chunk().await.expect("a chunk").expect("some bytes");
        assert_eq!(&chunk[..], b"hi");
    }

    #[tokio::test]
    async fn a_name_that_rebinds_to_a_private_address_is_refused() {
        let fetcher = fetcher_with(
            vec![entry("rebind.test", None, false)],
            vec![("rebind.test", vec!["10.0.0.5".parse().unwrap()])],
        );
        let upstream = upstream("http://rebind.test/", serde_json::json!({}));
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
        assert!(matches!(error, FetchError::Egress(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_redirect_to_a_name_that_resolves_to_a_forbidden_address_is_refused() {
        let second = "second.test";
        let port = serve_always(format!(
            "HTTP/1.1 302 Found\r\nLocation: http://{second}/ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ))
        .await;
        // El primer salto es legitimo (loopback autorizado); el segundo
        // nombre apunta a la red de metadatos: la politica lo ve en cuanto
        // se resuelve, antes de conectar.
        let fetcher = fetcher_with(
            vec![
                entry("first.test", Some(port), true),
                entry(second, None, false),
            ],
            vec![
                ("first.test", vec!["127.0.0.1".parse().unwrap()]),
                (second, vec!["169.254.169.254".parse().unwrap()]),
            ],
        );
        let upstream = upstream(
            &format!("http://first.test:{port}/"),
            serde_json::json!({ "redirects": { "max": 2, "cross_origin": true } }),
        );
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
        assert!(
            matches!(
                error,
                FetchError::Egress(crate::proxy::egress::EgressDenied::Address(_))
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_redirect_across_origins_is_refused_unless_the_repository_allows_it() {
        let port = serve_always(
            "HTTP/1.1 302 Found\r\nLocation: http://other.test/ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
        )
        .await;
        let fetcher = fetcher_with(
            vec![
                entry("first.test", Some(port), true),
                entry("other.test", None, true),
            ],
            vec![
                ("first.test", vec!["127.0.0.1".parse().unwrap()]),
                ("other.test", vec!["127.0.0.1".parse().unwrap()]),
            ],
        );
        let upstream = upstream(&format!("http://first.test:{port}/"), serde_json::json!({}));
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
        assert!(matches!(error, FetchError::Redirect(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_compressed_answer_is_refused_because_none_was_asked_for() {
        let port = serve_always(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi"
                .to_string(),
        )
        .await;
        let fetcher = fetcher_with(vec![entry("lab.test", Some(port), true)], lab_names());
        let upstream = upstream(&format!("http://lab.test:{port}/"), serde_json::json!({}));
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
        assert!(matches!(error, FetchError::BadResponse(_)), "{error:?}");
    }

    #[tokio::test]
    async fn only_a_full_answer_is_a_content() {
        for status in ["206 Partial Content", "304 Not Modified", "204 No Content"] {
            let port = serve_always(format!(
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ))
            .await;
            let fetcher = fetcher_with(vec![entry("lab.test", Some(port), true)], lab_names());
            let upstream = upstream(&format!("http://lab.test:{port}/"), serde_json::json!({}));
            let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
            assert!(
                matches!(error, FetchError::Status(_)),
                "{status}: {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_saturated_service_answers_busy_and_a_path_out_of_scope_does_not_wait() {
        let port = serve_always(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".to_string(),
        )
        .await;
        // El fetcher de prueba admite dos salidas a la vez.
        let fetcher = fetcher_with(vec![entry("lab.test", Some(port), true)], lab_names());
        let upstream = upstream(
            &format!("http://lab.test:{port}/"),
            serde_json::json!({ "scopes": ["acme/"] }),
        );
        let first = fetcher.open(&upstream, "acme/a").await.expect("first");
        let second = fetcher.open(&upstream, "acme/b").await.expect("second");
        let busy = fetcher.open(&upstream, "acme/c").await.expect_err("busy");
        assert_eq!(busy, FetchError::Busy);
        // Fuera del scope se dice antes de pedir un permiso.
        let outside = fetcher.open(&upstream, "other/c").await.expect_err("scope");
        assert_eq!(outside, FetchError::OutOfScope);
        drop((first, second));
        fetcher
            .open(&upstream, "acme/d")
            .await
            .expect("a permit came back");
    }

    #[tokio::test]
    async fn the_time_limit_is_for_the_whole_resolution() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        // Un upstream que acepta y no contesta nunca.
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    let _ = stream.read(&mut buffer).await;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                });
            }
        });
        let fetcher = ProxyFetcher::new(
            crate::proxy::egress::EgressPolicy::new(vec![entry("lab.test", Some(port), true)])
                .expect("policy"),
            FetchLimits {
                total_timeout: Duration::from_millis(300),
                ..FetchLimits::default()
            },
            1,
        )
        .with_resolver(Arc::new(Scripted(lab_names().into_iter().collect())));
        let upstream = upstream(&format!("http://lab.test:{port}/"), serde_json::json!({}));
        let started = std::time::Instant::now();
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("timeout");
        assert_eq!(error, FetchError::Timeout);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_variable_is_only_a_credential_if_the_operator_allowed_it() {
        // `PATH` existe en cualquier entorno: lo que se mide es la lista.
        let reference = CredentialRef::parse("env:PATH").expect("a reference");
        assert!(EnvSecrets::default().get(&reference).is_none());
        assert!(EnvSecrets::allowing(["OTHER"]).get(&reference).is_none());
        assert!(EnvSecrets::allowing(["PATH"]).get(&reference).is_some());
    }

    fn lab_names() -> Vec<(&'static str, Vec<std::net::IpAddr>)> {
        vec![("lab.test", vec!["127.0.0.1".parse().unwrap()])]
    }

    // -- Peticiones condicionales y modo offline --------------------------

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    use super::super::egress::AllowedHost;

    /// Un servidor de un solo uso por conexion: guarda cada peticion
    /// entera y contesta lo que dice `reply`.
    fn serve(reply: impl Fn(&str) -> String + Send + 'static) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => request.extend_from_slice(&buffer[..read]),
                    }
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let answer = reply(&text);
                log.lock().expect("log").push(text);
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        (port, seen)
    }

    fn fetcher_for(ports: &[u16]) -> ProxyFetcher {
        let hosts = ports
            .iter()
            .map(|port| AllowedHost::from_spec(&format!("127.0.0.1:{port};private;http")))
            .collect::<Result<Vec<_>, _>>()
            .expect("hosts");
        ProxyFetcher::new(
            EgressPolicy::new(hosts).expect("policy"),
            FetchLimits::default(),
            4,
        )
    }

    fn upstream_at(port: u16, cross_origin: bool) -> UpstreamConfig {
        let mut config = hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global);
        config.config_settings = serde_json::json!({ "proxy": { "upstream": {
            "id": "lab",
            "url": format!("http://127.0.0.1:{port}/"),
            "scopes": ["/"],
            "redirects": { "max": 3, "cross_origin": cross_origin },
        }}});
        UpstreamConfig::of(&config)
            .expect("valid")
            .expect("an upstream")
    }

    fn validators() -> Validators {
        Validators {
            etag: Some("\"v1\"".to_string()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
        }
    }

    const NOT_MODIFIED: &str = "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nLast-Modified: Wed, 21 Oct 2015 07:28:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    #[tokio::test]
    async fn a_304_confirms_the_copy_and_the_validators_travel_to_the_upstream() {
        let (port, seen) = serve(|_| NOT_MODIFIED.to_string());
        let fetched = fetcher_for(&[port])
            .open_conditional(&upstream_at(port, false), "a.bin", Some(&validators()))
            .await
            .expect("answers");
        let Fetched::NotModified(confirmation) = fetched else {
            panic!("a 304 must be a confirmation");
        };
        assert_eq!(confirmation.etag.as_deref(), Some("\"v1\""));
        assert_eq!(
            confirmation.last_modified.as_deref(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
        assert!(!confirmation.source_url.contains('?'));
        let request = seen.lock().expect("log")[0].to_ascii_lowercase();
        assert!(request.contains("if-none-match: \"v1\""), "{request}");
        assert!(
            request.contains("if-modified-since: wed, 21 oct 2015 07:28:00 gmt"),
            "{request}"
        );
    }

    #[tokio::test]
    async fn without_validators_nothing_conditional_is_sent_and_a_304_is_an_error() {
        let (port, seen) = serve(|_| NOT_MODIFIED.to_string());
        let outcome = fetcher_for(&[port])
            .open_conditional(&upstream_at(port, false), "a.bin", None)
            .await;
        assert!(matches!(outcome, Err(FetchError::Status(304))));
        let request = seen.lock().expect("log")[0].to_ascii_lowercase();
        assert!(!request.contains("if-none-match"), "{request}");
        assert!(!request.contains("if-modified-since"), "{request}");
    }

    #[tokio::test]
    async fn validators_stay_at_the_upstream_origin_and_another_origin_cannot_confirm() {
        let (stranger, stranger_seen) = serve(|_| NOT_MODIFIED.to_string());
        let (port, _) = serve(move |_| {
            format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{stranger}/a.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
        });
        let outcome = fetcher_for(&[port, stranger])
            .open_conditional(&upstream_at(port, true), "a.bin", Some(&validators()))
            .await;
        // El otro origen no recibio los validadores, y su `304` no vale.
        assert!(
            matches!(outcome, Err(FetchError::Status(304))),
            "{outcome:?}"
        );
        let request = stranger_seen.lock().expect("log")[0].to_ascii_lowercase();
        assert!(!request.contains("if-none-match"), "{request}");
        assert!(!request.contains("if-modified-since"), "{request}");
    }

    #[tokio::test]
    async fn a_200_carries_the_last_modified_it_was_served_with() {
        let (port, _) = serve(|_| {
            "HTTP/1.1 200 OK\r\nLast-Modified: Wed, 21 Oct 2015 07:28:00 GMT\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                .to_string()
        });
        let fetched = fetcher_for(&[port])
            .open_conditional(&upstream_at(port, false), "a.bin", Some(&validators()))
            .await
            .expect("answers");
        let Fetched::Content(opened) = fetched else {
            panic!("a 200 must be content");
        };
        assert_eq!(
            opened.last_modified(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
    }

    #[tokio::test]
    async fn offline_refuses_before_opening_any_connection() {
        let (port, seen) = serve(|_| NOT_MODIFIED.to_string());
        let fetcher = fetcher_for(&[port]).with_offline(true);
        assert!(fetcher.is_offline());
        let outcome = fetcher
            .open_conditional(&upstream_at(port, false), "a.bin", None)
            .await;
        assert!(matches!(outcome, Err(FetchError::Offline)), "{outcome:?}");
        assert!(seen.lock().expect("log").is_empty());
    }
}
