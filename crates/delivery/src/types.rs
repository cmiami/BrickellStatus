use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Why policy emitted this material incident revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryReason {
    /// Bridge or alert stage changed materially.
    StateTransition,
    /// ETA crossed a configured material band.
    EtaBandChanged,
    /// An authority materially revised its notice.
    OfficialUpdate,
    /// Incident resolved and the user requested all-clear messages.
    AllClear,
    /// Explicit operator test.
    Test,
}

/// Delivery-oriented bridge vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeState {
    /// A non-bridge channel has a material active alert.
    Alert,
    /// A non-bridge channel's prior active alert resolved.
    Resolved,
    /// No opening is currently likely.
    Clear,
    /// Predictive evidence supports an opening warning.
    Likely,
    /// Span is open and road traffic is blocked.
    Open,
    /// A prior incident resolved.
    AllClear,
    /// Current source set cannot establish status.
    Unknown,
}

impl NoticeState {
    /// Plain user-facing state line.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Alert => "Alert active",
            Self::Resolved => "Alert resolved",
            Self::Clear => "No opening likely",
            Self::Likely => "Likely to open",
            Self::Open => "Bridge open",
            Self::AllClear => "Bridge clear",
            Self::Unknown => "Bridge status unknown",
        }
    }

    /// Whether a model confidence belongs in outbound copy.
    pub const fn is_predictive(self) -> bool {
        matches!(self, Self::Likely)
    }
}

/// Closed whole-minute ETA interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtaRange {
    /// Earliest expected impact.
    pub earliest_minutes: u16,
    /// Latest expected impact.
    pub latest_minutes: u16,
}

impl EtaRange {
    /// Creates an ordered interval.
    pub const fn new(earliest_minutes: u16, latest_minutes: u16) -> Self {
        Self {
            earliest_minutes,
            latest_minutes: if latest_minutes < earliest_minutes {
                earliest_minutes
            } else {
                latest_minutes
            },
        }
    }

    fn render(self) -> String {
        if self.earliest_minutes == self.latest_minutes {
            format!("{} min", self.earliest_minutes)
        } else {
            format!("{}-{} min", self.earliest_minutes, self.latest_minutes)
        }
    }
}

/// Provider-independent alert content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// Short subject, normally `Brickell Avenue Bridge`.
    pub subject: String,
    /// Incident stage.
    pub state: NoticeState,
    /// Unambiguous road consequence, such as `Road open now`.
    pub road_meaning: String,
    /// Recommended user action.
    pub action: String,
    /// Estimated time to impact.
    pub eta: Option<EtaRange>,
    /// Model score for predictive states only.
    pub confidence_percent: Option<u8>,
    /// Ordered plain-language evidence statements.
    pub evidence: Vec<String>,
    /// Compact provenance label.
    pub source_label: String,
    /// Age of the newest relevant evidence.
    pub source_age_seconds: u64,
}

impl Notice {
    /// Produces the exact human-readable text used by the WhatsApp utility
    /// template.
    pub fn render_message(&self) -> Result<String, RequestError> {
        self.validate()?;
        let mut lines = vec![
            format!("{} - {}", self.subject.trim(), self.state.label()),
            format!(
                "{}: {}",
                if matches!(self.state, NoticeState::Alert | NoticeState::Resolved) {
                    "Signal"
                } else {
                    "Road"
                },
                sentence(&self.road_meaning)
            ),
            format!("Action: {}", sentence(&self.action)),
        ];
        if let Some(eta) = self.eta {
            lines.push(format!("ETA: {}", eta.render()));
        }
        if self.state.is_predictive()
            && let Some(confidence) = self.confidence_percent
        {
            lines.push(format!("Confidence: {confidence}%"));
        }
        if !self.evidence.is_empty() {
            lines.push(format!("Evidence: {}", self.evidence.join(" + ")));
        }
        lines.push(format!(
            "Source: {} ({} ago)",
            self.source_label.trim(),
            age_label(self.source_age_seconds)
        ));
        Ok(lines.join("\n"))
    }

    /// Enforces truthful message semantics before any provider call.
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.subject.trim().is_empty()
            || self.road_meaning.trim().is_empty()
            || self.action.trim().is_empty()
            || self.source_label.trim().is_empty()
        {
            return Err(RequestError::EmptyNoticeField);
        }
        if self.evidence.iter().any(|value| value.trim().is_empty()) {
            return Err(RequestError::EmptyEvidence);
        }
        if let Some(confidence) = self.confidence_percent {
            if confidence > 100 {
                return Err(RequestError::Confidence(confidence));
            }
            if !self.state.is_predictive() {
                return Err(RequestError::ConfidenceForConfirmedState(self.state));
            }
        }
        Ok(())
    }
}

/// One configured recipient.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    /// Stable user-owned destination identifier.
    pub id: String,
    /// Provider address, such as an E.164 WhatsApp number.
    pub address: String,
    /// Optional locale used by provider-specific templates.
    pub locale: Option<String>,
    /// Explicit permission state for user-directed messaging channels.
    pub messaging_consent: MessagingConsent,
}

/// Locally authoritative recipient permission state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MessagingConsent {
    /// No auditable opt-in exists; outbound WhatsApp is blocked.
    #[default]
    NotRecorded,
    /// User affirmatively opted in at the recorded Unix epoch millisecond.
    OptedIn {
        /// When consent was captured.
        recorded_at_millis: i64,
    },
    /// User unsubscribed; all non-essential messaging is suppressed.
    Unsubscribed {
        /// When suppression was captured.
        recorded_at_millis: i64,
    },
}

/// Persistable request stored in the transactional outbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryRequest {
    /// Unique outbox row identifier.
    pub outbox_id: Uuid,
    /// Stable incident grouping all bridge-stage revisions.
    pub incident_id: Uuid,
    /// Material revision within the incident.
    pub material_revision: u32,
    /// Stable semantic key used to suppress duplicate sends.
    pub deduplication_key: String,
    /// Why this revision was eligible for outbound delivery.
    pub reason: DeliveryReason,
    /// Recipient selected by user policy.
    pub destination: Destination,
    /// Provider-independent content.
    pub notice: Notice,
    /// Outbox creation time in Unix epoch milliseconds.
    pub created_at_millis: i64,
}

impl DeliveryRequest {
    /// Validates all fields needed for retry-safe dispatch.
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.deduplication_key.trim().is_empty()
            || self.destination.id.trim().is_empty()
            || self.destination.address.trim().is_empty()
        {
            return Err(RequestError::EmptyRoutingField);
        }
        self.notice.validate()
    }
}

/// Provider lifecycle stage observed by this adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStage {
    /// Provider accepted the request; human delivery remains unconfirmed.
    Accepted,
    /// A separate provider receipt confirmed delivery.
    Delivered,
}

/// Persistable successful adapter outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    /// Outbox row this result resolves.
    pub outbox_id: Uuid,
    /// Stable adapter identifier.
    pub adapter: String,
    /// Strongest lifecycle fact observed.
    pub stage: DeliveryStage,
    /// Provider message identifier, if returned.
    pub provider_message_id: Option<String>,
    /// Provider's own non-secret status label.
    pub provider_status: Option<String>,
}

/// Stable failure class used by outbox retry policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryFailureKind {
    /// Persisted request is invalid and must not be retried unchanged.
    InvalidRequest,
    /// Adapter or secret reference is incomplete.
    Misconfigured,
    /// Provider rejected credentials or permissions.
    Authentication,
    /// Provider permanently rejected this message.
    Rejected,
    /// Local consent/suppression policy deliberately prevented delivery.
    Suppressed,
    /// Provider throttled this sender.
    RateLimited,
    /// Network or upstream failure may recover.
    Transient,
}

impl DeliveryFailureKind {
    /// Whether a worker should schedule a later attempt without mutation.
    pub const fn retryable(self) -> bool {
        matches!(self, Self::RateLimited | Self::Transient)
    }
}

/// Persistable, redacted adapter failure.
#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize, Deserialize)]
#[error("{kind:?}: {message}")]
pub struct DeliveryFailure {
    /// Stable failure category.
    pub kind: DeliveryFailureKind,
    /// Redacted explanation suitable for local diagnostics.
    pub message: String,
    /// Provider error code, if safe and available.
    pub provider_code: Option<String>,
    /// Provider-directed retry delay.
    pub retry_after_seconds: Option<u64>,
}

impl DeliveryFailure {
    /// Creates a redacted failure.
    pub fn new(kind: DeliveryFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            provider_code: None,
            retry_after_seconds: None,
        }
    }

    /// Whether this failure is safe to retry unchanged.
    pub const fn retryable(&self) -> bool {
        self.kind.retryable()
    }
}

/// Invalid outbox request.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RequestError {
    /// Required route/destination field is empty.
    #[error("delivery routing fields cannot be empty")]
    EmptyRoutingField,
    /// Required user-facing message field is empty.
    #[error("notice subject, road meaning, action, and source are required")]
    EmptyNoticeField,
    /// Evidence contains an empty line.
    #[error("evidence lines cannot be empty")]
    EmptyEvidence,
    /// Confidence exceeded 100%.
    #[error("confidence must be 0..=100, got {0}")]
    Confidence(u8),
    /// Confirmed facts must not masquerade as model confidence.
    #[error("confidence is not valid for confirmed state {0:?}")]
    ConfidenceForConfirmedState(NoticeState),
}

impl From<RequestError> for DeliveryFailure {
    fn from(error: RequestError) -> Self {
        Self::new(DeliveryFailureKind::InvalidRequest, error.to_string())
    }
}

fn sentence(value: &str) -> String {
    let mut value = value.trim().to_owned();
    if !value.ends_with(['.', '!', '?']) {
        value.push('.');
    }
    value
}

fn age_label(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds} sec"),
        60..=3_599 => format!("{} min", seconds / 60),
        _ => format!("{} hr", seconds / 3_600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_messages_omit_model_confidence_by_invariant() {
        let notice = Notice {
            subject: "Brickell Avenue Bridge".into(),
            state: NoticeState::Open,
            road_meaning: "Road closed".into(),
            action: "Bridge open".into(),
            eta: None,
            confidence_percent: Some(100),
            evidence: vec!["FL511 controller state".into()],
            source_label: "FL511".into(),
            source_age_seconds: 12,
        };
        assert_eq!(
            notice.validate(),
            Err(RequestError::ConfidenceForConfirmedState(NoticeState::Open))
        );
    }

    #[test]
    fn rendered_notice_contains_decision_context() {
        let notice = Notice {
            subject: "Brickell Avenue Bridge".into(),
            state: NoticeState::Likely,
            road_meaning: "Road open now".into(),
            action: "Detour advised".into(),
            eta: Some(EtaRange::new(6, 9)),
            confidence_percent: Some(82),
            evidence: vec!["Outbound vessel".into(), "Upstream bridge".into()],
            source_label: "AIS + FL511".into(),
            source_age_seconds: 74,
        };
        let message = notice.render_message().unwrap();
        assert!(message.contains("Road: Road open now."));
        assert!(message.contains("Action: Detour advised."));
        assert!(message.contains("ETA: 6-9 min"));
        assert!(message.contains("Confidence: 82%"));
        assert!(message.contains("Source: AIS + FL511 (1 min ago)"));
    }

    #[test]
    fn generic_alert_uses_signal_copy_instead_of_bridge_copy() {
        let notice = Notice {
            subject: "Rain heads-up".into(),
            state: NoticeState::Alert,
            road_meaning: "Rain probability crossed 70 percent".into(),
            action: "Plan for wet conditions".into(),
            eta: None,
            confidence_percent: None,
            evidence: vec!["Open-Meteo threshold".into()],
            source_label: "Open-Meteo".into(),
            source_age_seconds: 40,
        };
        let message = notice.render_message().unwrap();
        assert!(message.contains("Signal: Rain probability crossed 70 percent."));
        assert!(!message.contains("Road:"));
    }
}
