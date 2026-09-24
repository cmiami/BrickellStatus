"""Shadow opening model: the features and scoring shared by training and the app.

Standard library only, so the unit tests run without the research venv.

Every input is a row the app already stores, as it existed at the forecast
minute: recent AIS fixes, crossing outcomes resolved at least 20 minutes
earlier, bridge lifts, pilots'-board revisions still on the board, and the
legal schedule. Nothing a later reading revealed is used. The Rust scorer in
`crates/runtime/src/shadow.rs` must compute identical values; the shared
fixture `crates/runtime/fixtures/opening_shadow_parity.json` pins both.

The model predicts whether Brickell lifts within 30 minutes of a bridge-down
minute. It runs in shadow: its probability is recorded next to the live
forecast and never drives an alert. See docs/MODEL_AUDIT.md.
"""
from __future__ import annotations

import math
from collections import defaultdict
from datetime import date, datetime, timedelta, timezone
from zoneinfo import ZoneInfo

MINUTE = 60_000
LOCAL = ZoneInfo("America/New_York")
KNOT_METERS_PER_SECOND = 0.5144

# Latest fix per hull comes from this window before the forecast minute.
FIX_WINDOW_MS = 7 * MINUTE
# The fix a hull's motion is measured against: the newest one from 45 s to
# 10 min before its latest fix.
PREVIOUS_FIX_MIN_GAP_MS = 45_000
PREVIOUS_FIX_LOOKBACK_MS = 10 * MINUTE
# Crossing outcomes count only once resolved this long before the minute, so
# the passage being predicted can never label itself.
HISTORY_BUFFER_MS = 20 * MINUTE
UPSTREAM_LOOKBACK_MS = 30 * MINUTE
# The board is polled every 10 minutes; a row stays visible one poll past its
# last sighting.
BOARD_GRACE_MS = 10 * MINUTE
# Channel metres a hull must have closed since its previous fix.
CLOSING_METERS = 20.0
MIN_UNDERWAY_KNOTS = 0.8
COMMITTED_APPROACH_METERS = 1_600.0
FAR_APPROACH_MINUTES = 40.0
WAITING_METERS = 500.0
KNOWN_OPENER_PROPENSITY = 0.6

UPSTREAM_RANK = {
    "sw_2_ave": 1,
    "sw_1_st": 2,
    "w_flagler": 3,
    "nw_5_st": 4,
    "nw_12_ave": 5,
    "nw_17_ave": 6,
    "nw_22_ave": 7,
    "nw_27_ave": 8,
}

FEATURES = [
    "ais_closing", "ais_known", "ais_sail", "ais_eta30", "ais_out", "ais_in",
    "ais_wait", "ais_far_known", "ais_far_unknown",
    "up_any15", "up_two20", "up_sw2ave12", "up_sw1st12", "up_ordered20",
    "bbp_down", "bbp_up",
    "sched_weekday", "blackout", "slot_soon", "daytime", "recent_open30",
]

RESTRICTED_START, RESTRICTED_END = 7 * 60, 19 * 60
BLACKOUTS = ((7 * 60 + 35, 9 * 60), (12 * 60 + 5, 13 * 60), (16 * 60 + 35, 18 * 60))


def _nth_weekday(year: int, month: int, weekday: int, n: int) -> date:
    if n > 0:
        first = date(year, month, 1)
        return first + timedelta(days=(weekday - first.weekday()) % 7 + 7 * (n - 1))
    last = (date(year + (month == 12), month % 12 + 1, 1) - timedelta(days=1))
    return last - timedelta(days=(last.weekday() - weekday) % 7)


def _observed(actual: date) -> date:
    if actual.weekday() == 5:
        return actual - timedelta(days=1)
    if actual.weekday() == 6:
        return actual + timedelta(days=1)
    return actual


def federal_holiday(day: date) -> bool:
    """Mirror of `federal_holiday` in crates/policy/src/schedule.rs."""
    for year in (day.year, day.year + 1):
        for month, dom in ((1, 1), (6, 19), (7, 4), (11, 11), (12, 25)):
            actual = date(year, month, dom)
            if day in (actual, _observed(actual)):
                return True
    y = day.year
    floating = (
        _nth_weekday(y, 1, 0, 3), _nth_weekday(y, 2, 0, 3), _nth_weekday(y, 5, 0, -1),
        _nth_weekday(y, 9, 0, 1), _nth_weekday(y, 10, 0, 2), _nth_weekday(y, 11, 3, 4),
    )
    return day in floating


def schedule_context(now_ms: int) -> dict:
    """Legal mode, local hour, and seconds to the next :00 or :30."""
    stamp = datetime.fromtimestamp(now_ms / 1000, timezone.utc).astimezone(LOCAL)
    minute_of_day = stamp.hour * 60 + stamp.minute
    if stamp.weekday() >= 5 or federal_holiday(stamp.date()):
        mode = "on_signal"
    elif not RESTRICTED_START <= minute_of_day < RESTRICTED_END:
        mode = "on_signal"
    elif any(start <= minute_of_day < end for start, end in BLACKOUTS):
        mode = "blackout"
    else:
        mode = "scheduled"
    offset_seconds = int(stamp.utcoffset().total_seconds())
    into_half_hour = (now_ms // 1000 + offset_seconds) % 1800
    return {
        "mode": mode,
        "local_hour": stamp.hour,
        "seconds_to_slot": 0 if into_half_hour == 0 else 1800 - into_half_hour,
    }


def features(inputs: dict, board_windows: dict) -> dict:
    """The model's inputs at `inputs["now_ms"]`. Keys match FEATURES."""
    now = inputs["now_ms"]
    out = dict.fromkeys(FEATURES, 0.0)

    by_hull = defaultdict(list)
    for fix in inputs["fixes"]:
        if fix["at"] < now:
            by_hull[fix["mmsi"]].append(fix)
    best_eta = None
    for mmsi in sorted(by_hull):
        fixes = sorted(by_hull[mmsi], key=lambda fix: fix["at"])
        latest = fixes[-1]
        if latest["at"] < now - FIX_WINDOW_MS:
            continue
        s = latest["s"]
        if latest["posture"] == "waiting" and s is not None and abs(s) <= WAITING_METERS:
            out["ais_wait"] = 1.0
        sog = latest["sog"]
        if latest["posture"] != "underway" or sog is None or sog <= MIN_UNDERWAY_KNOTS or s is None:
            continue
        previous = None
        for fix in fixes:
            if (latest["at"] - PREVIOUS_FIX_LOOKBACK_MS <= fix["at"] <= latest["at"] - PREVIOUS_FIX_MIN_GAP_MS
                    and fix["s"] is not None):
                previous = fix
        if previous is None or not abs(s) < abs(previous["s"]) - CLOSING_METERS:
            continue
        eta = abs(s) / (sog * KNOT_METERS_PER_SECOND) / 60
        opened, fits_under = inputs["history"].get(mmsi, (0, 0))
        known = opened + fits_under > 0 and (opened + 1) / (opened + fits_under + 2) >= KNOWN_OPENER_PROPENSITY
        if not (latest["branch"] == "river" or abs(s) <= COMMITTED_APPROACH_METERS):
            if eta <= FAR_APPROACH_MINUTES:
                out["ais_far_known" if known else "ais_far_unknown"] = 1.0
            continue
        out["ais_closing"] = 1.0
        if s > 0:
            out["ais_out"] = 1.0
        elif s < 0:
            out["ais_in"] = 1.0
        if known:
            out["ais_known"] = 1.0
        if inputs["classes"].get(mmsi) == "sailing":
            out["ais_sail"] = 1.0
        best_eta = eta if best_eta is None else min(best_eta, eta)
    if best_eta is not None and best_eta <= 30:
        out["ais_eta30"] = 1.0

    upstream = [lift for lift in inputs["up_starts"]
                if lift["relation"] == "upstream" and lift["key"] in UPSTREAM_RANK
                and now - UPSTREAM_LOOKBACK_MS <= lift["at"] <= now]

    def lifted(minutes: int) -> set:
        return {lift["key"] for lift in upstream if lift["at"] >= now - minutes * MINUTE}

    out["up_any15"] = float(len(lifted(15)) >= 1)
    out["up_two20"] = float(len(lifted(20)) >= 2)
    out["up_sw2ave12"] = float("sw_2_ave" in lifted(12))
    out["up_sw1st12"] = float("sw_1_st" in lifted(12))
    run = sorted((lift["at"], UPSTREAM_RANK[lift["key"]]) for lift in upstream
                 if lift["at"] >= now - 20 * MINUTE)
    out["up_ordered20"] = float(len(run) >= 2 and all(b < a for (_, a), (_, b) in zip(run, run[1:])))
    out["recent_open30"] = float(any(
        lift["relation"] == "target" and now - 30 * MINUTE <= lift["at"] < now
        for lift in inputs["up_starts"]))

    for booking in inputs["board"]:
        if not booking["first_seen"] <= now <= booking["last_seen"] + BOARD_GRACE_MS:
            continue
        direction = booking["direction"]
        if direction not in board_windows:
            continue
        low, high = board_windows[direction]
        if booking["scheduled_at"] + low * MINUTE <= now <= booking["scheduled_at"] + high * MINUTE:
            out["bbp_down" if direction == "downriver" else "bbp_up"] = 1.0

    schedule = inputs["schedule"]
    out["sched_weekday"] = float(schedule["mode"] == "scheduled")
    out["blackout"] = float(schedule["mode"] == "blackout")
    out["daytime"] = float(7 <= schedule["local_hour"] < 22)
    out["slot_soon"] = float(out["daytime"] == 1.0 and schedule["seconds_to_slot"] <= 12 * 60)
    return out


def probability(values: dict, model: dict) -> float:
    z = model["intercept"] + sum(c * values[name] for name, c in zip(model["features"], model["coefficients"]))
    return 1 / (1 + math.exp(-max(-30.0, min(30.0, z))))
