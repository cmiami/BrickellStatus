"""Known-opener and pilots'-board audit. Read-only. See docs/MODEL_AUDIT.md.

    python3 scripts/audit_known_openers.py SNAPSHOT.sqlite3 [--test-start 2026-09-13]

Standard library only. Reproduces the evidence behind brickell-v7's
first-seen opener prior and its pilots'-board offsets:

* Each hull's first labelled crossing by class and length.
* A chronological test of the first-seen prior against the Beta(1,1) ledger
  alone: rates before --test-start, scored on crossings from it on, using only
  each hull's outcomes resolved at least 20 minutes before the crossing.
* Board time to FL511 lift, pairing the board revision the app could see 30
  minutes before a lift with that hull's own AIS crossing inside the lift.

Crossings at 0.5 kn or less are a stationary hull's jitter and are excluded.
"""
from __future__ import annotations

import argparse
import sqlite3
import statistics
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

import calibrate_bridge as cb

MIN = 60_000
STATIONARY_KNOTS = 0.5


def first_seen_prior(vessel_class, length):
    """Mirror of `first_seen_opening_propensity` in crates/runtime/src/engine.rs."""
    if vessel_class == "sailing":
        return 0.90
    if vessel_class == "passenger" or (length or 0) >= 24:
        return 0.85
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("--test-start", default="2026-09-13")
    args = parser.parse_args()
    split = int(datetime.fromisoformat(args.test_start).replace(tzinfo=cb.LOCAL).timestamp() * 1000)
    con = cb.connect(args.database)
    ledger = {m: (c or "unknown", length) for m, c, length in con.execute(
        "SELECT mmsi, vessel_class, length_meters FROM ais_vessel_ledger")}
    crossings = [row for row in con.execute(
        "SELECT mmsi, crossed_at_ms, outcome, resolved_at_ms, speed_knots FROM ais_transits ORDER BY crossed_at_ms")
        if row[2] in ("opened", "fits_under") and (row[4] or 0) > STATIONARY_KNOTS]

    print("FIRST LABELLED CROSSING OF EACH HULL")
    first, seen = Counter(), set()
    for mmsi, _, outcome, _, _ in crossings:
        if mmsi in seen:
            continue
        seen.add(mmsi)
        cls, length = ledger.get(mmsi, ("unknown", None))
        size = ">=24 m" if (length or 0) >= 24 else ("<24 m" if length else "no length")
        first[(cls, size, outcome)] += 1
    for cls, size in sorted({(c, s) for c, s, _ in first}):
        print(f"  {cls:<16}{size:<10} opened {first[(cls, size, 'opened')]:3d}"
              f"  fits under {first[(cls, size, 'fits_under')]:3d}")

    history = defaultdict(list)
    for mmsi, _, outcome, resolved, _ in crossings:
        history[mmsi].append((resolved, outcome))

    def score(rule):
        right = wrong = first_right = first_wrong = 0
        for mmsi, at, outcome, _, _ in crossings:
            if at < split:
                continue
            past = [o for r, o in history[mmsi] if r <= at - 20 * MIN]
            opened, fits_under = past.count("opened"), past.count("fits_under")
            p = rule(mmsi, opened, fits_under)
            if p is None or p < 0.6:
                continue
            if outcome == "opened":
                right += 1
                first_right += not past
            else:
                wrong += 1
                first_wrong += not past
        return right, wrong, first_right, first_wrong

    def today(mmsi, opened, fits_under):
        if opened + fits_under:
            return (opened + 1) / (opened + fits_under + 2)
        return 0.9 if ledger.get(mmsi, ("",))[0] == "sailing" else None

    def v7(mmsi, opened, fits_under):
        if opened + fits_under:
            return (opened + 1) / (opened + fits_under + 2)
        return first_seen_prior(*ledger.get(mmsi, ("unknown", None)))

    total = sum(1 for _, at, outcome, _, _ in crossings if at >= split and outcome == "opened")
    print(f"\nKNOWN-OPENER FLAGS ON CROSSINGS FROM {args.test_start} ({total} opened)")
    for name, rule in (("Beta(1,1) + sailing (v6)", today), ("+ first-seen prior (v7)", v7)):
        right, wrong, first_right, first_wrong = score(rule)
        print(f"  {name:<26} right {right:4d}  wrong {wrong:3d}  precision {right / (right + wrong):.0%}"
              f"  recall {right / total:.0%}  first-seen right/wrong {first_right}/{first_wrong}")

    print("\nBOARD TIME TO LIFT (revision visible 30 min before the lift)")
    intervals, _ = cb.read_intervals(args.database)
    clean = set(cb.clean_lifts(intervals, "brickell"))
    lifts = [r for r in intervals if r.key == "brickell" and r.started in clean]
    names = defaultdict(list)
    for mmsi, name in con.execute("SELECT mmsi, name FROM ais_vessel_ledger WHERE name IS NOT NULL"):
        names[name.strip().casefold()].append(mmsi)
    board = con.execute("SELECT vessel, river_direction, scheduled_at_ms, first_seen_at_ms, last_seen_at_ms "
                        "FROM river_transits WHERE river_direction IS NOT NULL").fetchall()
    every = con.execute("SELECT mmsi, crossed_at_ms FROM ais_transits").fetchall()
    con.close()
    pairs = defaultdict(dict)
    for vessel in {row[0] for row in board}:
        for mmsi in names.get(vessel.strip().casefold(), []):
            for m, crossed in every:
                if m != mmsi:
                    continue
                lift = next((r.started for r in lifts
                             if r.started <= crossed <= r.confirmed and crossed - r.started <= 15 * MIN), None)
                if lift is None:
                    continue
                visible = [row for row in board if row[0] == vessel
                           and row[3] <= lift - 30 * MIN <= row[4] + 10 * MIN
                           and row[2] - 60 * MIN <= lift <= row[2] + 300 * MIN]
                if visible:
                    row = max(visible, key=lambda item: item[3])
                    pairs[row[1]][(row[2], lift)] = (lift - row[2]) / MIN
    for direction, found in sorted(pairs.items()):
        values = sorted(found.values())
        print(f"  {direction:<10} n={len(values):3d}  median {statistics.median(values):+.0f} min"
              f"  IQR {cb.percentile(values, .25):+.0f} to {cb.percentile(values, .75):+.0f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
