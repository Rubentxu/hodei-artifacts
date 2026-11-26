//! The public address of the deployment.
//!
//! A registry tells its clients where the artifacts are: the tarball of an
//! npm package, the file of a PyPI release, the address of a NuGet feed, the
//! index of a Cargo repository and the token endpoint of an OCI registry are
//! all **absolute** URLs, and all of them are built out of the request the
//! client made.
//!
//! A request is not a trustworthy source of that address. A client can send
//! any `Host` and any `X-Forwarded-Proto` it wants, and a URL that a client
//! can move is a URL that a client can poison: the link of a package would
//! point somewhere else, and the credential of the registry would be asked
//! for somewhere else.
//!
//! So the origin is decided **here**, in one place, from what the
//! deployment declares and from what a proxy the deployment trusts says:
//!
//! - the **public base URL** the deployment declares, when it declares one;
//! - the **forwarded headers**, and only when the peer of the request is a
//!   proxy the deployment put in its list of trusted proxies.
//!
//! Trust is never the default. A deployment that says nothing about its
//! proxies hears nothing from a forwarded header, and a request whose peer
//! is unknown is never a trusted proxy.
//!
//! The `Host` of the request is the last resort, and only when it is a
//! valid authority: an authority with a control character, with embedded
//! credentials or with a path in it is not an authority, and what is not an
//! authority is never reflected in a response.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRef, FromRequestParts};
use axum::http::{HeaderMap, header};

use crate::app_state::AppState;

/// The longest authority that is read: a hostname, an address and a port
/// do not need more, and a longer one is not an authority but an attempt.
const THE_MAXIMUM_AUTHORITY: usize = 255;

/// The header that carries the scheme the client saw.
const FORWARDED_PROTO: &str = "x-forwarded-proto";

/// The header that carries the authority the client saw.
const FORWARDED_HOST: &str = "x-forwarded-host";

/// The scheme of an address that is neither declared nor forwarded.
const THE_PLAIN_SCHEME: &str = "http";

/// The scheme and the authority of the public address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    scheme: String,
    authority: String,
}

impl Origin {
    /// The canonical origin string (`scheme://authority`).
    pub fn as_str(&self) -> String {
        format!("{}://{}", self.scheme, self.authority)
    }

    /// The absolute URL of a path of the deployment.
    ///
    /// The path is a path: a query or a fragment inside it would be a
    /// credential smuggled into a link, so it is cut where it starts.
    pub fn url(&self, path: &str) -> String {
        format!("{}://{}{}", self.scheme, self.authority, the_path(path))
    }
}

/// The path of a public URL: without a query and without a fragment.
fn the_path(path: &str) -> String {
    let path = match path.find(['?', '#']) {
        Some(at) => &path[..at],
        None => path,
    };
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

/// What the deployment says about its public address.
///
/// It is the **resolved** form of the configuration: the base URL already
/// parsed and checked, and the trusted proxies already parsed. A
/// configuration that does not parse does not boot, so everything here is
/// known to be usable.
#[derive(Debug, Clone, Default)]
pub struct PublicUrl {
    /// The canonical public address, when the deployment declares one.
    base: Option<Origin>,
    /// The proxies whose forwarded headers are believed.
    trusted_proxies: Vec<TrustedProxy>,
}

impl PublicUrl {
    /// The public address of the deployment, as the configuration says it.
    ///
    /// # Errors
    ///
    /// A message saying what is wrong when the base URL is not a usable
    /// public address, or when a trusted proxy is not an address or a
    /// network. A configuration that cannot be understood is a
    /// configuration that does not boot.
    pub fn of(base: Option<&str>, trusted_proxies: &[String]) -> Result<Self, String> {
        let base = match base {
            Some(base) => Some(the_base(base)?),
            None => None,
        };
        let trusted_proxies = trusted_proxies
            .iter()
            .map(|proxy| TrustedProxy::of(proxy))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            base,
            trusted_proxies,
        })
    }

    /// The origin of a request, decided by what the deployment declared.
    ///
    /// The peer is the address the request came from, and it is what says
    /// whether the forwarded headers are heard: a request whose peer is
    /// unknown is never a trusted proxy, and its forwarded headers are
    /// ignored.
    pub fn the_origin(&self, peer: Option<SocketAddr>, headers: &HeaderMap) -> Option<Origin> {
        let trusted = self.the_proxy_is_trusted(peer);
        let scheme = if trusted {
            the_forwarded_scheme(headers)
        } else {
            None
        }
        .or_else(|| self.base.as_ref().map(|base| base.scheme.clone()))
        .unwrap_or_else(|| THE_PLAIN_SCHEME.to_string());

        let authority = if trusted {
            the_forwarded_authority(headers)
        } else {
            None
        }
        .or_else(|| self.base.as_ref().map(|base| base.authority.clone()))
        .or_else(|| the_request_authority(headers))?;

        Some(Origin { scheme, authority })
    }
    /// The configured public base URL, if one was declared.
    pub fn public_base_url(&self) -> Option<String> {
        self.base.as_ref().map(Origin::as_str)
    }

    /// Whether the peer of the request is a proxy the deployment trusts.
    fn the_proxy_is_trusted(&self, peer: Option<SocketAddr>) -> bool {
        let Some(peer) = peer else {
            return false;
        };
        self.trusted_proxies
            .iter()
            .any(|proxy| proxy.contains(peer.ip()))
    }
}

/// The public base URL of the deployment, parsed.
///
/// The base is an **origin**: a scheme and an authority. A path, a query or
/// a fragment in it would be part of every URL the registry hands out, and
/// a credential in it would be a credential in every link.
fn the_base(base: &str) -> Result<Origin, String> {
    let parsed = url::Url::parse(base)
        .map_err(|error| format!("the public base URL cannot be read: {error}"))?;
    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "the public base URL has to be an http or an https address, and {scheme} was given"
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("the public base URL cannot carry credentials".to_string());
    }
    let authority = parsed
        .host_str()
        .map(|host| match parsed.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        })
        .ok_or_else(|| "the public base URL has no host".to_string())?;
    if !a_valid_authority(&authority) {
        return Err(format!(
            "the public base URL has an authority that is not one: {authority}"
        ));
    }
    let path = parsed.path();
    if path != "/" && !path.is_empty() {
        return Err(format!(
            "the public base URL has to be an address and not a path, and {path} was given"
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("the public base URL cannot carry a query nor a fragment".to_string());
    }
    Ok(Origin { scheme, authority })
}

/// The scheme the client saw, when a trusted proxy says it.
///
/// The last value of the header is the one the **closest** proxy wrote, and
/// it is the one that describes the connection the client had. Only `http`
/// and `https` are schemes a public address can have.
fn the_forwarded_scheme(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(FORWARDED_PROTO)?.to_str().ok()?;
    let last = value.split(',').next_back()?.trim().to_ascii_lowercase();
    if last == "http" || last == "https" {
        Some(last)
    } else {
        None
    }
}

/// The authority the client saw, when a trusted proxy says it.
///
/// The last value of the header is the one the closest proxy wrote. An
/// authority that is not valid is not used: a proxy that says something
/// that is not an authority has not said where the client is.
fn the_forwarded_authority(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(FORWARDED_HOST)?.to_str().ok()?;
    let last = value.split(',').next_back()?.trim();
    if a_valid_authority(last) {
        Some(last.to_string())
    } else {
        None
    }
}

/// The authority of the request itself, when it is a valid one.
fn the_request_authority(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::HOST)?.to_str().ok()?;
    if a_valid_authority(value) {
        Some(value.to_string())
    } else {
        None
    }
}

/// Whether a string is an authority: a host and, maybe, a port.
///
/// An authority is not a place to put anything else. It cannot carry a
/// control character (a CRLF in it is a header of another request), it
/// cannot carry credentials (an `@` in it is a user and a password), and
/// it cannot carry a path (a `/` in it is a request, not an address).
fn a_valid_authority(authority: &str) -> bool {
    if authority.is_empty() || authority.len() > THE_MAXIMUM_AUTHORITY {
        return false;
    }
    if authority
        .bytes()
        .any(|byte| byte <= 0x20 || byte == 0x7f || byte == b'@')
    {
        return false;
    }
    if authority.contains('/') || authority.contains('?') || authority.contains('#') {
        return false;
    }
    // An IPv6 literal carries its own colons, and its port goes
    // after the closing bracket.
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((literal, after)) = rest.split_once(']') else {
            return false;
        };
        if !a_valid_ipv6_literal(literal) {
            return false;
        }
        return match after {
            "" => true,
            other => match other.strip_prefix(':') {
                Some(port) => a_valid_port(port),
                None => false,
            },
        };
    }
    // An IPv6 without brackets has no port: the colons all belong to it.
    if authority.matches(':').count() > 1 {
        return authority.parse::<IpAddr>().is_ok();
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => a_valid_host(host) && a_valid_port(port),
        None => a_valid_host(authority),
    }
}

/// Whether a string is a port: a number between one and 65535.
fn a_valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|number| number > 0)
}

/// Whether a string is the inside of an IPv6 literal.
fn a_valid_ipv6_literal(literal: &str) -> bool {
    !literal.is_empty()
        && literal
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b':' || byte == b'.')
}

/// Whether a string is a host: a name, an address or an address literal.
fn a_valid_host(host: &str) -> bool {
    if host.is_empty() || host.len() > THE_MAXIMUM_AUTHORITY {
        return false;
    }
    if host.starts_with('[') {
        // An IPv6 literal: `[::1]`.
        let Some(literal) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) else {
            return false;
        };
        return !literal.is_empty()
            && literal
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b':' || byte == b'.');
    }
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

/// A proxy the deployment trusts: an address or a network of addresses.
#[derive(Debug, Clone)]
struct TrustedProxy {
    address: IpAddr,
    /// The bits of the address that are the network. A host has all of them.
    prefix: u8,
}

impl TrustedProxy {
    /// The proxy as the configuration says it.
    fn of(proxy: &str) -> Result<Self, String> {
        let proxy = proxy.trim();
        let (address, prefix) = match proxy.split_once('/') {
            Some((address, prefix)) => {
                let prefix = prefix.trim().parse::<u8>().map_err(|_| {
                    format!("the trusted proxy {proxy} has a prefix that is not a number")
                })?;
                (address.trim(), Some(prefix))
            }
            None => (proxy, None),
        };
        let address = address
            .parse::<IpAddr>()
            .map_err(|_| format!("the trusted proxy {proxy} is not an address"))?;
        let maximum = if address.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(prefix) if prefix <= maximum => prefix,
            Some(_) => {
                return Err(format!(
                    "the trusted proxy {proxy} has a prefix longer than its address"
                ));
            }
            // An address without a prefix is that address and no other.
            None => maximum,
        };
        Ok(Self { address, prefix })
    }

    /// Whether the address is inside the proxy's network.
    fn contains(&self, address: IpAddr) -> bool {
        match (self.address, address) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                the_prefix(&network.octets(), &candidate.octets(), self.prefix)
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                the_prefix(&network.octets(), &candidate.octets(), self.prefix)
            }
            _ => false,
        }
    }
}

/// Whether the first bits of two addresses are the same.
fn the_prefix(network: &[u8], candidate: &[u8], prefix: u8) -> bool {
    let whole = (prefix / 8) as usize;
    if network[..whole] != candidate[..whole] {
        return false;
    }
    let rest = prefix % 8;
    if rest == 0 {
        return true;
    }
    let mask = 0xff_u8 << (8 - rest);
    network[whole] & mask == candidate[whole] & mask
}

/// The public origin of a request, resolved once.
///
/// It is the extractor that carries the answer of [`PublicUrl::the_origin`]
/// to the handler: the handler asks for the URL of a path and gets the one
/// the deployment declared, never one the client chose.
#[derive(Debug, Clone, Default)]
pub struct TheOrigin {
    origin: Option<Origin>,
}

impl TheOrigin {
    /// The origin of a request, for the deployment that says what it says.
    pub fn of(public_url: &PublicUrl, peer: Option<SocketAddr>, headers: &HeaderMap) -> Self {
        Self {
            origin: public_url.the_origin(peer, headers),
        }
    }

    /// The absolute URL of a path, when the deployment could say where it
    /// lives.
    pub fn url(&self, path: &str) -> Option<String> {
        self.origin.as_ref().map(|origin| origin.url(path))
    }

    /// The absolute URL of a path, or the fallback when nothing could be
    /// said about where the deployment lives.
    pub fn url_or(&self, path: &str, fallback: &str) -> String {
        self.url(path).unwrap_or_else(|| fallback.to_string())
    }
}

impl<S> FromRequestParts<S> for TheOrigin
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state);
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|connect| connect.0);
        Ok(Self::of(&state.public_url, peer, &parts.headers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The headers of a request that a client wrote.
    fn the_headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
                axum::http::HeaderValue::from_str(value).expect("a header value"),
            );
        }
        headers
    }

    /// The peer of a request that came from an address.
    ///
    /// The port is the one of an ephemeral client: what says whether the
    /// peer is a trusted proxy is its address, not its port.
    fn the_peer(address: &str) -> Option<SocketAddr> {
        Some(SocketAddr::new(
            address.parse().expect("an address"),
            40_000,
        ))
    }

    /// The public URL of a path, for a deployment that says what it says.
    fn the_url(
        base: Option<&str>,
        proxies: &[&str],
        peer: Option<SocketAddr>,
        headers: &HeaderMap,
        path: &str,
    ) -> Option<String> {
        let proxies: Vec<String> = proxies.iter().map(|proxy| proxy.to_string()).collect();
        let public = PublicUrl::of(base, &proxies).expect("the configuration is usable");
        public
            .the_origin(peer, headers)
            .map(|origin| origin.url(path))
    }

    /// A forwarded header that a client sent on its own changes nothing.
    ///
    /// The scheme of the public address is decided by the deployment, and a
    /// client that says "https" does not get to say it.
    #[test]
    fn a_forwarded_header_of_an_untrusted_peer_is_ignored() {
        let headers = the_headers(&[
            ("host", "registry.example"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "evil.example"),
        ]);
        let url = the_url(
            None,
            &[],
            the_peer("203.0.113.9"),
            &headers,
            "/npm/releases",
        );
        assert_eq!(url.as_deref(), Some("http://registry.example/npm/releases"));
    }

    /// A trusted proxy says the scheme the client saw.
    #[test]
    fn a_trusted_proxy_says_the_scheme_the_client_saw() {
        let headers = the_headers(&[
            ("host", "registry.internal:3000"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "registry.example"),
        ]);
        let url = the_url(
            None,
            &["10.0.0.0/8"],
            the_peer("10.1.2.3"),
            &headers,
            "/npm/releases",
        );
        assert_eq!(
            url.as_deref(),
            Some("https://registry.example/npm/releases")
        );
    }

    /// A proxy that is not trusted is not trusted, even in the same network
    /// as the one that is.
    #[test]
    fn a_proxy_outside_the_trusted_networks_is_not_trusted() {
        let headers = the_headers(&[
            ("host", "registry.internal:3000"),
            ("x-forwarded-proto", "https"),
        ]);
        let url = the_url(
            None,
            &["10.0.0.0/8"],
            the_peer("203.0.113.9"),
            &headers,
            "/npm/releases",
        );
        assert_eq!(
            url.as_deref(),
            Some("http://registry.internal:3000/npm/releases")
        );
    }

    /// The declared base is the public address, and the `Host` of the
    /// request is not reflected in it.
    #[test]
    fn the_declared_base_is_the_public_address() {
        let headers = the_headers(&[("host", "evil.example")]);
        let url = the_url(
            Some("https://artifacts.example"),
            &[],
            the_peer("203.0.113.9"),
            &headers,
            "/npm/releases",
        );
        assert_eq!(
            url.as_deref(),
            Some("https://artifacts.example/npm/releases")
        );
    }

    /// An authority that is not an authority is never reflected.
    #[test]
    fn an_authority_that_is_not_one_is_not_reflected() {
        for hostile in [
            "evil.example/path",
            "evil.example?token=secret",
            "user:secret@evil.example",
            "evil example",
        ] {
            let headers = the_headers(&[("host", hostile)]);
            let url = the_url(
                None,
                &[],
                the_peer("203.0.113.9"),
                &headers,
                "/npm/releases",
            );
            assert_eq!(url, None, "the host {hostile} was reflected");
        }
    }

    /// A control character in the `Host` is not an authority either.
    ///
    /// The value is checked where the decision is taken: a header value
    /// with a control character cannot even be built by the library, but
    /// the guard that refuses it is the one that has to be right.
    #[test]
    fn a_host_with_a_control_character_is_not_an_authority() {
        for hostile in [
            "evil.example\r\nX-Injected: 1",
            "evil.example\nX-Injected: 1",
            "evil.example\0",
            "evil.example ",
        ] {
            assert!(
                !a_valid_authority(hostile),
                "the host {hostile:?} was taken for an authority"
            );
        }
    }

    /// The path of a public URL is a path: a query in it is a credential
    /// that would travel in every link.
    #[test]
    fn a_path_with_a_query_is_cut() {
        let headers = the_headers(&[("host", "registry.example")]);
        let url = the_url(
            None,
            &[],
            the_peer("203.0.113.9"),
            &headers,
            "/npm/releases?token=secret",
        );
        assert_eq!(url.as_deref(), Some("http://registry.example/npm/releases"));
    }

    /// The configuration that cannot be understood is a configuration that
    /// does not boot.
    #[test]
    fn a_base_that_is_not_an_address_is_refused() {
        for hostile in [
            "not a url",
            "ftp://artifacts.example",
            "https://user:secret@artifacts.example",
            "https://artifacts.example/path",
            "https://artifacts.example?token=secret",
            "https://",
        ] {
            assert!(
                PublicUrl::of(Some(hostile), &[]).is_err(),
                "the base {hostile} was accepted"
            );
        }
        assert!(PublicUrl::of(None, &[]).is_ok());
    }

    /// A trusted proxy that is not an address is a configuration that does
    /// not boot.
    #[test]
    fn a_trusted_proxy_that_is_not_an_address_is_refused() {
        for hostile in ["proxy.example", "10.0.0.0/64", "10.0.0.0/x"] {
            assert!(
                PublicUrl::of(None, &[hostile.to_string()]).is_err(),
                "the proxy {hostile} was accepted"
            );
        }
        assert!(PublicUrl::of(None, &["10.0.0.0/8".to_string()]).is_ok());
        assert!(PublicUrl::of(None, &["127.0.0.1".to_string()]).is_ok());
    }

    /// A trusted proxy is the address it names, or the network it names.
    #[test]
    fn a_trusted_proxy_is_an_address_or_a_network() {
        let headers = the_headers(&[
            ("host", "registry.internal"),
            ("x-forwarded-proto", "https"),
        ]);
        let exact = the_url(
            None,
            &["127.0.0.1"],
            the_peer("127.0.0.1"),
            &headers,
            "/npm/releases",
        );
        assert_eq!(
            exact.as_deref(),
            Some("https://registry.internal/npm/releases")
        );
        let other = the_url(
            None,
            &["127.0.0.1"],
            the_peer("127.0.0.2"),
            &headers,
            "/npm/releases",
        );
        assert_eq!(
            other.as_deref(),
            Some("http://registry.internal/npm/releases")
        );
    }
}
