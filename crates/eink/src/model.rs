use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Presentation-stage vocabulary for the bridge signal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotState {
    /// No current evidence of a likely opening.
    #[default]
    Clear,
    /// Predictive evidence is strong enough to advise a route change.
    Likely,
    /// Controller evidence says the span is open.
    Open,
    /// No sufficiently fresh state is available.
    Offline,
}

impl SnapshotState {
    /// Short, unambiguous display label.
    pub const fn label(self) -> &'static str {
        match self {
            // The title immediately above already names the bridge. These
            // mechanical positions stay whole on the smaller E213 panel.
            Self::Clear => "CLOSED",
            Self::Likely => "OPENING",
            Self::Open => "OPEN",
            Self::Offline => "OFFLINE",
        }
    }

    /// Whether a numeric confidence is meaningful for this state.
    pub const fn is_predictive(self) -> bool {
        matches!(self, Self::Likely)
    }

    /// Whether the layout should use its hard interruption treatment.
    pub const fn is_interrupting(self) -> bool {
        matches!(self, Self::Likely | Self::Open)
    }

    /// Plain road consequence, independent of the internal bridge state.
    pub const fn road_meaning(self) -> &'static str {
        match self {
            Self::Clear | Self::Likely => "TRAFFIC FLOWING",
            Self::Open => "TRAFFIC BLOCKED",
            Self::Offline => "TRAFFIC STATUS UNKNOWN",
        }
    }
}

/// Closed ETA interval in whole minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtaRange {
    /// Earliest estimated impact.
    pub earliest_minutes: u16,
    /// Latest estimated impact.
    pub latest_minutes: u16,
}

impl EtaRange {
    /// Creates an ordered interval, clamping `latest` to `earliest`.
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

    /// Formats an e-paper-safe ETA without ambiguous punctuation.
    pub fn display(self) -> String {
        if self.earliest_minutes == self.latest_minutes {
            format!("{} MIN", self.earliest_minutes)
        } else {
            format!("{}-{} MIN", self.earliest_minutes, self.latest_minutes)
        }
    }
}

/// Human interpretation of a numeric model score.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfidenceBand {
    /// Weak signal below 50%.
    Low,
    /// Material but unresolved signal from 50–74%.
    Moderate,
    /// Strong signal from 75–89%.
    High,
    /// Very strong predictive signal at 90% or above.
    VeryHigh,
}

impl ConfidenceBand {
    /// Maps a bounded percentage to a plain-language band.
    pub const fn from_percent(percent: u8) -> Self {
        match percent {
            0..=49 => Self::Low,
            50..=74 => Self::Moderate,
            75..=89 => Self::High,
            _ => Self::VeryHigh,
        }
    }

    /// Compact label suitable for the confidence stamp.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Moderate => "MOD",
            Self::High => "HIGH",
            Self::VeryHigh => "V HIGH",
        }
    }
}

/// One plain-language reason behind the current status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// Concise evidence statement, such as `OUTBOUND VESSEL`.
    pub summary: String,
    /// Source label, such as `AISSTREAM` or `FL511`.
    pub source: String,
}

impl Evidence {
    /// Creates an evidence line.
    pub fn new(summary: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            source: source.into(),
        }
    }
}

/// Freshness of the source set behind a rendered snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Freshness {
    /// Compact owning source label.
    pub source: String,
    /// Age of the newest relevant observation.
    pub age_seconds: u64,
    /// Age at which the display must mark the data stale.
    pub stale_after_seconds: u64,
}

impl Freshness {
    /// Creates freshness metadata.
    pub fn new(source: impl Into<String>, age_seconds: u64, stale_after_seconds: u64) -> Self {
        Self {
            source: source.into(),
            age_seconds,
            stale_after_seconds,
        }
    }

    /// Whether this source exceeded its configured freshness target.
    pub const fn is_stale(&self) -> bool {
        self.age_seconds > self.stale_after_seconds
    }

    /// Compact age value for the provenance tape.
    pub fn age_label(&self) -> String {
        match self.age_seconds {
            0..=59 => format!("{}S", self.age_seconds),
            60..=3_599 => format!("{}M", self.age_seconds / 60),
            _ => format!("{}H", self.age_seconds / 3_600),
        }
    }
}

/// One upstream Miami River span, shown beside the target bridge so a reader can
/// tell whether an opening lines up with a vessel working the river.
///
/// An upstream span lifting shortly before or after Brickell is the same vessel
/// under way. That correlation is the whole reason these appear on a display
/// this small, so the opening time matters as much as the state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanStatus {
    /// Two- or three-character mark, e.g. `2AV`. The display has room for
    /// nothing longer beside a clock time.
    pub code: String,
    /// Whether the span is up right now.
    pub open: bool,
    /// Pre-formatted local clock time the span opened, e.g. `14:20`. Rendering
    /// stays free of time-zone logic; the caller has already resolved it.
    pub opened_at: Option<String>,
}

impl SpanStatus {
    /// Creates a span status with no recorded opening time.
    pub fn new(code: impl Into<String>, open: bool) -> Self {
        Self {
            code: code.into(),
            open,
            opened_at: None,
        }
    }

    /// Attaches the pre-formatted local clock time the span opened.
    pub fn opened_at(mut self, at: impl Into<String>) -> Self {
        self.opened_at = Some(at.into());
        self
    }
}

/// Complete, transport-neutral content for one live e-paper frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveSnapshot {
    /// Short channel title, normally `BRICKELL BRIDGE`.
    pub channel: String,
    /// Current semantic stage.
    pub state: SnapshotState,
    /// Optional ETA for pre-impact states.
    pub eta: Option<EtaRange>,
    /// Model confidence, present only for predictive states.
    pub confidence_percent: Option<u8>,
    /// Explicit road consequence; defaults are available on the state.
    pub road_meaning: String,
    /// Ordered, human-readable reasons for the status.
    pub evidence: Vec<Evidence>,
    /// Source identity and age.
    pub freshness: Freshness,
    /// Upstream river spans, in river order. At most two are drawn.
    #[serde(default)]
    pub spans: Vec<SpanStatus>,
}

impl LiveSnapshot {
    /// Creates a Brickell snapshot with a factual road-status default.
    pub fn brickell(state: SnapshotState, freshness: Freshness) -> Self {
        Self {
            channel: "BRICKELL BRIDGE".into(),
            state,
            eta: None,
            confidence_percent: None,
            road_meaning: state.road_meaning().into(),
            evidence: Vec::new(),
            freshness,
            spans: Vec::new(),
        }
    }

    /// Validates semantic invariants before pixels are produced.
    pub fn validate(&self) -> Result<(), SnapshotError> {
        if self.channel.trim().is_empty() {
            return Err(SnapshotError::EmptyChannel);
        }
        if self.road_meaning.trim().is_empty() {
            return Err(SnapshotError::EmptyRoadCopy);
        }
        if self.freshness.source.trim().is_empty() {
            return Err(SnapshotError::EmptySource);
        }
        if let Some(percent) = self.confidence_percent {
            if percent > 100 {
                return Err(SnapshotError::Confidence(percent));
            }
            if !self.state.is_predictive() {
                return Err(SnapshotError::ConfidenceForConfirmedState(self.state));
            }
        }
        if self
            .evidence
            .iter()
            .any(|item| item.summary.trim().is_empty() || item.source.trim().is_empty())
        {
            return Err(SnapshotError::EmptyEvidence);
        }
        if self.spans.iter().any(|span| span.code.trim().is_empty()) {
            return Err(SnapshotError::EmptySpanCode);
        }
        Ok(())
    }
}

/// Invalid presentation snapshot.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SnapshotError {
    /// Channel title is blank.
    #[error("snapshot channel cannot be empty")]
    EmptyChannel,
    /// Road consequence or action is blank.
    #[error("snapshot road meaning and action cannot be empty")]
    EmptyRoadCopy,
    /// Freshness source is blank.
    #[error("snapshot source cannot be empty")]
    EmptySource,
    /// Evidence text or source is blank.
    #[error("snapshot evidence needs both a summary and source")]
    EmptyEvidence,
    /// An upstream span carried no display code.
    #[error("span code cannot be empty")]
    EmptySpanCode,
    /// Confidence exceeded 100%.
    #[error("confidence must be 0..=100, got {0}")]
    Confidence(u8),
    /// A confirmed state incorrectly carried a predictive confidence score.
    #[error("confidence is not shown for non-predictive state {0:?}")]
    ConfidenceForConfirmedState(SnapshotState),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_state_rejects_predictive_confidence() {
        let mut snapshot =
            LiveSnapshot::brickell(SnapshotState::Open, Freshness::new("FL511", 10, 120));
        snapshot.confidence_percent = Some(100);
        assert_eq!(
            snapshot.validate(),
            Err(SnapshotError::ConfidenceForConfirmedState(
                SnapshotState::Open
            ))
        );
    }

    #[test]
    fn eta_ranges_are_ordered() {
        assert_eq!(EtaRange::new(8, 3), EtaRange::new(8, 8));
    }

    #[test]
    fn bridge_position_and_traffic_consequence_are_unambiguous() {
        assert_eq!(SnapshotState::Clear.label(), "CLOSED");
        assert_eq!(SnapshotState::Clear.road_meaning(), "TRAFFIC FLOWING");
        assert_eq!(SnapshotState::Likely.label(), "OPENING");
        assert_eq!(SnapshotState::Open.label(), "OPEN");
        assert_eq!(SnapshotState::Open.road_meaning(), "TRAFFIC BLOCKED");
    }
}
