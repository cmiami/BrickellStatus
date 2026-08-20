//! The TLS posture every BrickellStatus HTTP client shares.
//!
//! Two things have to be settled before any `reqwest` client in this workspace
//! can connect, and both used to be restated at each call site:
//!
//! 1. reqwest's provider-free rustls refuses to build a client until a
//!    process-default `CryptoProvider` exists.
//! 2. On Android, reqwest reaches for `rustls-platform-verifier` whenever no
//!    roots were named. That verifier's Android backend calls into the JVM
//!    through a Kotlin component this app does not ship, so it builds happily
//!    and then fails on the first handshake — every request, silently, with a
//!    working-looking app on screen. Naming the roots takes that path out of
//!    the picture entirely.
//!
//! Keeping it in one crate means the answer to "what does this app trust?" has
//! exactly one place to look, and one place to change.

use thiserror::Error;

/// The trust anchors could not be assembled.
#[derive(Debug, Error)]
pub enum TlsError {
    /// A bundled root certificate was not valid DER.
    #[error("bundled trust anchors could not be loaded")]
    TrustAnchors,
}

/// A `reqwest` builder carrying this app's TLS posture.
///
/// Callers chain their own timeouts, redirect policy and DNS pinning onto the
/// result; this only settles the crypto provider and the trust anchors.
pub fn client_builder() -> Result<reqwest::ClientBuilder, TlsError> {
    // Installing is idempotent, and every TLS client in the process resolves
    // the same default: whoever arrives first settles it for all of them.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let builder = reqwest::Client::builder();
    #[cfg(target_os = "android")]
    let builder = builder.tls_certs_only(android_trust_anchors()?);
    Ok(builder)
}

/// The Mozilla root program, which is the same set the AIS websocket already
/// trusts through `tokio-tungstenite`'s webpki roots — so pinning it here
/// leaves every connection the app opens agreeing on one list.
///
/// The cost is that CAs a user or employer installed on the device are not
/// honoured, and the list moves only when the app is updated. For a client
/// that only ever talks to a handful of fixed public hosts, that is the
/// cheaper side of the trade against shipping a JVM bridge for TLS.
#[cfg(target_os = "android")]
fn android_trust_anchors() -> Result<Vec<reqwest::Certificate>, TlsError> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .map(|der| reqwest::Certificate::from_der(der).map_err(|_| TlsError::TrustAnchors))
        .collect()
}
