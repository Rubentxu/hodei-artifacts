//! The service's egress policy: where a request may go.
//!
//! A proxy repository makes the **service** go out to the network to fetch
//! content on behalf of a client. That egress is the classic SSRF surface:
//! what the client (or a hostile upstream, via a redirect) gets the service
//! to visit using the service's network. That is why the decision of where
//! to go is not made by a repository's configuration but by the **service
//! administrator**, in an allowlist of
//! hosts, and it is applied on every hop.
//!
//! Two rules underpin everything else:
//!
//! - A host is only visited if it is in the list, with its port and its
//!   scheme.
//! - A private or loopback address is only visited if that host's entry
//!   **explicitly** allows it (`allow_private`). Addresses that are never a
//!   legitimate destination (unspecified, multicast, broadcast, link-local
//!   and the cloud metadata service, reserved) have no permission that
//!   opens them.
//!
//! Resolution is validated and pinned **before** connecting: the connection
//! is made to the addresses that were validated, so a DNS that changes its
//! answer between the check and the connection (rebind) has no
//! window.

use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::{Host, Url};

/// An entry in the allowed-hosts list.
///
/// In a configuration file it is an object
/// (`{ host = "...", port = 8080, allow_private = true }`). In an environment
/// variable, where a list can only be strings, it is
/// `host[:port][;private][;http]`, for example
/// `HODEI_PROXY__ALLOWED_HOSTS=repo.example.com,127.0.0.1:8080;private;http`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "AllowedHostSpec")]
pub struct AllowedHost {
    /// The host name, or the literal IP. No scheme, no port and no
    /// wildcards.
    pub host: String,
    /// The port. Without a port, only the scheme's own (443 or 80).
    #[serde(default)]
    pub port: Option<u16>,
    /// Allows this host to be (or resolve to) a private or loopback
    /// address. It is the explicit administrative permission for egress to the
    /// internal network; without it, those addresses are rejected.
    #[serde(default)]
    pub allow_private: bool,
    /// Allows `http` without TLS for this host.
    #[serde(default)]
    pub allow_http: bool,
}

/// The kind of an address that is not public.
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
    /// `169.254.0.0/16` (includes the cloud metadata service) and
    /// `fe80::/10`.
    LinkLocal,
    /// Multicast.
    Multicast,
    /// `255.255.255.255`.
    Broadcast,
    /// Documentation, benchmarking, protocol and reserved.
    Reserved,
}

impl IpClass {
    /// Whether an administrative permission can open this class.
    ///
    /// Only the legitimate internal network can be opened: loopback, private
    /// networks, shared space and ULA. Everything else is not a destination.
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

/// The two ways to write an entry.
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
    /// Reads `host[:port][;private][;http]`.
    ///
    /// A literal IPv6 takes brackets if it has a port (`[::1]:8080`).
    /// An unknown flag is an error: a misspelled security option
    /// is not ignored.
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
            // `[v6]` or `[v6]:port`
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

/// Why an egress is denied.
///
/// The message is for the service trace: it carries neither the whole URL nor
/// anything that could be a secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EgressDenied {
    /// The scheme is neither `http` nor `https`.
    #[error("the scheme is not http or https")]
    Scheme,
    /// The URL carries a user or password.
    #[error("the url carries credentials")]
    Credentials,
    /// The URL has no host.
    #[error("the url has no host")]
    NoHost,
    /// The host is not in the list.
    #[error("the host is not in the allow-list")]
    HostNotAllowed,
    /// The port is not the one allowed for that host.
    #[error("the port is not the one allowed for that host")]
    PortNotAllowed,
    /// `http` without the entry allowing it.
    #[error("plain http is not allowed for that host")]
    PlainHttp,
    /// The host is, or resolves to, an address that is not visited.
    #[error("the host is or resolves to an address that is not allowed ({0})")]
    Address(IpClass),
    /// The host did not resolve to any address.
    #[error("the host did not resolve")]
    Unresolved,
}

/// The class of an address, or `None` if it is public.
///
/// Addresses that embed an IPv4 (mapped, NAT64 and 6to4) are
/// classified by the IPv4 they carry: `::ffff:127.0.0.1` is loopback,
/// not just any IPv6 address.
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
    // Protocol (192.0.0.0/24), documentation (TEST-NET), benchmarking
    // (198.18.0.0/15) and reserved (240.0.0.0/4).
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
    // An IPv4 inside an IPv6 is classified by the IPv4.
    if let Some(inner) = ip.to_ipv4_mapped() {
        return classify_v4(inner).or(Some(IpClass::Reserved));
    }
    let segments = ip.segments();
    // NAT64 (64:ff9b::/96): carries the IPv4 in the last 32 bits.
    if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6].iter().all(|s| *s == 0) {
        let inner = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        return classify_v4(inner).or(Some(IpClass::Reserved));
    }
    // 6to4 (2002::/16): carries the IPv4 in bits 16..48.
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
    // Reserved ranges that a network translator could map to an internal IPv4 or that
    // belong to nobody: documentation 2001:db8::/32, Teredo 2001::/32,
    // local NAT64 64:ff9b:1::/48, discard 100::/64, site-local fec0::/10,
    // translated IPv4 ::ffff:0:0:0/96 and IPv4-compatible, deprecated.
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

/// Who resolves names.
///
/// It is a port so rebind can be tested with a resolver that answers
/// what the test wants: the system one is [`SystemResolver`].
#[async_trait]
pub trait HostResolver: Send + Sync {
    /// The addresses of a name.
    async fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<IpAddr>>;
}

/// The system resolver.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemResolver;

#[async_trait]
impl HostResolver for SystemResolver {
    async fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<IpAddr>> {
        let addresses = tokio::net::lookup_host((host, port)).await?;
        Ok(addresses.map(|address| address.ip()).collect())
    }
}

/// The service's egress policy.
#[derive(Debug, Clone, Default)]
pub struct EgressPolicy {
    allowed: Vec<AllowedHost>,
}

/// The policy is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the egress allow-list is not valid: {0}")]
pub struct EgressPolicyError(pub String);

impl EgressPolicy {
    /// Builds the policy, or says which entry makes no sense.
    ///
    /// A list that is not understood prevents startup: an unreadable security
    /// policy is not interpreted generously.
    pub fn new(allowed: Vec<AllowedHost>) -> Result<Self, EgressPolicyError> {
        let mut normalized = Vec::with_capacity(allowed.len());
        for entry in allowed {
            let host = entry.host.trim().trim_end_matches('.').to_ascii_lowercase();
            // A literal IPv6 is written without brackets in the list.
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

    /// Whether the list is empty: the service goes nowhere.
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    /// Validates the URL against the list, without resolving anything.
    ///
    /// Returns the entry that allows it. A literal IP is validated right here
    /// against its class; a name is validated afterwards, with what it
    /// resolves to ([`Self::validate_addresses`]).
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
        // A literal IP does not go through the resolver: it is judged now.
        match host {
            Host::Ipv4(ip) => Self::validate_addresses(entry, &[IpAddr::V4(ip)])?,
            Host::Ipv6(ip) => Self::validate_addresses(entry, &[IpAddr::V6(ip)])?,
            Host::Domain(_) => {}
        }
        Ok(entry)
    }

    /// Validates the addresses a host resolved to.
    ///
    /// **All** of them must be admissible: a name that resolves to a public
    /// address and to an internal one is the rebind trick, and which one the
    /// client picks is not left to chance.
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

/// Whether the text is a host name or a literal IP, and nothing else.
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

        // A misspelled security option is not ignored.
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
        // A field that does not exist is not ignored.
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
            // What a network translator could map to an internal IPv4.
            ("2001::1", IpClass::Reserved),
            ("64:ff9b:1::1", IpClass::Reserved),
            ("100::1", IpClass::Reserved),
            ("fec0::1", IpClass::Reserved),
            ("::ffff:0:7f00:1", IpClass::Reserved),
            // An IPv4 inside an IPv6 is judged by the IPv4.
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
        // A subdomain does not inherit the domain's entry.
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
        // It is in the list, but it is loopback and the entry does not allow it.
        assert_eq!(
            policy.check_url(&url("https://127.0.0.1/a")),
            Err(EgressDenied::Address(IpClass::Loopback))
        );
        // And no permission opens the cloud metadata.
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
        // Another port of the same host is not allowed.
        assert_eq!(
            policy.check_url(&url("http://127.0.0.1:9090/a")),
            Err(EgressDenied::PortNotAllowed)
        );
        // And one entry's permission does not open the others.
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
        // An IPv6 is written with or without brackets.
        assert!(EgressPolicy::new(vec![host("::1"), host("[::1]")]).is_ok());
    }
}
