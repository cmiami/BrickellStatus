use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::{
    DeliveryAdapter, DeliveryFailure, DeliveryFailureKind, DeliveryReceipt, DeliveryRequest,
    DeliveryStage, HttpExecutor, HttpRequest, MessagingConsent, SecretError, SecretResolver,
    TokenSource,
    http::{enforce_response_body_limit, validate_public_https_url},
    secret::resolve_token,
};

/// Official Meta WhatsApp Cloud API utility-template configuration.
#[derive(Clone, Debug)]
pub struct WhatsAppConfig {
    /// Graph endpoint root, normally `https://graph.facebook.com/`.
    pub base_url: Url,
    /// Pinned Graph API version, supplied by the host configuration.
    pub graph_api_version: String,
    /// WhatsApp Business sender phone-number ID.
    pub phone_number_id: String,
    /// Pre-approved utility template with exactly one body text variable.
    pub template_name: String,
    /// Meta template language code such as `en_US`.
    pub language_code: String,
    /// Short-lived dev token or production secret reference.
    pub access_token: TokenSource,
}

impl WhatsAppConfig {
    /// Creates config using Meta's official Cloud API base URL.
    pub fn cloud(
        graph_api_version: impl Into<String>,
        phone_number_id: impl Into<String>,
        template_name: impl Into<String>,
        language_code: impl Into<String>,
        access_token: TokenSource,
    ) -> Self {
        Self {
            base_url: Url::parse("https://graph.facebook.com/")
                .expect("static Meta Graph URL is valid"),
            graph_api_version: graph_api_version.into(),
            phone_number_id: phone_number_id.into(),
            template_name: template_name.into(),
            language_code: language_code.into(),
            access_token,
        }
    }

    fn endpoint(&self) -> Result<Url, DeliveryFailure> {
        validate_public_https_url(&self.base_url).map_err(|error| {
            DeliveryFailure::new(
                DeliveryFailureKind::Misconfigured,
                format!("WhatsApp Cloud endpoint rejected: {}", error.message),
            )
        })?;
        if !self
            .template_name
            .chars()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '_')
            || self.template_name.is_empty()
        {
            return Err(DeliveryFailure::new(
                DeliveryFailureKind::Misconfigured,
                "WhatsApp template name must contain lowercase ASCII letters, digits, or underscores",
            ));
        }
        if !valid_language_code(&self.language_code) {
            return Err(DeliveryFailure::new(
                DeliveryFailureKind::Misconfigured,
                "WhatsApp language code has an invalid shape",
            ));
        }
        let graph_version = self.graph_api_version.trim();
        if graph_version.is_empty()
            || !graph_version
                .chars()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '-' | '_'))
        {
            return Err(DeliveryFailure::new(
                DeliveryFailureKind::Misconfigured,
                "WhatsApp Graph API version contains invalid characters",
            ));
        }
        let phone_number_id = self.phone_number_id.trim();
        if phone_number_id.is_empty()
            || !phone_number_id.chars().all(|value| value.is_ascii_digit())
        {
            return Err(DeliveryFailure::new(
                DeliveryFailureKind::Misconfigured,
                "WhatsApp phone-number ID must contain only digits",
            ));
        }
        self.base_url
            .join(&format!("{}/{}/messages", graph_version, phone_number_id))
            .map_err(|_| {
                DeliveryFailure::new(
                    DeliveryFailureKind::Misconfigured,
                    "invalid WhatsApp Cloud endpoint",
                )
            })
    }
}

fn valid_language_code(value: &str) -> bool {
    let mut parts = value.split('_');
    let language = parts.next().unwrap_or_default();
    let region = parts.next();
    parts.next().is_none()
        && (2..=3).contains(&language.len())
        && language
            .chars()
            .all(|character| character.is_ascii_lowercase())
        && region.is_none_or(|region| {
            region.len() == 2
                && region
                    .chars()
                    .all(|character| character.is_ascii_uppercase())
        })
}

/// Official WhatsApp Cloud API template sender.
pub struct WhatsAppCloud {
    config: WhatsAppConfig,
    secrets: Arc<dyn SecretResolver>,
    http: Arc<dyn HttpExecutor>,
}

impl WhatsAppCloud {
    /// Creates a sender using injected secret and HTTP boundaries.
    pub fn new(
        config: WhatsAppConfig,
        secrets: Arc<dyn SecretResolver>,
        http: Arc<dyn HttpExecutor>,
    ) -> Self {
        Self {
            config,
            secrets,
            http,
        }
    }
}

#[derive(Serialize)]
struct CloudRequest<'a> {
    messaging_product: &'static str,
    recipient_type: &'static str,
    to: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    template: Template<'a>,
}

#[derive(Serialize)]
struct Template<'a> {
    name: &'a str,
    language: Language<'a>,
    components: [Component<'a>; 1],
}

#[derive(Serialize)]
struct Language<'a> {
    code: &'a str,
}

#[derive(Serialize)]
struct Component<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    parameters: [Parameter<'a>; 1],
}

#[derive(Serialize)]
struct Parameter<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    text: &'a str,
}

#[derive(Deserialize)]
struct CloudResponse {
    #[serde(default)]
    messages: Vec<CloudMessage>,
}

#[derive(Deserialize)]
struct CloudMessage {
    id: String,
    #[serde(default)]
    message_status: Option<String>,
}

#[async_trait]
impl DeliveryAdapter for WhatsAppCloud {
    fn adapter_id(&self) -> &'static str {
        "meta_whatsapp_cloud"
    }

    async fn deliver(&self, request: &DeliveryRequest) -> Result<DeliveryReceipt, DeliveryFailure> {
        request.validate()?;
        match &request.destination.messaging_consent {
            MessagingConsent::OptedIn { .. } => {}
            MessagingConsent::NotRecorded => {
                return Err(DeliveryFailure::new(
                    DeliveryFailureKind::Suppressed,
                    "WhatsApp send blocked: recipient opt-in is not recorded",
                ));
            }
            MessagingConsent::Unsubscribed { .. } => {
                return Err(DeliveryFailure::new(
                    DeliveryFailureKind::Suppressed,
                    "WhatsApp send blocked: recipient is unsubscribed",
                ));
            }
        }
        let recipient = normalize_e164(&request.destination.address)?;
        let message = request.notice.render_message()?;
        let endpoint = self.config.endpoint()?;
        let token = resolve_token(&self.config.access_token, self.secrets.as_ref())
            .await
            .map_err(secret_failure)?;

        let body = serde_json::to_vec(&CloudRequest {
            messaging_product: "whatsapp",
            recipient_type: "individual",
            to: &recipient,
            kind: "template",
            template: Template {
                name: self.config.template_name.trim(),
                language: Language {
                    code: self.config.language_code.trim(),
                },
                components: [Component {
                    kind: "body",
                    parameters: [Parameter {
                        kind: "text",
                        text: &message,
                    }],
                }],
            },
        })
        .map_err(|error| {
            DeliveryFailure::new(
                DeliveryFailureKind::InvalidRequest,
                format!("could not encode WhatsApp template: {error}"),
            )
        })?;
        let mut headers = standard_headers(request);
        headers.insert("authorization".into(), format!("Bearer {}", token.expose()));

        let response = self
            .http
            .execute(HttpRequest {
                url: endpoint,
                headers,
                body,
            })
            .await
            .map_err(|error| {
                DeliveryFailure::new(DeliveryFailureKind::Transient, error.to_string())
            })?;
        enforce_response_body_limit(&response).map_err(|error| {
            DeliveryFailure::new(DeliveryFailureKind::Transient, error.to_string())
        })?;
        if !response.is_success() {
            return Err(classify_meta_failure(&response));
        }
        let parsed: CloudResponse = serde_json::from_slice(&response.body).map_err(|error| {
            DeliveryFailure::new(
                DeliveryFailureKind::Transient,
                format!("WhatsApp accepted an unreadable response: {error}"),
            )
        })?;
        let accepted = parsed.messages.into_iter().next().ok_or_else(|| {
            DeliveryFailure::new(
                DeliveryFailureKind::Transient,
                "WhatsApp response did not include a message ID",
            )
        })?;

        Ok(DeliveryReceipt {
            outbox_id: request.outbox_id,
            adapter: self.adapter_id().into(),
            // The send response is queue acceptance. A later status webhook
            // is the only path which may persist `Delivered`.
            stage: DeliveryStage::Accepted,
            provider_message_id: Some(accepted.id),
            provider_status: accepted.message_status.or(Some("accepted".into())),
        })
    }
}

fn standard_headers(request: &DeliveryRequest) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("content-type".into(), "application/json".into()),
        ("idempotency-key".into(), request.deduplication_key.clone()),
    ])
}

fn secret_failure(error: SecretError) -> DeliveryFailure {
    DeliveryFailure::new(
        DeliveryFailureKind::Misconfigured,
        format!("WhatsApp access token is unavailable: {error}"),
    )
}

fn classify_http_failure(response: &crate::HttpResponse) -> DeliveryFailure {
    let kind = match response.status {
        401 | 403 => DeliveryFailureKind::Authentication,
        408 | 425 | 500..=599 => DeliveryFailureKind::Transient,
        429 => DeliveryFailureKind::RateLimited,
        _ => DeliveryFailureKind::Rejected,
    };
    let mut failure = DeliveryFailure::new(
        kind,
        format!("WhatsApp provider returned HTTP {}", response.status),
    );
    failure.retry_after_seconds = response
        .headers
        .get("retry-after")
        .and_then(|value| value.parse().ok());
    failure
}

fn normalize_e164(value: &str) -> Result<String, DeliveryFailure> {
    let trimmed = value.trim();
    if !trimmed.starts_with('+') {
        return Err(DeliveryFailure::new(
            DeliveryFailureKind::InvalidRequest,
            "WhatsApp destination must be an E.164 number beginning with +",
        ));
    }
    let digits: String = trimmed
        .chars()
        .skip(1)
        .filter(|value| value.is_ascii_digit())
        .collect();
    let has_invalid = trimmed
        .chars()
        .skip(1)
        .any(|value| !value.is_ascii_digit() && !matches!(value, ' ' | '-' | '(' | ')'));
    if has_invalid || !(8..=15).contains(&digits.len()) {
        return Err(DeliveryFailure::new(
            DeliveryFailureKind::InvalidRequest,
            "WhatsApp destination is not a valid E.164 number",
        ));
    }
    // Meta's Cloud API expects digits without the leading plus sign.
    Ok(digits)
}

fn classify_meta_failure(response: &crate::HttpResponse) -> DeliveryFailure {
    let mut failure = classify_http_failure(response);
    let Ok(body) = serde_json::from_slice::<Value>(&response.body) else {
        return failure;
    };
    let Some(error) = body.get("error") else {
        return failure;
    };
    let code = error.get("code").and_then(safe_numeric_code);
    let subcode = error.get("error_subcode").and_then(safe_numeric_code);
    failure.provider_code = match (code.as_deref(), subcode.as_deref()) {
        (Some(code), Some(subcode)) => Some(format!("{code}/{subcode}")),
        (Some(code), None) => Some(code.to_owned()),
        _ => failure.provider_code,
    };
    let detail = error
        .pointer("/error_data/details")
        .and_then(Value::as_str)
        .or_else(|| error.get("message").and_then(Value::as_str))
        .map(safe_fragment);
    if let Some(detail) = detail {
        failure.message = match &failure.provider_code {
            Some(code) => format!("WhatsApp rejected request ({code}): {detail}"),
            None => format!("WhatsApp rejected request: {detail}"),
        };
    }
    failure
}

fn safe_numeric_code(value: &Value) -> Option<String> {
    match value {
        Value::Number(value) => Some(value.to_string()),
        Value::String(value)
            if !value.is_empty()
                && value.len() <= 16
                && value.chars().all(|character| character.is_ascii_digit()) =>
        {
            Some(value.clone())
        }
        _ => None,
    }
}

fn safe_fragment(value: &str) -> String {
    let normalized: String = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect();
    let lowercase = normalized.to_ascii_lowercase();
    if lowercase.contains("bearer ")
        || lowercase.contains("access_token")
        || lowercase.contains("access token")
        || lowercase.contains("authorization")
        || contains_phone_like_sequence(&normalized)
    {
        "[redacted provider detail]".into()
    } else {
        normalized
    }
}

fn contains_phone_like_sequence(value: &str) -> bool {
    let mut digits = 0_u8;
    let mut candidate = false;
    for character in value.chars() {
        if character.is_ascii_digit() {
            digits = digits.saturating_add(1);
            candidate = true;
        } else if candidate && matches!(character, ' ' | '-' | '(' | ')' | '.') {
            continue;
        } else {
            if digits >= 8 {
                return true;
            }
            digits = 0;
            candidate = character == '+';
        }
    }
    digits >= 8
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use uuid::Uuid;

    use super::*;
    use crate::{
        DeliveryReason, Destination, EnvironmentSecretResolver, EtaRange, HttpError, HttpResponse,
        Notice, NoticeState, SecretValue,
    };

    struct MockHttp {
        seen: Mutex<Vec<HttpRequest>>,
        response: HttpResponse,
    }

    #[async_trait]
    impl HttpExecutor for MockHttp {
        async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
            self.seen.lock().unwrap().push(request);
            Ok(self.response.clone())
        }
    }

    fn request() -> DeliveryRequest {
        DeliveryRequest {
            outbox_id: Uuid::new_v4(),
            incident_id: Uuid::new_v4(),
            material_revision: 1,
            deduplication_key: "brickell:incident:1".into(),
            reason: DeliveryReason::StateTransition,
            destination: Destination {
                id: "friend".into(),
                address: "+1 (305) 555-0123".into(),
                locale: Some("en_US".into()),
                messaging_consent: MessagingConsent::OptedIn {
                    recorded_at_millis: 1_699_999_000_000,
                },
            },
            notice: Notice {
                subject: "Brickell Avenue Bridge".into(),
                state: NoticeState::Likely,
                road_meaning: "Road open now".into(),
                action: "Detour advised".into(),
                eta: Some(EtaRange::new(6, 9)),
                confidence_percent: Some(82),
                evidence: vec!["Outbound vessel + upstream bridge".into()],
                source_label: "AIS + FL511".into(),
                source_age_seconds: 28,
            },
            created_at_millis: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn cloud_sender_uses_template_and_only_claims_acceptance() {
        let http = Arc::new(MockHttp {
            seen: Mutex::new(Vec::new()),
            response: HttpResponse::new(
                200,
                br#"{"messaging_product":"whatsapp","messages":[{"id":"wamid.abc","message_status":"accepted"}]}"#
                    .to_vec(),
            ),
        });
        let config = WhatsAppConfig {
            base_url: Url::parse("https://1.1.1.1/").unwrap(),
            graph_api_version: "v23.0".into(),
            phone_number_id: "123456789".into(),
            template_name: "bridge_status_update".into(),
            language_code: "en_US".into(),
            access_token: TokenSource::Inline(SecretValue::new("secret-token").unwrap()),
        };
        let adapter = WhatsAppCloud::new(config, Arc::new(EnvironmentSecretResolver), http.clone());

        let receipt = adapter.deliver(&request()).await.unwrap();
        assert_eq!(receipt.stage, DeliveryStage::Accepted);
        assert_eq!(receipt.provider_message_id.as_deref(), Some("wamid.abc"));
        let seen = http.seen.lock().unwrap();
        assert_eq!(
            seen[0].url.as_str(),
            "https://1.1.1.1/v23.0/123456789/messages"
        );
        assert_eq!(seen[0].headers["authorization"], "Bearer secret-token");
        assert!(!format!("{:?}", seen[0]).contains("secret-token"));
        let body: Value = serde_json::from_slice(&seen[0].body).unwrap();
        assert_eq!(body["messaging_product"], "whatsapp");
        assert_eq!(body["to"], "13055550123");
        assert_eq!(body["type"], "template");
        assert_eq!(body["template"]["name"], "bridge_status_update");
        assert!(
            body["template"]["components"][0]["parameters"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Confidence: 82%")
        );
    }

    #[test]
    fn destination_requires_plus_and_reasonable_length() {
        assert!(normalize_e164("3055550123").is_err());
        assert!(normalize_e164("+1 (305) 555-0123").is_ok());
        assert!(normalize_e164("+12ext").is_err());
    }

    #[tokio::test]
    async fn missing_or_revoked_consent_never_reaches_meta() {
        let http = Arc::new(MockHttp {
            seen: Mutex::new(Vec::new()),
            response: HttpResponse::new(200, Vec::new()),
        });
        let config = WhatsAppConfig {
            base_url: Url::parse("https://1.1.1.1/").unwrap(),
            graph_api_version: "v23.0".into(),
            phone_number_id: "123456789".into(),
            template_name: "bridge_status_update".into(),
            language_code: "en_US".into(),
            access_token: TokenSource::Inline(SecretValue::new("secret-token").unwrap()),
        };
        let adapter = WhatsAppCloud::new(config, Arc::new(EnvironmentSecretResolver), http.clone());
        for consent in [
            MessagingConsent::NotRecorded,
            MessagingConsent::Unsubscribed {
                recorded_at_millis: 1_700_000_000_001,
            },
        ] {
            let mut request = request();
            request.destination.messaging_consent = consent;
            let failure = adapter.deliver(&request).await.unwrap_err();
            assert_eq!(failure.kind, DeliveryFailureKind::Suppressed);
            assert!(!failure.retryable());
        }
        assert!(http.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn meta_error_is_redacted_classified_and_retry_aware() {
        let response = HttpResponse::new(
            429,
            br#"{"error":{"message":"Rate limit reached","type":"OAuthException","code":4,"error_subcode":2446079,"fbtrace_id":"trace"}}"#.to_vec(),
        );
        let failure = classify_meta_failure(&response);
        assert_eq!(failure.kind, DeliveryFailureKind::RateLimited);
        assert_eq!(failure.provider_code.as_deref(), Some("4/2446079"));
        assert!(failure.retryable());
        assert!(failure.message.contains("Rate limit reached"));
        assert!(!failure.message.contains("trace"));
    }

    #[test]
    fn provider_detail_redacts_credentials_and_phone_numbers() {
        for detail in [
            "Bearer secret-token is invalid",
            "Access token abcdef was rejected",
            "Recipient +1 (305) 555-0123 is unavailable",
            "Recipient 13055550123 is unavailable",
        ] {
            assert_eq!(safe_fragment(detail), "[redacted provider detail]");
        }
        assert_eq!(safe_fragment("Rate limit reached"), "Rate limit reached");

        let response = HttpResponse::new(
            400,
            br#"{"error":{"message":"Recipient 13055550123 rejected Bearer secret-token","code":"secret-token"}}"#
                .to_vec(),
        );
        let failure = classify_meta_failure(&response);
        assert!(!failure.message.contains("13055550123"));
        assert!(!failure.message.contains("secret-token"));
        assert!(failure.provider_code.is_none());
    }

    #[tokio::test]
    async fn cloud_sender_rejects_unsafe_base_before_http_execution() {
        let http = Arc::new(MockHttp {
            seen: Mutex::new(Vec::new()),
            response: HttpResponse::new(200, Vec::new()),
        });
        let config = WhatsAppConfig {
            base_url: Url::parse("http://169.254.169.254/").unwrap(),
            graph_api_version: "v23.0".into(),
            phone_number_id: "123456789".into(),
            template_name: "bridge_status_update".into(),
            language_code: "en_US".into(),
            access_token: TokenSource::Inline(SecretValue::new("secret-token").unwrap()),
        };
        let adapter = WhatsAppCloud::new(config, Arc::new(EnvironmentSecretResolver), http.clone());
        let failure = adapter.deliver(&request()).await.unwrap_err();
        assert_eq!(failure.kind, DeliveryFailureKind::Misconfigured);
        assert!(http.seen.lock().unwrap().is_empty());
        assert!(!failure.message.contains("169.254.169.254"));
    }

    #[test]
    fn cloud_route_rejects_invalid_template_and_language_shapes() {
        let base = || WhatsAppConfig {
            base_url: Url::parse("https://graph.facebook.com/").unwrap(),
            graph_api_version: "v23.0".into(),
            phone_number_id: "123456789".into(),
            template_name: "bridge_status_update".into(),
            language_code: "en_US".into(),
            access_token: TokenSource::Inline(SecretValue::new("secret-token").unwrap()),
        };

        for invalid in [
            "Bridge_Status",
            "bridge status",
            "bad\nname",
            "bridge/status",
        ] {
            let mut config = base();
            config.template_name = invalid.into();
            assert!(config.endpoint().is_err());
        }
        for invalid in ["EN_us", "not a locale", "en/US", "e_US", "en_US_extra"] {
            let mut config = base();
            config.language_code = invalid.into();
            assert!(config.endpoint().is_err());
        }
        let mut valid = base();
        valid.language_code = "fil_PH".into();
        assert!(valid.endpoint().is_ok());
    }

    #[tokio::test]
    async fn cloud_sender_caps_even_injected_executor_responses() {
        let http = Arc::new(MockHttp {
            seen: Mutex::new(Vec::new()),
            response: HttpResponse::new(200, vec![b'x'; crate::MAX_RESPONSE_BODY_BYTES + 1]),
        });
        let config = WhatsAppConfig {
            base_url: Url::parse("https://1.1.1.1/").unwrap(),
            graph_api_version: "v23.0".into(),
            phone_number_id: "123456789".into(),
            template_name: "bridge_status_update".into(),
            language_code: "en_US".into(),
            access_token: TokenSource::Inline(SecretValue::new("secret-token").unwrap()),
        };
        let adapter = WhatsAppCloud::new(config, Arc::new(EnvironmentSecretResolver), http);
        let failure = adapter.deliver(&request()).await.unwrap_err();
        assert_eq!(failure.kind, DeliveryFailureKind::Transient);
        assert!(failure.message.contains("262144-byte limit"));
    }
}
