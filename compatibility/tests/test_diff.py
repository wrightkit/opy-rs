import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path


COMPATIBILITY_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(COMPATIBILITY_DIR))

import diff  # noqa: E402
import run_oracle  # noqa: E402


class DiffTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        snapshot = (
            COMPATIBILITY_DIR
            / "fixtures"
            / "synthetic"
            / "basic-rule"
            / "oracle.json"
        )
        cls.oracle = json.loads(snapshot.read_text(encoding="utf-8"))

    def write_result(self, root: Path, result: dict):
        path = root / result["fixture"] / "result.json"
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps(result), encoding="utf-8")

    def test_matching_result_has_distinct_stages(self):
        result = copy.deepcopy(self.oracle)
        result["semantic"] = {"rules": 1}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_fixture(
                COMPATIBILITY_DIR / "fixtures",
                "synthetic/basic-rule",
                root,
                None,
            )
        self.assertEqual(report_result["status"], "inconclusive")
        stages = {item["name"]: item["outcome"] for item in report_result["stages"]}
        self.assertEqual(stages["exact-output"], "match")
        self.assertEqual(stages["normalized-output"], "match")
        self.assertEqual(stages["semantic"], "inconclusive")

    def test_exact_difference_can_still_have_normalized_match(self):
        result = copy.deepcopy(self.oracle)
        exact = result["compile"]["workshopExact"]
        result["compile"]["workshopExact"] = exact.replace("\n", "   \n", 1)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_fixture(
                COMPATIBILITY_DIR / "fixtures",
                "synthetic/basic-rule",
                root,
                None,
            )
        stages = {item["name"]: item["outcome"] for item in report_result["stages"]}
        self.assertEqual(stages["exact-output"], "difference")
        self.assertEqual(stages["normalized-output"], "match")

    def test_normalized_difference_is_regression(self):
        result = copy.deepcopy(self.oracle)
        result["compile"]["workshop"] += "extra\n"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_fixture(
                COMPATIBILITY_DIR / "fixtures",
                "synthetic/basic-rule",
                root,
                None,
            )
        self.assertEqual(report_result["status"], "regression")
        self.assertIn("normalized-output", report_result["regressionStages"])

    def test_diagnostic_difference_is_regression(self):
        result = copy.deepcopy(self.oracle)
        result["compile"]["diagnostics"] = [
            {"severity": "error", "text": "Error: different"}
        ]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_fixture(
                COMPATIBILITY_DIR / "fixtures",
                "synthetic/basic-rule",
                root,
                None,
            )
        self.assertEqual(report_result["status"], "regression")
        self.assertIn("diagnostics", report_result["regressionStages"])

    def test_input_hash_mismatch_is_rejected(self):
        result = copy.deepcopy(self.oracle)
        result["input"]["sha256"] = "different"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            with self.assertRaises(diff.DiffError):
                diff.compare_fixture(
                    COMPATIBILITY_DIR / "fixtures",
                    "synthetic/basic-rule",
                    root,
                    None,
                )

    def test_missing_producer_is_inconclusive(self):
        report_result = diff.compare_fixture(
            COMPATIBILITY_DIR / "fixtures",
            "synthetic/basic-rule",
            None,
            None,
        )
        self.assertEqual(report_result["status"], "inconclusive")

    def test_normalize_text_matches_snapshot_contract(self):
        self.assertEqual(
            run_oracle.normalize_text(self.oracle["compile"]["workshopExact"]),
            self.oracle["compile"]["workshop"],
        )


if __name__ == "__main__":
    unittest.main()
