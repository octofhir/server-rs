//! Guard for requests the server sends to URLs its clients choose: subscription rest-hook
//! endpoints and `$import` sources.
//!
//! Without it anyone allowed to create a Subscription or start an import could make the server
//! call its own network: loopback admin ports, other services on the private network, or the
//! cloud metadata endpoint at 169.254.169.254. An [`EgressPolicy`] allows public addresses, plus
//! the networks and host names listed in `server.outbound_allowlist` (for example a source
//! service on the same Docker network), or everything with `server.allow_private_outbound`.
//! Two checks enforce it:
//! - [`EgressPolicy::check_url`] rejects URLs whose host is a disallowed IP literal;
//! - [`EgressPolicy::client_builder`] installs a resolver that drops disallowed addresses from
//!   DNS answers. Connections only go to the addresses it returns, so a name that later
//!   resolves elsewhere (DNS rebinding) is checked on every connection.
//!
//! Redirects are the caller's concern: a redirect to an IP literal bypasses the resolver, so
//! clients built here either don't follow redirects or re-check each target with `check_url`.

use std::error::Error as StdError;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;

use ipnetwork::IpNetwork;
use reqwest::Url;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use crate::config::ServerConfig;

/// Whether `ip` is reachable on the public internet (not loopback, private, link-local,
/// shared, multicast, documentation or otherwise reserved space).
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(mapped) => is_public_v4(mapped),
            None => is_public_v6(ip),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        // Shared address space (carrier-grade NAT), 100.64.0.0/10
        || (a == 100 && (64..=127).contains(&b))
        // IETF protocol assignments, 192.0.0.0/24
        || (a == 192 && b == 0 && c == 0)
        // Benchmarking, 198.18.0.0/15
        || (a == 198 && (b == 18 || b == 19))
        // Reserved, 240.0.0.0/4
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    // NAT64 (64:ff9b::/96) reaches the IPv4 address in the low 32 bits.
    if ip.segments()[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        let [.., a, b, c, d] = ip.octets();
        return is_public_v4(Ipv4Addr::new(a, b, c, d));
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        // Unique local, fc00::/7
        || (first & 0xfe00) == 0xfc00
        // Link-local, fe80::/10
        || (first & 0xffc0) == 0xfe80
        // Documentation, 2001:db8::/32
        || (first == 0x2001 && ip.segments()[1] == 0x0db8))
}

/// Which addresses outbound requests to client-chosen URLs may reach.
#[derive(Debug, Clone, Default)]
pub struct EgressPolicy {
    allow_private: bool,
    networks: Vec<IpNetwork>,
    /// Lower-case host names; a leading `*.` matches any subdomain.
    hosts: Vec<String>,
}

impl EgressPolicy {
    /// `allowlist` entries are networks (`10.0.0.0/8`), single addresses (`172.18.0.5`), host
    /// names (`etl-service`) or subdomain patterns (`*.internal.example`).
    pub fn new(allow_private: bool, allowlist: &[String]) -> Result<Self, String> {
        let mut policy = Self {
            allow_private,
            ..Self::default()
        };
        for entry in allowlist.iter().map(|entry| entry.trim()) {
            if entry.is_empty() {
                continue;
            }
            if let Ok(network) = entry.parse::<IpNetwork>() {
                policy.networks.push(network);
            } else if entry.contains('/') || entry.parse::<IpAddr>().is_ok() {
                return Err(format!(
                    "server.outbound_allowlist: {entry:?} is not a valid network"
                ));
            } else {
                policy.hosts.push(entry.to_ascii_lowercase());
            }
        }
        Ok(policy)
    }

    pub fn from_server_config(server: &ServerConfig) -> Result<Self, String> {
        Self::new(server.allow_private_outbound, &server.outbound_allowlist)
    }

    /// Allows everything; for tests that talk to local mock servers.
    pub fn permissive() -> Self {
        Self {
            allow_private: true,
            ..Self::default()
        }
    }

    fn allows_ip(&self, ip: IpAddr) -> bool {
        self.allow_private
            || is_public(ip)
            || self.networks.iter().any(|network| network.contains(ip))
    }

    fn allows_host(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        self.hosts
            .iter()
            .any(|pattern| match pattern.strip_prefix("*.") {
                Some(suffix) => host.ends_with(&format!(".{suffix}")),
                None => host == *pattern,
            })
    }

    /// Rejects a URL whose host is a disallowed IP literal. Host names are checked when they
    /// are resolved (see [`Self::client_builder`]).
    pub fn check_url(&self, url: &Url) -> Result<(), String> {
        if !matches!(url.scheme(), "http" | "https" | "ws" | "wss")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Outbound URL must use HTTP(S)/WS(S) without user information".into());
        }
        let ip = match url.host() {
            Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip),
            Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip),
            Some(url::Host::Domain(_)) => return Ok(()),
            None => return Err("URL has no host".into()),
        };
        if self.allows_ip(ip) {
            Ok(())
        } else {
            Err(format!(
                "{ip} is a private or reserved address; allow it in server.outbound_allowlist"
            ))
        }
    }

    /// Resolve and connect a WebSocket transport without allowing its client
    /// library to re-resolve an unchecked address. TLS still uses the original
    /// hostname. Redirects and proxies are not involved in this TCP boundary.
    pub(crate) async fn connect_tcp(&self, url: &Url) -> Result<tokio::net::TcpStream, String> {
        self.check_url(url)?;
        let host = url.host_str().ok_or("Outbound host is missing")?;
        let port = url
            .port_or_known_default()
            .ok_or("Outbound port is missing")?;
        let addresses = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| "Outbound DNS resolution failed")?;
        for address in addresses
            .filter(|address| self.allows_host(host) || self.allows_ip(address.ip()))
            .take(16)
        {
            if let Ok(Ok(stream)) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::net::TcpStream::connect(address),
            )
            .await
            {
                return Ok(stream);
            }
        }
        Err("No permitted outbound address is reachable".into())
    }

    /// A client builder whose connections only reach addresses this policy allows.
    pub fn client_builder(self: &Arc<Self>) -> reqwest::ClientBuilder {
        // Redirects can bypass the resolver with an IP literal, and proxies can
        // resolve unchecked destinations themselves. Validate the initial URL
        // with check_url before sending, too.
        let builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
        if self.allow_private {
            builder
        } else {
            builder
                .no_proxy()
                .dns_resolver(Arc::new(PolicyResolver(self.clone())))
        }
    }
}

/// DNS resolver that keeps only the addresses an [`EgressPolicy`] allows. An allowlisted host
/// name may resolve anywhere.
#[derive(Debug, Clone)]
struct PolicyResolver(Arc<EgressPolicy>);

impl Resolve for PolicyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.0.clone();
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let resolved: Vec<SocketAddr> = tokio::task::spawn_blocking({
                let host = host.clone();
                move || (host.as_str(), 0).to_socket_addrs().map(Iterator::collect)
            })
            .await??;
            let trusted_name = policy.allows_host(&host);
            let allowed: Vec<SocketAddr> = resolved
                .into_iter()
                .filter(|address| trusted_name || policy.allows_ip(address.ip()))
                .collect();
            if allowed.is_empty() {
                return Err(format!(
                    "{host} resolves only to private or reserved addresses; allow it in \
                     server.outbound_allowlist"
                )
                .into());
            }
            let addrs: Addrs = Box::new(allowed.into_iter());
            Ok(addrs)
        })
    }
}

/// An error with its causes, `a: b: c`. reqwest's own message hides why a connection failed
/// (including a refusal from the resolver above).
pub fn describe_error(error: &(dyn StdError + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(allow_private: bool, allowlist: &[&str]) -> EgressPolicy {
        let allowlist: Vec<String> = allowlist.iter().map(|entry| entry.to_string()).collect();
        EgressPolicy::new(allow_private, &allowlist).unwrap()
    }

    #[test]
    fn classifies_addresses() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a9fe:a9fe",
        ] {
            assert!(
                !is_public(private.parse().unwrap()),
                "{private} should be private"
            );
        }
        for public in [
            "93.184.216.34",
            "8.8.8.8",
            "2606:4700::1111",
            "64:ff9b::808:808",
        ] {
            assert!(
                is_public(public.parse().unwrap()),
                "{public} should be public"
            );
        }
    }

    #[test]
    fn allowlisted_networks_and_hosts_are_reachable() {
        let url = |text: &str| Url::parse(text).unwrap();
        let strict = policy(false, &[]);
        assert!(
            strict
                .check_url(&url("http://169.254.169.254/latest/meta-data"))
                .is_err()
        );
        assert!(strict.check_url(&url("http://[::1]:8888/admin")).is_err());
        assert!(
            strict
                .check_url(&url("https://hooks.example.org/x"))
                .is_ok()
        );

        let docker = policy(
            false,
            &["172.18.0.0/16", "etl-service", "*.internal.example"],
        );
        assert!(
            docker
                .check_url(&url("http://172.18.4.2:8080/Patient.ndjson"))
                .is_ok()
        );
        assert!(docker.check_url(&url("http://172.19.0.1/")).is_err());
        assert!(docker.allows_host("etl-service"));
        assert!(docker.allows_host("api.internal.example."));
        assert!(!docker.allows_host("internal.example"));

        assert!(
            policy(true, &[])
                .check_url(&url("http://127.0.0.1:8888/"))
                .is_ok()
        );
    }

    #[test]
    fn rejects_malformed_networks() {
        assert!(EgressPolicy::new(false, &["10.0.0.0/40".to_string()]).is_err());
        assert!(EgressPolicy::new(false, &["10.0.0/8".to_string()]).is_err());
    }

    #[test]
    fn rejects_url_credentials_and_non_http_transports() {
        let strict = policy(false, &[]);
        for url in [
            "https://user:secret@example.org/",
            "ftp://example.org/",
            "file:///etc/passwd",
        ] {
            assert!(strict.check_url(&Url::parse(url).unwrap()).is_err());
        }
    }

    #[tokio::test]
    async fn resolver_refuses_names_that_only_reach_private_space() {
        let strict = PolicyResolver(Arc::new(policy(false, &[])));
        assert!(strict.resolve("localhost".parse().unwrap()).await.is_err());
        let trusted = PolicyResolver(Arc::new(policy(false, &["localhost"])));
        assert!(trusted.resolve("localhost".parse().unwrap()).await.is_ok());
    }
}
