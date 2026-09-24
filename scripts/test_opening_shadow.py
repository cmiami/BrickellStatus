import json
import unittest
from datetime import date
from pathlib import Path

import opening_shadow as shadow

FIXTURE = Path(__file__).resolve().parents[1] / "crates/runtime/fixtures/opening_shadow_parity.json"
ARTIFACT = Path(__file__).resolve().parents[1] / "crates/runtime/models/opening_shadow_v1.json"


class OpeningShadowTests(unittest.TestCase):
    def setUp(self):
        self.fixture = json.loads(FIXTURE.read_text())

    def test_features_and_probability_match_the_fixture_rust_also_checks(self):
        for case in self.fixture["cases"]:
            inputs = dict(case["inputs"])
            inputs["history"] = {k: tuple(v) for k, v in inputs["history"].items()}
            values = shadow.features(inputs, self.fixture["board_windows"])
            self.assertEqual(values, case["expected_features"])
            self.assertAlmostEqual(shadow.probability(values, self.fixture["model"]),
                                   case["expected_probability"], places=10)

    def test_schedule_context_matches_the_fixture_rust_also_checks(self):
        for case in self.fixture["schedule_cases"]:
            expected = {k: case[k] for k in ("mode", "local_hour", "seconds_to_slot")}
            self.assertEqual(shadow.schedule_context(case["now_ms"]), expected)

    def test_federal_holidays_follow_the_bridge_schedule(self):
        self.assertTrue(shadow.federal_holiday(date(2026, 9, 7)))  # Labor Day
        self.assertTrue(shadow.federal_holiday(date(2026, 7, 3)))  # July 4 observed
        self.assertTrue(shadow.federal_holiday(date(2027, 12, 31)))  # 2028 New Year observed
        self.assertFalse(shadow.federal_holiday(date(2026, 9, 8)))

    def test_a_fix_at_or_after_the_forecast_minute_is_never_used(self):
        case = self.fixture["cases"][0]["inputs"]
        now = case["now_ms"]
        inputs = dict(case, fixes=[
            dict(mmsi="123456789", at=now - 120_000, sog=4.0, branch="river", s=1700.0, posture="underway"),
            dict(mmsi="123456789", at=now, sog=4.0, branch="river", s=1500.0, posture="underway"),
        ], history={}, up_starts=[], board=[])
        self.assertEqual(shadow.features(inputs, {})["ais_closing"], 0.0)

    def test_the_shipped_artifact_names_only_known_features(self):
        artifact = json.loads(ARTIFACT.read_text())
        self.assertEqual(artifact["features"], shadow.FEATURES)
        self.assertEqual(len(artifact["coefficients"]), len(shadow.FEATURES))
        self.assertLess(artifact["exit"], artifact["enter"])
        self.assertIn("shadow", artifact["mode"])


if __name__ == "__main__":
    unittest.main()
