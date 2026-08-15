use async_trait::async_trait;
use thiserror::Error;
use zeroize::Zeroizing;

/// Opaque locator resolved by the host's configured secret store.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SecretRef(String);

impl SecretRef {
    /// Creates a non-empty secret reference.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SecretError::InvalidReference);
        }
        Ok(Self(value))
    }

    /// Returns the opaque locator, never the resolved value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Secret text whose debug representation is always redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    /// Wraps a non-empty secret value.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SecretError::EmptyValue);
        }
        Ok(Self(Zeroizing::new(value)))
    }

    /// Exposes the value only at the HTTP authorization boundary.
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

/// Inline development token or production secret-store reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenSource {
    /// Explicit token parameter; useful for tests and short-lived dev tokens.
    Inline(SecretValue),
    /// Locator resolved immediately before each provider request.
    Reference(SecretRef),
}

/// Secret resolution failure.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SecretError {
    /// Reference string is blank.
    #[error("secret reference cannot be empty")]
    InvalidReference,
    /// Resolved or inline value is blank.
    #[error("secret value cannot be empty")]
    EmptyValue,
    /// Provider could not resolve the named secret.
    #[error("secret {reference:?} is unavailable")]
    Unavailable {
        /// Opaque locator which could not be resolved.
        reference: String,
    },
}

/// Host-owned secret lookup boundary.
#[async_trait]
pub trait SecretResolver: Send + Sync {
    /// Resolves one reference without logging its value.
    async fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, SecretError>;
}

/// Resolver for `env:VARIABLE` references in local installations.
#[derive(Clone, Copy, Debug, Default)]
pub struct EnvironmentSecretResolver;

#[async_trait]
impl SecretResolver for EnvironmentSecretResolver {
    async fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, SecretError> {
        let name = reference
            .as_str()
            .strip_prefix("env:")
            .unwrap_or(reference.as_str());
        let value = std::env::var(name).map_err(|_| SecretError::Unavailable {
            reference: reference.as_str().to_owned(),
        })?;
        SecretValue::new(value)
    }
}

pub(crate) async fn resolve_token(
    source: &TokenSource,
    resolver: &dyn SecretResolver,
) -> Result<SecretValue, SecretError> {
    match source {
        TokenSource::Inline(value) => Ok(value.clone()),
        TokenSource::Reference(reference) => resolver.resolve(reference).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_exposes_secret() {
        let secret = SecretValue::new("top-secret").unwrap();
        let debug = format!("{secret:?}");
        assert!(!debug.contains("top-secret"));
        assert!(debug.contains("REDACTED"));
    }
}
