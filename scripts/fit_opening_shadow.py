"""Fit the shadow opening model chronologically. Read-only. See docs/MODEL_AUDIT.md.

    <venv>/bin/python scripts/fit_opening_shadow.py SNAPSHOT.sqlite3 \
        --artifact crates/runtime/models/opening_shadow_v1.json

Needs scikit-learn (scripts/model-audit-requirements.txt, in its own venv).
Features come from scripts/opening_shadow.py, which the app mirrors in Rust.

Protocol, fixed before the test period is scored:
* Board windows come from pilots'-board lifts before the validation split.
* The alert threshold is chosen on a validation week by episode F1, with the
  model fitted only on earlier minutes.
* The test period is scored once, against the recorded live forecast on the
  same minutes and against a chance policy.
* The shipped coefficients are refitted on every eligible minute.
Outcome windows overlap, so every split embargoes the 30-minute horizon plus
the 30-second confirmation delay. Minutes are never shuffled.
"""
from __future__ import annotations

import argparse
import bisect
import json
import random
import sqlite3
import statistics
from collections import defaultdict
from datetime import datetime
from pathlib import Path

import calibrate_bridge as cb
import opening_shadow as shadow

MIN = shadow.MINUTE
HORIZON = 30 * MIN
EMBARGO = HORIZON + 30_000
STATIONARY_KNOTS = 0.5
FEATURE_LOOKBACK = shadow.FIX_WINDOW_MS + shadow.PREVIOUS_FIX_LOOKBACK_MS


def at_local(text: str) -> int:
    return int(datetime.fromisoformat(text).replace(tzinfo=shadow.LOCAL).timestamp() * 1000)


def load(path: Path):
    intervals, _ = cb.read_intervals(path)
    con = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        fixes = [dict(mmsi=m, at=at, sog=sog, branch=b, s=s, posture=p) for m, at, sog, b, s, p in con.execute(
            "SELECT mmsi, observed_at_ms, speed_knots, branch, s_meters, posture FROM ais_track_fixes "
            "ORDER BY observed_at_ms")]
        outcomes = defaultdict(list)
        crossings = []
        for m, crossed, out, resolved, speed in con.execute(
                "SELECT mmsi, crossed_at_ms, outcome, resolved_at_ms, speed_knots FROM ais_transits"):
            if out in ("opened", "fits_under") and resolved is not None and (speed or 0) > STATIONARY_KNOTS:
                outcomes[m].append((resolved, out))
            crossings.append((m, crossed))
        classes = {m: c for m, c in con.execute(
            "SELECT mmsi, vessel_class FROM ais_vessel_ledger WHERE vessel_class IS NOT NULL")}
        names = defaultdict(list)
        for m, name in con.execute("SELECT mmsi, name FROM ais_vessel_ledger WHERE name IS NOT NULL"):
            names[name.strip().casefold()].append(m)
        board = [dict(direction=d, scheduled_at=s, first_seen=f, last_seen=l, vessel=v) for v, d, s, f, l in con.execute(
            "SELECT vessel, river_direction, scheduled_at_ms, first_seen_at_ms, last_seen_at_ms FROM river_transits "
            "WHERE river_direction IS NOT NULL")]
    finally:
        con.close()
    for values in outcomes.values():
        values.sort()
    openings = cb.clean_lifts(intervals, "brickell")
    clean = set(openings)
    target_up = [r for r in intervals if r.key == "brickell" and r.state == "up" and r.reason != "legacy"]
    up_spans = cb.merge_spans([(r.started, r.confirmed) for r in target_up], cb.CONTINUITY_GAP)
    ambiguous = sorted(r.started for r in target_up if r.started not in clean)
    lifts = sorted((r.started, r.key, r.relation) for r in intervals if r.reason == "state_change" and r.state == "up")
    return dict(intervals=intervals, fixes=fixes, fix_times=[f["at"] for f in fixes], outcomes=outcomes,
                crossings=crossings, classes=classes, names=names, board=board, openings=openings,
                coverage=cb.observed_spans(intervals), up_spans=up_spans, ambiguous=ambiguous,
                target_up=[r for r in target_up if r.started in clean], lifts=lifts,
                lift_times=[t for t, _, _ in lifts])


def board_windows(data, before_ms: int):
    """[p25 - 30, p75] of board-time-to-lift minutes, from lifts before `before_ms`."""
    offsets = defaultdict(dict)
    for booking in data["board"]:
        for mmsi in data["names"].get(booking["vessel"].strip().casefold(), []):
            for m, crossed in data["crossings"]:
                if m != mmsi:
                    continue
                lift = next((r.started for r in data["target_up"]
                             if r.started <= crossed <= r.confirmed and crossed - r.started <= 15 * MIN), None)
                if lift is None or lift >= before_ms:
                    continue
                visible = [b for b in data["board"] if b["vessel"] == booking["vessel"]
                           and b["first_seen"] <= lift - HORIZON <= b["last_seen"] + shadow.BOARD_GRACE_MS
                           and b["scheduled_at"] - 60 * MIN <= lift <= b["scheduled_at"] + 300 * MIN]
                if visible and max(visible, key=lambda b: b["first_seen"]) is booking:
                    # One pair per booking and lift, however many hulls share the name.
                    offsets[booking["direction"]][(booking["scheduled_at"], lift)] = (
                        lift - booking["scheduled_at"]) / MIN
    windows = {}
    for direction, pairs in offsets.items():
        values = sorted(pairs.values())
        if len(values) >= 8:
            windows[direction] = [round(cb.percentile(values, .25) - 30), round(cb.percentile(values, .75))]
    return windows, {d: len(v) for d, v in offsets.items()}


def inputs_at(data, now: int) -> dict:
    lo = bisect.bisect_left(data["fix_times"], now - FEATURE_LOOKBACK)
    hi = bisect.bisect_left(data["fix_times"], now)
    fixes = data["fixes"][lo:hi]
    cutoff = now - shadow.HISTORY_BUFFER_MS
    history = {}
    for mmsi in {fix["mmsi"] for fix in fixes}:
        past = data["outcomes"].get(mmsi, [])
        past = past[:bisect.bisect_right(past, (cutoff, "~"))]
        opened = sum(1 for _, out in past if out == "opened")
        if past:
            history[mmsi] = (opened, len(past) - opened)
    a = bisect.bisect_left(data["lift_times"], now - shadow.UPSTREAM_LOOKBACK_MS)
    b = bisect.bisect_right(data["lift_times"], now)
    ups = [dict(key=k, relation=r, at=t) for t, k, r in data["lifts"][a:b]]
    return dict(now_ms=now, fixes=fixes, history=history, classes=data["classes"], up_starts=ups,
                board=data["board"], schedule=shadow.schedule_context(now))


def grid(data):
    minutes = []
    for start, end in data["coverage"]:
        first = start - start % MIN + MIN
        for m in range(first, end - HORIZON + 1, MIN):
            if cb.covers_window(data["up_spans"], m, m):
                continue
            i = bisect.bisect_right(data["ambiguous"], m)
            if i < len(data["ambiguous"]) and data["ambiguous"][i] <= m + HORIZON:
                continue
            minutes.append(m)
    return minutes


def label(data, m: int) -> int:
    i = bisect.bisect_right(data["openings"], m)
    return int(i < len(data["openings"]) and data["openings"][i] <= m + HORIZON)


def episodes(minutes, flags, openings):
    """One-to-one matching of alert episodes to openings within the horizon."""
    starts, active, previous = [], False, None
    for m, on in zip(minutes, flags):
        if previous is None or m - previous > 2 * MIN:
            active = False
        if on and not active:
            starts.append(m)
        active, previous = on, m
    spans = cb.merge_spans([(m, m + HORIZON) for m in minutes], 0)
    eligible = [o for o in openings if cb.covers_window(spans, o, o)]
    used, leads = set(), []
    for start in starts:
        i = bisect.bisect_right(eligible, start)
        while i < len(eligible) and eligible[i] in used:
            i += 1
        if i < len(eligible) and eligible[i] <= start + HORIZON:
            used.add(eligible[i])
            leads.append((eligible[i] - start) / MIN)
    return dict(alerts=len(starts), hits=len(leads), false=len(starts) - len(leads), openings=len(eligible),
                precision=len(leads) / len(starts) if starts else 0.0,
                recall=len(used) / len(eligible) if eligible else 0.0,
                lead=statistics.median(leads) if leads else None, started=starts, matched=used)


def hysteresis(probabilities, minutes, enter, exit_):
    flags, active, previous = [], False, None
    for m, p in zip(minutes, probabilities):
        if previous is None or m - previous > 2 * MIN:
            active = False
        active = p >= (exit_ if active else enter)
        flags.append(active)
        previous = m
    return flags


def fit(X, y):
    from sklearn.linear_model import LogisticRegression
    return LogisticRegression(C=0.5, max_iter=2000).fit(X, y)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--validation-start", default="2026-09-06")
    parser.add_argument("--test-start", default="2026-09-13")
    args = parser.parse_args()
    data = load(args.database)
    validation_start, test_start = at_local(args.validation_start), at_local(args.test_start)
    windows, pairs = board_windows(data, validation_start)
    print(f"board windows from lifts before {args.validation_start}: {windows} (pairs {pairs})")
    minutes = grid(data)
    X = [[shadow.features(inputs_at(data, m), windows)[name] for name in shadow.FEATURES] for m in minutes]
    y = [label(data, m) for m in minutes]
    print(f"{len(minutes)} eligible bridge-down minutes, base rate {sum(y) / len(y):.1%}")

    def rows(lo, hi, embargo=0):
        return [i for i, m in enumerate(minutes) if lo <= m and m + embargo < hi]

    # 1. Threshold on the validation week, model fitted on earlier minutes only.
    train = rows(0, validation_start, EMBARGO)
    valid = rows(validation_start, test_start, EMBARGO)
    model = fit([X[i] for i in train], [y[i] for i in train])
    p_valid = model.predict_proba([X[i] for i in valid])[:, 1]
    best = None
    for enter in [x / 100 for x in range(40, 80, 5)]:
        result = episodes([minutes[i] for i in valid],
                          hysteresis(p_valid, [minutes[i] for i in valid], enter, 0.75 * enter), data["openings"])
        f1 = 2 * result["precision"] * result["recall"] / (result["precision"] + result["recall"] or 1)
        if best is None or f1 > best[0]:
            best = (f1, enter)
    enter, exit_ = best[1], round(0.75 * best[1], 4)
    print(f"validation {args.validation_start}..{args.test_start}: enter {enter:.2f} exit {exit_:.2f} (episode F1 {best[0]:.3f})")

    # 2. Test once: fitted on everything before the test split.
    train = rows(0, test_start, EMBARGO)
    model = fit([X[i] for i in train], [y[i] for i in train])
    recorded = {}
    for sample in cb.read_forecasts(args.database):
        recorded[sample.minute] = sample
    test = [i for i in rows(test_start, 1 << 62) if minutes[i] - minutes[i] % MIN in recorded]
    t_minutes = [minutes[i] for i in test]
    p_test = model.predict_proba([X[i] for i in test])[:, 1]
    live_flags = [(lambda s: s.state == "likely" and s.eta_max is not None and s.eta_max <= 30)(recorded[m - m % MIN])
                  for m in t_minutes]
    shadow_flags = hysteresis(p_test, t_minutes, enter, exit_)
    live = episodes(t_minutes, live_flags, data["openings"])
    challenger = episodes(t_minutes, shadow_flags, data["openings"])
    chance = episodes(t_minutes, [True] * len(t_minutes), data["openings"])
    models = sorted({recorded[m - m % MIN].model for m in t_minutes})
    print(f"\ntest {args.test_start} onward: {len(t_minutes)} minutes with a recorded forecast ({', '.join(models)})")
    print(f"  {'policy':<22}{'alerts':>7}{'warned':>8}{'false':>7}{'precision':>10}{'recall':>8}{'lead':>7}")
    for name, r in (("recorded live model", live), ("shadow model", challenger), ("chance", chance)):
        lead = f"{r['lead']:.1f}" if r["lead"] is not None else "--"
        print(f"  {name:<22}{r['alerts']:>7}{r['hits']:>4}/{r['openings']:<3}{r['false']:>7}"
              f"{r['precision']:>10.0%}{r['recall']:>8.0%}{lead:>7}")
    # Informational only; nothing below is selected on the test period.
    print("  operating curve on the test period (information for promotion, not selection):")
    for level in [x / 100 for x in range(30, 85, 5)]:
        r = episodes(t_minutes, hysteresis(p_test, t_minutes, level, 0.75 * level), data["openings"])
        print(f"    enter {level:.2f}: alerts {r['alerts']:4d}  warned {r['hits']:3d}/{r['openings']}  false {r['false']:4d}  "
              f"precision {r['precision']:.0%}  recall {r['recall']:.0%}  lead {r['lead'] or 0:.1f}")
    # Day-block bootstrap of the paired difference: days carry convoys and weather together.
    days = defaultdict(list)
    for k, m in enumerate(t_minutes):
        days[datetime.fromtimestamp(m / 1000, shadow.LOCAL).date()].append(k)
    keys = sorted(days)
    rng = random.Random(20260923)
    deltas = []
    for _ in range(2000):
        pick = [rng.choice(keys) for _ in keys]
        idx = sorted(k for d in pick for k in days[d])
        sub = [t_minutes[k] for k in idx]
        a = episodes(sub, [live_flags[k] for k in idx], data["openings"])
        b = episodes(sub, [shadow_flags[k] for k in idx], data["openings"])
        deltas.append((b["false"] - a["false"], b["hits"] - a["hits"]))
    false_d = sorted(d[0] for d in deltas)
    hit_d = sorted(d[1] for d in deltas)
    ci = lambda v: (v[int(.025 * len(v))], v[int(.975 * len(v)) - 1])
    print(f"  day-block bootstrap, shadow minus live: false alerts {ci(false_d)}, warned openings {ci(hit_d)}")

    # 3. Ship: refit on every eligible minute with the chosen threshold.
    final = fit(X, y)
    artifact = dict(
        version="opening-shadow-v1",
        target="FL511 Brickell lift within 30 minutes of a bridge-down minute",
        mode="shadow: recorded beside the live forecast, never drives an alert",
        trained_through_local=datetime.fromtimestamp(minutes[-1] / 1000, shadow.LOCAL).isoformat(timespec="minutes"),
        eligible_minutes=len(minutes), openings=len(data["openings"]),
        features=shadow.FEATURES,
        coefficients=[round(float(c), 6) for c in final.coef_[0]],
        intercept=round(float(final.intercept_[0]), 6),
        enter=enter, exit=exit_,
        board_windows=windows,
        validation=dict(start=args.validation_start, episode_f1=round(best[0], 4)),
        test=dict(start=args.test_start, minutes=len(t_minutes),
                  live={k: live[k] for k in ("alerts", "hits", "false", "openings", "lead")},
                  shadow={k: challenger[k] for k in ("alerts", "hits", "false", "openings", "lead")},
                  chance={k: chance[k] for k in ("alerts", "hits", "false", "openings", "lead")},
                  bootstrap95_false_delta=ci(false_d), bootstrap95_warned_delta=ci(hit_d)),
    )
    args.artifact.write_text(json.dumps(artifact, indent=2) + "\n")
    print("\nshipped coefficients (log-odds):")
    for name, c in sorted(zip(shadow.FEATURES, artifact["coefficients"]), key=lambda kv: -abs(kv[1])):
        print(f"  {name:<16}{c:+.3f}")
    print(f"  intercept       {artifact['intercept']:+.3f}")
    print(f"wrote {args.artifact}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
