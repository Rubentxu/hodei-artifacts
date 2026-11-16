//! La politica de salida del servicio: a donde puede ir una peticion.
//!
//! Un repositorio proxy hace que el **servicio** salga a la red a buscar
//! contenido por cuenta de un cliente. Esa salida es la superficie clasica
//! del SSRF: lo que el cliente (o un upstream hostil, por una redireccion)
//! consigue que el servicio visite con la red del servicio. Por eso la
//! decision de a donde se puede ir no la toma la configuracion de un
//! repositorio, sino el **administrador del servicio**, en una lista de
//! hosts permitidos, y se aplica en cada salto.
//!
//! Dos reglas sostienen todo lo demas:
//!
//! - Un host solo se visita si esta en la lista, con su puerto y su
//!   esquema.
//! - Una direccion privada o de bucle local solo se visita si la entrada
//!   de ese host lo permite **explicitamente** (`allow_private`). Las
//!   direcciones que nunca son un destino legitimo (sin especificar,
//!   multicast, difusion, enlace local y el servicio de metadatos de la
//!   nube, reservadas) no tienen permiso que las abra.
//!
//! La resolucion se valida y se fija **antes** de conectar: la conexion se
//! hace a las direcciones que se validaron, de modo que un DNS que cambie
//! de respuesta entre la comprobacion y la conexion (rebind) no tiene
//! ventana.

use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::{Host, Url};

/// Una entrada de la lista de hosts permitidos.
///
/// En un fichero de configuracion es un objeto
/// (`{ host = "...", port = 8080, allow_private = true }`). En una variable
/// de entorno, donde una lista solo puede ser de cadenas, es
/// `host[:puerto][;private][;http]`, por ejemplo
/// `HODEI_PROXY__ALLOWED_HOSTS=repo.example.com,127.0.0.1:8080;private;http`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "AllowedHostSpec")]
pub struct AllowedHost {
    /// El nombre del host, o la IP literal. Sin esquema, sin puerto y sin
    /// comodines.
    pub host: String,
    /// El puerto. Sin puerto, solo el del esquema (443 o 80).
    #[serde(default)]
    pub port: Option<u16>,
    /// Permite que este host sea (o resuelva a) una direccion privada o de
    /// bucle local. Es el permiso administrativo explicito de la salida a
    /// la red interna; sin el, esas direcciones se rechazan.
    #[serde(default)]
    pub allow_private: bool,
    /// Permite `http` sin TLS para este host.
    #[serde(default)]
    pub allow_http: bool,
}

/// El tipo de una direccion que no es publica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpClass {
    /// `0.0.0.0/8`, `::`.
    Unspecified,
    /// `127.0.0.0/8`, `::1`.
    Loopback,
    /// RFC 1918.
    Private,
    /// `100.64.0.0/10`.
    SharedAddressSpace,
    /// `fc00::/7`.
    UniqueLocal,
    /// `169.254.0.0/16` (incluye el servicio de metadatos de la nube) y
    /// `fe80::/10`.
    LinkLocal,
    /// Multicast.
    Multicast,
    /// `255.255.255.255`.
    Broadcast,
    /// Documentacion, pruebas de rendimiento, protocolo y reservados.
    Reserved,
}

impl IpClass {
    /// Si un permiso administrativo puede abrir esta clase.
    ///
    /// Solo la red interna legitima se puede abrir: bucle local, redes
    /// privadas, espacio compartido y ULA. Lo demas no es un destino.
    pub fn can_be_allowed(self) -> bool {
        matches!(
            self,
            Self::Loopback | Self::Private | Self::SharedAddressSpace | Self::UniqueLocal
        )
    }

    fn name(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Loopback => "loopback",
            Self::Private => "private",
            Self::SharedAddressSpace => "shared address space",
            Self::UniqueLocal => "unique local",
            Self::LinkLocal => "link-local or metadata",
            Self::Multicast => "multicast",
            Self::Broadcast => "broadcast",
            Self::Reserved => "reserved",
        }
    }
}

impl fmt::Display for IpClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Las dos formas de escribir una entrada.
#[derive(Deserialize)]
#[serde(untagged)]
enum AllowedHostSpec {
    Text(String),
    Full(AllowedHostFull),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowedHostFull {
    host: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    allow_private: bool,
    #[serde(default)]
    allow_http: bool,
}

impl TryFrom<AllowedHostSpec> for AllowedHost {
    type Error = String;

    fn try_from(spec: AllowedHostSpec) -> Result<Self, Self::Error> {
        match spec {
            AllowedHostSpec::Full(full) => Ok(Self {
                host: full.host,
                port: full.port,
                allow_private: full.allow_private,
                allow_http: full.allow_http,
            }),
            AllowedHostSpec::Text(text) => Self::from_spec(&text),
        }
    }
}

impl AllowedHost {
    /// Lee `host[:puerto][;private][;http]`.
    ///
    /// Una IPv6 literal lleva corchetes si lleva puerto (`[::1]:8080`).
    /// Un indicador que no se conoce es un error: una opcion de seguridad
    /// mal escrita no se ignora.
    pub fn from_spec(text: &str) -> Result<Self, String> {
        let mut parts = text.split(';').map(str::trim);
        let address = parts.next().unwrap_or_default();
        let mut entry = Self {
            host: address.to_string(),
            port: None,
            allow_private: false,
            allow_http: false,
        };
        if let Some(rest) = address.strip_prefix('[') {
            // `[v6]` o `[v6]:puerto`
            let (host, tail) = rest
                .split_once(']')
                .ok_or_else(|| format!("unclosed bracket in `{address}`"))?;
            entry.host = host.to_string();
            if let Some(port) = tail.strip_prefix(':') {
                entry.port = Some(
                    port.parse()
                        .map_err(|_| format!("bad port in `{address}`"))?,
                );
            } else if !tail.is_empty() {
                return Err(format!("unexpected text after the bracket in `{address}`"));
            }
        } else if address.matches(':').count() == 1
            && let Some((host, port)) = address.split_once(':')
        {
            entry.host = host.to_string();
            entry.port = Some(
                port.parse()
                    .map_err(|_| format!("bad port in `{address}`"))?,
            );
        }
        for flag in parts {
            match flag {
                "private" => entry.allow_private = true,
                "http" => entry.allow_http = true,
                other => return Err(format!("unknown flag `{other}`")),
            }
        }
        Ok(entry)
    }
}

/// Por que se deniega una salida.
///
/// El mensaje es para el rastro del servicio: no lleva la URL entera ni
/// nada que pueda ser un secreto.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EgressDenied {
    /// El esquema no es `http` ni `https`.
    #[error("the scheme is not http or https")]
    Scheme,
    /// La URL trae usuario o contrasena.
    #[error("the url carries credentials")]
    Credentials,
    /// La URL no tiene host.
    #[error("the url has no host")]
    NoHost,
    /// El host no esta en la lista.
    #[error("the host is not in the allow-list")]
    HostNotAllowed,
    /// El puerto no es el permitido para ese host.
    #[error("the port is not the one allowed for that host")]
    PortNotAllowed,
    /// `http` sin que la entrada lo permita.
    #[error("plain http is not allowed for that host")]
    PlainHttp,
    /// El host es, o resuelve a, una direccion que no se visita.
    #[error("the host is or resolves to an address that is not allowed ({0})")]
    Address(IpClass),
    /// El host no resolvio a ninguna direccion.
    #[error("the host did not resolve")]
    Unresolved,
}

/// La clase de una direccion, o `None` si es publica.
///
/// Las direcciones que embeben una IPv4 (mapeadas, NAT64 y 6to4) se
/// clasifican por la IPv4 que llevan: `::ffff:127.0.0.1` es el bucle
/// local, no una direccion IPv6 cualquiera.
pub fn classify(ip: IpAddr) -> Option<IpClass> {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => classify_v6(v6),
    }
}

fn classify_v4(ip: Ipv4Addr) -> Option<IpClass> {
    let [a, b, c, _] = ip.octets();
    if ip.is_broadcast() {
        return Some(IpClass::Broadcast);
    }
    if a == 0 {
        return Some(IpClass::Unspecified);
    }
    if a == 127 {
        return Some(IpClass::Loopback);
    }
    if a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168) {
        return Some(IpClass::Private);
    }
    if a == 100 && (64..=127).contains(&b) {
        return Some(IpClass::SharedAddressSpace);
    }
    if a == 169 && b == 254 {
        return Some(IpClass::LinkLocal);
    }
    if (224..=239).contains(&a) {
        return Some(IpClass::Multicast);
    }
    // Protocolo (192.0.0.0/24), documentacion (TEST-NET), pruebas de
    // rendimiento (198.18.0.0/15) y reservado (240.0.0.0/4).
    if (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 240
    {
        return Some(IpClass::Reserved);
    }
    None
}

fn classify_v6(ip: Ipv6Addr) -> Option<IpClass> {
    if ip.is_unspecified() {
        return Some(IpClass::Unspecified);
    }
    if ip.is_loopback() {
        return Some(IpClass::Loopback);
    }
    // Una IPv4 dentro de una IPv6 se clasifica por la IPv4.
    if let Some(inner) = ip.to_ipv4_mapped() {
        return classify_v4(inner).or(Some(IpClass::Reserved));
    }
    let segments = ip.segments();
    // NAT64 (64:ff9b::/96): lleva la IPv4 en los ultimos 32 bits.
    if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6].iter().all(|s| *s == 0) {
        let inner = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        return classify_v4(inner).or(Some(IpClass::Reserved));
    }
    // 6to4 (2002::/16): lleva la IPv4 en los bits 16..48.
    if segments[0] == 0x2002 {
        let inner = Ipv4Addr::new(
            (segments[1] >> 8) as u8,
            segments[1] as u8,
            (segments[2] >> 8) as u8,
            segments[2] as u8,
        );
        return classify_v4(inner).or(Some(IpClass::Reserved));
    }
    if (segments[0] & 0xfe00) == 0xfc00 {
        return Some(IpClass::UniqueLocal);
    }
    if (segments[0] & 0xffc0) == 0xfe80 {
        return Some(IpClass::LinkLocal);
    }
    if (segments[0] & 0xff00) == 0xff00 {
        return Some(IpClass::Multicast);
    }
    // Reservados que un traductor de red podria llevar a IPv4 interna o que
    // no son de nadie: documentacion 2001:db8::/32, Teredo 2001::/32,
    // NAT64 local 64:ff9b:1::/48, descarte 100::/64, site-local fec0::/10,
    // IPv4 traducida ::ffff:0:0:0/96 y la IPv4 compatible, deprecada.
    let reserved = (segments[0] == 0x2001 && (segments[1] == 0x0db8 || segments[1] == 0))
        || (segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 1)
        || (segments[0] == 0x0100 && segments[1..4].iter().all(|s| *s == 0))
        || (segments[0] & 0xffc0) == 0xfec0
        || (segments[..4].iter().all(|s| *s == 0) && segments[4] == 0xffff && segments[5] == 0)
        || segments[..6].iter().all(|s| *s == 0);
    if reserved {
        return Some(IpClass::Reserved);
    }
    None
}

/// Quien resuelve nombres.
///
/// Es un puerto para poder probar el rebind con un resolutor que responde
/// lo que el test quiere: el del sistema es [`SystemResolver`].
#[async_trait]
pub trait HostResolver: Send + Sync {
    /// Las direcciones de un nombre.
    async fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<IpAddr>>;
}

/// El resolutor del sistema.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemResolver;

#[async_trait]
impl HostResolver for SystemResolver {
    async fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<IpAddr>> {
        let addresses = tokio::net::lookup_host((host, port)).await?;
        Ok(addresses.map(|address| address.ip()).collect())
    }
}

/// La politica de salida del servicio.
#[derive(Debug, Clone, Default)]
pub struct EgressPolicy {
    allowed: Vec<AllowedHost>,
}

/// La politica no es valida.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the egress allow-list is not valid: {0}")]
pub struct EgressPolicyError(pub String);

impl EgressPolicy {
    /// Construye la politica, o dice que entrada no tiene sentido.
    ///
    /// Una lista que no se entiende impide arrancar: una politica de
    /// seguridad ilegible no se interpreta con generosidad.
    pub fn new(allowed: Vec<AllowedHost>) -> Result<Self, EgressPolicyError> {
        let mut normalized = Vec::with_capacity(allowed.len());
        for entry in allowed {
            let host = entry.host.trim().trim_end_matches('.').to_ascii_lowercase();
            // Una IPv6 literal se escribe sin corchetes en la lista.
            let host = host
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
                .map_or(host.clone(), str::to_string);
            let well_formed = is_well_formed_host(&host);
            if !well_formed {
                return Err(EgressPolicyError(
                    "a host is empty, has a wildcard, a scheme, a port or a path".to_string(),
                ));
            }
            if entry.port == Some(0) {
                return Err(EgressPolicyError("port 0 is not a port".to_string()));
            }
            normalized.push(AllowedHost { host, ..entry });
        }
        Ok(Self {
            allowed: normalized,
        })
    }

    /// Si la lista esta vacia: el servicio no sale a ningun sitio.
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    /// Valida la URL contra la lista, sin resolver nada.
    ///
    /// Devuelve la entrada que la permite. Una IP literal se valida aqui
    /// mismo contra su clase; un nombre se valida despues, con lo que
    /// resuelva ([`Self::validate_addresses`]).
    pub fn check_url(&self, url: &Url) -> Result<&AllowedHost, EgressDenied> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(EgressDenied::Scheme);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(EgressDenied::Credentials);
        }
        let host = url.host().ok_or(EgressDenied::NoHost)?;
        let port = url
            .port_or_known_default()
            .ok_or(EgressDenied::PortNotAllowed)?;

        let mut host_listed = false;
        let mut entry = None;
        for candidate in &self.allowed {
            if !host_matches(&host, &candidate.host) {
                continue;
            }
            host_listed = true;
            let allowed_port = candidate.port.unwrap_or(default_port(url.scheme()));
            if allowed_port == port {
                entry = Some(candidate);
                break;
            }
        }
        let entry = match (entry, host_listed) {
            (Some(entry), _) => entry,
            (None, true) => return Err(EgressDenied::PortNotAllowed),
            (None, false) => return Err(EgressDenied::HostNotAllowed),
        };
        if url.scheme() == "http" && !entry.allow_http {
            return Err(EgressDenied::PlainHttp);
        }
        // Una IP literal no pasa por el resolutor: se juzga ya.
        match host {
            Host::Ipv4(ip) => Self::validate_addresses(entry, &[IpAddr::V4(ip)])?,
            Host::Ipv6(ip) => Self::validate_addresses(entry, &[IpAddr::V6(ip)])?,
            Host::Domain(_) => {}
        }
        Ok(entry)
    }

    /// Valida las direcciones a las que resolvio un host.
    ///
    /// **Todas** tienen que ser admisibles: un nombre que resuelve a una
    /// direccion publica y a otra interna es el truco del rebind, y no se
    /// deja a la suerte de cual elija el cliente.
    pub fn validate_addresses(
        entry: &AllowedHost,
        addresses: &[IpAddr],
    ) -> Result<(), EgressDenied> {
        if addresses.is_empty() {
            return Err(EgressDenied::Unresolved);
        }
        for address in addresses {
            if let Some(class) = classify(*address)
                && !(entry.allow_private && class.can_be_allowed())
            {
                return Err(EgressDenied::Address(class));
            }
        }
        Ok(())
    }
}

/// Si el texto es un nombre de host o una IP literal, y nada mas.
fn is_well_formed_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

fn default_port(scheme: &str) -> u16 {
    if scheme == "https" { 443 } else { 80 }
}

fn host_matches(host: &Host<&str>, listed: &str) -> bool {
    match host {
        Host::Domain(domain) => domain.trim_end_matches('.').eq_ignore_ascii_case(listed),
        Host::Ipv4(ip) => listed.parse::<IpAddr>().is_ok_and(|l| l == IpAddr::V4(*ip)),
        Host::Ipv6(ip) => listed.parse::<IpAddr>().is_ok_and(|l| l == IpAddr::V6(*ip)),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn an_entry_is_read_from_its_text_form() {
        let entry = AllowedHost::from_spec("127.0.0.1:8080;private;http").expect("entry");
        assert_eq!(entry.host, "127.0.0.1");
        assert_eq!(entry.port, Some(8080));
        assert!(entry.allow_private && entry.allow_http);

        let plain = AllowedHost::from_spec("repo.example.com").expect("entry");
        assert_eq!(plain.port, None);
        assert!(!plain.allow_private && !plain.allow_http);

        let v6 = AllowedHost::from_spec("[::1]:9000;private").expect("entry");
        assert_eq!(v6.host, "::1");
        assert_eq!(v6.port, Some(9000));

        // Una opcion de seguridad mal escrita no se ignora.
        assert!(AllowedHost::from_spec("a.example;privat").is_err());
        assert!(AllowedHost::from_spec("a.example:port").is_err());
    }

    #[test]
    fn an_entry_is_read_from_both_config_forms() {
        let list: Vec<AllowedHost> = serde_json::from_value(serde_json::json!([
            "a.example;http",
            { "host": "b.example", "port": 8443, "allow_private": true }
        ]))
        .expect("list");
        assert!(list[0].allow_http);
        assert_eq!(list[1].port, Some(8443));
        // Un campo que no existe no se ignora.
        assert!(
            serde_json::from_value::<Vec<AllowedHost>>(serde_json::json!([
                { "host": "b.example", "allow_privte": true }
            ]))
            .is_err()
        );
    }
    use super::*;

    fn host(name: &str) -> AllowedHost {
        AllowedHost {
            host: name.to_string(),
            port: None,
            allow_private: false,
            allow_http: false,
        }
    }

    fn url(text: &str) -> Url {
        Url::parse(text).expect("url")
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("ip")
    }

    #[test]
    fn the_non_public_ranges_are_classified() {
        for (text, class) in [
            ("127.0.0.1", IpClass::Loopback),
            ("127.255.255.254", IpClass::Loopback),
            ("10.1.2.3", IpClass::Private),
            ("172.16.0.1", IpClass::Private),
            ("172.31.255.255", IpClass::Private),
            ("192.168.1.1", IpClass::Private),
            ("100.64.0.1", IpClass::SharedAddressSpace),
            ("169.254.169.254", IpClass::LinkLocal),
            ("0.0.0.0", IpClass::Unspecified),
            ("224.0.0.1", IpClass::Multicast),
            ("255.255.255.255", IpClass::Broadcast),
            ("240.0.0.1", IpClass::Reserved),
            ("192.0.2.1", IpClass::Reserved),
            ("198.18.0.1", IpClass::Reserved),
            ("::", IpClass::Unspecified),
            ("::1", IpClass::Loopback),
            ("fd00:ec2::254", IpClass::UniqueLocal),
            ("fe80::1", IpClass::LinkLocal),
            ("ff02::1", IpClass::Multicast),
            ("2001:db8::1", IpClass::Reserved),
            // Lo que un traductor de red podria llevar a IPv4 interna.
            ("2001::1", IpClass::Reserved),
            ("64:ff9b:1::1", IpClass::Reserved),
            ("100::1", IpClass::Reserved),
            ("fec0::1", IpClass::Reserved),
            ("::ffff:0:7f00:1", IpClass::Reserved),
            // Una IPv4 dentro de una IPv6 se juzga por la IPv4.
            ("::ffff:127.0.0.1", IpClass::Loopback),
            ("::ffff:10.0.0.1", IpClass::Private),
            ("::ffff:169.254.169.254", IpClass::LinkLocal),
            ("64:ff9b::7f00:1", IpClass::Loopback),
            ("2002:7f00:1::1", IpClass::Loopback),
        ] {
            assert_eq!(classify(ip(text)), Some(class), "{text}");
        }
    }

    #[test]
    fn the_public_addresses_are_not_classified() {
        for text in [
            "8.8.8.8",
            "1.1.1.1",
            "172.15.0.1",
            "172.32.0.1",
            "100.63.0.1",
            "100.128.0.1",
            "203.0.114.1",
            "2606:4700:4700::1111",
        ] {
            assert_eq!(classify(ip(text)), None, "{text}");
        }
    }

    #[test]
    fn a_host_outside_the_list_is_denied() {
        let policy = EgressPolicy::new(vec![host("repo.example.org")]).expect("policy");
        assert!(policy.check_url(&url("https://repo.example.org/a")).is_ok());
        assert!(
            policy
                .check_url(&url("https://REPO.example.org./a"))
                .is_ok()
        );
        assert_eq!(
            policy.check_url(&url("https://evil.example.org/a")),
            Err(EgressDenied::HostNotAllowed)
        );
        // Un subdominio no hereda la entrada del dominio.
        assert_eq!(
            policy.check_url(&url("https://x.repo.example.org/a")),
            Err(EgressDenied::HostNotAllowed)
        );
        assert_eq!(
            policy.check_url(&url("https://repo.example.org:8443/a")),
            Err(EgressDenied::PortNotAllowed)
        );
    }

    #[test]
    fn an_empty_list_goes_nowhere() {
        let policy = EgressPolicy::new(Vec::new()).expect("policy");
        assert!(policy.is_empty());
        assert_eq!(
            policy.check_url(&url("https://repo.example.org/a")),
            Err(EgressDenied::HostNotAllowed)
        );
    }

    #[test]
    fn plain_http_needs_its_own_permission() {
        let policy = EgressPolicy::new(vec![host("repo.example.org")]).expect("policy");
        assert_eq!(
            policy.check_url(&url("http://repo.example.org/a")),
            Err(EgressDenied::PlainHttp)
        );
        let mut open = host("repo.example.org");
        open.allow_http = true;
        let policy = EgressPolicy::new(vec![open]).expect("policy");
        assert!(policy.check_url(&url("http://repo.example.org/a")).is_ok());
    }

    #[test]
    fn other_schemes_and_credentials_are_denied() {
        let policy = EgressPolicy::new(vec![host("repo.example.org")]).expect("policy");
        assert_eq!(
            policy.check_url(&url("ftp://repo.example.org/a")),
            Err(EgressDenied::Scheme)
        );
        assert_eq!(
            policy.check_url(&url("https://user:pw@repo.example.org/a")),
            Err(EgressDenied::Credentials)
        );
    }

    #[test]
    fn an_ip_literal_is_judged_without_resolving() {
        let policy =
            EgressPolicy::new(vec![host("127.0.0.1"), host("169.254.169.254")]).expect("policy");
        // Esta en la lista, pero es el bucle local y la entrada no lo permite.
        assert_eq!(
            policy.check_url(&url("https://127.0.0.1/a")),
            Err(EgressDenied::Address(IpClass::Loopback))
        );
        // Y los metadatos de la nube no los abre ningun permiso.
        let mut metadata = host("169.254.169.254");
        metadata.allow_private = true;
        let policy = EgressPolicy::new(vec![metadata]).expect("policy");
        assert_eq!(
            policy.check_url(&url("https://169.254.169.254/latest/meta-data")),
            Err(EgressDenied::Address(IpClass::LinkLocal))
        );
    }

    #[test]
    fn the_private_permission_is_per_entry() {
        let mut local = host("127.0.0.1");
        local.allow_private = true;
        local.allow_http = true;
        local.port = Some(8080);
        let policy = EgressPolicy::new(vec![local, host("repo.example.org")]).expect("policy");
        assert!(policy.check_url(&url("http://127.0.0.1:8080/a")).is_ok());
        // Otro puerto del mismo host no esta permitido.
        assert_eq!(
            policy.check_url(&url("http://127.0.0.1:9090/a")),
            Err(EgressDenied::PortNotAllowed)
        );
        // Y el permiso de una entrada no abre las demas.
        let entry = policy
            .check_url(&url("https://repo.example.org/a"))
            .expect("allowed");
        assert_eq!(
            EgressPolicy::validate_addresses(entry, &[ip("10.0.0.5")]),
            Err(EgressDenied::Address(IpClass::Private))
        );
    }

    #[test]
    fn one_forbidden_address_is_enough_to_deny_the_name() {
        let entry = host("repo.example.org");
        assert_eq!(
            EgressPolicy::validate_addresses(&entry, &[ip("8.8.8.8"), ip("127.0.0.1")]),
            Err(EgressDenied::Address(IpClass::Loopback))
        );
        assert!(EgressPolicy::validate_addresses(&entry, &[ip("8.8.8.8")]).is_ok());
        assert_eq!(
            EgressPolicy::validate_addresses(&entry, &[]),
            Err(EgressDenied::Unresolved)
        );
    }

    #[test]
    fn the_private_permission_does_not_open_what_is_never_a_destination() {
        let mut entry = host("internal.example.org");
        entry.allow_private = true;
        for text in ["10.0.0.1", "127.0.0.1", "fd00::1", "100.64.0.1"] {
            assert!(
                EgressPolicy::validate_addresses(&entry, &[ip(text)]).is_ok(),
                "{text}"
            );
        }
        for text in [
            "0.0.0.0",
            "169.254.169.254",
            "224.0.0.1",
            "255.255.255.255",
            "fe80::1",
        ] {
            assert!(
                EgressPolicy::validate_addresses(&entry, &[ip(text)]).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn an_allow_list_that_does_not_make_sense_is_refused() {
        for bad in [
            "",
            "*.example.org",
            "https://example.org",
            "example.org/path",
            "exa mple.org",
            "a..b",
        ] {
            assert!(
                EgressPolicy::new(vec![host(bad)]).is_err(),
                "{bad:?} should be refused"
            );
        }
        let mut zero = host("example.org");
        zero.port = Some(0);
        assert!(EgressPolicy::new(vec![zero]).is_err());
        // Una IPv6 se escribe con o sin corchetes.
        assert!(EgressPolicy::new(vec![host("::1"), host("[::1]")]).is_ok());
    }
}
