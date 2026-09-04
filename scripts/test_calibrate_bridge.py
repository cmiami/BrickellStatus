import contextlib
import io
import tempfile
import unittest
from pathlib import Path

from calibrate_bridge import (
    MINUTE, Forecast, Interval, binomial_tail, connect, covers_window,
    forecast_section, observed_spans, score_forecasts,
)


def interval(start, end, state="down", reason="state_change", session="a"):
    return Interval("fl511", "brickell", "target", state, start, end, end, reason, session)


def forecast(at=0):
    return Forecast(at, at, "brickell-v5", "likely", 8000, 8000, 1, 30, "scheduled")


class CalibrationTests(unittest.TestCase):
    def test_only_observed_state_changes_bridge_the_polling_gap(self):
        rows = [interval(0, MINUTE), interval(2 * MINUTE, 40 * MINUTE, "up")]
        self.assertEqual(observed_spans(rows), [(0, 40 * MINUTE)])
        for boundary in (
            interval(2 * MINUTE, 40 * MINUTE, reason="session_start", session="b"),
            interval(2 * MINUTE, 40 * MINUTE, reason="continuity_gap"),
        ):
            self.assertFalse(covers_window(observed_spans([rows[0], boundary]), 0, 30 * MINUTE))

    def test_an_explicit_unknown_period_cannot_be_counted_as_coverage(self):
        rows = [interval(0, MINUTE), interval(MINUTE, 2 * MINUTE, "unknown"), interval(2 * MINUTE, 40 * MINUTE)]
        spans = observed_spans(rows)
        self.assertFalse(covers_window(spans, 0, 30 * MINUTE))
        self.assertTrue(covers_window(spans, 2 * MINUTE, 40 * MINUTE))
        self.assertFalse(covers_window(spans, 2 * MINUTE, 40 * MINUTE + 1))

    def test_incomplete_outcomes_are_excluded_from_scores(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            forecast_section([forecast()], [], [], [(0, 29 * MINUTE)])
        self.assertIn("excluded 1 samples", output.getvalue())
        self.assertIn("waiting for complete", output.getvalue())
        self.assertNotIn("brickell-v5 raw", output.getvalue())

    def test_opening_is_eligible_through_the_last_forecasts_horizon(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            score_forecasts([forecast()], [25 * MINUTE], [], "fixture")
        columns = output.getvalue().splitlines()[0].split()
        self.assertEqual(columns[1:5], ["1", "100%", "100%", "0"])

    def test_binomial_tail_survives_a_large_history(self):
        self.assertAlmostEqual(binomial_tail(1, 2, 0.5), 0.75)
        self.assertAlmostEqual(binomial_tail(1001, 2001, 0.5), 0.5, places=10)
        self.assertEqual(binomial_tail(1, 20, 0), 0)
        self.assertEqual(binomial_tail(20, 20, 1), 1)

    def test_read_only_connection_handles_reserved_uri_characters(self):
        import sqlite3
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "history?#.sqlite3"
            sqlite3.connect(path).close()
            connection = connect(path)
            try:
                with self.assertRaises(sqlite3.OperationalError):
                    connection.execute("CREATE TABLE forbidden (id INTEGER)")
            finally:
                connection.close()


if __name__ == "__main__":
    unittest.main()
