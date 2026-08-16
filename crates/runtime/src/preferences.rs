use std::collections::{BTreeMap, BTreeSet};

use jiff::tz::TimeZone;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    AisSettings, AlertArea, AlertAreaSource, AppPreferences, ChannelKindDto, ChannelPreference,
    DestinationIdDto, DisplaySettings, DisplayTransport, InterruptPreset, PolicyProfile,
    ProfilePreset, QuietHours, SurfacePresence, WhatsAppRecipientConsent, WhatsAppSettings,
};

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum PreferencesError {
    #[error("{0}")]
    Invalid(String),
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            unit_system: crate::UnitSystem::Imperial,
            areas: default_alert_areas(),
            profile: PolicyProfile {
                id: "profile.bridge-first".into(),
                name: "Bridge First".into(),
                preset: ProfilePreset::BridgeFirst,
                home_channel_id: "bridge.brickell".into(),
                quiet_hours: QuietHours {
                    enabled: true,
                    start: "22:00".into(),
                    end: "06:30".into(),
                    time_zone: "America/New_York".into(),
                    bypass_emergency: true,
                },
                channels: default_channel_preferences(),
            },
            display: DisplaySettings {
                // Fresh installs never touch a serial or BLE device until the
                // user explicitly chooses one from the Outputs desk.
                transport: DisplayTransport::Preview,
                serial_port: "auto".into(),
                ble_name: "InkDock E213".into(),
                dwell_seconds: 28,
                return_home_after: 2,
                full_refresh_every: 12,
            },
            whatsapp: WhatsAppSettings {
                enabled: false,
                phone_number_id: String::new(),
                recipient: String::new(),
                graph_version: "v23.0".into(),
                template_name: "bridge_status_update".into(),
                language_code: "en_US".into(),
                token_configured: false,
                consent: WhatsAppRecipientConsent::NotRecorded,
                consent_recipient: None,
                consent_recorded_at_millis: None,
            },
            ais: AisSettings::default(),
        }
    }
}

pub fn default_alert_areas() -> Vec<AlertArea> {
    vec![AlertArea {
        id: "area.miami".into(),
        label: "Miami, Florida".into(),
        latitude: 25.7617,
        longitude: -80.1918,
        time_zone: "America/New_York".into(),
        country_code: Some("US".into()),
        admin_area: Some("Florida".into()),
        source: AlertAreaSource::Preset,
        enabled: true,
        weather_enabled: true,
        official_alerts_enabled: true,
        tropical_context_enabled: true,
    }]
}

pub fn default_channel_preferences() -> Vec<ChannelPreference> {
    vec![
        ChannelPreference {
            id: "bridge.brickell".into(),
            kind: ChannelKindDto::Bridge,
            title: "Brickell bridge".into(),
            enabled: true,
            presence: SurfacePresence::Home,
            interrupt_preset: InterruptPreset::Recommended,
            destinations: vec![
                DestinationIdDto::Epaper,
                DestinationIdDto::Whatsapp,
                DestinationIdDto::Desktop,
            ],
            max_age_minutes: 2,
            max_items: 1,
            rotation_seconds: 28,
            scope: BTreeMap::from([
                ("bridge".into(), json!("Brickell Avenue Bridge")),
                ("latitude".into(), json!(25.7699)),
                ("longitude".into(), json!(-80.19005)),
                ("radiusMeters".into(), json!(250)),
                ("timeZone".into(), json!("America/New_York")),
                ("useFl511".into(), json!(true)),
                ("useUpstream".into(), json!(true)),
            ]),
        },
        ChannelPreference {
            id: "weather.miami".into(),
            kind: ChannelKindDto::Weather,
            title: "Miami weather".into(),
            enabled: true,
            presence: SurfacePresence::Rotation,
            interrupt_preset: InterruptPreset::Recommended,
            destinations: vec![DestinationIdDto::Epaper, DestinationIdDto::Desktop],
            max_age_minutes: 15,
            max_items: 2,
            rotation_seconds: 26,
            scope: BTreeMap::from([
                ("place".into(), json!("Miami, FL")),
                ("latitude".into(), json!(25.7617)),
                ("longitude".into(), json!(-80.1918)),
                ("areaIds".into(), json!(["area.miami"])),
                ("rainAlertEnabled".into(), json!(true)),
                ("windAlertEnabled".into(), json!(true)),
                ("radarEnabled".into(), json!(true)),
                ("rainProbabilityThreshold".into(), json!(60)),
                ("rainLeadMinutes".into(), json!(90)),
                ("windGustMph".into(), json!(40)),
            ]),
        },
        ChannelPreference {
            id: "official.miami".into(),
            kind: ChannelKindDto::Official,
            title: "Official alerts".into(),
            enabled: true,
            presence: SurfacePresence::ActiveOnly,
            interrupt_preset: InterruptPreset::ConfirmedOnly,
            destinations: vec![
                DestinationIdDto::Epaper,
                DestinationIdDto::Whatsapp,
                DestinationIdDto::Desktop,
            ],
            max_age_minutes: 2,
            max_items: 2,
            rotation_seconds: 30,
            scope: BTreeMap::from([
                ("place".into(), json!("Miami, FL")),
                ("point".into(), json!("25.7617,-80.1918")),
                ("areaIds".into(), json!(["area.miami"])),
                ("severity".into(), json!(["Severe", "Extreme"])),
                ("includeStatements".into(), json!(false)),
            ]),
        },
        ChannelPreference {
            id: "hurricane.atlantic".into(),
            kind: ChannelKindDto::Hurricane,
            title: "Atlantic hurricanes".into(),
            enabled: true,
            presence: SurfacePresence::ActiveOnly,
            interrupt_preset: InterruptPreset::Meaningful,
            destinations: vec![DestinationIdDto::Epaper, DestinationIdDto::Desktop],
            max_age_minutes: 360,
            max_items: 2,
            rotation_seconds: 28,
            scope: BTreeMap::from([
                ("place".into(), json!("Miami, FL")),
                ("areaIds".into(), json!(["area.miami"])),
            ]),
        },
        ChannelPreference {
            id: "news.local".into(),
            kind: ChannelKindDto::News,
            title: "Local news".into(),
            enabled: true,
            presence: SurfacePresence::Rotation,
            interrupt_preset: InterruptPreset::Off,
            destinations: vec![DestinationIdDto::Epaper],
            max_age_minutes: 180,
            max_items: 3,
            rotation_seconds: 24,
            scope: BTreeMap::from([
                (
                    "feeds".into(),
                    json!([
                        "https://www.miamidade.gov/global/rss-news.page",
                        "https://wsvn.com/feed/",
                        "https://www.local10.com/arc/outboundfeeds/rss/?outputType=xml"
                    ]),
                ),
                ("topics".into(), json!(["Miami", "transportation"])),
                ("excludeTopics".into(), json!([])),
                ("breakingOnly".into(), json!(false)),
            ]),
        },
        ChannelPreference {
            id: "earthquake.significant".into(),
            kind: ChannelKindDto::Earthquake,
            title: "Significant earthquakes".into(),
            enabled: false,
            presence: SurfacePresence::ActiveOnly,
            interrupt_preset: InterruptPreset::ConfirmedOnly,
            destinations: vec![DestinationIdDto::Desktop],
            max_age_minutes: 60,
            max_items: 1,
            rotation_seconds: 24,
            scope: BTreeMap::from([
                ("feed".into(), json!("significant_hour")),
                ("minimumMagnitude".into(), json!(5.5)),
                ("eventAgeMinutes".into(), json!(60)),
            ]),
        },
        ChannelPreference {
            id: "markets.watchlist".into(),
            kind: ChannelKindDto::Markets,
            title: "Market watchlist".into(),
            enabled: false,
            presence: SurfacePresence::Rotation,
            interrupt_preset: InterruptPreset::Off,
            destinations: vec![DestinationIdDto::Epaper],
            max_age_minutes: 20,
            max_items: 2,
            rotation_seconds: 20,
            scope: BTreeMap::from([
                ("symbols".into(), json!(["AMD"])),
                ("movePercent".into(), json!(5)),
                ("pollSeconds".into(), json!(300)),
            ]),
        },
    ]
}

pub fn validate_preferences(preferences: &AppPreferences) -> Result<(), PreferencesError> {
    if !preferences.ais.radius_kilometers.is_finite()
        || !(2.0..=30.0).contains(&preferences.ais.radius_kilometers)
    {
        return invalid("ais.radiusKilometers must be between 2 and 30");
    }
    if preferences.areas.len() > 64 {
        return invalid("areas must contain at most 64 entries");
    }
    let mut area_ids = BTreeSet::new();
    for area in &preferences.areas {
        validate_alert_area(area)?;
        if !area_ids.insert(area.id.as_str()) {
            return invalid(format!("duplicate alert area id {:?}", area.id));
        }
    }
    bounded_text("profile.id", &preferences.profile.id, 1, 128)?;
    bounded_text("profile.name", &preferences.profile.name, 1, 120)?;
    bounded_text(
        "profile.homeChannelId",
        &preferences.profile.home_channel_id,
        1,
        128,
    )?;
    validate_quiet_hours(&preferences.profile.quiet_hours)?;
    if preferences.profile.channels.is_empty() || preferences.profile.channels.len() > 64 {
        return invalid("profile.channels must contain between 1 and 64 channels");
    }
    let mut ids = BTreeSet::new();
    for channel in &preferences.profile.channels {
        validate_channel(channel, &area_ids)?;
        if !ids.insert(channel.id.as_str()) {
            return invalid(format!("duplicate channel id {:?}", channel.id));
        }
    }
    if !ids.contains(preferences.profile.home_channel_id.as_str()) {
        return invalid("profile.homeChannelId does not name a configured channel");
    }

    if !(5..=3_600).contains(&preferences.display.dwell_seconds) {
        return invalid("display.dwellSeconds must be between 5 and 3600");
    }
    if preferences.display.return_home_after > 1_000 {
        return invalid("display.returnHomeAfter must be at most 1000");
    }
    if !(1..=1_000).contains(&preferences.display.full_refresh_every) {
        return invalid("display.fullRefreshEvery must be between 1 and 1000");
    }
    bounded_text(
        "display.serialPort",
        &preferences.display.serial_port,
        0,
        512,
    )?;
    bounded_text("display.bleName", &preferences.display.ble_name, 0, 248)?;

    bounded_text(
        "whatsapp.graphVersion",
        &preferences.whatsapp.graph_version,
        1,
        32,
    )?;
    bounded_text(
        "whatsapp.templateName",
        &preferences.whatsapp.template_name,
        1,
        128,
    )?;
    bounded_text(
        "whatsapp.languageCode",
        &preferences.whatsapp.language_code,
        1,
        32,
    )?;
    if !preferences
        .whatsapp
        .graph_version
        .chars()
        .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '-' | '_'))
    {
        return invalid("whatsapp.graphVersion contains invalid characters");
    }
    if !preferences
        .whatsapp
        .template_name
        .chars()
        .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '_')
    {
        return invalid(
            "whatsapp.templateName must contain lowercase ASCII letters, digits, or underscores",
        );
    }
    if !valid_whatsapp_language_code(&preferences.whatsapp.language_code) {
        return invalid(
            "whatsapp.languageCode must be a two- or three-letter lowercase language with an optional uppercase region (for example en_US)",
        );
    }
    if preferences.whatsapp.enabled {
        bounded_text(
            "whatsapp.phoneNumberId",
            &preferences.whatsapp.phone_number_id,
            1,
            128,
        )?;
        if !preferences
            .whatsapp
            .phone_number_id
            .chars()
            .all(|value| value.is_ascii_digit())
        {
            return invalid("whatsapp.phoneNumberId must contain only digits");
        }
        bounded_text("whatsapp.recipient", &preferences.whatsapp.recipient, 1, 64)?;
        validate_whatsapp_recipient(&preferences.whatsapp.recipient)?;
    }
    if let Some(consent_recipient) = &preferences.whatsapp.consent_recipient {
        bounded_text("whatsapp.consentRecipient", consent_recipient, 1, 64)?;
        validate_whatsapp_recipient(consent_recipient)?;
    }
    if preferences.whatsapp.consent == WhatsAppRecipientConsent::OptedIn {
        if preferences.whatsapp.recipient.trim().is_empty() {
            return invalid("whatsapp.recipient is required when consent=opted_in");
        }
        if !whatsapp_consent_is_current(&preferences.whatsapp) {
            return invalid(
                "whatsapp opted-in consent must match the current trimmed recipient and include a positive capture time",
            );
        }
    }
    Ok(())
}

/// True only when opt-in consent is bound to the exact current recipient and
/// carries a usable capture time. Delivery routes use this as a fail-closed
/// predicate rather than trusting the consent enum by itself.
pub fn whatsapp_consent_is_current(settings: &WhatsAppSettings) -> bool {
    settings.consent == WhatsAppRecipientConsent::OptedIn
        && settings.consent_recipient.as_deref() == Some(settings.recipient.trim())
        && settings
            .consent_recorded_at_millis
            .is_some_and(|recorded_at| recorded_at > 0)
}

fn validate_whatsapp_recipient(recipient: &str) -> Result<(), PreferencesError> {
    let trimmed = recipient.trim();
    if !trimmed.starts_with('+') {
        return invalid("whatsapp.recipient must be an E.164 number beginning with +");
    }
    let digits = trimmed
        .chars()
        .skip(1)
        .filter(|value| value.is_ascii_digit())
        .count();
    let invalid_character = trimmed
        .chars()
        .skip(1)
        .any(|value| !value.is_ascii_digit() && !matches!(value, ' ' | '-' | '(' | ')'));
    if invalid_character || !(8..=15).contains(&digits) {
        return invalid("whatsapp.recipient is not a valid E.164 number");
    }
    Ok(())
}

fn valid_whatsapp_language_code(value: &str) -> bool {
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

fn validate_alert_area(area: &AlertArea) -> Result<(), PreferencesError> {
    bounded_text("area.id", &area.id, 1, 128)?;
    bounded_text("area.label", &area.label, 1, 100)?;
    if !area.latitude.is_finite()
        || !area.longitude.is_finite()
        || !(-90.0..=90.0).contains(&area.latitude)
        || !(-180.0..=180.0).contains(&area.longitude)
    {
        return invalid(format!("area {:?} contains invalid coordinates", area.id));
    }
    TimeZone::get(&area.time_zone).map_err(|error| {
        PreferencesError::Invalid(format!(
            "area {:?} timeZone {:?} is invalid: {error}",
            area.id, area.time_zone
        ))
    })?;
    if let Some(country_code) = &area.country_code
        && (country_code.len() != 2
            || !country_code
                .chars()
                .all(|value| value.is_ascii_alphabetic()))
    {
        return invalid(format!(
            "area {:?} countryCode must contain two ASCII letters",
            area.id
        ));
    }
    if let Some(admin_area) = &area.admin_area {
        bounded_text("area.adminArea", admin_area, 1, 120)?;
    }
    Ok(())
}

fn validate_channel(
    channel: &ChannelPreference,
    area_ids: &BTreeSet<&str>,
) -> Result<(), PreferencesError> {
    bounded_text("channel.id", &channel.id, 1, 128)?;
    if channel.kind == ChannelKindDto::System {
        return invalid(format!(
            "{} uses the reserved system channel kind",
            channel.id
        ));
    }
    bounded_text("channel.title", &channel.title, 1, 160)?;
    if !(1..=10_080).contains(&channel.max_age_minutes) {
        return invalid(format!(
            "{}.maxAgeMinutes must be between 1 and 10080",
            channel.id
        ));
    }
    if !(1..=100).contains(&channel.max_items) {
        return invalid(format!("{}.maxItems must be between 1 and 100", channel.id));
    }
    if !(5..=3_600).contains(&channel.rotation_seconds) {
        return invalid(format!(
            "{}.rotationSeconds must be between 5 and 3600",
            channel.id
        ));
    }
    if channel.scope.len() > 64 {
        return invalid(format!("{}.scope has more than 64 fields", channel.id));
    }
    let mut destinations = BTreeSet::new();
    for destination in &channel.destinations {
        if !destinations.insert(*destination) {
            return invalid(format!("{} contains a duplicate destination", channel.id));
        }
    }
    for (key, value) in &channel.scope {
        bounded_text("scope key", key, 1, 64)?;
        validate_scope_value(&channel.id, key, value)?;
    }
    if let Some(selected) = channel.scope.get("areaIds").and_then(Value::as_array) {
        let mut unique = BTreeSet::new();
        for area_id in selected.iter().filter_map(Value::as_str) {
            if !area_ids.contains(area_id) {
                return invalid(format!(
                    "{}.scope.areaIds refers to unknown area {area_id:?}",
                    channel.id
                ));
            }
            if !unique.insert(area_id) {
                return invalid(format!(
                    "{}.scope.areaIds contains duplicate {area_id:?}",
                    channel.id
                ));
            }
        }
    }
    if channel.kind == ChannelKindDto::Weather {
        validate_weather_scope(channel)?;
    } else if channel.kind == ChannelKindDto::Earthquake {
        validate_earthquake_scope(channel)?;
    } else if channel.kind == ChannelKindDto::Markets {
        validate_markets_scope(channel)?;
    }
    Ok(())
}

fn validate_earthquake_scope(channel: &ChannelPreference) -> Result<(), PreferencesError> {
    let feed = channel
        .scope
        .get("feed")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            PreferencesError::Invalid(format!("{}.scope.feed must be a string", channel.id))
        })?;
    if !matches!(feed, "significant_hour" | "significant_day") {
        return invalid(format!(
            "{}.scope.feed must be significant_hour or significant_day",
            channel.id
        ));
    }
    bounded_number(channel, "minimumMagnitude", 0.0, 10.0)?;
    bounded_number(channel, "eventAgeMinutes", 1.0, 10_080.0)
}

fn validate_weather_scope(channel: &ChannelPreference) -> Result<(), PreferencesError> {
    for key in ["rainAlertEnabled", "windAlertEnabled", "radarEnabled"] {
        if let Some(value) = channel.scope.get(key)
            && !value.is_boolean()
        {
            return invalid(format!("{}.scope.{key} must be boolean", channel.id));
        }
    }
    bounded_number(channel, "rainProbabilityThreshold", 1.0, 100.0)?;
    bounded_number(channel, "rainLeadMinutes", 0.0, 1_440.0)?;
    // No UI, but still validated: an override written by hand into preferences
    // must not be able to arm the rain rule on drizzle or silence it entirely.
    bounded_number(channel, "rainAmountMm", 0.01, 50.0)?;
    bounded_number(channel, "rainWindowMinutes", 5.0, 180.0)?;
    bounded_number(channel, "windGustMph", 10.0, 160.0)?;
    Ok(())
}

fn validate_markets_scope(channel: &ChannelPreference) -> Result<(), PreferencesError> {
    let symbols = channel
        .scope
        .get("symbols")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            PreferencesError::Invalid(format!("{}.scope.symbols must be an array", channel.id))
        })?;
    if symbols.len() > 16 {
        return invalid(format!(
            "{}.scope.symbols must contain at most 16 entries",
            channel.id
        ));
    }
    if channel.enabled && symbols.is_empty() {
        return invalid(format!(
            "{}.scope.symbols requires at least one symbol while the channel is enabled",
            channel.id
        ));
    }
    let mut unique = BTreeSet::new();
    for symbol in symbols.iter().filter_map(Value::as_str) {
        let normalized = symbol.trim().to_ascii_uppercase();
        if !(1..=32).contains(&normalized.chars().count())
            || !normalized.chars().all(|value| {
                value.is_ascii_alphanumeric() || matches!(value, '.' | '^' | '=' | '_' | '-' | '/')
            })
        {
            return invalid(format!(
                "{}.scope.symbols contains unsupported symbol {symbol:?}",
                channel.id
            ));
        }
        if !unique.insert(normalized) {
            return invalid(format!(
                "{}.scope.symbols contains duplicate {symbol:?}",
                channel.id
            ));
        }
    }
    if symbols.len() != unique.len() {
        return invalid(format!(
            "{}.scope.symbols must contain only strings",
            channel.id
        ));
    }
    bounded_number(channel, "movePercent", 0.1, 100.0)?;
    bounded_number(channel, "pollSeconds", 60.0, 3_600.0)?;
    Ok(())
}

fn bounded_number(
    channel: &ChannelPreference,
    key: &str,
    minimum: f64,
    maximum: f64,
) -> Result<(), PreferencesError> {
    let Some(value) = channel.scope.get(key) else {
        return Ok(());
    };
    if value
        .as_f64()
        .is_some_and(|value| (minimum..=maximum).contains(&value))
    {
        Ok(())
    } else {
        invalid(format!(
            "{}.scope.{key} must be between {minimum} and {maximum}",
            channel.id
        ))
    }
}

fn validate_scope_value(
    channel_id: &str,
    key: &str,
    value: &Value,
) -> Result<(), PreferencesError> {
    match value {
        Value::String(value) => bounded_text("scope string", value, 0, 2_048),
        Value::Number(_) | Value::Bool(_) => Ok(()),
        Value::Array(values)
            if values.len() <= 100
                && values.iter().all(|value| {
                    value
                        .as_str()
                        .is_some_and(|value| value.chars().count() <= 2_048)
                }) =>
        {
            Ok(())
        }
        _ => invalid(format!(
            "{channel_id}.scope.{key} must be a string, number, boolean, or string array"
        )),
    }
}

fn validate_quiet_hours(value: &QuietHours) -> Result<(), PreferencesError> {
    parse_hhmm("quietHours.start", &value.start)?;
    parse_hhmm("quietHours.end", &value.end)?;
    TimeZone::get(&value.time_zone).map_err(|error| {
        PreferencesError::Invalid(format!(
            "quietHours.timeZone {:?} is invalid: {error}",
            value.time_zone
        ))
    })?;
    Ok(())
}

fn parse_hhmm(field: &str, value: &str) -> Result<(u8, u8), PreferencesError> {
    let Some((hour, minute)) = value.split_once(':') else {
        return invalid(format!("{field} must use HH:MM"));
    };
    if hour.len() != 2 || minute.len() != 2 {
        return invalid(format!("{field} must use HH:MM"));
    }
    let hour: u8 = hour
        .parse()
        .map_err(|_| PreferencesError::Invalid(format!("{field} has an invalid hour")))?;
    let minute: u8 = minute
        .parse()
        .map_err(|_| PreferencesError::Invalid(format!("{field} has an invalid minute")))?;
    if hour > 23 || minute > 59 {
        return invalid(format!("{field} is outside a valid day"));
    }
    Ok((hour, minute))
}

fn bounded_text(
    field: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), PreferencesError> {
    let length = value.chars().count();
    if (minimum..=maximum).contains(&length) {
        Ok(())
    } else {
        invalid(format!(
            "{field} must contain between {minimum} and {maximum} characters"
        ))
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, PreferencesError> {
    Err(PreferencesError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_first_defaults_match_the_console_contract() {
        let value = serde_json::to_value(AppPreferences::default()).unwrap();
        assert_eq!(value["unitSystem"], "imperial");
        assert_eq!(value["profile"]["preset"], "bridge_first");
        assert_eq!(value["profile"]["homeChannelId"], "bridge.brickell");
        assert_eq!(value["display"]["transport"], "preview");
        assert_eq!(value["display"]["bleName"], "InkDock E213");
        assert_eq!(value["whatsapp"]["tokenConfigured"], false);
        assert_eq!(value["whatsapp"]["consent"], "not_recorded");
        assert_eq!(value["whatsapp"]["consentRecipient"], Value::Null);
        assert_eq!(value["whatsapp"]["consentRecordedAtMillis"], Value::Null);
        assert_eq!(value["ais"]["enabled"], false);
        assert_eq!(value["ais"]["provider"], "aisstream");
        assert_eq!(value["ais"]["apiKeyConfigured"], false);
        assert_eq!(value["ais"]["radiusKilometers"], 12.0);
        assert_eq!(value["areas"][0]["id"], "area.miami");
        assert_eq!(
            value["profile"]["channels"][1]["scope"]["areaIds"][0],
            "area.miami"
        );
        assert_eq!(value["profile"]["channels"].as_array().unwrap().len(), 7);
        validate_preferences(&AppPreferences::default()).unwrap();
    }

    #[test]
    fn rejects_values_outside_the_typescript_scope_contract() {
        let mut preferences = AppPreferences::default();
        preferences.profile.channels[0]
            .scope
            .insert("nested".into(), json!({"not": "allowed"}));
        assert!(validate_preferences(&preferences).is_err());
    }

    #[test]
    fn opted_in_whatsapp_requires_recipient_bound_timestamped_consent() {
        let mut preferences = AppPreferences::default();
        preferences.whatsapp.consent = WhatsAppRecipientConsent::OptedIn;
        assert!(validate_preferences(&preferences).is_err());

        preferences.whatsapp.recipient = " +13055550123 ".into();
        preferences.whatsapp.consent_recipient = Some("+13055559999".into());
        preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
        assert!(validate_preferences(&preferences).is_err());

        preferences.whatsapp.consent_recipient = Some("+13055550123".into());
        preferences.whatsapp.consent_recorded_at_millis = Some(0);
        assert!(validate_preferences(&preferences).is_err());

        preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
        validate_preferences(&preferences).unwrap();
        assert!(whatsapp_consent_is_current(&preferences.whatsapp));

        preferences.whatsapp.consent = WhatsAppRecipientConsent::Unsubscribed;
        preferences.whatsapp.recipient.clear();
        validate_preferences(&preferences).unwrap();
    }

    #[test]
    fn enabled_whatsapp_route_matches_the_delivery_adapters_static_shape() {
        let mut preferences = AppPreferences::default();
        preferences.whatsapp.enabled = true;
        preferences.whatsapp.phone_number_id = "1234567890".into();
        preferences.whatsapp.recipient = "+1 (305) 555-0123".into();
        validate_preferences(&preferences).unwrap();

        for invalid in ["abc", "123/456", ""] {
            preferences.whatsapp.phone_number_id = invalid.into();
            assert!(validate_preferences(&preferences).is_err());
        }
        preferences.whatsapp.phone_number_id = "1234567890".into();

        for invalid in ["3055550123", "+123", "+1305x5550123"] {
            preferences.whatsapp.recipient = invalid.into();
            assert!(validate_preferences(&preferences).is_err());
        }
        preferences.whatsapp.recipient = "+13055550123".into();

        preferences.whatsapp.graph_version = "v23/0".into();
        assert!(validate_preferences(&preferences).is_err());
        preferences.whatsapp.graph_version = "v23.0".into();
        for invalid in [
            "bridge/status",
            "Bridge_Status",
            "bridge status",
            "bad\nname",
        ] {
            preferences.whatsapp.template_name = invalid.into();
            assert!(validate_preferences(&preferences).is_err());
        }
        preferences.whatsapp.template_name = "bridge_status_update".into();
        for invalid in ["en/US", "EN_us", "not a locale", "e_US", "en_US_extra"] {
            preferences.whatsapp.language_code = invalid.into();
            assert!(validate_preferences(&preferences).is_err());
        }
        preferences.whatsapp.language_code = "fil_PH".into();
        validate_preferences(&preferences).unwrap();
    }

    #[test]
    fn markets_default_to_a_disabled_real_collector() {
        let preferences = AppPreferences::default();
        let market = &preferences.profile.channels[6];
        assert!(!market.enabled);
        assert_eq!(market.scope["symbols"], json!(["AMD"]));
        assert_eq!(market.scope["pollSeconds"], json!(300));
        validate_preferences(&preferences).unwrap();
    }

    #[test]
    fn yahoo_market_scope_is_bounded_and_requires_symbols() {
        let mut preferences = AppPreferences::default();
        let market = &mut preferences.profile.channels[6];
        market.enabled = true;
        market.scope.insert("symbols".into(), json!([]));
        assert!(validate_preferences(&preferences).is_err());

        preferences.profile.channels[6]
            .scope
            .insert("symbols".into(), json!(["AMD", "amd"]));
        assert!(validate_preferences(&preferences).is_err());

        preferences.profile.channels[6]
            .scope
            .insert("symbols".into(), json!(["DROP TABLE;"]));
        assert!(validate_preferences(&preferences).is_err());
    }
}
