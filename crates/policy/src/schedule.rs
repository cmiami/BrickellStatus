//! Brickell Avenue Bridge legal operating schedule.
//!
//! On non-holiday weekdays, ordinary openings from 07:00 through 19:00 local
//! time are limited to the hour and half-hour, with three traffic blackout
//! periods. Outside that weekday period the bridge opens on signal. Public
//! vessels, tugs with tows, and emergencies are exceptions, so this module
//! describes *ordinary* eligibility and never claims an opening is impossible.

use brickellstatus_model::{BridgeOperatingMode, TimestampMillis};
use jiff::{
    Timestamp,
    civil::{Date, Weekday},
    tz::TimeZone,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// IANA time zone governing the bridge schedule.
pub const BRICKELL_TIME_ZONE: &str = "America/New_York";

/// Hours over which observed openings track the hour/half-hour pattern even on
/// days the regulation leaves on signal. Outside these, observed openings
/// scattered across the hour at chance level.
const DAYTIME_START_HOUR: u8 = 7;
const DAYTIME_END_HOUR: u8 = 22;

const RESTRICTED_START: u16 = 7 * 60;
const RESTRICTED_END: u16 = 19 * 60;
const BLACKOUTS: [(u16, u16); 3] = [
    (7 * 60 + 35, 9 * 60),
    (12 * 60 + 5, 13 * 60),
    (16 * 60 + 35, 18 * 60),
];

/// A named United States federal holiday recognized by the schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederalHoliday {
    /// January 1, including its observed weekday.
    NewYearsDay,
    /// Third Monday in January.
    MartinLutherKingJrDay,
    /// Third Monday in February.
    WashingtonsBirthday,
    /// Last Monday in May.
    MemorialDay,
    /// June 19, including its observed weekday.
    Juneteenth,
    /// July 4, including its observed weekday.
    IndependenceDay,
    /// First Monday in September.
    LaborDay,
    /// Second Monday in October.
    ColumbusDay,
    /// November 11, including its observed weekday.
    VeteransDay,
    /// Fourth Thursday in November.
    ThanksgivingDay,
    /// December 25, including its observed weekday.
    ChristmasDay,
}

/// Civil clock fields included for an inspectable schedule decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrickellLocalTime {
    /// Local Gregorian year.
    pub year: i16,
    /// Local month in `1..=12`.
    pub month: u8,
    /// Local day of month.
    pub day: u8,
    /// Local hour in `0..=23`.
    pub hour: u8,
    /// Local minute in `0..=59`.
    pub minute: u8,
    /// UTC offset in seconds, proving which side of a DST transition was used.
    pub offset_seconds: i32,
}

/// The schedule engine's decision at one instant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleStatus {
    /// Absolute instant that was evaluated.
    pub evaluated_at: TimestampMillis,
    /// Corresponding civil time in [`BRICKELL_TIME_ZONE`].
    pub local_time: BrickellLocalTime,
    /// Governing ordinary operating mode.
    pub mode: BridgeOperatingMode,
    /// Whether an ordinary opening may commence in the current local minute.
    /// In scheduled mode this is true only during an `:00` or `:30` slot.
    pub ordinary_opening_allowed: bool,
    /// Next ordinary opening opportunity. This is `None` when the bridge is
    /// already continuously on signal.
    pub next_ordinary_opening_at: Option<TimestampMillis>,
    /// The next `:00` or `:30` on the clock, whether or not the schedule
    /// restricts openings then.
    ///
    /// Deliberately not a legal claim — it says nothing about what is allowed.
    /// It exists because observed openings cluster on those minutes even on
    /// days the regulation does not restrict, and a field that only appears in
    /// scheduled mode cannot express that.
    #[serde(default = "epoch_millis")]
    pub next_clock_slot_at: TimestampMillis,
    /// Federal holiday responsible for `on_signal` mode, when applicable.
    pub holiday: Option<FederalHoliday>,
    /// Always true: public vessels, tugs with tows, and emergencies may be
    /// handled outside the ordinary schedule.
    pub exceptions_may_open: bool,
}

/// Serde fallback for [`ScheduleStatus::next_clock_slot_at`] when reading a
/// status written before the field existed.
fn epoch_millis() -> TimestampMillis {
    TimestampMillis(0)
}

/// Schedule construction or conversion failure.
#[derive(Debug, Error)]
pub enum ScheduleError {
    /// The bundled/system IANA database could not resolve New York.
    #[error("could not load {BRICKELL_TIME_ZONE}: {0}")]
    TimeZone(#[source] jiff::Error),
    /// Epoch milliseconds fell outside Jiff's supported range.
    #[error("timestamp {value}ms is outside the supported range: {source}")]
    Timestamp {
        /// Rejected Unix epoch millisecond value.
        value: i64,
        /// Jiff conversion error.
        #[source]
        source: jiff::Error,
    },
    /// An internal civil-date operation failed.
    #[error("could not construct schedule date: {0}")]
    Civil(#[source] jiff::Error),
}

/// Evaluates the Brickell schedule using the bundled/system IANA time-zone
/// database rather than a fixed UTC offset.
#[derive(Clone, Debug)]
pub struct BrickellSchedule {
    time_zone: TimeZone,
}

impl BrickellSchedule {
    /// Loads `America/New_York` from Jiff's time-zone database.
    pub fn new() -> Result<Self, ScheduleError> {
        Ok(Self {
            time_zone: TimeZone::get(BRICKELL_TIME_ZONE).map_err(ScheduleError::TimeZone)?,
        })
    }

    /// Evaluates ordinary opening eligibility at an absolute instant.
    pub fn evaluate(&self, instant: TimestampMillis) -> Result<ScheduleStatus, ScheduleError> {
        let timestamp =
            Timestamp::from_millisecond(instant.0).map_err(|source| ScheduleError::Timestamp {
                value: instant.0,
                source,
            })?;
        let local = timestamp.to_zoned(self.time_zone.clone());
        let date = local.date();
        let minute_of_day = local.hour() as u16 * 60 + local.minute() as u16;
        let holiday = federal_holiday(date)?;
        let mode = operating_mode(date.weekday(), holiday, minute_of_day);
        let at_slot = minute_of_day.is_multiple_of(30);
        let ordinary_opening_allowed = match mode {
            BridgeOperatingMode::OnSignal => true,
            BridgeOperatingMode::Scheduled => at_slot,
            BridgeOperatingMode::Blackout => false,
        };
        let next_ordinary_opening_at = if mode == BridgeOperatingMode::OnSignal {
            None
        } else {
            Some(self.next_ordinary_opening(&local, mode, at_slot)?)
        };
        // Pure arithmetic on the clock: minutes to the next half-hour boundary.
        let minutes_to_clock_slot = if at_slot && local.second() == 0 {
            0
        } else {
            30 - (minute_of_day % 30)
        };
        let next_clock_slot_at = TimestampMillis(
            instant
                .0
                .saturating_add(i64::from(minutes_to_clock_slot) * 60_000)
                .saturating_sub(i64::from(local.second()) * 1_000),
        );

        Ok(ScheduleStatus {
            evaluated_at: instant,
            local_time: BrickellLocalTime {
                year: local.year(),
                month: local.month() as u8,
                day: local.day() as u8,
                hour: local.hour() as u8,
                minute: local.minute() as u8,
                offset_seconds: local.offset().seconds(),
            },
            mode,
            ordinary_opening_allowed,
            next_ordinary_opening_at,
            next_clock_slot_at,
            holiday,
            exceptions_may_open: true,
        })
    }

    /// First ordinary opening opportunity at or after `instant`.
    ///
    /// `None` when the bridge is on signal then, which is the same as saying
    /// the schedule imposes no wait. A prediction has to ask this rather than
    /// assume its arrival window is the answer: an ordinary vessel that reaches
    /// the bridge at 18:32 during the hour/half-hour period does not open it at
    /// 18:32, it waits for 19:00.
    pub fn ordinary_opening_at_or_after(
        &self,
        instant: TimestampMillis,
    ) -> Result<Option<TimestampMillis>, ScheduleError> {
        let status = self.evaluate(instant)?;
        if status.mode == BridgeOperatingMode::OnSignal {
            return Ok(None);
        }
        if status.ordinary_opening_allowed {
            return Ok(Some(instant));
        }
        Ok(status.next_ordinary_opening_at)
    }

    /// The next `:00`/`:30` at or after `instant`, but only during the daytime
    /// hours where observed openings actually track those minutes.
    ///
    /// This is the on-signal counterpart to
    /// [`Self::ordinary_opening_at_or_after`]. It makes no claim about what is
    /// permitted — on signal everything is — only about when traffic has been
    /// seen to move.
    pub fn daytime_clock_slot_at_or_after(
        &self,
        instant: TimestampMillis,
    ) -> Result<Option<TimestampMillis>, ScheduleError> {
        let status = self.evaluate(instant)?;
        if !(DAYTIME_START_HOUR..DAYTIME_END_HOUR).contains(&status.local_time.hour) {
            return Ok(None);
        }
        Ok(Some(status.next_clock_slot_at))
    }

    fn next_ordinary_opening(
        &self,
        local: &jiff::Zoned,
        current_mode: BridgeOperatingMode,
        at_slot: bool,
    ) -> Result<TimestampMillis, ScheduleError> {
        let mut date = local.date();
        let current_minute = local.hour() as u16 * 60 + local.minute() as u16;
        let at_start_of_minute = local.second() == 0 && local.subsec_nanosecond() == 0;
        let mut minute =
            if current_mode == BridgeOperatingMode::Scheduled && at_slot && at_start_of_minute {
                current_minute
            } else {
                ((current_minute / 30) + 1) * 30
            };

        // The longest weekday blackout is under two hours; the wider bound
        // also makes future schedule edits fail safely instead of looping.
        for _ in 0..=96 {
            if minute >= 24 * 60 {
                date = date.tomorrow().map_err(ScheduleError::Civil)?;
                minute = 0;
            }

            let holiday = federal_holiday(date)?;
            let mode = operating_mode(date.weekday(), holiday, minute);
            if mode == BridgeOperatingMode::OnSignal
                || (mode == BridgeOperatingMode::Scheduled && minute.is_multiple_of(30))
            {
                let candidate = date
                    .at((minute / 60) as i8, (minute % 60) as i8, 0, 0)
                    .to_zoned(self.time_zone.clone())
                    .map_err(ScheduleError::Civil)?;
                return Ok(TimestampMillis::new(candidate.timestamp().as_millisecond()));
            }
            minute += 30;
        }

        // This cannot occur under the finite rules above, but returning a
        // typed error is preferable to a panic if those rules change.
        Err(ScheduleError::Civil(jiff::Error::from_args(format_args!(
            "no ordinary opening opportunity found within 48 hours"
        ))))
    }
}

fn operating_mode(
    weekday: Weekday,
    holiday: Option<FederalHoliday>,
    minute_of_day: u16,
) -> BridgeOperatingMode {
    if matches!(weekday, Weekday::Saturday | Weekday::Sunday) || holiday.is_some() {
        return BridgeOperatingMode::OnSignal;
    }
    if !(RESTRICTED_START..RESTRICTED_END).contains(&minute_of_day) {
        return BridgeOperatingMode::OnSignal;
    }
    if BLACKOUTS
        .iter()
        .any(|&(start, end)| (start..end).contains(&minute_of_day))
    {
        BridgeOperatingMode::Blackout
    } else {
        BridgeOperatingMode::Scheduled
    }
}

/// Returns the federal holiday applying on `date`, including observed fixed
/// holidays. A New Year's observance on December 31 is correctly associated
/// with the following year's January 1 holiday.
fn federal_holiday(date: Date) -> Result<Option<FederalHoliday>, ScheduleError> {
    let year = date.year();

    for candidate_year in [year, year.saturating_add(1)] {
        for (month, day, holiday) in [
            (1, 1, FederalHoliday::NewYearsDay),
            (6, 19, FederalHoliday::Juneteenth),
            (7, 4, FederalHoliday::IndependenceDay),
            (11, 11, FederalHoliday::VeteransDay),
            (12, 25, FederalHoliday::ChristmasDay),
        ] {
            let actual = Date::new(candidate_year, month, day).map_err(ScheduleError::Civil)?;
            if date == actual || date == observed_date(actual)? {
                return Ok(Some(holiday));
            }
        }
    }

    let floating = [
        (
            Date::new(year, 1, 1).and_then(|date| date.nth_weekday_of_month(3, Weekday::Monday)),
            FederalHoliday::MartinLutherKingJrDay,
        ),
        (
            Date::new(year, 2, 1).and_then(|date| date.nth_weekday_of_month(3, Weekday::Monday)),
            FederalHoliday::WashingtonsBirthday,
        ),
        (
            Date::new(year, 5, 1).and_then(|date| date.nth_weekday_of_month(-1, Weekday::Monday)),
            FederalHoliday::MemorialDay,
        ),
        (
            Date::new(year, 9, 1).and_then(|date| date.nth_weekday_of_month(1, Weekday::Monday)),
            FederalHoliday::LaborDay,
        ),
        (
            Date::new(year, 10, 1).and_then(|date| date.nth_weekday_of_month(2, Weekday::Monday)),
            FederalHoliday::ColumbusDay,
        ),
        (
            Date::new(year, 11, 1).and_then(|date| date.nth_weekday_of_month(4, Weekday::Thursday)),
            FederalHoliday::ThanksgivingDay,
        ),
    ];

    for (holiday_date, holiday) in floating {
        if holiday_date.map_err(ScheduleError::Civil)? == date {
            return Ok(Some(holiday));
        }
    }
    Ok(None)
}

fn observed_date(actual: Date) -> Result<Date, ScheduleError> {
    match actual.weekday() {
        Weekday::Saturday => actual.yesterday().map_err(ScheduleError::Civil),
        Weekday::Sunday => actual.tomorrow().map_err(ScheduleError::Civil),
        _ => Ok(actual),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn millis(text: &str) -> TimestampMillis {
        let timestamp: Timestamp = text.parse().expect("valid test timestamp");
        TimestampMillis::new(timestamp.as_millisecond())
    }

    fn schedule() -> BrickellSchedule {
        BrickellSchedule::new().expect("New York time zone")
    }

    #[test]
    fn weekday_boundaries_and_blackouts_are_exact() {
        let schedule = schedule();
        let cases = [
            ("2026-08-14T10:59:00Z", BridgeOperatingMode::OnSignal, true), // 06:59
            ("2026-08-14T11:00:00Z", BridgeOperatingMode::Scheduled, true), // 07:00
            (
                "2026-08-14T11:34:59Z",
                BridgeOperatingMode::Scheduled,
                false,
            ),
            ("2026-08-14T11:35:00Z", BridgeOperatingMode::Blackout, false),
            ("2026-08-14T12:59:59Z", BridgeOperatingMode::Blackout, false),
            ("2026-08-14T13:00:00Z", BridgeOperatingMode::Scheduled, true), // 09:00
            ("2026-08-14T16:05:00Z", BridgeOperatingMode::Blackout, false), // 12:05
            ("2026-08-14T17:00:00Z", BridgeOperatingMode::Scheduled, true), // 13:00
            ("2026-08-14T20:35:00Z", BridgeOperatingMode::Blackout, false), // 16:35
            ("2026-08-14T22:00:00Z", BridgeOperatingMode::Scheduled, true), // 18:00
            ("2026-08-14T23:00:00Z", BridgeOperatingMode::OnSignal, true),  // 19:00
        ];

        for (at, expected_mode, expected_allowed) in cases {
            let status = schedule.evaluate(millis(at)).expect("schedule status");
            assert_eq!(status.mode, expected_mode, "mode at {at}");
            assert_eq!(
                status.ordinary_opening_allowed, expected_allowed,
                "eligibility at {at}"
            );
        }
    }

    #[test]
    fn blackout_points_to_first_valid_slot() {
        let status = schedule()
            .evaluate(millis("2026-08-14T11:36:00Z")) // 07:36 EDT
            .expect("schedule status");
        assert_eq!(
            status.next_ordinary_opening_at,
            Some(millis("2026-08-14T13:00:00Z")) // 09:00 EDT
        );
    }

    #[test]
    fn weekends_and_federal_holidays_are_on_signal() {
        let schedule = schedule();

        let saturday = schedule
            .evaluate(millis("2026-08-15T16:00:00Z"))
            .expect("Saturday status");
        assert_eq!(saturday.mode, BridgeOperatingMode::OnSignal);
        assert_eq!(saturday.holiday, None);

        let thanksgiving = schedule
            .evaluate(millis("2026-11-26T17:00:00Z"))
            .expect("Thanksgiving status");
        assert_eq!(thanksgiving.mode, BridgeOperatingMode::OnSignal);
        assert_eq!(thanksgiving.holiday, Some(FederalHoliday::ThanksgivingDay));

        // July 4 is Saturday in 2026, so Friday July 3 is the observed holiday.
        let observed_independence = schedule
            .evaluate(millis("2026-07-03T16:00:00Z"))
            .expect("observed holiday status");
        assert_eq!(
            observed_independence.holiday,
            Some(FederalHoliday::IndependenceDay)
        );
        assert_eq!(observed_independence.mode, BridgeOperatingMode::OnSignal);
    }

    #[test]
    fn every_current_federal_holiday_is_recognized() {
        let schedule = schedule();
        let cases = [
            ("2026-01-01T17:00:00Z", FederalHoliday::NewYearsDay),
            (
                "2026-01-19T17:00:00Z",
                FederalHoliday::MartinLutherKingJrDay,
            ),
            ("2026-02-16T17:00:00Z", FederalHoliday::WashingtonsBirthday),
            ("2026-05-25T16:00:00Z", FederalHoliday::MemorialDay),
            ("2026-06-19T16:00:00Z", FederalHoliday::Juneteenth),
            ("2026-07-03T16:00:00Z", FederalHoliday::IndependenceDay),
            ("2026-09-07T16:00:00Z", FederalHoliday::LaborDay),
            ("2026-10-12T16:00:00Z", FederalHoliday::ColumbusDay),
            ("2026-11-11T17:00:00Z", FederalHoliday::VeteransDay),
            ("2026-11-26T17:00:00Z", FederalHoliday::ThanksgivingDay),
            ("2026-12-25T17:00:00Z", FederalHoliday::ChristmasDay),
        ];

        for (at, expected) in cases {
            let status = schedule.evaluate(millis(at)).expect("holiday status");
            assert_eq!(status.holiday, Some(expected), "holiday at {at}");
            assert_eq!(status.mode, BridgeOperatingMode::OnSignal, "mode at {at}");
        }
    }

    #[test]
    fn new_year_observed_in_previous_calendar_year_is_recognized() {
        // New Year's Day 2022 was Saturday; federal observance was Fri Dec 31.
        let status = schedule()
            .evaluate(millis("2021-12-31T17:00:00Z"))
            .expect("observed New Year status");
        assert_eq!(status.holiday, Some(FederalHoliday::NewYearsDay));
        assert_eq!(status.mode, BridgeOperatingMode::OnSignal);
    }

    #[test]
    fn daylight_saving_offsets_come_from_iana_rules() {
        let schedule = schedule();
        let before_jump = schedule
            .evaluate(millis("2026-03-08T06:59:00Z"))
            .expect("pre-DST status");
        let after_jump = schedule
            .evaluate(millis("2026-03-08T07:01:00Z"))
            .expect("post-DST status");

        assert_eq!(before_jump.local_time.hour, 1);
        assert_eq!(before_jump.local_time.offset_seconds, -5 * 60 * 60);
        assert_eq!(after_jump.local_time.hour, 3);
        assert_eq!(after_jump.local_time.offset_seconds, -4 * 60 * 60);
        assert_eq!(before_jump.mode, BridgeOperatingMode::OnSignal);
        assert_eq!(after_jump.mode, BridgeOperatingMode::OnSignal);
    }

    #[test]
    fn winter_and_summer_use_the_same_local_schedule() {
        let schedule = schedule();
        let winter = schedule
            .evaluate(millis("2026-01-05T12:00:00Z")) // 07:00 EST
            .expect("winter status");
        let summer = schedule
            .evaluate(millis("2026-08-10T11:00:00Z")) // 07:00 EDT
            .expect("summer status");
        assert_eq!(winter.mode, BridgeOperatingMode::Scheduled);
        assert_eq!(summer.mode, BridgeOperatingMode::Scheduled);
        assert!(winter.ordinary_opening_allowed);
        assert!(summer.ordinary_opening_allowed);
    }
}
