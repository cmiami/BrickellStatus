"""Chronological, read-only model tournament. See docs/MODEL_AUDIT.md.

Install scripts/model-audit-requirements.txt in a separate Python 3.13 venv.
Never shuffle minutes: outcome windows overlap and individual minutes are not
independent trials. The last period is evaluated only after validation selects
the model and its alert threshold.
"""
from __future__ import annotations

import argparse
from bisect import bisect_right
from collections import defaultdict
from dataclasses import dataclass, replace
from datetime import datetime
import hashlib
import json
import math
from pathlib import Path

from calibrate_bridge import (
    ALERT_HORIZON, LOCAL, MINUTE, RIVER, clean_lifts, connect, covers_window,
    local, merge_spans, observed_spans, read_intervals,
)


@dataclass
class Sample:
    at: int
    session: str
    model: str
    state: str
    score: float
    eta_min: int | None
    eta_max: int | None
    mode: str
    contributions: dict
    freshness: dict
    down: bool
    eligible: bool
    label: bool
    features: dict[str, float]


def at_time(value: str) -> int:
    return int(datetime.fromisoformat(value).replace(tzinfo=LOCAL).timestamp() * 1000)


def past_age(events: list[int], at: int, cap: float = 120) -> float:
    index = bisect_right(events, at) - 1
    return min(cap, (at - events[index]) / MINUTE) if index >= 0 else cap


def read_dataset(path: Path):
    intervals, _ = read_intervals(path)
    coverage = observed_spans(intervals)
    openings = clean_lifts(intervals, "brickell")
    up = merge_spans([(r.started, r.confirmed) for r in intervals
                     if r.key == "brickell" and r.state == "up" and r.reason != "legacy"])
    # A brief/unwitnessed up state cannot become a negative training label just
    # because it failed the clean-opening definition.
    ambiguous = sorted(r.started for r in intervals if r.key == "brickell"
                       and r.state == "up" and r.started not in set(openings))
    events = defaultdict(list)
    for r in intervals:
        # A feature can use an observed transition immediately. Requiring the
        # future confirmation used by outcome labels here would leak the future.
        if r.reason == "state_change" and r.state == "up":
            events[r.key].append(r.started)
    for values in events.values():
        values.sort()
    c = connect(path)
    rows = c.execute(
        "SELECT evaluated_at_ms,session_id,model_version,state,predictive_score_bps,"
        "eta_min_minutes,eta_max_minutes,schedule_mode,contribution_bps_json,"
        "source_freshness_json FROM bridge_forecast_samples "
        "WHERE target_key='brickell' ORDER BY evaluated_at_ms"
    ).fetchall()
    c.close()
    samples = []
    for at, session, model, state, score, eta_min, eta_max, mode, terms, fresh in rows:
        terms, fresh = json.loads(terms), json.loads(fresh)
        stop = at + ALERT_HORIZON * MINUTE
        next_index = bisect_right(openings, at)
        label = next_index < len(openings) and openings[next_index] <= stop
        amb_index = bisect_right(ambiguous, at)
        uncertain = amb_index < len(ambiguous) and ambiguous[amb_index] <= stop
        down = not covers_window(up, at, at)
        eligible = down and covers_window(coverage, at, stop) and not uncertain
        f = {kind: max(0, min(2, terms.get(kind, 0) / 10_000))
             for kind in ("ais", "outbound", "transit", "schedule", "corroboration")}
        # Remove the known historical change in schedule weight rather than
        # letting version changes act as calendar labels.
        f["schedule"] = min(f["schedule"], .30)
        f.update(score=score / 10_000,
                 eta_min=min(eta_min if eta_min is not None else 60, 60) / 30,
                 eta_max=min(eta_max if eta_max is not None else 60, 60) / 30,
                 has_eta=float(eta_max is not None),
                 scheduled=float(mode == "scheduled"), blackout=float(mode == "blackout"))
        stamp = local(at)
        hour = stamp.hour + stamp.minute / 60
        f.update(hour_sin=math.sin(2 * math.pi * hour / 24),
                 hour_cos=math.cos(2 * math.pi * hour / 24),
                 weekend=float(stamp.weekday() >= 5))
        for kind in ("ais", "outbound", "transit"):
            summary = fresh.get(kind, {})
            f[kind + "_fresh"] = (summary.get("average_freshness_bps") or 0) / 10_000
            f[kind + "_missing"] = float(not (summary.get("current", 0) + summary.get("informational", 0)))
        f["target_recency"] = math.exp(-past_age(events["brickell"], at) / 30)
        for key in RIVER:
            f["lift_" + key] = math.exp(-past_age(events[key], at) / 15)
        samples.append(Sample(at, session, model, state, score / 10_000, eta_min,
                              eta_max, mode, terms, fresh, down, eligible, label, f))
    return samples, openings, coverage, intervals


def alert_flags(samples, probabilities=None, enter=.64, exit=.45):
    active = False
    previous = None
    flags = []
    for i, s in enumerate(samples):
        if previous is None or s.session != previous.session or s.at - previous.at > 2 * MINUTE or not s.down:
            active = False
        if probabilities is None:
            active = s.state == "likely"
        else:
            active = probabilities[i] >= (exit if active else enter)
        horizon = s.eta_max is not None and s.eta_max <= ALERT_HORIZON
        flags.append(active and horizon and s.down)
        previous = s
    return flags


def stable_alerts(samples, flags, enter_minutes=1.0, exit_minutes=0.0):
    """Causal dwell filter. Never carries a state across a session/data gap."""
    active = False
    pending = None
    previous = None
    result = []
    for s, flag in zip(samples, flags):
        if previous is None or s.session != previous.session or s.at - previous.at > 2 * MINUTE or not s.down:
            active, pending = False, None
        if flag == active:
            pending = None
        else:
            pending = s.at if pending is None else pending
            dwell = enter_minutes if flag else exit_minutes
            if s.at - pending >= dwell * MINUTE:
                active, pending = flag, None
        result.append(active and s.down)
        previous = s
    return result


def evaluate(samples, openings, probabilities, flags):
    import numpy as np
    from sklearn.metrics import average_precision_score, balanced_accuracy_score, roc_auc_score
    usable = [i for i, s in enumerate(samples) if s.eligible]
    y = np.array([samples[i].label for i in usable], dtype=int)
    p = np.array([probabilities[i] for i in usable])
    pred = np.array([flags[i] for i in usable])
    eligible_spans = merge_spans([(samples[i].at, samples[i].at + ALERT_HORIZON * MINUTE) for i in usable])
    eligible_openings = [t for t in openings if covers_window(eligible_spans, t, t)]
    episodes = []
    previous = None
    last_active = False
    for s, active in zip(samples, flags):
        gap = previous is None or s.session != previous.session or s.at - previous.at > 2 * MINUTE
        if active and (gap or not last_active) and s.eligible:
            episodes.append(s)
        previous, last_active = s, active
    used = set()
    matched = []
    for episode in episodes:
        index = bisect_right(eligible_openings, episode.at)
        while index < len(eligible_openings) and eligible_openings[index] in used:
            index += 1
        if index < len(eligible_openings):
            target = eligible_openings[index]
            if target <= episode.at + ALERT_HORIZON * MINUTE:
                used.add(target)
                matched.append((episode, target))
    tp = len(matched)
    precision = tp / len(episodes) if episodes else 0
    recall = tp / len(eligible_openings) if eligible_openings else 0
    leads = [(t - s.at) / MINUTE for s, t in matched]
    eta_hits = sum(s.eta_min <= lead <= s.eta_max for (s, _), lead in zip(matched, leads))
    return dict(minutes=len(y), openings=len(eligible_openings), alerts=len(episodes),
                hits=tp, false_alerts=len(episodes) - tp, precision=precision, recall=recall,
                f1=2 * precision * recall / (precision + recall) if precision + recall else 0,
                lead_minutes=float(np.median(leads)) if leads else None,
                eta_coverage=eta_hits / tp if tp else None,
                accuracy=float(np.mean(pred == y)), balanced_accuracy=float(balanced_accuracy_score(y, pred)),
                brier=float(np.mean((p - y) ** 2)), base_rate=float(np.mean(y)),
                average_precision=float(average_precision_score(y, p)),
                auc=float(roc_auc_score(y, p)) if len(set(y)) > 1 else None)


FUSION = ["ais", "outbound", "transit", "schedule", "corroboration", "eta_min", "eta_max",
          "has_eta", "scheduled", "blackout", "hour_sin", "hour_cos", "weekend"]
HISTORY = FUSION + ["target_recency"] + ["lift_" + key for key in RIVER]


def candidates():
    from sklearn.ensemble import HistGradientBoostingClassifier, RandomForestClassifier
    from sklearn.linear_model import LogisticRegression
    from sklearn.pipeline import make_pipeline
    from sklearn.preprocessing import StandardScaler
    result = []
    for name, features in (("score", ["score"]), ("fusion", FUSION), ("history", HISTORY)):
        for c in (.01, .1, 1):
            result.append((f"logistic_{name}_{c}", features,
                           make_pipeline(StandardScaler(), LogisticRegression(C=c, max_iter=1000))))
    for leaves in (3, 7):
        result.append((f"boosted_history_{leaves}", HISTORY, HistGradientBoostingClassifier(
            max_leaf_nodes=leaves, max_iter=60, learning_rate=.05, min_samples_leaf=120,
            l2_regularization=10, early_stopping=False, random_state=41)))
    result.append(("forest_history", HISTORY, RandomForestClassifier(
        n_estimators=160, max_depth=4, min_samples_leaf=80, max_features=.8, n_jobs=2, random_state=41)))
    return result


def matrix(samples, features):
    import numpy as np
    return np.array([[s.features[f] for f in features] for s in samples])


def fit_predict(estimator, features, samples, start, stop):
    from sklearn.base import clone
    # Clean lift labels require a subsequent confirming reading. Include that
    # maturity delay as well as the forecast horizon in the split embargo.
    maturity = ALERT_HORIZON * MINUTE + 30_000
    training = [s for s in samples if s.eligible and s.at + maturity < start]
    validation = [replace(s, eligible=s.eligible and s.at + maturity < stop)
                  for s in samples if start <= s.at < stop]
    if len({s.label for s in training}) < 2 or not any(s.eligible for s in validation):
        raise ValueError("insufficient observed outcomes for a chronological split; collect more history or use appropriate split dates")
    model = clone(estimator)
    model.fit(matrix(training, features), [s.label for s in training])
    p = model.predict_proba(matrix(validation, features))[:, 1]
    return model, validation, p


def main():
    import numpy as np
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--test-start", default="2026-09-02")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    samples, openings, coverage, intervals = read_dataset(args.database)
    if not samples:
        parser.error("no forecast history available for model evaluation")
    test_start = at_time(args.test_start)
    folds = [(at_time(a), at_time(b)) for a, b in
             (("2026-08-26", "2026-08-28"), ("2026-08-28", "2026-08-30"),
              ("2026-08-30", args.test_start))]
    # Fixed before inspecting holdout outcomes: optimize equal weight for
    # episode precision and recall, with Brier score as a tie-breaker.
    thresholds = [(t, max(.20, t - .15)) for t in (.40, .45, .50, .55, .60, .65, .70, .75)]
    rankings = []
    configs = candidates()
    for name, features, estimator in configs:
        evaluations = []
        for start, stop in folds:
            _, rows, p = fit_predict(estimator, features, samples, start, stop)
            evaluations.append((rows, p))
        sweep = []
        for enter, exit in thresholds:
            metrics = [evaluate(rows, openings, p, alert_flags(rows, p, enter, exit))
                       for rows, p in evaluations]
            sweep.append((float(np.mean([m["f1"] for m in metrics])), enter, exit, metrics))
        best = max(sweep, key=lambda row: row[0])
        result = dict(name=name, features=features, validation_f1=best[0], enter=best[1], exit=best[2],
                      validation_brier=float(np.mean([m["brier"] for m in best[3]])), folds=best[3])
        rankings.append(result)
        print(json.dumps({k: v for k, v in result.items() if k not in ("folds", "features")}), flush=True)
    rankings.sort(key=lambda r: (-r["validation_f1"], r["validation_brier"]))
    winner = rankings[0]
    # Write the selection before opening the held-out evaluation.
    (args.output / "selection.json").write_text(json.dumps(rankings, indent=2) + "\n")
    name, features, estimator = next(c for c in configs if c[0] == winner["name"])
    model, test, p = fit_predict(estimator, features, samples, test_start, samples[-1].at + 1)
    flags = alert_flags(test, p, winner["enter"], winner["exit"])
    before = evaluate(test, openings, [s.score for s in test], alert_flags(test))
    after = evaluate(test, openings, p, flags)
    base = float(np.mean([s.label for s in samples if s.eligible and s.at + 30 * MINUTE + 30_000 < test_start]))
    baseline = evaluate(test, openings, np.full(len(test), base), [False] * len(test))
    report = dict(snapshot_sha256=hashlib.sha256(args.database.read_bytes()).hexdigest(),
                  test_start=args.test_start, counts=dict(samples=len(samples), clean_openings=len(openings),
                  confirmed_hours=sum(b-a for a,b in coverage)/3_600_000), selection=winner,
                  before=before, after=after, no_alert_base_rate=baseline)
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    if name.startswith("logistic"):
        scaler, logistic = model.steps[0][1], model.steps[1][1]
        coefficients = logistic.coef_[0] / scaler.scale_
        intercept = float(logistic.intercept_[0] - coefficients @ scaler.mean_)
        artifact = dict(features=features, coefficients=coefficients.tolist(), intercept=intercept,
                        enter=winner["enter"], exit=winner["exit"], trained_before=args.test_start)
        (args.output / "model.json").write_text(json.dumps(artifact, indent=2) + "\n")
    print(json.dumps(report, indent=2), flush=True)


if __name__ == "__main__":
    main()
