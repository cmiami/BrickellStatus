//! Small value objects shared across model modules.

use serde::{Deserialize, Deserializer, Serialize, de};
use std::fmt;

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash,
            Serialize, Deserialize,         )]
        pub struct $name(pub String);

        impl $name {
            /// Creates an identifier from an owned or borrowed string.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the identifier as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

string_id!(
    /// Stable identifier for a configured channel.
    ChannelId
);
string_id!(
    /// Stable identifier for a source observation.
    ObservationId
);
string_id!(
    /// Stable identifier for a collector or upstream source.
    SourceId
);

/// An absolute instant represented as Unix epoch milliseconds.
///
/// Milliseconds are unambiguous across time zones, compact over serial links,
/// and exactly representable by JavaScript for all practical project dates.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct TimestampMillis(pub i64);

impl TimestampMillis {
    /// Creates a timestamp from Unix epoch milliseconds.
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Returns the Unix epoch millisecond value.
    pub const fn as_i64(self) -> i64 {
        self.0
    }

    /// Returns the non-negative whole-second age at `now`.
    ///
    /// Clock skew and future-dated samples intentionally have age zero.
    pub fn age_seconds_at(self, now: Self) -> u64 {
        now.0.saturating_sub(self.0).max(0) as u64 / 1_000
    }
}

/// A bounded confidence value expressed in basis points (`0..=10_000`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Confidence {
    /// Confidence in basis points; policy code must keep this at or below 10,000.
    pub basis_points: u16,
}

impl<'de> Deserialize<'de> for Confidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct EncodedConfidence {
            basis_points: u16,
        }

        let encoded = EncodedConfidence::deserialize(deserializer)?;
        if encoded.basis_points > 10_000 {
            return Err(de::Error::custom(
                "confidence basis_points must be at most 10000",
            ));
        }
        Ok(Self {
            basis_points: encoded.basis_points,
        })
    }
}

impl Confidence {
    /// No confidence.
    pub const ZERO: Self = Self { basis_points: 0 };

    /// Certain confidence.
    pub const CERTAIN: Self = Self {
        basis_points: 10_000,
    };

    /// Creates a confidence value, clamping values above 10,000.
    pub const fn from_basis_points(value: u16) -> Self {
        Self {
            basis_points: if value > 10_000 { 10_000 } else { value },
        }
    }

    /// Creates a confidence value from a normalized score.
    pub fn from_score(value: f32) -> Self {
        let basis_points = (value.clamp(0.0, 1.0) * 10_000.0).round() as u16;
        Self::from_basis_points(basis_points)
    }

    /// Returns a normalized score in `0.0..=1.0`.
    pub fn as_score(self) -> f32 {
        f32::from(self.basis_points) / 10_000.0
    }
}

/// A closed interval of estimated minutes from the evaluation time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtaRangeMinutes {
    /// Earliest estimated arrival.
    pub earliest: u16,
    /// Latest estimated arrival; never less than `earliest` when constructed
    /// through [`EtaRangeMinutes::new`].
    pub latest: u16,
}

impl EtaRangeMinutes {
    /// Creates an ordered ETA range.
    pub const fn new(earliest: u16, latest: u16) -> Self {
        Self {
            earliest,
            latest: if latest < earliest { earliest } else { latest },
        }
    }
}

/// Day of the week, independent of any particular date library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    /// Monday.
    Monday,
    /// Tuesday.
    Tuesday,
    /// Wednesday.
    Wednesday,
    /// Thursday.
    Thursday,
    /// Friday.
    Friday,
    /// Saturday.
    Saturday,
    /// Sunday.
    Sunday,
}

impl Weekday {
    /// Whether this is Monday through Friday.
    pub const fn is_weekday(self) -> bool {
        matches!(
            self,
            Self::Monday | Self::Tuesday | Self::Wednesday | Self::Thursday | Self::Friday
        )
    }
}

/// A local wall-clock time without a date or offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LocalTime {
    /// Hour in `0..=23`.
    pub hour: u8,
    /// Minute in `0..=59`.
    pub minute: u8,
}

impl LocalTime {
    /// Creates a local time when both components are valid.
    pub const fn new(hour: u8, minute: u8) -> Option<Self> {
        if hour <= 23 && minute <= 59 {
            Some(Self { hour, minute })
        } else {
            None
        }
    }

    /// Minutes elapsed since local midnight.
    pub const fn minute_of_day(self) -> u16 {
        self.hour as u16 * 60 + self.minute as u16
    }
}
