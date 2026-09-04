import math
import sqlite3
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path

from audit_bridge_model import MINUTE, Sample, alert_flags, past_age, read_dataset, stable_alerts


class ModelAuditTests(unittest.TestCase):
    def test_future_lifts_do_not_become_predictive_features(self):
        self.assertEqual(past_age([20 * MINUTE], 10 * MINUTE), 120)
        self.assertEqual(past_age([5 * MINUTE, 20 * MINUTE], 10 * MINUTE), 5)

    def test_brief_unconfirmed_up_reading_is_not_a_negative_training_label(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "history.sqlite3"
            c = sqlite3.connect(path)
            c.executescript((Path(__file__).resolve().parents[1] / "crates/storage/schema.sql").read_text())
            for state, start, end in [("down", 0, 10 * MINUTE), ("up", 10 * MINUTE, 10 * MINUTE + 15_000), ("down", 10 * MINUTE + 15_000, 60 * MINUTE)]:
                c.execute("INSERT INTO bridge_state_intervals VALUES (?,?,?,?,?,?,?,?,?,?)",
                          ("fl511", "brickell", "Brickell", "target", state, start, end, end, "state_change", "a"))
            for at in (5 * MINUTE, 20 * MINUTE):
                c.execute("INSERT INTO bridge_forecast_samples VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                          ("brickell", at, at, "brickell-v5", "clear", 0, 0, None, None,
                           "on_signal", "{}", "{}", "a"))
            c.commit()
            c.close()
            rows, openings, *_ = read_dataset(path)
            self.assertEqual(openings, [])
            self.assertFalse(rows[0].eligible)
            self.assertTrue(rows[1].eligible)
            self.assertFalse(rows[1].label)
            self.assertAlmostEqual(rows[0].features["target_recency"], math.exp(-4))

    def test_alert_dwell_never_crosses_session_or_missing_observation_gaps(self):
        base = Sample(0, "a", "brickell-v5", "likely", .8, 2, 10, "on_signal", {}, {}, True, True, True, {})
        rows = [base, replace(base, at=MINUTE), replace(base, at=2*MINUTE, session="b"), replace(base, at=6*MINUTE, session="b")]
        self.assertEqual(stable_alerts(rows, alert_flags(rows), 1, 0), [False, True, False, False])


if __name__ == "__main__":
    unittest.main()
