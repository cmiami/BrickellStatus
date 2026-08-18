use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum accepted custom channel label length before display fitting.
pub const MAX_CHANNEL_LABEL_CHARS: usize = 48;
/// Maximum accepted card title length before display fitting.
pub const MAX_CARD_TITLE_CHARS: usize = 96;
/// Maximum accepted headline length before display fitting.
pub const MAX_HEADLINE_CHARS: usize = 240;
/// Maximum accepted detail length before display fitting.
pub const MAX_DETAIL_CHARS: usize = 320;
/// Maximum accepted action length before display fitting.
pub const MAX_ACTION_CHARS: usize = 160;
/// Maximum accepted source label length before display fitting.
pub const MAX_SOURCE_CHARS: usize = 96;

/// Identity of a signal channel occupying one display rotation slot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    /// Local conditions, rain, wind, and other weather rules.
    Weather,
    /// Official life-safety notices such as NWS alerts.
    OfficialAlert,
    /// Tropical outlooks, watches, warnings, and track changes.
    Tropical,
    /// User-selected news and RSS sources.
    News,
    /// User-selected leagues, teams, and transaction desks.
    Sports,
    /// Earthquake observations and rules.
    Earthquake,
    /// User-selected market watch items.
    Markets,
    /// A future or user-defined channel with an explicit display label and code.
    Custom {
        /// Header label shown on the card.
        label: String,
        /// One- or two-character rail registration code.
        code: String,
    },
}

impl ChannelKind {
    /// Human-readable channel label.
    pub fn label(&self) -> &str {
        match self {
            Self::Weather => "WEATHER",
            Self::OfficialAlert => "OFFICIAL ALERT",
            Self::Tropical => "TROPICAL",
            Self::News => "NEWS",
            Self::Sports => "SPORTS",
            Self::Earthquake => "EARTHQUAKE",
            Self::Markets => "MARKETS",
            Self::Custom { label, .. } => label,
        }
    }

    /// Compact rail code.
    pub fn code(&self) -> &str {
        match self {
            Self::Weather => "WX",
            Self::OfficialAlert => "AL",
            Self::Tropical => "TS",
            Self::News => "NW",
            Self::Sports => "SP",
            Self::Earthquake => "EQ",
            Self::Markets => "MK",
            Self::Custom { code, .. } => code,
        }
    }

    fn validate(&self) -> Result<(), ChannelCardError> {
        if let Self::Custom { label, code } = self {
            validate_text(label, "channel label", MAX_CHANNEL_LABEL_CHARS)?;
            let code = code.trim();
            if !(1..=2).contains(&code.chars().count())
                || !code
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
            {
                return Err(ChannelCardError::InvalidChannelCode);
            }
        }
        Ok(())
    }
}

/// How strongly a card should claim the user's attention.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelUrgency {
    /// Background context with no meaningful action due now.
    #[default]
    Routine,
    /// A noteworthy change the user asked to see.
    Advisory,
    /// A time-sensitive condition that may justify interruption.
    Urgent,
    /// A life-safety or immediate-action condition.
    Critical,
}

impl ChannelUrgency {
    /// Plain display label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Routine => "ROUTINE",
            Self::Advisory => "ADVISORY",
            Self::Urgent => "URGENT",
            Self::Critical => "CRITICAL",
        }
    }

    /// Whether the card receives the hard interruption treatment.
    pub const fn is_interrupting(self) -> bool {
        matches!(self, Self::Urgent | Self::Critical)
    }
}

/// Whether the source behind a channel card can currently be trusted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelAvailability {
    /// Source data is within the user's freshness expectation.
    #[default]
    Current,
    /// Source data exists but exceeded the freshness expectation.
    Stale,
    /// A configured source cannot currently be reached.
    Offline,
    /// The channel cannot run until configuration or credentials are supplied.
    Unavailable,
}

impl ChannelAvailability {
    /// Plain display label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Current => "LIVE",
            Self::Stale => "STALE",
            Self::Offline => "OFFLINE",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

/// Provenance and optional age of the observation behind a card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelSource {
    /// Concise source name, such as `NWS` or `OPEN-METEO`.
    pub name: String,
    /// Age of the newest relevant observation, when one exists.
    pub age_seconds: Option<u64>,
}

impl ChannelSource {
    /// Creates a source stamp with a known observation age.
    pub fn aged(name: impl Into<String>, age_seconds: u64) -> Self {
        Self {
            name: name.into(),
            age_seconds: Some(age_seconds),
        }
    }

    /// Creates a source stamp before an observation exists.
    pub fn unavailable(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            age_seconds: None,
        }
    }

    /// Compact age value suitable for the source tape.
    pub fn age_label(&self) -> Option<String> {
        self.age_seconds.map(compact_age)
    }
}

/// Transport-neutral content for a non-bridge e-paper rotation slot.
///
/// The semantic fields stay independent from USB, BLE, and the INK1 wire
/// format. Text is validated here and fitted only while rendering, so callers
/// retain the complete source copy for other destinations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelCard {
    /// Owning channel and its stable rail identity.
    pub channel: ChannelKind,
    /// Attention level chosen by the channel policy.
    pub urgency: ChannelUrgency,
    /// Health of the source used to produce this card.
    pub availability: ChannelAvailability,
    /// Location, watch item, desk, or other short scope label.
    pub title: String,
    /// Primary fact a glance should communicate.
    pub headline: String,
    /// One concise supporting measurement or explanation.
    pub detail: String,
    /// One concise status line for the current channel state.
    pub action: String,
    /// Source identity and observation age.
    pub source: ChannelSource,
}

/// Semantic frame is an alias for a channel card when used in rotation APIs.
pub type ChannelFrame = ChannelCard;

impl ChannelCard {
    /// Creates a complete card. Call [`Self::validate`] before retaining it, or
    /// pass it to the renderer, which validates automatically.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        channel: ChannelKind,
        urgency: ChannelUrgency,
        availability: ChannelAvailability,
        title: impl Into<String>,
        headline: impl Into<String>,
        detail: impl Into<String>,
        action: impl Into<String>,
        source: ChannelSource,
    ) -> Self {
        Self {
            channel,
            urgency,
            availability,
            title: title.into(),
            headline: headline.into(),
            detail: detail.into(),
            action: action.into(),
            source,
        }
    }

    /// Validates semantic and resource bounds before pixels are produced.
    pub fn validate(&self) -> Result<(), ChannelCardError> {
        self.channel.validate()?;
        validate_text(&self.title, "title", MAX_CARD_TITLE_CHARS)?;
        validate_text(&self.headline, "headline", MAX_HEADLINE_CHARS)?;
        validate_text(&self.detail, "detail", MAX_DETAIL_CHARS)?;
        validate_text(&self.action, "action", MAX_ACTION_CHARS)?;
        validate_text(&self.source.name, "source", MAX_SOURCE_CHARS)?;

        if matches!(
            self.availability,
            ChannelAvailability::Current | ChannelAvailability::Stale
        ) && self.source.age_seconds.is_none()
        {
            return Err(ChannelCardError::MissingSourceAge(self.availability));
        }

        Ok(())
    }
}

/// Invalid generic channel card.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ChannelCardError {
    /// A required field had no characters supported by the panel font.
    #[error("channel card {0} cannot be empty")]
    Empty(&'static str),
    /// A field exceeded its defensive input bound.
    #[error("channel card {field} is {actual} characters; maximum is {maximum}")]
    TooLong {
        /// Field name.
        field: &'static str,
        /// Accepted character count.
        maximum: usize,
        /// Supplied character count.
        actual: usize,
    },
    /// A custom channel code was not one or two ASCII letters or digits.
    #[error("custom channel code must be one or two ASCII letters or digits")]
    InvalidChannelCode,
    /// A current or stale card did not include source age.
    #[error("{0:?} channel card requires a source age")]
    MissingSourceAge(ChannelAvailability),
}

fn validate_text(value: &str, field: &'static str, maximum: usize) -> Result<(), ChannelCardError> {
    let actual = value.chars().count();
    if actual > maximum {
        return Err(ChannelCardError::TooLong {
            field,
            maximum,
            actual,
        });
    }
    if display_ascii(value).is_empty() {
        return Err(ChannelCardError::Empty(field));
    }
    Ok(())
}

/// A Latin letter carrying a diacritic, folded to the ASCII base the panel
/// faces can actually draw.
///
/// Dropping these to a space is worse than dropping the accent: "se sintió en
/// La Guaira" became "SE SINTI EN LA GUAIRA", and a reader cannot recover the
/// missing letter. Every Spanish, French, and Portuguese feed we carry hits
/// this on an ordinary day.
///
/// Every replacement is ASCII, and that is load-bearing. `wrap` slices by byte
/// offset while counting characters, so it is only sound while everything
/// downstream of here is single-byte.
fn fold_latin(character: char) -> Option<&'static str> {
    Some(match character {
        'À'..='Å' | 'à'..='å' | 'Ā'..='ą' => "A",
        'Æ' | 'æ' => "AE",
        'Ç' | 'ç' | 'Ć'..='č' => "C",
        'Ð' | 'ð' | 'Ď'..='đ' => "D",
        'È'..='Ë' | 'è'..='ë' | 'Ē'..='ě' => "E",
        'Ĝ'..='ģ' => "G",
        'Ĥ'..='ħ' => "H",
        'Ì'..='Ï' | 'ì'..='ï' | 'Ĩ'..='ı' => "I",
        'Ĳ' | 'ĳ' => "IJ",
        'Ĵ' | 'ĵ' => "J",
        'Ķ'..='ĸ' => "K",
        'Ĺ'..='ł' => "L",
        'Ñ' | 'ñ' | 'Ń'..='ň' => "N",
        'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' | 'Ō'..='ő' => "O",
        'Œ' | 'œ' => "OE",
        'Ŕ'..='ř' => "R",
        'Ś'..='š' => "S",
        'ß' => "SS",
        'Þ' | 'þ' => "TH",
        'Ţ'..='ŧ' => "T",
        'Ù'..='Ü' | 'ù'..='ü' | 'Ũ'..='ų' => "U",
        'Ŵ' | 'ŵ' => "W",
        'Ý' | 'ý' | 'ÿ' | 'Ŷ'..='Ÿ' => "Y",
        'Ź'..='ž' => "Z",
        _ => return None,
    })
}

pub(crate) fn display_ascii(value: &str) -> String {
    let mut folded = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\u{2018}' | '\u{2019}' => folded.push('\''),
            '\u{201c}' | '\u{201d}' => folded.push('"'),
            '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}' => folded.push('-'),
            '\u{2022}' | '\u{00b7}' => folded.push('/'),
            character if character.is_ascii_graphic() => {
                folded.push(character.to_ascii_uppercase());
            }
            character if character.is_whitespace() => folded.push(' '),
            character => match fold_latin(character) {
                Some(replacement) => folded.push_str(replacement),
                None => folded.push(' '),
            },
        }
    }
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compact_age(age_seconds: u64) -> String {
    match age_seconds {
        0..=59 => format!("{age_seconds}S"),
        60..=3_599 => format!("{}M", age_seconds / 60),
        3_600..=86_399 => format!("{}H", age_seconds / 3_600),
        _ => format!("{}D", age_seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_card() -> ChannelCard {
        ChannelCard::new(
            ChannelKind::Weather,
            ChannelUrgency::Advisory,
            ChannelAvailability::Current,
            "Miami / Brickell",
            "Heavy rain in 12 min",
            "0.6 in/hr, gusts 31 mph",
            "Take cover by 4:20 PM",
            ChannelSource::aged("Open-Meteo", 42),
        )
    }

    #[test]
    fn a_complete_card_validates() {
        assert_eq!(valid_card().validate(), Ok(()));
    }

    #[test]
    fn accented_headlines_keep_every_letter() {
        // Real headlines pulled from the Venezuelan, Colombian, Haitian, and
        // Cuban feeds this build ships. Folding to a space used to leave a hole
        // where the letter was, and a reader cannot guess it back.
        for (raw, expected) in [
            (
                "Sismo de magnitud 4 se sintió en La Guaira",
                "SISMO DE MAGNITUD 4 SE SINTIO EN LA GUAIRA",
            ),
            (
                "El Niño trajo una sequía histórica a la región",
                "EL NINO TRAJO UNA SEQUIA HISTORICA A LA REGION",
            ),
            (
                "Piratage du site des impôts : le gouvernement détaille",
                "PIRATAGE DU SITE DES IMPOTS : LE GOUVERNEMENT DETAILLE",
            ),
            (
                "La Defensa Civil desmiente alerta",
                "LA DEFENSA CIVIL DESMIENTE ALERTA",
            ),
        ] {
            assert_eq!(display_ascii(raw), expected);
        }
    }

    #[test]
    fn folded_panel_copy_is_always_single_byte() {
        // `wrap` slices by byte offset while counting characters. That is only
        // sound while nothing downstream of `display_ascii` is multi-byte, so a
        // fold that ever let a non-ASCII character through would panic on a
        // character boundary rather than merely look wrong.
        let sampler = "àáâãäåæçèéêëìíîïñòóôõöøùúûüýÿßœšžłğđŧůř";
        let folded = display_ascii(sampler);
        assert!(folded.is_ascii(), "fold leaked a multi-byte char: {folded}");
        for character in sampler.chars() {
            assert!(
                fold_latin(character).is_some(),
                "U+{:04X} '{character}' has no ASCII base and would print as a hole",
                character as u32,
            );
        }
    }

    #[test]
    fn visible_panel_copy_is_required() {
        let mut card = valid_card();
        card.headline = "🌧️".into();
        assert_eq!(card.validate(), Err(ChannelCardError::Empty("headline")));
    }

    #[test]
    fn current_and_stale_cards_require_source_age() {
        for availability in [ChannelAvailability::Current, ChannelAvailability::Stale] {
            let mut card = valid_card();
            card.availability = availability;
            card.source.age_seconds = None;
            assert_eq!(
                card.validate(),
                Err(ChannelCardError::MissingSourceAge(availability))
            );
        }
    }

    #[test]
    fn offline_cards_can_exist_before_the_first_observation() {
        let mut card = valid_card();
        card.availability = ChannelAvailability::Offline;
        card.source = ChannelSource::unavailable("NWS");
        assert_eq!(card.validate(), Ok(()));
    }

    #[test]
    fn custom_codes_are_strict_and_small() {
        let mut card = valid_card();
        card.channel = ChannelKind::Custom {
            label: "COMMUTE".into(),
            code: "ROAD".into(),
        };
        assert_eq!(card.validate(), Err(ChannelCardError::InvalidChannelCode));
    }

    #[test]
    fn oversized_input_is_rejected_before_rendering() {
        let mut card = valid_card();
        card.detail = "A".repeat(MAX_DETAIL_CHARS + 1);
        assert_eq!(
            card.validate(),
            Err(ChannelCardError::TooLong {
                field: "detail",
                maximum: MAX_DETAIL_CHARS,
                actual: MAX_DETAIL_CHARS + 1,
            })
        );
    }

    #[test]
    fn common_editor_punctuation_has_a_deterministic_panel_form() {
        assert_eq!(
            display_ascii("storm’s track — west • now"),
            "STORM'S TRACK - WEST / NOW"
        );
    }

    #[test]
    fn ages_use_compact_stable_units() {
        assert_eq!(
            ChannelSource::aged("NWS", 59).age_label().as_deref(),
            Some("59S")
        );
        assert_eq!(
            ChannelSource::aged("NWS", 60).age_label().as_deref(),
            Some("1M")
        );
        assert_eq!(
            ChannelSource::aged("NWS", 86_400).age_label().as_deref(),
            Some("1D")
        );
    }
}
