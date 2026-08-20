use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use async_trait::async_trait;
use thiserror::Error;
use tokio::net::lookup_host;
use url::{Host, Url};

/// Maximum provider response retained by a delivery adapter (256 KiB).
pub const MAX_RESPONSE_BODY_BYTES: usize = 256 * 1024;

const DNS_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Provider request after adapter-specific rendering.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Absolute HTTPS provider endpoint.
    pub url: Url,
    /// HTTP headers. `Debug` redacts authorization values.
    pub headers: BTreeMap<String, String>,
    /// Serialized JSON body.
    pub body: Vec<u8>,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers = self
            .headers
            .iter()
            .map(|(name, value)| {
                let value = if name.eq_ignore_ascii_case("authorization") {
                    "[REDACTED]"
                } else {
                    value
                };
                (name, value)
            })
            .collect::<BTreeMap<_, _>>();
        // Provider paths can contain sender identifiers. Keep even an
        // accidental debug log free of phone-number IDs and query secrets.
        let redacted_url = format!("{}://[REDACTED]", self.url.scheme());
        formatter
            .debug_struct("HttpRequest")
            .field("url", &redacted_url)
            .field("headers", &headers)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// Minimal provider response needed by delivery classification.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Numeric HTTP status.
    pub status: u16,
    /// Lower-cased response headers.
    pub headers: BTreeMap<String, String>,
    /// Raw response body, bounded by [`MAX_RESPONSE_BODY_BYTES`] in production.
    pub body: Vec<u8>,
}

impl std::fmt::Debug for HttpResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

impl HttpResponse {
    /// Creates a response without headers.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    /// Whether the provider returned any 2xx status.
    pub const fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }
}

/// HTTP transport failure before a usable provider response existed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("HTTP transport failed: {message}")]
pub struct HttpError {
    /// Redacted transport detail. URLs, credentials, and request bodies are
    /// deliberately omitted.
    pub message: String,
}

impl HttpError {
    fn safe(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Mockable HTTP execution boundary.
///
/// Alternate implementations are trusted transport boundaries. Production
/// delivery uses [`ReqwestExecutor`], which validates and pins public DNS
/// answers before connecting.
#[async_trait]
pub trait HttpExecutor: Send + Sync {
    /// Executes one POST request.
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// Production executor with public-HTTPS-only routing and bounded responses.
///
/// A fresh client is built for each request so the DNS addresses validated as
/// public can be pinned into the connector. Redirects and proxies are disabled:
/// either could otherwise cause a second, unchecked destination lookup.
#[derive(Clone, Copy, Debug)]
pub struct ReqwestExecutor {
    dns_timeout: Duration,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_response_body_bytes: usize,
}

impl ReqwestExecutor {
    fn client_for(
        &self,
        url: &Url,
        resolved_addresses: &[SocketAddr],
    ) -> Result<reqwest::Client, HttpError> {
        let mut builder = brickellstatus_tls::client_builder()
            .map_err(|_| HttpError::safe("secure HTTP client configuration failed"))?
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .user_agent("BrickellStatus/0.1 delivery (+https://github.com/cmiami/BrickellStatus)");

        if let Some(Host::Domain(domain)) = url.host() {
            // The TLS SNI and certificate check still use `domain`; only the
            // connector's DNS answer is replaced with the one checked above.
            builder = builder.resolve_to_addrs(domain, resolved_addresses);
        }

        builder
            .build()
            .map_err(|_| HttpError::safe("secure HTTP client configuration failed"))
    }
}

impl Default for ReqwestExecutor {
    fn default() -> Self {
        Self {
            dns_timeout: DNS_TIMEOUT,
            connect_timeout: CONNECT_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
            max_response_body_bytes: MAX_RESPONSE_BODY_BYTES,
        }
    }
}

#[async_trait]
impl HttpExecutor for ReqwestExecutor {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let resolved_addresses =
            resolve_public_https_endpoint(&request.url, self.dns_timeout).await?;
        let client = self.client_for(&request.url, &resolved_addresses)?;

        let mut builder = client.post(request.url).body(request.body);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        let mut response = builder
            .send()
            .await
            .map_err(|error| redacted_reqwest_error(&error))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect();

        if response
            .content_length()
            .is_some_and(|length| length > self.max_response_body_bytes as u64)
        {
            return Err(response_too_large());
        }

        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| redacted_reqwest_error(&error))?
        {
            append_bounded(&mut body, &chunk, self.max_response_body_bytes)?;
        }

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

pub(crate) fn validate_public_https_url(url: &Url) -> Result<(), HttpError> {
    if url.scheme() != "https" {
        return Err(HttpError::safe("destination must use HTTPS"));
    }
    if authority_contains_userinfo(url) || !url.username().is_empty() || url.password().is_some() {
        return Err(HttpError::safe(
            "destination must not contain embedded credentials",
        ));
    }
    if url.fragment().is_some() {
        return Err(HttpError::safe("destination must not contain a fragment"));
    }
    if url.port() == Some(0) {
        return Err(HttpError::safe("destination port is invalid"));
    }

    let host = url
        .host()
        .ok_or_else(|| HttpError::safe("destination has no host"))?;
    match host {
        Host::Domain(domain) => validate_domain(domain),
        Host::Ipv4(address) if !is_public_ipv4(address) => Err(HttpError::safe(
            "destination IP address is not publicly routable",
        )),
        Host::Ipv6(address) if !is_public_ipv6(address) => Err(HttpError::safe(
            "destination IP address is not publicly routable",
        )),
        Host::Ipv4(_) | Host::Ipv6(_) => Ok(()),
    }
}

pub(crate) async fn resolve_public_https_endpoint(
    url: &Url,
    timeout: Duration,
) -> Result<Vec<SocketAddr>, HttpError> {
    validate_public_https_url(url)?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| HttpError::safe("destination has no valid port"))?;

    let addresses = match url
        .host()
        .ok_or_else(|| HttpError::safe("destination has no host"))?
    {
        Host::Ipv4(address) => vec![SocketAddr::new(IpAddr::V4(address), port)],
        Host::Ipv6(address) => vec![SocketAddr::new(IpAddr::V6(address), port)],
        Host::Domain(domain) => {
            let resolved = tokio::time::timeout(timeout, lookup_host((domain, port)))
                .await
                .map_err(|_| HttpError::safe("destination DNS lookup timed out"))?
                .map_err(|_| HttpError::safe("destination DNS lookup failed"))?;
            resolved.collect()
        }
    };

    validate_resolved_addresses(&addresses)?;
    Ok(addresses)
}

pub(crate) fn enforce_response_body_limit(response: &HttpResponse) -> Result<(), HttpError> {
    if response.body.len() > MAX_RESPONSE_BODY_BYTES {
        return Err(response_too_large());
    }
    Ok(())
}

fn validate_domain(domain: &str) -> Result<(), HttpError> {
    let normalized = domain.trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty()
        || normalized == "localhost"
        || normalized.ends_with(".localhost")
        || normalized.ends_with(".local")
        || normalized.ends_with(".localdomain")
        || normalized.ends_with(".internal")
        || normalized.ends_with(".lan")
        || normalized == "home.arpa"
        || normalized.ends_with(".home.arpa")
        || normalized.ends_with(".test")
        || normalized.ends_with(".invalid")
        || normalized.ends_with(".example")
    {
        return Err(HttpError::safe(
            "destination hostname is not publicly routable",
        ));
    }
    Ok(())
}

fn authority_contains_userinfo(url: &Url) -> bool {
    url.as_str()
        .split_once("://")
        .map(|(_, remainder)| {
            remainder
                .split(['/', '?', '#'])
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
        .unwrap_or(false)
}

fn validate_resolved_addresses(addresses: &[SocketAddr]) -> Result<(), HttpError> {
    if addresses.is_empty() {
        return Err(HttpError::safe(
            "destination DNS lookup returned no addresses",
        ));
    }
    if addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(HttpError::safe(
            "destination DNS resolved to a non-public address",
        ));
    }
    Ok(())
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    // Apply the IPv4 policy to both mapped and deprecated compatible forms.
    if let Some(v4) = address.to_ipv4() {
        return is_public_ipv4(v4);
    }

    let segments = address.segments();
    let well_known_nat64 =
        segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6] == [0, 0, 0, 0];
    let local_use_nat64 = segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 0x0001;
    let global_unicast = (segments[0] & 0xe000) == 0x2000;
    let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    let benchmark = segments[0] == 0x2001 && segments[1] == 0x0002;
    let teredo = segments[0] == 0x2001 && segments[1] == 0;
    let orchid = segments[0] == 0x2001 && matches!(segments[1] & 0xfff0, 0x0010 | 0x0020);
    let six_to_four = segments[0] == 0x2002;

    global_unicast
        && !well_known_nat64
        && !local_use_nat64
        && !documentation
        && !benchmark
        && !teredo
        && !orchid
        && !six_to_four
}

fn redacted_reqwest_error(error: &reqwest::Error) -> HttpError {
    let message = if error.is_timeout() {
        "request timed out"
    } else if error.is_connect() {
        "connection failed"
    } else if error.is_builder() {
        "request could not be constructed"
    } else if error.is_body() || error.is_decode() {
        "provider response could not be read"
    } else {
        "request failed"
    };
    HttpError::safe(message)
}

fn append_bounded(body: &mut Vec<u8>, chunk: &[u8], limit: usize) -> Result<(), HttpError> {
    if body.len().saturating_add(chunk.len()) > limit {
        return Err(response_too_large());
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn response_too_large() -> HttpError {
    HttpError::safe(format!(
        "provider response exceeded the {}-byte limit",
        MAX_RESPONSE_BODY_BYTES
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_https_validation_rejects_ssrf_destinations() {
        for (index, value) in [
            "http://example.com/hook",
            "https://user:secret@example.com/hook",
            "https://user@example.com/hook",
            "https://example.com/hook#fragment",
            "https://localhost/hook",
            "https://service.internal/hook",
            "https://printer.local/hook",
            "https://service.test/hook",
            "https://127.0.0.1/hook",
            "https://10.2.3.4/hook",
            "https://100.100.100.200/latest/meta-data",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/hook",
            "https://[::ffff:127.0.0.1]/hook",
            "https://[::127.0.0.1]/hook",
            "https://[::10.0.0.1]/hook",
            "https://[64:ff9b::7f00:1]/hook",
            "https://[64:ff9b:1::a00:1]/hook",
            "https://[2001:db8::1]/hook",
        ]
        .into_iter()
        .enumerate()
        {
            let url = Url::parse(value).unwrap();
            assert!(
                validate_public_https_url(&url).is_err(),
                "accepted unsafe endpoint case {index}"
            );
        }
    }

    #[test]
    fn public_https_validation_allows_public_ips_and_nonstandard_ports() {
        for value in [
            "https://1.1.1.1/hook",
            "https://8.8.8.8:8443/hook?event=bridge",
            "https://[2606:4700:4700::1111]/hook",
            "https://hooks.example.com/hook",
        ] {
            assert!(validate_public_https_url(&Url::parse(value).unwrap()).is_ok());
        }
    }

    #[test]
    fn mixed_public_and_private_dns_answers_fail_closed() {
        let addresses = [
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 443),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 443),
        ];
        assert!(validate_resolved_addresses(&addresses).is_err());
        assert!(validate_resolved_addresses(&addresses[..1]).is_ok());
        assert!(validate_resolved_addresses(&[]).is_err());
    }

    #[tokio::test]
    async fn literal_ip_resolution_is_offline_and_preserves_the_port() {
        let url = Url::parse("https://1.1.1.1:8443/hook").unwrap();
        let addresses = resolve_public_https_endpoint(&url, Duration::from_millis(1))
            .await
            .unwrap();
        assert_eq!(
            addresses,
            [SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 8443)]
        );
    }

    #[test]
    fn response_accumulation_stops_at_the_fixed_cap() {
        let mut body = vec![0; MAX_RESPONSE_BODY_BYTES - 1];
        append_bounded(&mut body, &[1], MAX_RESPONSE_BODY_BYTES).unwrap();
        assert_eq!(body.len(), MAX_RESPONSE_BODY_BYTES);
        let error = append_bounded(&mut body, &[2], MAX_RESPONSE_BODY_BYTES).unwrap_err();
        assert!(error.message.contains("262144-byte limit"));
        assert_eq!(body.len(), MAX_RESPONSE_BODY_BYTES);
    }

    #[test]
    fn request_debug_redacts_url_path_authorization_and_body() {
        let request = HttpRequest {
            url: Url::parse("https://graph.facebook.com/v23.0/123456789/messages?token=query")
                .unwrap(),
            headers: BTreeMap::from([("authorization".into(), "Bearer top-secret".into())]),
            body: b"+13055550123".to_vec(),
        };
        let debug = format!("{request:?}");
        for secret in ["123456789", "query", "top-secret", "+13055550123"] {
            assert!(!debug.contains(secret));
        }
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn default_transport_policy_is_bounded() {
        let executor = ReqwestExecutor::default();
        assert_eq!(executor.dns_timeout, Duration::from_secs(5));
        assert_eq!(executor.connect_timeout, Duration::from_secs(8));
        assert_eq!(executor.request_timeout, Duration::from_secs(20));
        assert_eq!(executor.max_response_body_bytes, 256 * 1024);
    }
}
