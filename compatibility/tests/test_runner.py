import json
import sys
import unittest
from pathlib import Path


COMPATIBILITY_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(COMPATIBILITY_DIR))

import run_oracle  # noqa: E402


class RunnerTests(unittest.TestCase):
    def test_normalize_text_keeps_content_and_removes_presentation_noise(self):
        self.assertEqual(
            run_oracle.normalize_text("one  \r\ntwo\r\n\r\n"),
            "one\ntwo\n",
        )

    def test_normalize_diagnostics_preserves_error_and_location_text(self):
        diagnostics = run_oracle.normalize_diagnostics(
            "Error: broken\n    | line 1, col 1\n\nWarning: check this\n"
        )
        self.assertEqual(
            diagnostics,
            [
                {
                    "severity": "error",
                    "text": "Error: broken\n    | line 1, col 1",
                },
                {"severity": "warning", "text": "Warning: check this"},
            ],
        )

    def test_repository_fixture_metadata_and_snapshots_are_valid(self):
        fixtures = run_oracle.discover_fixtures(
            COMPATIBILITY_DIR / "fixtures"
        )
        self.assertEqual(len(fixtures), 31)
        for fixture_path, fixture in fixtures:
            snapshot = fixture_path.parent / "oracle.json"
            self.assertTrue(snapshot.is_file(), fixture["id"])
            snapshot_data = json.loads(snapshot.read_text(encoding="utf-8"))
            self.assertEqual(snapshot_data["fixture"], fixture["id"])

        real_world = [
            fixture for _, fixture in fixtures if fixture["category"] == "real-world"
        ]
        self.assertGreaterEqual(len(real_world), 6)
        for fixture in real_world:
            self.assertTrue(fixture["provenance"]["redistributable"])
            self.assertEqual(fixture["provenance"]["modifications"], "none")
            self.assertEqual(
                len(fixture["provenance"]["sourceCommit"]), 40,
                fixture["id"],
            )

        overpy_cake = next(
            fixture for _, fixture in fixtures if fixture["id"] == "real-world/overpy-cake"
        )
        self.assertEqual(overpy_cake["provenance"]["kind"], "imported-example")

    def test_real_world_gaps_have_minimized_provenance_linked_regressions(self):
        fixtures = run_oracle.discover_fixtures(COMPATIBILITY_DIR / "fixtures")
        gaps = [
            fixture
            for _, fixture in fixtures
            if fixture["id"]
            in {
                "real-world/overpy-santa",
                "real-world/overpy-cronch",
                "real-world/overpy-broken-weapons",
                "real-world/overpy-client-to-server",
                "real-world/overpy-crosshair",
                "real-world/overpy-inputhud",
                "real-world/overpy-parabola",
            }
        ]
        self.assertEqual(len(gaps), 7)
        for fixture in gaps:
            regressions = fixture.get("regressions")
            self.assertIsInstance(regressions, list, fixture["id"])
            self.assertGreaterEqual(len(regressions), 1, fixture["id"])
            for regression in regressions:
                source = (COMPATIBILITY_DIR / "fixtures" / fixture["id"] / regression["source"]).resolve()
                self.assertTrue(source.is_file(), regression["id"])
                self.assertEqual(regression["derivedFrom"], fixture["source"])
                self.assertEqual(regression["expectedReferenceStatus"], fixture["expectedStatus"])
                self.assertEqual(regression["kind"], "minimized-regression")
                self.assertIn("oracle:", regression["provenance"])

    def test_census_uses_opaque_workshop_owned_feature_ids(self):
        _, census = next(
            fixture
            for fixture in run_oracle.discover_fixtures(COMPATIBILITY_DIR / "fixtures")
            if fixture[1]["id"] == "census/workshop-feature-census"
        )
        self.assertEqual(census["censusContract"]["owner"], "workshop-rs")
        self.assertEqual(census["censusContract"]["status"], "pending")
        self.assertTrue(census["workshopFeatureIds"])
        self.assertTrue(all(isinstance(item, str) for item in census["workshopFeatureIds"]))


if __name__ == "__main__":
    unittest.main()
