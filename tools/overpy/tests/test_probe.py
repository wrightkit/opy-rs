import json
import sys
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_DIR))

import probe_builtins  # noqa: E402


def finding(status: str, probe_id: str) -> dict:
    return {"id": probe_id, "status": status, "detail": ""}


class ProbeClassificationTests(unittest.TestCase):
    GAPS = [
        {"id": "omission", "status": "native-rejects", "functions": ["createBeam"], "variant": ":omit-arg"},
        {"id": "folds", "status": "different", "functions": ["min", "max"]},
    ]

    def test_a_finding_belongs_to_the_first_matching_gap(self):
        explained, unexplained, stale = probe_builtins.classify(
            [
                finding("native-rejects", "default:createBeam:omit-arg4"),
                finding("different", "size:min:arg0=false"),
            ],
            self.GAPS,
        )
        self.assertEqual(len(explained["omission"]), 1)
        self.assertEqual(len(explained["folds"]), 1)
        self.assertEqual((unexplained, stale), ([], []))

    def test_an_unmatched_finding_is_unexplained_and_an_unused_gap_is_stale(self):
        _, unexplained, stale = probe_builtins.classify(
            [finding("different", "default:sqrt:base")], self.GAPS
        )
        self.assertEqual([f["id"] for f in unexplained], ["default:sqrt:base"])
        self.assertEqual(stale, ["omission", "folds"])

    def test_an_unrelated_rejection_stays_unexplained(self):
        _, unexplained, _ = probe_builtins.classify(
            [
                finding("native-rejects", "default:sqrt:base"),
                finding("native-rejects", "default:sqrt:omit-arg0"),
            ],
            self.GAPS,
        )
        self.assertEqual(len(unexplained), 2)

    def test_recorded_gaps_are_well_formed(self):
        gaps = json.loads(probe_builtins.GAPS.read_text(encoding="utf-8"))["gaps"]
        for gap in gaps:
            self.assertTrue({"id", "status", "functions", "cause", "owner", "decision"} <= gap.keys())
            self.assertIsInstance(gap["functions"], list)


if __name__ == "__main__":
    unittest.main()
