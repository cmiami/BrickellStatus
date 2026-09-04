"""Conditional AIS ETA audit, grouped by crossing, never by individual fix.

This tests *when* a correctly identified passage opens the span. It does not
measure detection recall or prove that a hull required the opening.
"""
from __future__ import annotations

import argparse
from bisect import bisect_right
from collections import defaultdict
import json
import math
from pathlib import Path

from audit_bridge_model import at_time
from calibrate_bridge import MINUTE, clean_lifts, connect, read_intervals


def load_passages(path):
    intervals, _ = read_intervals(path)
    clean = set(clean_lifts(intervals, "brickell"))
    ups = [r for r in intervals if r.key == "brickell" and r.started in clean]
    c = connect(path)
    crossings = c.execute("SELECT mmsi,crossed_at_ms,direction,outcome,resolved_at_ms FROM ais_transits ORDER BY crossed_at_ms").fetchall()
    by_vessel = defaultdict(list)
    for row in c.execute("SELECT mmsi,observed_at_ms,speed_knots,branch,s_meters,offset_meters,session_id FROM ais_track_fixes ORDER BY observed_at_ms"):
        by_vessel[row[0]].append(row)
    c.close()
    passages = []
    for vessel, crossed, direction, outcome, resolved in crossings:
        if outcome != "opened":
            continue
        candidates = [r for r in ups if r.started <= crossed <= r.confirmed
                      and crossed - r.started <= 15 * MINUTE]
        if len(candidates) != 1:
            continue
        opening = candidates[0].started
        fixes = by_vessel[vessel]
        rows = []
        for previous, fix in zip(fixes, fixes[1:]):
            _, at, sog, branch, s, offset, session = fix
            if not opening - 60 * MINUTE <= at < opening or s is None or sog is None:
                continue
            if previous[-1] != session or previous[3] != branch or previous[4] is None:
                continue
            delta = (at - previous[1]) / 1000
            if not 0 < delta <= 180 or sog <= .5:
                continue
            limit = {"river": 120, "government_cut": 220, "north_approach": 150, "south_approach": 150}.get(branch, 0)
            if offset is None or offset > limit or abs(s) <= 50:
                continue
            closing = (abs(previous[4]) - abs(s)) / delta
            if closing <= .25 or (s > 0) != (direction == "downriver"):
                continue
            # Past fixes only. Discard impossible jumps instead of fitting them.
            if closing > max(2, sog * .514444 * 2):
                continue
            minutes = abs(s) / closing / 60
            if not .25 <= minutes <= 75:
                continue
            group = "outbound_river" if branch == "river" and s > 0 else branch
            low, high = {"outbound_river": (.62, 1.12), "river": (1.5, 6),
                         "government_cut": (.35, 1.5), "north_approach": (.5, 2),
                         "south_approach": (.5, 2)}[group]
            earliest = max(1, math.floor(minutes * low))
            # Capture the production panic condition explicitly.
            baseline = None if earliest > 90 else (earliest, min(90, math.ceil(minutes * high)))
            rows.append(dict(at=at, group=group, dead_reckoning=minutes,
                             actual=(opening-at)/MINUTE, before=baseline))
        if rows:
            passages.append(dict(crossed=crossed, resolved=resolved, opened=opening, rows=rows))
    return passages


GROUPS = ["outbound_river", "river", "government_cut", "north_approach", "south_approach"]


def matrix(rows):
    return [[r["dead_reckoning"]] + [float(r["group"] == g) for g in GROUPS] for r in rows]


def flatten(passages):
    rows, weights = [], []
    for passage in passages:
        rows.extend(passage["rows"])
        weights.extend([1 / len(passage["rows"])] * len(passage["rows"]))
    return rows, weights


def fit(passages, alpha):
    from sklearn.linear_model import QuantileRegressor
    rows, weights = flatten(passages)
    return [QuantileRegressor(quantile=q, alpha=alpha, solver="highs")
            .fit(matrix(rows), [r["actual"] for r in rows], sample_weight=weights)
            for q in (.2, .5, .8)]


def metrics(passages, models=None):
    import numpy as np
    metrics = []
    for passage in passages:
        rows = [r for r in passage["rows"] if r["before"] is not None]
        if not rows:
            continue
        y = np.array([r["actual"] for r in rows])
        if models:
            low, middle, high = [m.predict(matrix(rows)) for m in models]
            low, high = np.minimum(low, high).clip(0, 90), np.maximum(low, high).clip(0, 90)
        else:
            low, high = np.array([r["before"] for r in rows]).T
            middle = (low + high) / 2
        # Proper central 60% interval score: width plus miss penalties.
        interval_score = high-low + 5*np.maximum(low-y, 0) + 5*np.maximum(y-high, 0)
        metrics.append(dict(mae=float(np.mean(abs(middle-y))), coverage=float(np.mean((low<=y)&(y<=high))),
                            width=float(np.mean(high-low)), interval_score=float(np.mean(interval_score))))
    return {"passages": len(metrics), **{k: float(np.mean([m[k] for m in metrics])) for k in metrics[0]}}


def shipped_metrics(passages, artifact):
    """The actual rounded, route-gated bounds used by Rust (before policy)."""
    adapted = []
    for passage in passages:
        rows = []
        for row in passage["rows"]:
            interval = row["before"]
            if row["group"] in artifact["supported_routes"]:
                minutes = row["dead_reckoning"]
                low = max(1, math.floor(artifact["lower"]["slope"] * minutes + artifact["lower"]["intercept"]))
                high = min(90, max(low, math.ceil(artifact["upper"]["slope"] * minutes + artifact["upper"]["intercept"])))
                interval = (low, high)
            rows.append({**row, "before": interval})
        adapted.append({**passage, "rows": rows})
    return metrics(adapted)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--chart", type=Path)
    args = parser.parse_args()
    passages = load_passages(args.database)
    train = [p for p in passages if p["resolved"] is not None and p["resolved"] < at_time("2026-08-29")]
    validation = [p for p in passages if at_time("2026-08-29") <= p["crossed"]
                  and p["resolved"] is not None and p["resolved"] < at_time("2026-09-02")]
    test = [p for p in passages if p["crossed"] >= at_time("2026-09-02")]
    if not train or not validation or not test:
        parser.error(f"need opening-linked passages in each chronological partition; got {len(train)} training, {len(validation)} validation, {len(test)} testing")
    candidates = []
    for alpha in (.01, .1, 1):
        fitted = fit(train, alpha)
        candidates.append((metrics(validation, fitted)["interval_score"], alpha))
    _, alpha = min(candidates)
    model = fit(train + validation, alpha)
    report = dict(train_passages=len(train), validation_passages=len(validation), test_passages=len(test),
                  candidates=candidates, before=metrics(test), after=metrics(test, model),
                  model=[dict(coef=m.coef_.tolist(), intercept=m.intercept_) for m in model],
                  out_of_horizon_fixes=sum(r["before"] is None for p in passages for r in p["rows"]))
    artifact = json.loads((Path(__file__).resolve().parents[1] / "crates/collectors/models/ais_timing_v1.json").read_text())
    report["shipped_formula"] = shipped_metrics(test, artifact)
    # A convoy can contain several AIS hulls in one lift. Resample openings,
    # not correlated fixes or hulls, for uncertainty in the measured change.
    import numpy as np
    groups = defaultdict(list)
    for passage in test:
        groups[passage["opened"]].append(passage)
    deltas = [{key: shipped_metrics(group, artifact)[key] - metrics(group)[key]
               for key in ("coverage", "mae", "interval_score")} for group in groups.values()]
    rng = np.random.default_rng(20260904)
    indices = rng.integers(len(deltas), size=(5000, len(deltas)))
    weights = np.array([len(group) for group in groups.values()])
    report["opening_cluster_deltas"] = {
        key: dict(mean=float(np.average([d[key] for d in deltas], weights=weights)),
                  interval95=np.quantile(
                      (np.array([d[key] for d in deltas])[indices] * weights[indices]).sum(axis=1)
                      / weights[indices].sum(axis=1), [.025, .975]).tolist())
        for key in ("coverage", "mae", "interval_score")
    }
    report["independent_test_openings"] = len(groups)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    if args.chart:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        before, after = report["before"], report["shipped_formula"]
        fig, axes = plt.subplots(1, 2, figsize=(9, 4.8))
        colors = ["#8b9299", "#177c72"]
        for ax, key, scale, title in (
            (axes[0], "coverage", 100, "ETA window coverage · higher is better"),
            (axes[1], "interval_score", 1, "Interval error score · lower is better"),
        ):
            values = [before[key] * scale, after[key] * scale]
            ax.bar(["Before", "New formula"], values, color=colors, width=.55)
            ax.set_ylim(0, 100 if key == "coverage" else max(values) * 1.25)
            ax.set_title(title, fontsize=11, loc="left", pad=15)
            ax.spines[["top", "right"]].set_visible(False)
            for i, value in enumerate(values):
                ax.text(i, value + (2 if key == "coverage" else 1),
                        f"{value:.1f}" + ("%" if key == "coverage" else ""), ha="center", fontsize=13)
        fig.suptitle("AIS timing: later-period comparison", x=.07, ha="left", fontsize=17)
        fig.text(.07, .055, "41 passages across 27 openings · Sep 2–4, 2026\nConditional timing accuracy; this does not measure opening-detection recall.", fontsize=10)
        fig.tight_layout(rect=(.035, .16, 1, .94), w_pad=3)
        fig.savefig(args.chart, dpi=160)
        plt.close(fig)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
