"""Audit Brickell forecasts against continuously observed bridge outcomes.

    python3 scripts/calibrate_bridge.py [path/to/brickellstatus.sqlite3]

This is deliberately read-only. It reports whether there is enough trustworthy
history to change a model; it never edits a weight or the database.

Pre-registered gates:

* Ordered upstream movement weights may be halved only after the sub-1.2 lift
  result survives 100 clean target openings and includes weekdays. They may be
  raised only above 2.0 lift with at least 50 weekday openings.
* A pilots-board transit offset may replace its placeholder only after 20
  paired movements in that direction, and only when its IQR is at most 40 min.
* Forecast quality is scored per alert episode, not per correlated minute:
  precision, recall, false alerts, warning lead, and ETA-interval coverage.
  Starting with brickell-v4, a predictive row is replayed as an alert only
  when its entire ETA interval reaches the product's 30-minute alert horizon;
  longer-range rows remain calibration data. Older traces retain their shipped
  near-edge behavior so historical model scores do not change retroactively.
* Every forecast table carries a chance row: one alert each time the bridge
  comes back down, matched the same way. With about twenty openings a day and
  a thirty-minute horizon that trivial policy scores near 50% precision, so
  precision in the forties or fifties is not skill on its own. Judge a model
  by its lead, its ETA coverage, and its reliability line instead.
* Pilots-board offsets are measured against the same hull's own AIS crossing
  of the bridge line, not against whichever opening came next. The sign of an
  offset is settled when every paired movement agrees on it; the value is
  published only after twenty pairs with an IQR of at most 40 minutes.

Rows created before successful-reading continuity was recorded are identified
as legacy data. They are never presented as continuous coverage.
"""

from __future__ import annotations

import math
import sqlite3
import statistics
import sys
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

DEFAULT_DB = (
    Path.home()
    / "Library/Application Support/com.cmiami.brickellstatus/brickellstatus.sqlite3"
)
LOCAL = ZoneInfo("America/New_York")
MINUTE = 60_000
FEATURE_HORIZON = 15
ALERT_HORIZON = 30
CONTINUITY_GAP = 2 * MINUTE
TARGET = "brickell"
RIVER = [
    "sw_2_ave",
    "sw_1_st",
    "w_flagler",
    "nw_5_st",
    "nw_12_ave",
    "nw_17_ave",
    "nw_22_ave",
    "nw_27_ave",
]


@dataclass(frozen=True)
class Interval:
    source: str
    key: str
    relation: str
    state: str
    started: int
    ended: int | None
    confirmed: int
    reason: str
    session: str | None


@dataclass(frozen=True)
class Forecast:
    at: int
    minute: int
    model: str
    state: str
    score: int
    confidence: int
    eta_min: int | None
    eta_max: int | None
    mode: str


def connect(path: Path) -> sqlite3.Connection:
    return sqlite3.connect(f"file:{path}?mode=ro", uri=True)


def table_exists(connection: sqlite3.Connection, table: str) -> bool:
    return connection.execute(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
        (table,),
    ).fetchone()[0] == 1


def columns(connection: sqlite3.Connection, table: str) -> set[str]:
    return {row[1] for row in connection.execute(f"PRAGMA table_info({table})")}


def read_intervals(path: Path) -> tuple[list[Interval], bool]:
    if not path.exists():
        sys.exit(f"no database at {path}\nrun the app once, or pass a path")
    connection = connect(path)
    try:
        names = columns(connection, "bridge_state_intervals")
        continuity = {"last_confirmed_at_ms", "start_reason"}.issubset(names)
        confirmed = (
            "last_confirmed_at_ms"
            if "last_confirmed_at_ms" in names
            else "COALESCE(ended_at_ms, started_at_ms)"
        )
        reason = "start_reason" if "start_reason" in names else "'legacy'"
        session = "session_id" if "session_id" in names else "NULL"
        rows = connection.execute(
            "SELECT source_id, bridge_key, relation, state, started_at_ms, "
            f"ended_at_ms, {confirmed}, {reason}, {session} "
            "FROM bridge_state_intervals ORDER BY started_at_ms"
        ).fetchall()
    finally:
        connection.close()
    intervals = [Interval(*row) for row in rows]
    if not intervals:
        sys.exit("no recorded bridge intervals yet")
    return intervals, continuity


def read_transits(path: Path):
    connection = connect(path)
    try:
        if not table_exists(connection, "river_transits"):
            return None
        rows = connection.execute(
            "SELECT vessel, action, river_direction, scheduled_at_ms, "
            "estimated_offset_minutes, first_seen_at_ms, last_seen_at_ms "
            "FROM river_transits ORDER BY first_seen_at_ms, scheduled_at_ms"
        ).fetchall()
    finally:
        connection.close()
    # A retime currently receives a new provider item ID. Collapse a row that
    # replaced the same vessel/action/direction within one observation window;
    # keep the latest revision rather than training twice on one booking.
    collapsed = []
    latest_by_identity: dict[tuple[str, str, str], int] = {}
    for row in rows:
        vessel, action, direction, scheduled, _placeholder, first_seen, last_seen = row
        identity = (vessel.strip().casefold(), action, direction or "unknown")
        prior_index = latest_by_identity.get(identity)
        if prior_index is not None:
            prior = collapsed[prior_index]
            same_revision_window = (
                first_seen <= prior[6] + 30 * MINUTE
                and abs(scheduled - prior[3]) <= 6 * 60 * MINUTE
            )
            if same_revision_window:
                if last_seen >= prior[6]:
                    collapsed[prior_index] = row
                continue
        latest_by_identity[identity] = len(collapsed)
        collapsed.append(row)
    return sorted(collapsed, key=lambda row: row[3])


def read_forecasts(path: Path) -> list[Forecast] | None:
    connection = connect(path)
    try:
        if not table_exists(connection, "bridge_forecast_samples"):
            return None
        rows = connection.execute(
            "SELECT evaluated_at_ms, minute_bucket_ms, model_version, state, "
            "predictive_score_bps, confidence_bps, eta_min_minutes, "
            "eta_max_minutes, schedule_mode FROM bridge_forecast_samples "
            "WHERE target_key=? ORDER BY evaluated_at_ms",
            (TARGET,),
        ).fetchall()
    finally:
        connection.close()
    # A material change may add another row inside a minute. Calibration uses
    # the last thing the app believed in that minute, not several correlated
    # votes from the same minute.
    by_minute: dict[tuple[str, int], Forecast] = {}
    for row in rows:
        sample = Forecast(*row)
        by_minute[(sample.model, sample.minute)] = sample
    return sorted(by_minute.values(), key=lambda sample: sample.at)


def model_generation(model: str) -> int:
    """Numeric suffix of a `brickell-vN` model version; 0 for anything else."""
    prefix = "brickell-v"
    if not model.startswith(prefix):
        return 0
    digits = "".join(ch for ch in model[len(prefix):] if ch.isdigit())
    return int(digits) if digits else 0


def local(ms: int) -> datetime:
    return datetime.fromtimestamp(ms / 1000, LOCAL)


def clock_offset(ms: int) -> float:
    """Signed minutes to the nearest :00 or :30. Negative is before it."""
    stamp = local(ms)
    minute = stamp.minute + stamp.second / 60
    return min((minute, minute - 30, minute - 60), key=abs)


def binomial_tail(hits: int, trials: int, chance: float) -> float:
    return sum(
        math.comb(trials, index)
        * chance**index
        * (1 - chance) ** (trials - index)
        for index in range(hits, trials + 1)
    )


def merge_spans(spans: list[tuple[int, int]], tolerance: int = 0):
    merged: list[list[int]] = []
    for start, end in sorted(spans):
        if end <= start:
            continue
        if not merged or start > merged[-1][1] + tolerance:
            merged.append([start, end])
        else:
            merged[-1][1] = max(merged[-1][1], end)
    return [(start, end) for start, end in merged]


def observed_spans(intervals: list[Interval]) -> list[tuple[int, int]]:
    return merge_spans(
        [
            (row.started, row.confirmed)
            for row in intervals
            if row.key == TARGET
            and row.relation == "target"
            and row.reason != "legacy"
            and row.state != "unknown"
        ],
        CONTINUITY_GAP,
    )


def clean_lifts(intervals: list[Interval], key: str) -> list[int]:
    """Down-to-up changes witnessed inside one continuous source session."""
    grouped: dict[tuple[str, str], list[Interval]] = defaultdict(list)
    for row in intervals:
        if row.key == key:
            grouped[(row.source, row.key)].append(row)
    lifts = []
    for rows in grouped.values():
        rows.sort(key=lambda row: row.started)
        for previous, current in zip(rows, rows[1:]):
            if (
                previous.state == "down"
                and current.state == "up"
                and current.reason == "state_change"
                and current.session is not None
                and current.session == previous.session
                and 0 <= current.started - previous.confirmed <= CONTINUITY_GAP
                and current.confirmed >= current.started + 30_000
            ):
                lifts.append(current.started)
    return sorted(set(lifts))


def completeness(intervals: list[Interval], continuity_supported: bool):
    trustworthy = [row for row in intervals if row.reason != "legacy"]
    print("COMPLETENESS")
    if not continuity_supported:
        print("  LEGACY DATABASE: successful-reading continuity was not stored.")
        print("  Recorded interval ends are not treated as proof the app was watching.")
        return []
    if not trustworthy:
        legacy = sum(row.reason == "legacy" for row in intervals)
        print(f"  {legacy} legacy intervals retained for display; 0 trainable intervals")
        print("  Collection with the new build has not established coverage yet.")
        return []
    spans = observed_spans(intervals)
    if not spans:
        print("  no continuously confirmed target coverage yet")
        return []
    lo, hi = spans[0][0], spans[-1][1]
    covered = sum(end - start for start, end in spans)
    holes = [
        (left[1], right[0])
        for left, right in zip(spans, spans[1:])
        if right[0] > left[1]
    ]
    print(
        f"  window          {local(lo):%a %d %b %H:%M} -> "
        f"{local(hi):%a %d %b %H:%M} local"
    )
    print(f"  elapsed         {(hi - lo) / 3.6e6:.1f} h")
    print(
        f"  confirmed       {covered / 3.6e6:.1f} h "
        f"({100 * covered / max(hi - lo, 1):.0f}%)"
    )
    print(
        f"  trainable rows  {len(trustworthy)} "
        f"({sum(row.reason == 'legacy' for row in intervals)} legacy excluded)"
    )
    if holes:
        print(f"  gaps            {len(holes)}; no outcomes are inferred across them")
        for start, end in holes[:5]:
            print(
                f"                  {local(start):%d %b %H:%M} "
                f"for {(end - start) / MINUTE:.0f} min"
            )
    else:
        print("  gaps            none")
    return spans


def day_types(openings: list[int]):
    kinds: dict[str, list[int]] = defaultdict(list)
    for opening in openings:
        kinds["weekend" if local(opening).weekday() >= 5 else "weekday"].append(opening)
    return kinds


def clock_section(openings: list[int]) -> None:
    print("\nCLOCK")
    daytime = [opening for opening in openings if 7 <= local(opening).hour < 22]
    if len(daytime) < 8:
        print(f"  only {len(daytime)} clean daytime openings — too few to judge")
        return
    offsets = sorted(clock_offset(opening) for opening in daytime)
    print(f"  {len(daytime)} clean daytime openings")
    print(f"  {'window':<14}{'captured':>10}{'chance':>9}{'lift':>7}{'p':>10}")
    for low, high, name in (
        (-5, 5, "+/-5 min"),
        (-10, 5, "-10..+5"),
        (-12, 5, "-12..+5"),
    ):
        hits = sum(low <= offset <= high for offset in offsets)
        chance = (high - low) * 2 / 60
        lift = (hits / len(offsets)) / chance if chance else 0
        p_value = binomial_tail(hits, len(offsets), chance)
        print(
            f"  {name:<14}{hits:>4}/{len(offsets):<5}{chance:>8.0%}"
            f"{lift:>7.2f}{p_value:>10.4f}"
        )


def covered_grid(spans: list[tuple[int, int]]) -> list[int]:
    grid = []
    horizon = FEATURE_HORIZON * MINUTE
    for start, end in spans:
        first = start - start % MINUTE + MINUTE
        grid.extend(range(first, end - horizon + 1, MINUTE))
    return sorted(set(grid))


def upstream_section(ups, openings, spans) -> None:
    print("\nUPSTREAM")
    grid = covered_grid(spans)
    if len(grid) < 120:
        print(f"  only {len(grid)} fully observed forecast minutes — too few to judge")
        return

    def opens(moment: int) -> bool:
        return any(
            moment < event <= moment + FEATURE_HORIZON * MINUTE
            for event in openings
        )

    base = sum(opens(moment) for moment in grid) / len(grid)
    print(
        f"  base rate       {base:.0%} of observed minutes precede an opening "
        f"within {FEATURE_HORIZON} min"
    )
    rank = {key: index for index, key in enumerate(RIVER)}

    def lifted(moment, window, keys):
        return [
            key
            for key in keys
            if any(
                moment - window * MINUTE <= event <= moment
                for event in ups.get(key, [])
            )
        ]

    def ordered(moment, window):
        events = sorted(
            (event, key)
            for key in RIVER
            for event in ups.get(key, [])
            if moment - window * MINUTE <= event <= moment
        )
        if len(events) < 2:
            return False
        ranks = [rank[key] for _, key in events]
        return all(later < earlier for earlier, later in zip(ranks, ranks[1:]))

    print(f"  {'feature':<30}{'fires':>7}{'precision':>11}{'lift':>7}")
    results = {}
    for name, rule in (
        ("ordered downstream run, 20m", lambda moment: ordered(moment, 20)),
        ("ordered downstream run, 30m", lambda moment: ordered(moment, 30)),
        (
            "any two upstream, 20m",
            lambda moment: len(lifted(moment, 20, RIVER)) >= 2,
        ),
        (
            "any one upstream, 15m",
            lambda moment: len(lifted(moment, 15, RIVER)) >= 1,
        ),
    ):
        fires = [moment for moment in grid if rule(moment)]
        precision = sum(opens(moment) for moment in fires) / len(fires) if fires else 0
        lift = precision / base if base else 0
        results[name] = lift
        print(f"  {name:<30}{len(fires):>7}{precision:>10.0%}{lift:>7.2f}")

    best = max(
        results["ordered downstream run, 20m"],
        results["ordered downstream run, 30m"],
    )
    weekday = sum(local(opening).weekday() < 5 for opening in openings)
    print("\n  VERDICT (pre-registered)")
    if len(openings) < 100 or weekday == 0:
        print(
            f"    gate not met: {len(openings)}/100 clean openings, "
            f"{weekday} weekday"
        )
        print(
            f"    ordered lift is {best:.2f}; keep collecting and do not change weights"
        )
    elif best < 1.2:
        print(
            f"    ordered lift {best:.2f} < 1.20 with the evidence gate met: "
            "halve both weights"
        )
    elif best > 2.0 and weekday >= 50:
        print(
            f"    ordered lift {best:.2f} > 2.00 with {weekday} weekday "
            "openings: raise weights"
        )
    else:
        print(f"    ordered lift {best:.2f} is inconclusive: keep current weights")


def read_ais_crossings(path: Path):
    """(mmsi, crossed_at_ms, direction, outcome) plus a name -> MMSIs map."""
    connection = connect(path)
    try:
        if not table_exists(connection, "ais_transits"):
            return [], {}
        crossings = connection.execute(
            "SELECT mmsi, crossed_at_ms, direction, outcome FROM ais_transits "
            "ORDER BY crossed_at_ms"
        ).fetchall()
        names = connection.execute(
            "SELECT mmsi, name FROM ais_vessel_ledger WHERE name IS NOT NULL"
        ).fetchall()
    finally:
        connection.close()
    by_name: dict[str, list[str]] = defaultdict(list)
    for mmsi, name in names:
        by_name[name.strip().casefold()].append(mmsi)
    return crossings, by_name


def transit_offsets_from_ais(transits, crossings, by_name) -> None:
    """Board time -> the same hull's own crossing of the bridge line.

    Pairing a booking with "the next opening within three hours" mostly pairs
    it with somebody else's opening when the bridge lifts twenty times a day.
    The hull's own AIS crossing is the measurement; this is what moved the
    downriver placeholder from +20 to -8 minutes in v5.
    """
    print("\n  AIS-PAIRED OFFSETS (board time -> the hull's own crossing)")
    if not crossings:
        print("    no AIS crossings recorded")
        return
    by_direction: dict[str, list[tuple[float, str]]] = defaultdict(list)
    for vessel, _action, direction, scheduled, *_ in transits:
        for mmsi in by_name.get(vessel.strip().casefold(), []):
            hit = next(
                (
                    row
                    for row in crossings
                    if row[0] == mmsi
                    and scheduled - 60 * MINUTE <= row[1] <= scheduled + 300 * MINUTE
                ),
                None,
            )
            if hit is not None:
                by_direction[direction or "unknown"].append(
                    ((hit[1] - scheduled) / MINUTE, hit[3] or "pending")
                )
                break
    if not by_direction:
        print("    no board row matched a ledger hull with a crossing")
        return
    print(f"    {'direction':<12}{'paired':>8}{'median':>9}{'IQR':>18}{'sign':>12}")
    for direction, pairs in sorted(by_direction.items()):
        offsets = sorted(pair[0] for pair in pairs)
        median = statistics.median(offsets)
        quartiles = statistics.quantiles(offsets, n=4) if len(offsets) >= 4 else None
        spread = (
            f"{quartiles[0]:+.0f} to {quartiles[2]:+.0f} min" if quartiles else "--"
        )
        if all(offset < 0 for offset in offsets):
            sign = "all before"
        elif all(offset > 0 for offset in offsets):
            sign = "all after"
        else:
            sign = "mixed"
        print(f"    {direction:<12}{len(offsets):>8}{median:>+6.0f} min{spread:>18}{sign:>12}")
        if len(offsets) < 20:
            print(f"      value gate not met: need {20 - len(offsets)} more {direction} pairs")
        elif quartiles and quartiles[2] - quartiles[0] <= 40:
            print(f"      supported: publish {median:+.0f} min and mark calibrated")
        else:
            print("      sample is large enough but too dispersed to publish as a countdown")


def transit_section(transits, openings, ais_pairing=None) -> None:
    print("\nRIVER TRANSITS")
    if transits is None:
        print("  no river_transits table")
        return
    if not transits:
        print("  no pilots-board movements recorded yet")
        return
    if ais_pairing is not None:
        transit_offsets_from_ais(transits, *ais_pairing)
    print("\n  NEXT-OPENING PAIRING (legacy; pairs with whichever opening came next)")
    by_direction = defaultdict(list)
    used_openings: set[int] = set()
    for _vessel, _action, direction, scheduled, placeholder, _first, _last in transits:
        following = [
            event
            for event in openings
            if event not in used_openings
            and scheduled < event <= scheduled + 180 * MINUTE
        ]
        if following:
            matched = min(following)
            used_openings.add(matched)
            by_direction[direction or "unknown"].append(
                ((matched - scheduled) / MINUTE, placeholder)
            )
    if not by_direction:
        print(f"  {len(transits)} movements, none paired to a clean opening")
        return
    print(
        f"  {'direction':<12}{'paired':>8}{'median':>9}"
        f"{'IQR':>16}{'placeholder':>13}"
    )
    for direction, pairs in sorted(by_direction.items()):
        offsets = sorted(pair[0] for pair in pairs)
        placeholder = next(
            (pair[1] for pair in pairs if pair[1] is not None), None
        )
        median = statistics.median(offsets)
        quartiles = statistics.quantiles(offsets, n=4) if len(offsets) >= 4 else None
        spread = (
            f"{quartiles[0]:.0f} to {quartiles[2]:.0f} min"
            if quartiles
            else "--"
        )
        shown = f"{placeholder} min" if placeholder is not None else "--"
        print(
            f"  {direction:<12}{len(offsets):>8}{median:>6.0f} min"
            f"{spread:>16}{shown:>13}"
        )
        if len(offsets) < 20:
            print(f"    gate not met: need {20 - len(offsets)} more {direction} pairs")
        elif quartiles and quartiles[2] - quartiles[0] <= 40:
            print(
                f"    supported: use {median:.0f} min and mark this direction calibrated"
            )
        else:
            print(
                "    sample is large enough but too dispersed to publish as a countdown"
            )


def alert_episodes(
    samples: list[Forecast],
    openings: list[int],
    up_spans: list[tuple[int, int]],
    apply_alert_horizon: bool = True,
) -> list[Forecast]:
    episodes = []
    previous: Forecast | None = None
    active = False
    for sample in samples:
        gap = previous is None or sample.minute - previous.minute > 2 * MINUTE
        changed_model = previous is not None and sample.model != previous.model
        opening_since_previous = previous is not None and any(
            previous.at < opening <= sample.at for opening in openings
        )
        bridge_is_up = any(start <= sample.at <= end for start, end in up_spans)
        if gap or changed_model or opening_since_previous or bridge_is_up:
            active = False
        if bridge_is_up:
            previous = sample
            continue
        eta_reached_horizon = (
            sample.eta_min is not None
            and sample.eta_max is not None
            and (
                sample.eta_max <= ALERT_HORIZON
                if model_generation(sample.model) >= 4
                else sample.eta_min <= ALERT_HORIZON
            )
        )
        alertable = (
            sample.state == "likely"
            and (not apply_alert_horizon or eta_reached_horizon)
        )
        if alertable and not active:
            episodes.append(sample)
            active = True
        elif not alertable:
            active = False
        previous = sample
    return episodes


def percentile(values: list[float], fraction: float) -> float:
    if len(values) == 1:
        return values[0]
    position = (len(values) - 1) * fraction
    low = math.floor(position)
    high = math.ceil(position)
    return values[low] + (values[high] - values[low]) * (position - low)


def score_forecasts(
    samples: list[Forecast],
    openings: list[int],
    up_spans: list[tuple[int, int]],
    label: str,
    apply_alert_horizon: bool = True,
) -> None:
    episodes = alert_episodes(
        samples, openings, up_spans, apply_alert_horizon
    )
    sample_spans = merge_spans(
        [
            (sample.at, sample.at + 2 * MINUTE)
            for sample in samples
        ],
        2 * MINUTE,
    )
    eligible = [
        opening
        for opening in openings
        if any(start <= opening <= end for start, end in sample_spans)
    ]
    matched: list[tuple[Forecast, int]] = []
    used: set[int] = set()
    for episode in episodes:
        candidate = next(
            (
                opening
                for opening in eligible
                if opening not in used
                and episode.at < opening <= episode.at + ALERT_HORIZON * MINUTE
            ),
            None,
        )
        if candidate is not None:
            matched.append((episode, candidate))
            used.add(candidate)
    precision = len(matched) / len(episodes) if episodes else 0
    recall = len(used) / len(eligible) if eligible else 0
    false_alerts = len(episodes) - len(matched)
    leads = sorted((opening - episode.at) / MINUTE for episode, opening in matched)
    eta_pairs = [
        (episode, opening)
        for episode, opening in matched
        if episode.eta_min is not None and episode.eta_max is not None
    ]
    eta_hits = sum(
        episode.eta_min <= (opening - episode.at) / MINUTE <= episode.eta_max
        for episode, opening in eta_pairs
    )
    print(
        f"  {label:<21}{len(episodes):>7}{precision:>9.0%}"
        f"{recall:>9.0%}{false_alerts:>8}",
        end="",
    )
    print(f"{statistics.median(leads):>8.1f}m" if leads else f"{'--':>9}", end="")
    print(f"{eta_hits / len(eta_pairs):>9.0%}" if eta_pairs else f"{'--':>9}")
    if leads:
        print(
            f"    lead range: p25 {percentile(leads, .25):.1f}m · "
            f"p75 {percentile(leads, .75):.1f}m"
        )


def score_chance(
    samples: list[Forecast],
    openings: list[int],
    up_spans: list[tuple[int, int]],
    label: str,
) -> None:
    """One alert each time the bridge comes back down, scored like a model.

    This is what "always on" looks like under episode accounting, and it is the
    number a model has to clear before its precision means anything.
    """
    episodes = []
    previous: Forecast | None = None
    was_up = True
    for sample in samples:
        gap = previous is None or sample.minute - previous.minute > 2 * MINUTE
        bridge_is_up = any(start <= sample.at <= end for start, end in up_spans)
        if not bridge_is_up and (was_up or gap):
            episodes.append(sample)
        was_up = bridge_is_up
        previous = sample
    sample_spans = merge_spans([(s.at, s.at + 2 * MINUTE) for s in samples], 2 * MINUTE)
    eligible = [
        opening
        for opening in openings
        if any(start <= opening <= end for start, end in sample_spans)
    ]
    used: set[int] = set()
    leads = []
    for episode in episodes:
        candidate = next(
            (
                opening
                for opening in eligible
                if opening not in used
                and episode.at < opening <= episode.at + ALERT_HORIZON * MINUTE
            ),
            None,
        )
        if candidate is not None:
            used.add(candidate)
            leads.append((candidate - episode.at) / MINUTE)
    precision = len(leads) / len(episodes) if episodes else 0
    recall = len(used) / len(eligible) if eligible else 0
    print(
        f"  {label:<21}{len(episodes):>7}{precision:>9.0%}"
        f"{recall:>9.0%}{len(episodes) - len(leads):>8}",
        end="",
    )
    print(f"{statistics.median(leads):>8.1f}m" if leads else f"{'--':>9}", end="")
    print(f"{'--':>9}")


def reliability_line(
    samples: list[Forecast],
    openings: list[int],
    up_spans: list[tuple[int, int]],
    model: str,
) -> None:
    """Is a 0.70 score a 70% chance? Brier score and a coarse reliability table.

    Each bridge-down minute sample is scored against whether a clean opening
    followed within the alert horizon. A perfectly calibrated score has the
    observed rate match the score in every band; the Brier score is the mean
    squared gap (0 is perfect, 0.25 is a coin flip).
    """
    pairs = []
    for sample in samples:
        if any(start <= sample.at <= end for start, end in up_spans):
            continue
        label = any(sample.at < opening <= sample.at + ALERT_HORIZON * MINUTE for opening in openings)
        pairs.append((sample.score / 10_000, label))
    if len(pairs) < 100:
        return
    brier = statistics.fmean((score - label) ** 2 for score, label in pairs)
    base = statistics.fmean(label for _, label in pairs)
    print(
        f"    reliability    Brier {brier:.3f} (coin flip 0.250, always-base-rate "
        f"{base * (1 - base):.3f}) over {len(pairs)} bridge-down minutes"
    )
    bands = ((0.0, 0.2), (0.2, 0.45), (0.45, 0.64), (0.64, 0.8), (0.8, 1.01))
    cells = []
    for low, high in bands:
        inside = [label for score, label in pairs if low <= score < high]
        if inside:
            cells.append(f"{low:.2f}-{min(high, 1.0):.2f}: {statistics.fmean(inside):.0%} of {len(inside)}")
    print("    score band -> observed opening rate: " + " · ".join(cells))


def forecast_section(samples, openings, up_spans) -> None:
    print(
        "\nFORECAST OUTCOMES  "
        f"(opening within {ALERT_HORIZON} min)"
    )
    if samples is None:
        print(
            "  no bridge_forecast_samples table — this build predates forecast history"
        )
        return
    if not samples:
        print("  forecast sampling is enabled; no samples have accumulated yet")
        return
    print(
        f"  {'sample':<21}{'alerts':>7}{'precision':>9}{'recall':>9}"
        f"{'false':>8}{'lead':>9}{'ETA hit':>9}"
    )
    for model in sorted({sample.model for sample in samples}):
        model_samples = [sample for sample in samples if sample.model == model]
        score_chance(model_samples, openings, up_spans, f"{model} chance")
        score_forecasts(
            model_samples,
            openings,
            up_spans,
            f"{model} raw",
            apply_alert_horizon=False,
        )
        score_forecasts(
            model_samples,
            openings,
            up_spans,
            f"{model} <= {ALERT_HORIZON}m",
        )
        for mode in ("scheduled", "blackout", "on_signal"):
            subset = [sample for sample in model_samples if sample.mode == mode]
            if subset:
                score_forecasts(subset, openings, up_spans, f"  {mode} <= {ALERT_HORIZON}m")
        reliability_line(model_samples, openings, up_spans, model)


def vessel_section(path: Path) -> None:
    print("\nVESSEL CATALOG")
    connection = connect(path)
    try:
        if not table_exists(connection, "ais_vessel_ledger"):
            print("  no AIS vessel ledger")
            return
        ledger_columns = columns(connection, "ais_vessel_ledger")

        def optional(name: str, fallback: str = "NULL") -> str:
            return name if name in ledger_columns else fallback

        rows = connection.execute(
            "SELECT l.mmsi, l.name, l.vessel_class, l.transits_opened, "
            "l.transits_fits_under, l.first_seen_ms, l.last_seen_ms, "
            f"{optional('call_sign')}, {optional('imo_number')}, "
            f"{optional('length_meters')}, {optional('beam_meters')}, "
            f"{optional('draught_meters')}, COUNT(f.observed_at_ms), "
            "MIN(f.observed_at_ms), MAX(f.observed_at_ms) "
            "FROM ais_vessel_ledger l LEFT JOIN ais_track_fixes f ON f.mmsi=l.mmsi "
            "GROUP BY l.mmsi ORDER BY l.transits_opened DESC, l.last_seen_ms DESC"
        ).fetchall()
        transit_count = connection.execute(
            "SELECT COUNT(*) FROM ais_transits"
        ).fetchone()[0]
        opener_offsets = connection.execute(
            "SELECT f.branch, f.offset_meters FROM ais_track_fixes f "
            "JOIN ais_vessel_ledger l ON l.mmsi=f.mmsi "
            "WHERE l.transits_opened > 0 AND f.branch IS NOT NULL "
            "AND f.offset_meters IS NOT NULL AND f.offset_meters >= 0 "
            "AND COALESCE(f.speed_knots, 0) > 0.5 "
            "AND COALESCE(f.posture, '') NOT IN ('moored', 'off_channel', 'deep_draft') "
            "AND EXISTS (SELECT 1 FROM ais_transits t WHERE t.mmsi=f.mmsi "
            "AND t.outcome='opened' "
            "AND ABS(t.crossed_at_ms-f.observed_at_ms) <= 90 * 60000)"
        ).fetchall()
    finally:
        connection.close()
    openers = [row for row in rows if row[3] > 0]
    known_outcomes = sum(row[3] + row[4] for row in rows)
    print(
        f"  {len(rows)} known hulls · {transit_count} crossings · "
        f"{known_outcomes} labelled outcomes"
    )
    print(
        f"  {len(openers)} vessels are confirmed bridge-openers; "
        "their raw fixes are retained"
    )
    if not openers:
        print("  no confirmed opener has been observed yet")
        return
    print(
        f"  {'vessel':<23}{'MMSI':<11}{'class':<12}{'open':>5}"
        f"{'under':>7}{'fixes':>8}{'track span':>13}"
    )
    for row in openers[:25]:
        (
            mmsi,
            name,
            vessel_class,
            opened,
            under,
            _first,
            _last,
            _call,
            _imo,
            _length,
            _beam,
            _draught,
            fixes,
            first_fix,
            last_fix,
        ) = row
        span = (last_fix - first_fix) / 3.6e6 if fixes and fixes > 1 else 0
        print(
            f"  {(name or 'Unnamed vessel')[:22]:<23}{mmsi:<11}"
            f"{(vessel_class or 'unknown')[:11]:<12}{opened:>5}{under:>7}"
            f"{fixes:>8}{span:>10.1f} h"
        )

    by_branch: dict[str, list[float]] = defaultdict(list)
    for branch, offset in opener_offsets:
        by_branch[branch].append(offset)
    print("\n  KNOWN-OPENER CORRIDOR FIT")
    if not by_branch:
        print("    no projected opener fixes yet")
        return
    limits = {
        "river": 120.0,
        "north_approach": 150.0,
        "government_cut": 220.0,
        "south_approach": 150.0,
    }
    print(f"    {'branch':<20}{'fixes':>7}{'median':>10}{'p90':>10}{'inside':>10}")
    for branch, offsets in sorted(by_branch.items()):
        offsets.sort()
        limit = limits.get(branch)
        inside = (
            sum(offset <= limit for offset in offsets) / len(offsets)
            if limit is not None
            else 0
        )
        print(
            f"    {branch:<20}{len(offsets):>7}"
            f"{statistics.median(offsets):>8.0f} m"
            f"{percentile(offsets, .90):>8.0f} m{inside:>9.0%}"
        )
        if len(offsets) < 25:
            print("      thin sample — retain the geometry and keep collecting")


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DB
    intervals, continuity_supported = read_intervals(path)
    spans = completeness(intervals, continuity_supported)
    openings = clean_lifts(intervals, TARGET)
    up_spans = merge_spans(
        [
            (row.started, row.confirmed)
            for row in intervals
            if row.key == TARGET
            and row.relation == "target"
            and row.state == "up"
            and row.reason != "legacy"
        ],
        CONTINUITY_GAP,
    )
    ups = {key: clean_lifts(intervals, key) for key in RIVER}

    print("\nOUTCOME SET")
    kinds = day_types(openings)
    print(f"  {len(openings)} clean Brickell down-to-up transitions")
    print(f"  weekday        {len(kinds.get('weekday', []))}")
    print(f"  weekend        {len(kinds.get('weekend', []))}")
    if len(openings) < 10:
        print("  fewer than 10 clean openings: all tuning verdicts remain locked")

    vessel_section(path)
    if openings:
        clock_section(openings)
        upstream_section(ups, openings, spans)
        transit_section(read_transits(path), openings, read_ais_crossings(path))
    else:
        print("\nCLOCK / UPSTREAM / RIVER TRANSITS")
        print("  waiting for continuously observed Brickell openings")
    forecast_section(read_forecasts(path), openings, up_spans)
    print("\nNothing was changed. Model weights remain human-reviewed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
