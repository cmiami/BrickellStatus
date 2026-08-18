"""Re-measures the bridge predictor against what the app has actually recorded.

    python3 scripts/calibrate_bridge.py [path/to/brickellstatus.sqlite3]

Reads the running app's history read-only and answers three questions the weight
table in `crates/policy/src/bridge.rs` currently answers from a single weekend:

  * is the record complete enough to calibrate on at all;
  * does the clock predict an opening, and in which window;
  * does upstream movement predict one, ordered or otherwise.

Everything is reported against a chance level and a held-out tail, because both
omissions have already bitten this project once. The first calibration compared
a clock statistic against the chance level for *one* anchor when there are two
per hour, which overstated its lift about twofold, and nothing was held out, so
a rule that scores below firing-constantly on unseen minutes looked strong.

DECISION RULES, pre-registered so a future window is not read by eye.

  Ordered upstream movement (weights `outbound_high`, `outbound_very_high`)
    Halve both if lift stays below 1.2 across at least two windows totalling
    100+ openings, with at least one of them a weekday sample. Raise them only
    if lift exceeds 2.0 on a weekday window with 50+ openings. Two weekends
    measuring 1.4x and 0.86x is not evidence either way; it is one regime,
    measured twice, on a river the regulation leaves on signal.

  Transit offset (`DEFAULT_INBOUND_TRANSIT_MINUTES`, `DEFAULT_OUTBOUND_TRANSIT_MINUTES`
  in crates/collectors/src/bbpilots.rs, currently 60 and 20 as placeholders)
    Replace a placeholder with the measured median once a direction has 20+
    movements pairing to an opening, and set `eta_calibrated` true only then.
    Report the interquartile range with it: an offset with a 40-minute spread is
    a number, not a countdown, and the panel should not treat it as one.

  Clock slot (weight `schedule_clock_slot`)
    Judge on the asymmetric window, which is what survived re-measurement: the
    tender lifts ahead of the scheduled minute. Compare -10..+5 against its own
    50% chance level, not +/-5 against 37%.

Nothing here writes to the database, and nothing here changes a weight. It
prints what the data supports; a human decides what to do about it.
"""

from __future__ import annotations

import math
import sqlite3
import statistics
import sys
from collections import defaultdict
from datetime import datetime, timedelta, timezone
from pathlib import Path

DEFAULT_DB = (
    Path.home()
    / "Library/Application Support/com.cmiami.brickellstatus/brickellstatus.sqlite3"
)
LOCAL = timezone(timedelta(hours=-4))  # Miami, EDT
MINUTE = 60_000
HORIZON = 15  # minutes of warning the panel promises
TARGET = "brickell"

# Mouth first. Used only to ask whether lifts descend the river in order; the
# answer so far is that they do not, which is itself worth re-checking.
RIVER = [
    "sw_2_ave", "sw_1_st", "w_flagler", "nw_5_st",
    "nw_12_ave", "nw_17_ave", "nw_22_ave", "nw_27_ave",
]


def read_transits(path: Path):
    """Booked river movements, if this build has been recording them."""
    connection = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        present = connection.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='river_transits'"
        ).fetchone()
        if not present:
            return None
        return connection.execute(
            "SELECT vessel, action, river_direction, scheduled_at_ms, "
            "estimated_offset_minutes FROM river_transits ORDER BY scheduled_at_ms"
        ).fetchall()
    finally:
        connection.close()


def transit_section(transits, openings) -> None:
    """Learns the offset between a pilot boarding time and a bridge opening."""
    print("\nRIVER TRANSITS  (the pilots' board against what the bridge did)")
    if transits is None:
        print("  no river_transits table — this database predates the build that")
        print("  records movements. The offset cannot be learned until the app")
        print("  runs long enough to collect them.")
        return
    if not transits:
        print("  table present, no movements recorded yet. The board publishes")
        print("  hours ahead, so give it a day of running before reading this.")
        return

    print(f"  {len(transits)} movements recorded")
    by_direction = defaultdict(list)
    for vessel, action, direction, scheduled, placeholder in transits:
        # The opening that followed the boarding time, within a generous bound:
        # a transit that never produced one inside three hours was not this
        # bridge's traffic.
        following = [t for t in openings if scheduled < t <= scheduled + 180 * MINUTE]
        if not following:
            continue
        by_direction[direction or "unknown"].append(
            ((min(following) - scheduled) / MINUTE, placeholder)
        )

    if not by_direction:
        print("  none of them pair to an opening yet")
        return

    print(f"  {'direction':<12}{'paired':>8}{'median':>9}{'IQR':>16}{'placeholder':>13}")
    for direction, pairs in sorted(by_direction.items()):
        offsets = sorted(p[0] for p in pairs)
        placeholder = next((p[1] for p in pairs if p[1] is not None), None)
        median = statistics.median(offsets)
        if len(offsets) >= 4:
            low, high = statistics.quantiles(offsets, n=4)[0], statistics.quantiles(offsets, n=4)[2]
            spread = f"{low:.0f} to {high:.0f} min"
        else:
            spread = "--"
        shown = f"{placeholder} min" if placeholder is not None else "--"
        print(f"  {direction:<12}{len(offsets):>8}{median:>6.0f} min{spread:>16}{shown:>13}")

    print("\n  VERDICT (pre-registered)")
    for direction, pairs in sorted(by_direction.items()):
        offsets = [p[0] for p in pairs]
        if len(offsets) < 20:
            print(f"    {direction}: {len(offsets)} pairs — need 20 before replacing a placeholder")
            continue
        median = statistics.median(offsets)
        low, high = statistics.quantiles(offsets, n=4)[0], statistics.quantiles(offsets, n=4)[2]
        if high - low > 40:
            print(f"    {direction}: median {median:.0f} min but IQR {high - low:.0f} min — too")
            print("      loose to publish as a countdown; keep eta_calibrated false")
        else:
            print(f"    {direction}: set the placeholder to {median:.0f} min "
                  f"(IQR {high - low:.0f}) and eta_calibrated true")


def read(path: Path):
    if not path.exists():
        sys.exit(f"no database at {path}\nrun the app once, or pass a path")
    # Read-only: the app may be running, and its history is the only copy.
    connection = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        rows = connection.execute(
            "SELECT bridge_key, relation, state, started_at_ms, ended_at_ms "
            "FROM bridge_state_intervals ORDER BY started_at_ms"
        ).fetchall()
    finally:
        connection.close()
    if not rows:
        sys.exit("no recorded intervals yet")
    return rows


def local(ms: int) -> datetime:
    return datetime.fromtimestamp(ms / 1000, LOCAL)


def clock_offset(ms: int) -> float:
    """Signed minutes to the nearest :00 or :30. Negative is before it."""
    stamp = local(ms)
    minute = stamp.minute + stamp.second / 60
    return min((minute, minute - 30, minute - 60), key=abs)


def binomial_tail(hits: int, trials: int, chance: float) -> float:
    """Probability of at least `hits` successes by chance alone."""
    return sum(
        math.comb(trials, i) * chance**i * (1 - chance) ** (trials - i)
        for i in range(hits, trials + 1)
    )


def completeness(rows) -> tuple[int, int]:
    """Prints coverage and returns the observed window."""
    lo = min(r[3] for r in rows)
    hi = max(r[4] or r[3] for r in rows)
    target = sorted((r[3], r[4] or hi, r[2]) for r in rows if r[0] == TARGET)

    covered, holes, previous_end = 0, [], None
    for start, end, _ in target:
        if previous_end is not None and start > previous_end + MINUTE:
            holes.append((previous_end, start))
        covered += end - max(start, previous_end or start)
        previous_end = max(previous_end or end, end)
    unknown = sum(e - s for s, e, state in target if state == "unknown")

    print("COMPLETENESS")
    print(f"  window          {local(lo):%a %d %b %H:%M} -> {local(hi):%a %d %b %H:%M} local")
    print(f"  duration        {(hi - lo) / 3.6e6:.1f} h")
    print(f"  coverage        {100 * covered / max(hi - lo, 1):.0f}% of the window has a recorded state")
    print(f"  unknown         {unknown / 3.6e6:.1f} h")
    if holes:
        print(f"  GAPS            {len(holes)} — the app was not recording:")
        for start, end in holes[:5]:
            print(f"                    {local(start):%d %b %H:%M} for {(end - start) / MINUTE:.0f} min")
        print("  Gaps bias every rate below. Treat the numbers as provisional.")
    else:
        print("  gaps            none")
    return lo, hi


def day_types(openings) -> dict[str, list[int]]:
    kinds = defaultdict(list)
    for opening in openings:
        kinds["weekend" if local(opening).weekday() >= 5 else "weekday"].append(opening)
    return kinds


def clock_section(openings) -> None:
    print("\nCLOCK")
    daytime = [o for o in openings if 7 <= local(o).hour < 19]
    if len(daytime) < 8:
        print(f"  only {len(daytime)} daytime openings — too few to judge")
        return
    offsets = sorted(clock_offset(o) for o in daytime)
    print(f"  {len(daytime)} daytime openings")
    print(f"  {'window':<14}{'captured':>10}{'chance':>9}{'lift':>7}{'p':>10}")
    for low, high, name in ((-5, 5, "+/-5 min"), (-10, 5, "-10..+5"), (-12, 5, "-12..+5")):
        hits = sum(1 for o in offsets if low <= o <= high)
        chance = (high - low) * 2 / 60
        lift = (hits / len(offsets)) / chance if chance else 0
        p = binomial_tail(hits, len(offsets), chance)
        print(f"  {name:<14}{hits:>4}/{len(offsets):<5}{chance:>8.0%}{lift:>7.2f}{p:>10.4f}")
    print("  The asymmetric window is the one to judge on: the tender lifts")
    print("  ahead of the scheduled minute, so a centred window measures the")
    print("  wrong thing and understates a real effect.")


def upstream_section(ups, openings, lo, hi) -> None:
    print("\nUPSTREAM")
    grid = list(range(lo, hi - HORIZON * MINUTE, MINUTE))
    if not grid:
        print("  window too short")
        return

    def opens(moment: int) -> bool:
        return any(moment < t <= moment + HORIZON * MINUTE for t in openings)

    base = sum(1 for m in grid if opens(m)) / len(grid)
    print(f"  base rate       {base:.0%} of minutes precede an opening within {HORIZON} min")

    rank = {key: index for index, key in enumerate(RIVER)}

    def lifted(moment, window, keys):
        return [k for k in keys if any(moment - window * MINUTE <= e <= moment for e in ups.get(k, []))]

    def ordered(moment, window) -> bool:
        events = sorted((e, k) for k in RIVER for e in ups.get(k, []) if moment - window * MINUTE <= e <= moment)
        if len(events) < 2:
            return False
        ranks = [rank[k] for _, k in events]
        return all(b < a for a, b in zip(ranks, ranks[1:]))

    print(f"  {'feature':<30}{'fires':>7}{'precision':>11}{'lift':>7}")
    results = {}
    for name, rule in (
        ("ordered downstream run, 20m", lambda m: ordered(m, 20)),
        ("ordered downstream run, 30m", lambda m: ordered(m, 30)),
        ("any two upstream, 20m", lambda m: len(lifted(m, 20, RIVER)) >= 2),
        ("any one upstream, 15m", lambda m: len(lifted(m, 15, RIVER)) >= 1),
    ):
        fires = [m for m in grid if rule(m)]
        precision = (sum(1 for m in fires if opens(m)) / len(fires)) if fires else 0
        lift = precision / base if base else 0
        results[name] = lift
        print(f"  {name:<30}{len(fires):>7}{precision:>10.0%}{lift:>7.2f}")

    print("\n  per-bridge, median minutes to the next opening after it lifts")
    baseline = statistics.median(
        [next((t - m) / MINUTE for t in openings if t > m) for m in grid if any(t > m for t in openings)]
    )
    print(f"  {'bridge':<14}{'lifts':>7}{'median wait':>13}{'vs base':>9}")
    for key in RIVER:
        events = ups.get(key, [])
        waits = [next((t - e) / MINUTE for t in openings if t > e) for e in events if any(t > e for t in openings)]
        if not waits:
            continue
        median = statistics.median(waits)
        flag = "" if len(waits) >= 20 else "  (thin)"
        print(f"  {key:<14}{len(events):>7}{median:>10.0f} min{median - baseline:>+8.0f}{flag}")

    print("\n  VERDICT (pre-registered)")
    best_ordered = max(results["ordered downstream run, 20m"], results["ordered downstream run, 30m"])
    if best_ordered < 1.2:
        print(f"    ordered movement lift {best_ordered:.2f} < 1.20 — does not support its weight")
        print("    in this window. Halve outbound_high/outbound_very_high only once")
        print("    this holds across 100+ openings including a weekday sample.")
    elif best_ordered > 2.0:
        print(f"    ordered movement lift {best_ordered:.2f} > 2.00 — supports raising its weight")
    else:
        print(f"    ordered movement lift {best_ordered:.2f} — inconclusive, keep collecting")


def backtest(ups, openings, lo, hi) -> None:
    print("\nHELD-OUT BACKTEST  (train on the first 60%, score the rest)")
    grid = list(range(lo, hi - HORIZON * MINUTE, MINUTE))
    split = lo + int((hi - lo) * 0.6)
    test = [m for m in grid if m >= split]
    if len(test) < 120:
        print("  held-out tail too short to score")
        return

    def opens(moment: int) -> bool:
        return any(moment < t <= moment + HORIZON * MINUTE for t in openings)

    def measure(name, rule):
        fires = [m for m in test if rule(m)]
        hits = sum(1 for m in fires if opens(m))
        misses = sum(1 for m in test if opens(m) and not rule(m))
        precision = hits / len(fires) if fires else 0
        recall = hits / (hits + misses) if hits + misses else 0
        f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0
        print(f"  {name:<26}{precision:>7.0%}{recall:>9.0%}{f1:>7.2f}")
        return f1

    def lifted(moment, window):
        return sum(1 for k in RIVER if any(moment - window * MINUTE <= e <= moment for e in ups.get(k, [])))

    print(f"  {'model':<26}{'prec':>7}{'recall':>9}{'F1':>7}")
    floor = measure("always fire", lambda m: True)
    measure("clock -10..+5", lambda m: -10 <= clock_offset(m) <= 5)
    measure("any upstream, 15m", lambda m: lifted(m, 15) >= 1)
    measure("clock or two upstream", lambda m: -10 <= clock_offset(m) <= 5 or lifted(m, 20) >= 2)
    print(f"\n  Any model scoring at or below the 'always fire' floor of {floor:.2f} is")
    print("  not a predictor. That floor is high because the bridge opens often.")


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DB
    rows = read(path)
    lo, hi = completeness(rows)

    ups = defaultdict(list)
    for key, _relation, state, started, _ended in rows:
        if state == "up":
            ups[key].append(started)
    for key in ups:
        ups[key].sort()
    openings = ups.get(TARGET, [])

    print("\nDAY TYPE")
    kinds = day_types(openings)
    for kind in ("weekday", "weekend"):
        events = kinds.get(kind, [])
        print(f"  {kind:<10}{len(events):>4} openings"
              f"{'' if events else '   <- none recorded yet'}")
    if not kinds.get("weekday"):
        print("  The regulation makes weekdays a different bridge: scheduled")
        print("  openings with rush-hour blackouts, against on-signal weekends.")
        print("  Every weight in the table is still calibrated on weekends only.")
    elif kinds.get("weekend"):
        print("  Both present — re-run this per day type before tuning anything.")

    if len(openings) < 10:
        print(f"\nonly {len(openings)} openings recorded; too few to calibrate on")
        return 0

    clock_section(openings)
    upstream_section(ups, openings, lo, hi)
    transit_section(read_transits(path), openings)
    backtest(ups, openings, lo, hi)
    print("\nNothing was changed. Weights live in crates/policy/src/bridge.rs.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
