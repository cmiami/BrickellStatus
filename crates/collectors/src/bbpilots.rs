//! Biscayne Bay Pilots dispatch schedule.
//!
//! FL511 reports that a bridge has already opened. This source is the other
//! half: the pilots' board lists scheduled ship movements hours ahead, and it
//! tags Miami River traffic with its own `RIVER` vessel type. Those are the
//! vessels that transit the river and force the bascule bridges up, so a
//! `RIVER` row is a forward-looking bridge event rather than a confirmation.
//!
//! The board is server-rendered HTML with no JSON API, so this collector parses
//! the schedule table. [`parse_bbp_schedule`] is deliberately a free function
//! over a string: it carries no network dependency and is exercised directly
//! against a captured fixture.
//!
//! The times on the board are pilot boarding times, not bridge times. Turning
//! one into a Brickell Avenue Bridge ETA needs a transit offset that has to be
//! learned from observed FL511 openings. This collector therefore reports the
//! movement, its direction of travel past the bridge, and an *uncalibrated*
//! estimate flagged as such; it does not pretend to know the offset yet.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use jiff::{civil, tz::TimeZone};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use url::Url;

#[cfg(feature = "native")]
use crate::SafeHttpFetcher;
use crate::{
    CollectContext, Collector, CollectorBatch, CollectorError, CollectorHealth, CollectorItem,
    HealthState, HttpFetcher, ItemKind, SourceLink,
};

const DEFAULT_SCHEDULE_URL: &str = "https://bbpilots.com/";
const DEFAULT_TIME_ZONE: &str = "America/New_York";

/// Transit allowances between the pilots' scheduled time and the Brickell
/// Avenue Bridge, in minutes after the board time.
///
/// Both started as guesses (60 and +20). Matching each board row to the same
/// hull's own AIS crossing of the bridge line (Aug 17 to Sep 1 2026) measured
/// them: arrivals cross a median 58 minutes after the board time (n = 7,
/// interquartile 54 to 65), so 60 stands. Departures cross a median 8 minutes
/// *before* it (n = 6, every one negative, interquartile 11 to 6 before), so
/// the old +20 had the wrong sign and missed by about half an hour: the board
/// time for a departure is not when the tow leaves the berth. Both samples are
/// below the pre-registered twenty-pair gate, so every emitted estimate still
/// carries `eta_calibrated: false`; the sign, however, is not in doubt.
const DEFAULT_INBOUND_TRANSIT_MINUTES: i64 = 60;
const DEFAULT_OUTBOUND_TRANSIT_MINUTES: i64 = -8;

/// A page far smaller than this means the board did not render at all, which is
/// worth distinguishing from a genuinely empty schedule.
const MIN_PLAUSIBLE_PAGE_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementAction {
    Arrival,
    Departure,
    Shift,
}

impl MovementAction {
    fn from_text(text: &str) -> Option<Self> {
        match text.trim().to_ascii_uppercase().as_str() {
            "ARRIVAL" => Some(Self::Arrival),
            "DEPARTURE" => Some(Self::Departure),
            "SHIFT" => Some(Self::Shift),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Arrival => "arrival",
            Self::Departure => "departure",
            Self::Shift => "shift",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementStatus {
    Confirmed,
    Scheduled,
    Unknown,
}

impl MovementStatus {
    fn from_text(text: &str) -> Self {
        match text.trim().to_ascii_uppercase().as_str() {
            "CONFIRMED" => Self::Confirmed,
            "SCHEDULED" => Self::Scheduled,
            _ => Self::Unknown,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Scheduled => "scheduled",
            Self::Unknown => "unknown",
        }
    }
}

/// Which way the vessel passes the Brickell Avenue Bridge. Brickell is the
/// first bascule upriver from the mouth, so an inbound vessel reaches it first
/// and an outbound vessel reaches it last.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiverDirection {
    /// Inbound from Government Cut, heading upriver.
    Upriver,
    /// Outbound from a river berth, heading to sea.
    Downriver,
}

impl RiverDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Upriver => "upriver",
            Self::Downriver => "downriver",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BbpMovement {
    pub vessel: String,
    /// Verbatim vessel-type cell: `RIVER`, `CRUISE`, `CARGO`, `TUG-BARGE`, ...
    pub vessel_type: String,
    pub action: MovementAction,
    pub status: MovementStatus,
    /// Local civil time as printed on the board.
    pub scheduled_local: civil::DateTime,
    pub scheduled_at: DateTime<Utc>,
    pub location: Option<String>,
    pub tug: Option<String>,
    /// "Lns" on the board: line handlers.
    pub lines_at: Option<String>,
    /// "Lbr" on the board: labor.
    pub labor_at: Option<String>,
}

impl BbpMovement {
    /// Miami River traffic, which is what moves the bascule bridges.
    pub fn is_river(&self) -> bool {
        self.vessel_type.eq_ignore_ascii_case("RIVER")
    }

    pub fn river_direction(&self) -> Option<RiverDirection> {
        if !self.is_river() {
            return None;
        }
        match self.action {
            MovementAction::Arrival => Some(RiverDirection::Upriver),
            MovementAction::Departure => Some(RiverDirection::Downriver),
            // A shift moves a vessel between berths; without knowing both ends
            // we cannot say which way it passes the bridge, or whether it does.
            MovementAction::Shift => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BbpSchedule {
    /// The board's own "As of" stamp, in local civil time.
    pub as_of_local: Option<civil::DateTime>,
    pub movements: Vec<BbpMovement>,
}

impl BbpSchedule {
    pub fn river_movements(&self) -> impl Iterator<Item = &BbpMovement> {
        self.movements.iter().filter(|movement| movement.is_river())
    }
}

#[derive(Debug, Error)]
pub enum BbpParseError {
    #[error("schedule page contained no service rows")]
    NoServiceRows,
    #[error("schedule page was {0} bytes, too small to contain a rendered board")]
    PageTooSmall(usize),
    #[error("unknown time zone {name:?}: {detail}")]
    TimeZone { name: String, detail: String },
    #[error("could not resolve {local} in {zone}: {detail}")]
    LocalTime {
        local: String,
        zone: String,
        detail: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BbPilotsConfig {
    pub schedule_url: Url,
    /// IANA zone the board's clock times are printed in.
    pub time_zone: String,
    /// Emit only Miami River movements. The board carries ~150 rows per refresh
    /// and all but a handful are deep-draft PortMiami traffic through
    /// Government Cut, which never touches a bascule bridge.
    pub river_only: bool,
    pub inbound_transit_minutes: i64,
    pub outbound_transit_minutes: i64,
}

impl Default for BbPilotsConfig {
    fn default() -> Self {
        Self {
            schedule_url: Url::parse(DEFAULT_SCHEDULE_URL).expect("constant URL is valid"),
            time_zone: DEFAULT_TIME_ZONE.into(),
            river_only: true,
            inbound_transit_minutes: DEFAULT_INBOUND_TRANSIT_MINUTES,
            outbound_transit_minutes: DEFAULT_OUTBOUND_TRANSIT_MINUTES,
        }
    }
}

pub struct BbPilotsCollector {
    config: BbPilotsConfig,
    fetcher: Arc<dyn HttpFetcher>,
}

impl BbPilotsCollector {
    /// Constructs the collector with the built-in network client.
    ///
    /// Native only: a Worker has no socket to give this, and supplies its own
    /// fetcher through [`Self::with_fetcher`] instead.
    #[cfg(feature = "native")]
    pub fn new(config: BbPilotsConfig) -> Result<Self, CollectorError> {
        Self::with_fetcher(config, Arc::new(SafeHttpFetcher::default()))
    }

    pub fn with_fetcher(
        config: BbPilotsConfig,
        fetcher: Arc<dyn HttpFetcher>,
    ) -> Result<Self, CollectorError> {
        TimeZone::get(&config.time_zone).map_err(|error| {
            CollectorError::Configuration(format!(
                "invalid Biscayne Bay Pilots time zone {:?}: {error}",
                config.time_zone
            ))
        })?;
        // A departure crosses the bridge before its board time, so the
        // allowance may be negative; two hours before is already implausible.
        if !(-120..=720).contains(&config.inbound_transit_minutes)
            || !(-120..=720).contains(&config.outbound_transit_minutes)
        {
            return Err(CollectorError::Configuration(
                "Biscayne Bay Pilots transit allowances must be between -120 and 720 minutes"
                    .into(),
            ));
        }
        Ok(Self { config, fetcher })
    }

    fn transit_minutes(&self, direction: RiverDirection) -> i64 {
        match direction {
            RiverDirection::Upriver => self.config.inbound_transit_minutes,
            RiverDirection::Downriver => self.config.outbound_transit_minutes,
        }
    }

    fn movement_item(&self, movement: &BbpMovement) -> CollectorItem {
        let direction = movement.river_direction();
        let mut attributes = BTreeMap::new();
        attributes.insert("vessel".into(), json!(movement.vessel));
        attributes.insert("vessel_type".into(), json!(movement.vessel_type));
        attributes.insert("action".into(), json!(movement.action.as_str()));
        attributes.insert("status".into(), json!(movement.status.as_str()));
        attributes.insert("river".into(), json!(movement.is_river()));
        attributes.insert(
            "scheduled_local".into(),
            json!(movement.scheduled_local.to_string()),
        );
        if let Some(location) = &movement.location {
            attributes.insert("berth".into(), json!(location));
        }
        if let Some(tug) = &movement.tug {
            attributes.insert("tug".into(), json!(tug));
        }
        if let Some(lines_at) = &movement.lines_at {
            attributes.insert("lines_at".into(), json!(lines_at));
        }
        if let Some(labor_at) = &movement.labor_at {
            attributes.insert("labor_at".into(), json!(labor_at));
        }

        if let Some(direction) = direction {
            let offset_minutes = self.transit_minutes(direction);
            let eta = movement.scheduled_at + chrono::Duration::minutes(offset_minutes);
            attributes.insert("river_direction".into(), json!(direction.as_str()));
            attributes.insert("bridge_eta_at".into(), json!(eta.to_rfc3339()));
            attributes.insert("bridge_eta_offset_minutes".into(), json!(offset_minutes));
            // The offset is an unvalidated placeholder. Anything consuming this
            // must present it as an estimate, and the learning pass that
            // replaces it should flip this flag.
            attributes.insert("eta_calibrated".into(), json!(false));
        }

        let action = movement.action.as_str();
        let summary = match direction {
            Some(RiverDirection::Upriver) => Some(format!(
                "Miami River arrival, heading upriver past the Brickell Avenue Bridge. {} at {}.",
                titlecase_status(movement.status),
                movement.scheduled_local
            )),
            Some(RiverDirection::Downriver) => Some(format!(
                "Miami River departure, heading downriver past the Brickell Avenue Bridge. {} at {}.",
                titlecase_status(movement.status),
                movement.scheduled_local
            )),
            None => Some(format!(
                "{} {} at {}.",
                movement.vessel_type, action, movement.scheduled_local
            )),
        };

        CollectorItem {
            // Vessel plus local slot is stable across refreshes while a movement
            // keeps its scheduled time, so an unchanged row keeps its identity
            // and a retimed one is correctly a different event.
            id: format!(
                "bbp:{}:{}:{}",
                movement.scheduled_local,
                action,
                slug(&movement.vessel)
            ),
            kind: ItemKind::VesselMovement,
            title: format!("{} — {}", movement.vessel, action),
            summary,
            observed_at: Some(movement.scheduled_at),
            starts_at: Some(movement.scheduled_at),
            ends_at: None,
            location: None,
            source: SourceLink {
                name: "Biscayne Bay Pilots".into(),
                url: Some(self.config.schedule_url.clone()),
            },
            attributes,
        }
    }
}

fn titlecase_status(status: MovementStatus) -> &'static str {
    match status {
        MovementStatus::Confirmed => "Confirmed",
        MovementStatus::Scheduled => "Scheduled",
        MovementStatus::Unknown => "Listed",
    }
}

fn slug(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    let mut pending_dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.extend(character.to_lowercase());
        } else {
            pending_dash = true;
        }
    }
    slug
}

/// Collapse a subtree's text into one whitespace-normalized line. The board uses
/// `&nbsp;` between labels and values, which arrives as U+00A0 and is not
/// matched by `char::is_whitespace` in a byte-oriented split, so normalize it
/// explicitly.
fn element_text(element: scraper::ElementRef<'_>) -> String {
    let raw: String = element.text().collect::<Vec<_>>().join(" ");
    raw.replace('\u{a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_class(element: scraper::ElementRef<'_>, name: &str) -> bool {
    element
        .value()
        .attr("class")
        .is_some_and(|classes| classes.split_whitespace().any(|class| class == name))
}

fn first_text(element: scraper::ElementRef<'_>, selector: &Selector) -> Option<String> {
    element
        .select(selector)
        .next()
        .map(element_text)
        .filter(|text| !text.is_empty())
}

/// Pull `Label: HH:MM` (or `Label: VALUE`) out of a normalized row line. The
/// board renders these as loose text in more than one cell depending on
/// viewport class, so scanning the row text is sturdier than pinning a path.
fn labeled_value(text: &str, label: &str) -> Option<String> {
    let needle = format!("{label}:");
    let start = text.find(&needle)? + needle.len();
    let value = text[start..].trim_start();
    let end = value.find(char::is_whitespace).unwrap_or(value.len());
    let value = &value[..end];
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_clock(value: &str) -> Option<(i8, i8)> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour: i8 = hour.trim().parse().ok()?;
    let minute: i8 = minute.trim().parse().ok()?;
    ((0..=23).contains(&hour) && (0..=59).contains(&minute)).then_some((hour, minute))
}

/// `Sat, Aug 15, 2026`, tolerating trailing text after the year.
///
/// The "As of" banner shares this format but sits in a container that also holds
/// a live refresh countdown, so the flattened text continues past the year with
/// something like `2026 2:00 · next update at --:--`. `NaiveDate::parse_from_str`
/// demands a full-string match, so trim to the year's digits before parsing
/// rather than depending on the banner's exact DOM shape.
fn parse_day_title(text: &str) -> Option<civil::Date> {
    let mut fields = text.trim().splitn(3, ',');
    let weekday = fields.next()?.trim();
    let month_day = fields.next()?.trim();
    let year: String = fields
        .next()?
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let date =
        NaiveDate::parse_from_str(&format!("{weekday}, {month_day}, {year}"), "%a, %b %d, %Y")
            .ok()?;
    use chrono::Datelike;
    civil::Date::new(
        i16::try_from(date.year()).ok()?,
        i8::try_from(date.month()).ok()?,
        i8::try_from(date.day()).ok()?,
    )
    .ok()
}

/// `As of 13:55, Sat, Aug 15, 2026`
fn parse_as_of(text: &str) -> Option<civil::DateTime> {
    let rest = text.trim().strip_prefix("As of")?.trim();
    let (clock, date) = rest.split_once(',')?;
    let (hour, minute) = parse_clock(clock)?;
    let date = parse_day_title(date.trim())?;
    date.at(hour, minute, 0, 0).into()
}

fn to_utc(
    local: civil::DateTime,
    zone: &TimeZone,
    zone_name: &str,
) -> Result<DateTime<Utc>, BbpParseError> {
    let zoned = local
        .to_zoned(zone.clone())
        .map_err(|error| BbpParseError::LocalTime {
            local: local.to_string(),
            zone: zone_name.to_string(),
            detail: error.to_string(),
        })?;
    DateTime::from_timestamp_millis(zoned.timestamp().as_millisecond()).ok_or_else(|| {
        BbpParseError::LocalTime {
            local: local.to_string(),
            zone: zone_name.to_string(),
            detail: "timestamp outside the representable range".into(),
        }
    })
}

/// Parse the pilots' schedule board.
///
/// Rows are read in document order because a `tr.day-title` establishes the date
/// for every `tr.service-row` that follows it; the rows themselves carry only a
/// clock time, so a movement past midnight belongs to the next day's heading.
pub fn parse_bbp_schedule(html: &str, time_zone: &str) -> Result<BbpSchedule, BbpParseError> {
    if html.len() < MIN_PLAUSIBLE_PAGE_BYTES {
        return Err(BbpParseError::PageTooSmall(html.len()));
    }
    let zone = TimeZone::get(time_zone).map_err(|error| BbpParseError::TimeZone {
        name: time_zone.to_string(),
        detail: error.to_string(),
    })?;

    let document = Html::parse_document(html);
    let row_selector =
        Selector::parse("tr.day-title, tr.service-row").expect("constant selector is valid");
    let as_of_selector = Selector::parse("div.bbp-asof").expect("constant selector is valid");
    let action_selector =
        Selector::parse(".bbp-service-badge-top > div:first-child").expect("valid");
    let time_selector = Selector::parse(".time-in-badge").expect("valid");
    let status_selector = Selector::parse(".bbp-service-badge-bottom").expect("valid");
    let vessel_selector = Selector::parse("td.vessel-col .font-weight-bold").expect("valid");
    let type_selector = Selector::parse(".vessel-type").expect("valid");
    let location_selector = Selector::parse("td.location-col").expect("valid");

    let as_of_local = document
        .select(&as_of_selector)
        .next()
        .map(element_text)
        .and_then(|text| parse_as_of(&text));

    let mut movements = Vec::new();
    let mut service_rows = 0usize;
    let mut current_date: Option<civil::Date> = None;

    for row in document.select(&row_selector) {
        if has_class(row, "day-title") {
            current_date = parse_day_title(&element_text(row));
            continue;
        }
        service_rows += 1;

        // A row before any day heading has no date we can trust. Skipping is
        // correct: guessing "today" would silently mis-date the movement.
        let Some(date) = current_date else {
            continue;
        };
        let Some(vessel) = first_text(row, &vessel_selector) else {
            continue;
        };
        let Some(action) = first_text(row, &action_selector)
            .as_deref()
            .and_then(MovementAction::from_text)
        else {
            continue;
        };
        let Some((hour, minute)) = first_text(row, &time_selector)
            .as_deref()
            .and_then(parse_clock)
        else {
            continue;
        };

        let scheduled_local = date.at(hour, minute, 0, 0);
        let scheduled_at = to_utc(scheduled_local, &zone, time_zone)?;
        let row_text = element_text(row);

        movements.push(BbpMovement {
            vessel,
            vessel_type: first_text(row, &type_selector).unwrap_or_default(),
            action,
            status: first_text(row, &status_selector)
                .as_deref()
                .map(MovementStatus::from_text)
                .unwrap_or(MovementStatus::Unknown),
            scheduled_local,
            scheduled_at,
            location: first_text(row, &location_selector)
                .map(|text| text.trim_start_matches("Loc:").trim().to_string())
                .filter(|text| !text.is_empty()),
            tug: labeled_value(&row_text, "Tug"),
            lines_at: labeled_value(&row_text, "Lns"),
            labor_at: labeled_value(&row_text, "Lbr"),
        });
    }

    if service_rows == 0 {
        return Err(BbpParseError::NoServiceRows);
    }

    Ok(BbpSchedule {
        as_of_local,
        movements,
    })
}

#[async_trait]
impl Collector for BbPilotsCollector {
    fn name(&self) -> &'static str {
        "bbpilots-schedule"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        let response = self
            .fetcher
            .get(
                &self.config.schedule_url,
                context.cursor.as_ref(),
                &[("accept", "text/html")],
            )
            .await?;
        if response.not_modified {
            return Ok(CollectorBatch {
                source: self.name().into(),
                items: Vec::new(),
                health: CollectorHealth::healthy(),
                cursor: response.cursor,
                not_modified: true,
            });
        }

        let html = String::from_utf8_lossy(&response.body);
        let schedule = parse_bbp_schedule(&html, &self.config.time_zone).map_err(|error| {
            match error {
                // A board that renders but lists nothing we recognize means the
                // markup moved, which is a schema change rather than a bad day.
                BbpParseError::NoServiceRows | BbpParseError::PageTooSmall(_) => {
                    CollectorError::SchemaChanged {
                        collector: "bbpilots",
                        detail: error.to_string(),
                    }
                }
                other => CollectorError::Parse {
                    collector: "bbpilots",
                    detail: other.to_string(),
                },
            }
        })?;

        let total = schedule.movements.len();
        let river = schedule.river_movements().count();
        let items = schedule
            .movements
            .iter()
            .filter(|movement| !self.config.river_only || movement.is_river())
            .map(|movement| self.movement_item(movement))
            .collect::<Vec<_>>();

        let mut cursor = response.cursor;
        cursor
            .metadata
            .insert("movement_count".into(), total.to_string());
        cursor
            .metadata
            .insert("river_movement_count".into(), river.to_string());
        if let Some(as_of) = schedule.as_of_local {
            cursor
                .metadata
                .insert("as_of_local".into(), as_of.to_string());
        }

        // Parsing rows but recognizing no river traffic is normal overnight; a
        // board that parsed no rows at all never reaches here. Report degraded
        // only when the board itself looks stale.
        let health = CollectorHealth {
            state: if total == 0 {
                HealthState::Degraded
            } else {
                HealthState::Healthy
            },
            checked_at: Utc::now(),
            message: (total == 0).then(|| "the pilots' board listed no movements".to_string()),
        };

        Ok(CollectorBatch {
            source: self.name().into(),
            items,
            health,
            cursor,
            not_modified: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../fixtures/bbpilots-schedule.html");

    fn schedule() -> BbpSchedule {
        parse_bbp_schedule(FIXTURE, DEFAULT_TIME_ZONE).unwrap()
    }

    #[test]
    fn reads_the_board_as_of_stamp() {
        assert_eq!(
            schedule().as_of_local.map(|stamp| stamp.to_string()),
            Some("2026-08-15T13:55:00".into())
        );
    }

    #[test]
    fn parses_every_service_row() {
        let schedule = schedule();
        assert_eq!(schedule.movements.len(), 5);
        assert_eq!(schedule.river_movements().count(), 3);
    }

    #[test]
    fn tags_river_traffic_and_leaves_cruise_and_cargo_alone() {
        let schedule = schedule();
        let river: Vec<_> = schedule
            .river_movements()
            .map(|movement| movement.vessel.as_str())
            .collect();
        assert_eq!(river, ["PEPIN EXPRESS", "BABUN EXPRESS", "VIOLET"]);
        assert!(
            schedule
                .movements
                .iter()
                .any(|movement| movement.vessel_type == "CRUISE" && !movement.is_river())
        );
    }

    #[test]
    fn carries_the_day_heading_across_a_midnight_rollover() {
        // BABUN EXPRESS departs 00:30, which the board prints under the *next*
        // day's heading. Reading rows in document order is what makes this right.
        let schedule = schedule();
        let babun = schedule
            .movements
            .iter()
            .find(|movement| movement.vessel == "BABUN EXPRESS")
            .expect("fixture contains BABUN EXPRESS");
        assert_eq!(babun.scheduled_local.to_string(), "2026-08-16T00:30:00");
        let pepin = schedule
            .movements
            .iter()
            .find(|movement| movement.vessel == "PEPIN EXPRESS")
            .expect("fixture contains PEPIN EXPRESS");
        assert_eq!(pepin.scheduled_local.to_string(), "2026-08-15T20:00:00");
    }

    #[test]
    fn resolves_local_board_time_to_utc_across_the_eastern_offset() {
        // August is EDT, UTC-4: 20:00 local is 00:00Z the following day.
        let schedule = schedule();
        let pepin = schedule
            .movements
            .iter()
            .find(|movement| movement.vessel == "PEPIN EXPRESS")
            .unwrap();
        assert_eq!(pepin.scheduled_at.to_rfc3339(), "2026-08-16T00:00:00+00:00");
    }

    #[test]
    fn reads_action_status_tug_and_line_times() {
        let schedule = schedule();
        let pepin = schedule
            .movements
            .iter()
            .find(|movement| movement.vessel == "PEPIN EXPRESS")
            .unwrap();
        assert_eq!(pepin.action, MovementAction::Arrival);
        assert_eq!(pepin.status, MovementStatus::Scheduled);
        assert_eq!(pepin.tug.as_deref(), Some("MRT"));
        assert_eq!(pepin.lines_at.as_deref(), Some("20:30"));

        let horizon = schedule
            .movements
            .iter()
            .find(|movement| movement.vessel == "CARNIVAL HORIZON")
            .unwrap();
        assert_eq!(horizon.action, MovementAction::Departure);
        assert_eq!(horizon.status, MovementStatus::Confirmed);
    }

    #[test]
    fn maps_arrivals_upriver_and_departures_downriver() {
        let schedule = schedule();
        let direction = |vessel: &str| {
            schedule
                .movements
                .iter()
                .find(|movement| movement.vessel == vessel)
                .unwrap()
                .river_direction()
        };
        assert_eq!(direction("PEPIN EXPRESS"), Some(RiverDirection::Upriver));
        assert_eq!(direction("BABUN EXPRESS"), Some(RiverDirection::Downriver));
        // Deep-draft traffic never enters the river, so it has no direction.
        assert_eq!(direction("CARNIVAL HORIZON"), None);
    }

    #[test]
    fn river_only_config_drops_deep_draft_traffic() {
        let collector = BbPilotsCollector::with_fetcher(
            BbPilotsConfig::default(),
            Arc::new(SafeHttpFetcher::default()),
        )
        .unwrap();
        let schedule = schedule();
        let items: Vec<_> = schedule
            .movements
            .iter()
            .filter(|movement| movement.is_river())
            .map(|movement| collector.movement_item(movement))
            .collect();
        assert_eq!(items.len(), 3);
        assert!(
            items
                .iter()
                .all(|item| item.kind == ItemKind::VesselMovement)
        );
        assert!(
            items
                .iter()
                .all(|item| item.attributes["river"] == serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn eta_is_offset_by_direction_and_flagged_uncalibrated() {
        let collector = BbPilotsCollector::new(BbPilotsConfig::default()).unwrap();
        let schedule = schedule();
        let item = |vessel: &str| {
            collector.movement_item(
                schedule
                    .movements
                    .iter()
                    .find(|movement| movement.vessel == vessel)
                    .unwrap(),
            )
        };

        let inbound = item("PEPIN EXPRESS");
        assert_eq!(inbound.attributes["river_direction"], json!("upriver"));
        assert_eq!(
            inbound.attributes["bridge_eta_offset_minutes"],
            json!(DEFAULT_INBOUND_TRANSIT_MINUTES)
        );
        // 20:00 local + 60m inbound allowance.
        assert_eq!(
            inbound.attributes["bridge_eta_at"],
            json!("2026-08-16T01:00:00+00:00")
        );
        assert_eq!(inbound.attributes["eta_calibrated"], json!(false));

        let outbound = item("BABUN EXPRESS");
        assert_eq!(outbound.attributes["river_direction"], json!("downriver"));
        assert_eq!(
            outbound.attributes["bridge_eta_offset_minutes"],
            json!(DEFAULT_OUTBOUND_TRANSIT_MINUTES)
        );
        // A departure reaches the bridge before its board time, not after.
        let outbound_eta = outbound.attributes["bridge_eta_at"]
            .as_str()
            .and_then(|value| value.parse::<chrono::DateTime<chrono::Utc>>().ok())
            .expect("departure eta");
        assert!(
            outbound_eta < outbound.starts_at.expect("board time"),
            "departure eta must precede the board time"
        );

        // Deep-draft traffic gets no ETA at all rather than a misleading one.
        let cruise = item("CARNIVAL HORIZON");
        assert!(!cruise.attributes.contains_key("bridge_eta_at"));
        assert!(!cruise.attributes.contains_key("river_direction"));
    }

    #[test]
    fn item_ids_are_stable_and_distinguish_retimed_movements() {
        let collector = BbPilotsCollector::new(BbPilotsConfig::default()).unwrap();
        let schedule = schedule();
        let pepin = schedule
            .movements
            .iter()
            .find(|movement| movement.vessel == "PEPIN EXPRESS")
            .unwrap();
        let first = collector.movement_item(pepin);
        assert_eq!(first.id, "bbp:2026-08-15T20:00:00:arrival:pepin-express");
        assert_eq!(first.id, collector.movement_item(pepin).id);

        let mut retimed = pepin.clone();
        retimed.scheduled_local = retimed.scheduled_local.with().hour(21).build().unwrap();
        assert_ne!(first.id, collector.movement_item(&retimed).id);
    }

    #[test]
    fn rejects_a_page_that_did_not_render_the_board() {
        let error = parse_bbp_schedule(
            &format!("<html><body>{}</body></html>", "x".repeat(4_096)),
            DEFAULT_TIME_ZONE,
        )
        .unwrap_err();
        assert!(matches!(error, BbpParseError::NoServiceRows));

        let error = parse_bbp_schedule("<html></html>", DEFAULT_TIME_ZONE).unwrap_err();
        assert!(matches!(error, BbpParseError::PageTooSmall(_)));
    }

    #[test]
    fn rejects_an_unknown_time_zone() {
        assert!(matches!(
            parse_bbp_schedule(FIXTURE, "Mars/Olympus_Mons").unwrap_err(),
            BbpParseError::TimeZone { .. }
        ));
        assert!(
            BbPilotsCollector::new(BbPilotsConfig {
                time_zone: "Mars/Olympus_Mons".into(),
                ..BbPilotsConfig::default()
            })
            .is_err()
        );
    }

    #[test]
    fn rejects_an_implausible_transit_allowance() {
        assert!(
            BbPilotsCollector::new(BbPilotsConfig {
                inbound_transit_minutes: 5_000,
                ..BbPilotsConfig::default()
            })
            .is_err()
        );
    }
}
