#[cfg(feature = "native")]
use std::net::IpAddr;
#[cfg(feature = "native")]
use std::net::SocketAddr;
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use async_trait::async_trait;
#[cfg(feature = "native")]
use reqwest::{
    StatusCode,
    header::{
        ETAG, HeaderMap, HeaderName, HeaderValue, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED,
        LOCATION,
    },
};
use serde::{Deserialize, Serialize};
#[cfg(feature = "native")]
use tokio::net::lookup_host;
use url::{Host, Url};

use crate::{CollectorCursor, CollectorError};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FetchLimits {
    pub timeout: Duration,
    pub max_body_bytes: usize,
    pub max_redirects: usize,
    /// HTTP is disabled by default. It must be explicitly enabled for a trusted,
    /// legacy feed; private and loopback destinations remain prohibited.
    pub allow_http: bool,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(12),
            max_body_bytes: 2 * 1024 * 1024,
            max_redirects: 3,
            allow_http: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FetchResponse {
    pub status: u16,
    pub final_url: Url,
    pub body: Vec<u8>,
    pub cursor: CollectorCursor,
    pub not_modified: bool,
    pub content_type: Option<String>,
}

#[async_trait]
pub trait HttpFetcher: Send + Sync {
    async fn get(
        &self,
        url: &Url,
        cursor: Option<&CollectorCursor>,
        headers: &[(&str, &str)],
    ) -> Result<FetchResponse, CollectorError>;
}

/// Network client used by built-in collectors and by arbitrary RSS/Atom feeds.
/// Redirects are followed manually so every hop receives the same scheme, host,
/// DNS, timeout, and body-limit checks.
#[derive(Clone, Debug)]
#[cfg(feature = "native")]
pub struct SafeHttpFetcher {
    limits: FetchLimits,
    user_agent: String,
    inherit_system_proxy: bool,
}

#[cfg(feature = "native")]
impl Default for SafeHttpFetcher {
    fn default() -> Self {
        Self::new(
            "PuenteGonorrea/0.1 (+https://github.com/cmiami/PuenteGonorrea)",
            FetchLimits::default(),
        )
        .expect("the built-in HTTP client configuration is valid")
    }
}

#[cfg(feature = "native")]
impl SafeHttpFetcher {
    pub fn new(user_agent: impl Into<String>, limits: FetchLimits) -> Result<Self, CollectorError> {
        let user_agent = user_agent.into();
        if user_agent.trim().is_empty() {
            return Err(CollectorError::Configuration(
                "HTTP User-Agent cannot be empty".into(),
            ));
        }
        if limits.max_body_bytes == 0 {
            return Err(CollectorError::Configuration(
                "max_body_bytes must be greater than zero".into(),
            ));
        }
        Ok(Self {
            limits,
            user_agent,
            inherit_system_proxy: false,
        })
    }

    pub fn limits(&self) -> &FetchLimits {
        &self.limits
    }

    async fn resolve_public(&self, url: &Url) -> Result<Vec<SocketAddr>, CollectorError> {
        validate_public_url(url, self.limits.allow_http)?;
        let host = url
            .host_str()
            .ok_or_else(|| CollectorError::UnsafeUrl("URL has no host".into()))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| CollectorError::UnsafeUrl("URL has no known port".into()))?;

        let resolved = tokio::time::timeout(self.limits.timeout, lookup_host((host, port)))
            .await
            .map_err(|_| CollectorError::Timeout {
                url: redacted_url(url),
                limit: self.limits.timeout,
            })?;
        let addresses: Vec<_> = resolved
            .map_err(|error| CollectorError::Dns {
                host: host.to_owned(),
                detail: error.to_string(),
            })?
            .collect();
        if addresses.is_empty() {
            return Err(CollectorError::Dns {
                host: host.to_owned(),
                detail: "lookup returned no addresses".into(),
            });
        }
        if let Some(address) = addresses.iter().find(|address| !is_public_ip(address.ip())) {
            return Err(CollectorError::UnsafeUrl(format!(
                "host {host} resolves to non-public address {}",
                address.ip()
            )));
        }
        Ok(addresses)
    }

    fn client(
        &self,
        url: &Url,
        resolved_addresses: &[SocketAddr],
    ) -> Result<reqwest::Client, CollectorError> {
        // Arbitrary feed URLs must connect to the exact public addresses
        // validated by this fetcher. A system proxy would resolve the origin
        // on our behalf and reopen the DNS-rebinding/SSRF gap.
        if self.inherit_system_proxy {
            return Err(CollectorError::Configuration(
                "system proxies are not supported by the safe collector client".into(),
            ));
        }
        // reqwest's provider-free rustls refuses to build a client until a
        // process-default CryptoProvider exists; installing is idempotent.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(self.limits.timeout)
            .user_agent(&self.user_agent);
        // Pin the DNS answer that passed the public-address check. This closes
        // the check/connect gap which otherwise permits DNS-rebinding attacks.
        if let Some(Host::Domain(domain)) = url.host() {
            builder = builder.resolve_to_addrs(domain, resolved_addresses);
        }
        builder.build().map_err(CollectorError::from)
    }
}

#[async_trait]
#[cfg(feature = "native")]
#[async_trait]
impl HttpFetcher for SafeHttpFetcher {
    async fn get(
        &self,
        url: &Url,
        cursor: Option<&CollectorCursor>,
        headers: &[(&str, &str)],
    ) -> Result<FetchResponse, CollectorError> {
        let mut current = url.clone();

        for redirect_count in 0..=self.limits.max_redirects {
            let resolved_addresses = self.resolve_public(&current).await?;
            let client = self.client(&current, &resolved_addresses)?;

            let mut request = client.get(current.clone());
            for (name, value) in headers {
                let name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                    CollectorError::Configuration(format!("invalid header name: {error}"))
                })?;
                let value = HeaderValue::from_str(value).map_err(|error| {
                    CollectorError::Configuration(format!("invalid header value: {error}"))
                })?;
                request = request.header(name, value);
            }
            if redirect_count == 0 {
                if let Some(etag) = cursor.and_then(|value| value.etag.as_deref()) {
                    request = request.header(IF_NONE_MATCH, etag);
                }
                if let Some(last_modified) = cursor.and_then(|value| value.last_modified.as_deref())
                {
                    request = request.header(IF_MODIFIED_SINCE, last_modified);
                }
            }

            let mut response = request.send().await?;
            let status = response.status();
            // A 304 is in the 3xx class but is a successful conditional-cache
            // response, not a redirect. It correctly carries no Location.
            if is_followable_redirect(status) {
                if redirect_count == self.limits.max_redirects {
                    return Err(CollectorError::TooManyRedirects(self.limits.max_redirects));
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| CollectorError::InvalidRedirect {
                        from: redacted_url(&current),
                    })?;
                current = current
                    .join(location)
                    .map_err(|_| CollectorError::InvalidRedirect {
                        from: redacted_url(&current),
                    })?;
                continue;
            }

            let response_cursor = cursor_from_headers(
                response.headers(),
                cursor,
                status == StatusCode::NOT_MODIFIED,
            );
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);

            if status == StatusCode::NOT_MODIFIED {
                return Ok(FetchResponse {
                    status: status.as_u16(),
                    final_url: current,
                    body: Vec::new(),
                    cursor: response_cursor,
                    not_modified: true,
                    content_type,
                });
            }
            if !status.is_success() {
                return Err(CollectorError::Http {
                    status: status.as_u16(),
                    url: redacted_url(&current),
                });
            }
            if response
                .content_length()
                .is_some_and(|length| length > self.limits.max_body_bytes as u64)
            {
                return Err(CollectorError::BodyTooLarge {
                    url: redacted_url(&current),
                    limit: self.limits.max_body_bytes,
                });
            }

            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if body.len().saturating_add(chunk.len()) > self.limits.max_body_bytes {
                    return Err(CollectorError::BodyTooLarge {
                        url: redacted_url(&current),
                        limit: self.limits.max_body_bytes,
                    });
                }
                body.extend_from_slice(&chunk);
            }

            return Ok(FetchResponse {
                status: status.as_u16(),
                final_url: current,
                body,
                cursor: response_cursor,
                not_modified: false,
                content_type,
            });
        }

        Err(CollectorError::TooManyRedirects(self.limits.max_redirects))
    }
}

#[cfg(feature = "native")]
fn is_followable_redirect(status: StatusCode) -> bool {
    status.is_redirection() && status != StatusCode::NOT_MODIFIED
}

#[cfg(feature = "native")]
fn cursor_from_headers(
    headers: &HeaderMap,
    previous: Option<&CollectorCursor>,
    preserve_missing_validators: bool,
) -> CollectorCursor {
    let mut cursor = CollectorCursor {
        metadata: previous
            .map(|value| value.metadata.clone())
            .unwrap_or_default(),
        ..CollectorCursor::default()
    };
    cursor.etag = headers
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| {
            preserve_missing_validators
                .then(|| previous.and_then(|value| value.etag.clone()))
                .flatten()
        });
    cursor.last_modified = headers
        .get(LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| {
            preserve_missing_validators
                .then(|| previous.and_then(|value| value.last_modified.clone()))
                .flatten()
        });
    cursor
}

/// Performs the non-network portion of feed URL validation. Domain names receive
/// an additional DNS check immediately before each request.
pub fn validate_public_url(url: &Url, allow_http: bool) -> Result<(), CollectorError> {
    match url.scheme() {
        "https" => {}
        "http" if allow_http => {}
        scheme => {
            return Err(CollectorError::UnsafeUrl(format!(
                "scheme {scheme:?} is not allowed"
            )));
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CollectorError::UnsafeUrl(
            "embedded URL credentials are not allowed".into(),
        ));
    }
    if url.fragment().is_some() {
        return Err(CollectorError::UnsafeUrl(
            "URL fragments are not accepted for public feeds".into(),
        ));
    }
    const CREDENTIAL_QUERY_KEYS: &[&str] = &[
        "access_token",
        "api_key",
        "apikey",
        "auth",
        "authorization",
        "key",
        "secret",
        "sig",
        "signature",
        "token",
    ];
    if url
        .query_pairs()
        .any(|(key, _)| CREDENTIAL_QUERY_KEYS.contains(&key.to_ascii_lowercase().as_str()))
    {
        return Err(CollectorError::UnsafeUrl(
            "credential-bearing query parameters are not accepted; use a public feed URL".into(),
        ));
    }
    let host = url
        .host()
        .ok_or_else(|| CollectorError::UnsafeUrl("URL has no host".into()))?;
    match host {
        Host::Domain(domain) => {
            let normalized = domain.trim_end_matches('.').to_ascii_lowercase();
            if normalized == "localhost"
                || normalized.ends_with(".localhost")
                || normalized.ends_with(".local")
                || normalized.ends_with(".internal")
            {
                return Err(CollectorError::UnsafeUrl(format!(
                    "local hostname {domain:?} is not allowed"
                )));
            }
        }
        Host::Ipv4(address) if !is_public_ipv4(address) => {
            return Err(CollectorError::UnsafeUrl(format!(
                "non-public address {address} is not allowed"
            )));
        }
        Host::Ipv6(address) if !is_public_ipv6(address) => {
            return Err(CollectorError::UnsafeUrl(format!(
                "non-public address {address} is not allowed"
            )));
        }
        _ => {}
    }
    Ok(())
}

#[cfg(feature = "native")]
fn redacted_url(url: &Url) -> String {
    let mut redacted = url.clone();
    if redacted.query().is_some() {
        redacted.set_query(Some("[redacted]"));
    }
    if redacted.fragment().is_some() {
        redacted.set_fragment(Some("[redacted]"));
    }
    redacted.to_string()
}

#[cfg(feature = "native")]
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
    // This also prevents special IPv4 destinations from becoming reachable by
    // spelling them as IPv6 literals.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_url_validation_rejects_local_targets() {
        for value in [
            "https://localhost/feed.xml",
            "https://127.0.0.1/feed.xml",
            "https://10.2.3.4/feed.xml",
            "https://[::1]/feed.xml",
            "https://[::127.0.0.1]/feed.xml",
            "https://[::10.0.0.1]/feed.xml",
            "https://[64:ff9b::7f00:1]/feed.xml",
            "https://[64:ff9b:1::a00:1]/feed.xml",
            "https://printer.local/feed.xml",
            "file:///etc/passwd",
        ] {
            let url = Url::parse(value).unwrap();
            assert!(
                validate_public_url(&url, false).is_err(),
                "accepted {value}"
            );
        }
    }

    #[test]
    fn ipv6_validation_rejects_transition_and_special_use_ranges() {
        for (label, address) in [
            ("unspecified", "::"),
            ("loopback", "::1"),
            ("unique-local", "fd12:3456:789a::1"),
            ("link-local", "fe80::1"),
            ("site-local", "fec0::1"),
            ("multicast", "ff02::1"),
            ("outside-global-unicast", "4000::1"),
            ("documentation", "2001:db8::1"),
            ("benchmark-start", "2001:2::1"),
            ("benchmark-upper-neighbor", "2001:2:ffff::1"),
            ("teredo", "2001:0:4136:e378:8000:63bf:3fff:fdd2"),
            ("orchid-v1-start", "2001:10::1"),
            ("orchid-v1-end", "2001:1f:ffff::1"),
            ("orchid-v2-start", "2001:20::1"),
            ("orchid-v2-end", "2001:2f:ffff::1"),
            ("six-to-four-private", "2002:a00:1::1"),
            ("six-to-four-public", "2002:808:808::1"),
            ("mapped-loopback", "::ffff:127.0.0.1"),
            ("mapped-private", "::ffff:10.0.0.1"),
            ("mapped-special", "::ffff:192.88.99.1"),
            ("compatible-link-local", "::169.254.169.254"),
            ("compatible-private", "::192.168.1.1"),
            ("well-known-nat64-private", "64:ff9b::a00:1"),
            ("well-known-nat64-public", "64:ff9b::808:808"),
            ("local-use-nat64-private", "64:ff9b:1::a00:1"),
            ("local-use-nat64-public", "64:ff9b:1::808:808"),
        ] {
            let address: Ipv6Addr = address.parse().unwrap();
            assert!(!is_public_ipv6(address), "accepted {label}: {address}");

            let url = Url::parse(&format!("https://[{address}]/feed.xml")).unwrap();
            assert!(
                validate_public_url(&url, false).is_err(),
                "URL validation accepted {label}: {address}"
            );
        }
    }

    #[test]
    fn ipv6_validation_preserves_public_native_mapped_and_compatible_addresses() {
        for (label, address) in [
            ("native-google", "2001:4860:4860::8888"),
            ("native-cloudflare", "2606:4700:4700::1111"),
            ("mapped-public-ipv4", "::ffff:8.8.8.8"),
            ("compatible-public-ipv4", "::8.8.8.8"),
        ] {
            let address: Ipv6Addr = address.parse().unwrap();
            assert!(is_public_ipv6(address), "rejected {label}: {address}");

            let url = Url::parse(&format!("https://[{address}]/feed.xml")).unwrap();
            assert!(
                validate_public_url(&url, false).is_ok(),
                "URL validation rejected {label}: {address}"
            );
        }
    }

    #[test]
    fn https_is_the_default_and_http_is_an_explicit_escape_hatch() {
        let https = Url::parse("https://example.com/feed.xml").unwrap();
        let http = Url::parse("http://example.com/feed.xml").unwrap();
        assert!(validate_public_url(&https, false).is_ok());
        assert!(validate_public_url(&http, false).is_err());
        assert!(validate_public_url(&http, true).is_ok());
    }

    #[test]
    fn public_query_feeds_work_but_credentials_and_fragments_fail_closed() {
        let public = Url::parse("https://example.com/search.xml?q=miami&lang=en").unwrap();
        let tokenized = Url::parse("https://example.com/feed?access_token=do-not-store").unwrap();
        let fragmented = Url::parse("https://example.com/feed#private").unwrap();
        assert!(validate_public_url(&public, false).is_ok());
        assert!(validate_public_url(&tokenized, false).is_err());
        assert!(validate_public_url(&fragmented, false).is_err());
    }

    #[test]
    fn diagnostic_urls_never_render_query_or_fragment_values() {
        let url = Url::parse("https://example.com/feed?q=secret#fragment").unwrap();
        let rendered = redacted_url(&url);
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("fragment"));
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn response_validators_replace_cursor_values_and_preserve_metadata() {
        let mut previous = CollectorCursor {
            etag: Some("\"old\"".into()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".into()),
            ..CollectorCursor::default()
        };
        previous.metadata.insert("page".into(), "7".into());
        let mut headers = HeaderMap::new();
        headers.insert(ETAG, HeaderValue::from_static("\"new\""));
        headers.insert(
            LAST_MODIFIED,
            HeaderValue::from_static("Fri, 14 Aug 2026 20:00:00 GMT"),
        );

        let cursor = cursor_from_headers(&headers, Some(&previous), false);
        assert_eq!(cursor.etag.as_deref(), Some("\"new\""));
        assert_eq!(
            cursor.last_modified.as_deref(),
            Some("Fri, 14 Aug 2026 20:00:00 GMT")
        );
        assert_eq!(cursor.metadata["page"], "7");
    }

    #[test]
    fn successful_response_clears_omitted_validators_but_304_preserves_them() {
        let previous = CollectorCursor {
            etag: Some("\"old\"".into()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".into()),
            ..CollectorCursor::default()
        };
        let headers = HeaderMap::new();

        let replaced = cursor_from_headers(&headers, Some(&previous), false);
        assert_eq!(replaced.etag, None);
        assert_eq!(replaced.last_modified, None);

        let preserved = cursor_from_headers(&headers, Some(&previous), true);
        assert_eq!(preserved.etag, previous.etag);
        assert_eq!(preserved.last_modified, previous.last_modified);
    }

    #[test]
    fn not_modified_is_cache_success_not_a_location_redirect() {
        assert!(!is_followable_redirect(StatusCode::NOT_MODIFIED));
        assert!(is_followable_redirect(StatusCode::MOVED_PERMANENTLY));
        assert!(is_followable_redirect(StatusCode::TEMPORARY_REDIRECT));
    }

    #[test]
    fn safe_fetcher_never_inherits_system_proxy_configuration() {
        assert!(!SafeHttpFetcher::default().inherit_system_proxy);
    }
}
