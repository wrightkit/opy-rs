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

    def test_expectations_cover_every_fixture_with_evidence(self):
        expectations = diff.load_expectations()
        fixtures = set(diff.fixture_ids(COMPATIBILITY_DIR / "fixtures"))
        self.assertEqual(set(expectations), fixtures)
        for fixture, expectation in expectations.items():
            self.assertTrue(expectation["evidence"], fixture)
            self.assertTrue(expectation["note"], fixture)

    def test_compiler_expectations_are_separate_and_cover_every_fixture(self):
        source = diff.load_expectations()
        compiler = diff.load_compiler_expectations()
        fixtures = set(diff.fixture_ids(COMPATIBILITY_DIR / "fixtures"))
        self.assertEqual(set(compiler), fixtures)
        self.assertNotIn("comparison", source["synthetic/basic-rule"])
        for fixture, expectation in compiler.items():
            self.assertIn(expectation["comparison"], diff.EXPECTED_COMPILER_COMPARISONS)
            self.assertTrue(expectation["evidence"], fixture)
            self.assertTrue(expectation["owner"], fixture)
            self.assertTrue(expectation["note"], fixture)

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

    def test_reference_success_native_failure_is_not_match(self):
        result = copy.deepcopy(self.oracle)
        result["compile"]["status"] = "failure"
        result["compile"]["exitCode"] = 1
        result["compile"]["diagnostics"] = [{"severity": "error", "text": "Error: gap"}]
        result["compile"]["workshopExact"] = ""
        result["compile"]["workshop"] = ""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_fixture(
                COMPATIBILITY_DIR / "fixtures",
                "synthetic/basic-rule",
                root,
                None,
            )
        self.assertEqual(report_result["status"], "unexpected-divergence")
        self.assertTrue(report_result["referenceGap"])

    def test_declared_reference_gap_is_reported_as_known_gap(self):
        oracle_path = (
            COMPATIBILITY_DIR
            / "fixtures"
            / "real-world"
            / "overpy-santa"
            / "oracle.json"
        )
        result = json.loads(oracle_path.read_text(encoding="utf-8"))
        result["compile"]["status"] = "failure"
        result["compile"]["exitCode"] = 1
        result["compile"]["diagnostics"] = [{"severity": "error", "text": "Error: do"}]
        result["compile"]["workshopExact"] = ""
        result["compile"]["workshop"] = ""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_fixture(
                COMPATIBILITY_DIR / "fixtures",
                "real-world/overpy-santa",
                root,
                None,
            )
        self.assertEqual(report_result["status"], "known-gap")
        self.assertTrue(report_result["referenceGap"])
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

    def test_compiler_status_mismatch_is_blocking(self):
        result = copy.deepcopy(self.oracle)
        result["compile"]["status"] = "failure"
        result["compile"]["exitCode"] = 1
        result["compile"]["workshopExact"] = ""
        result["compile"]["workshop"] = ""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                COMPATIBILITY_DIR / "fixtures",
                "synthetic/basic-rule",
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "unexpected-divergence")
        self.assertIn("compile-status", report_result["regressionStages"])

    def test_compiler_input_hash_mismatch_is_rejected(self):
        result = copy.deepcopy(self.oracle)
        result["input"]["sha256"] = "different"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            with self.assertRaises(diff.DiffError):
                diff.compare_compiler_fixture(
                    COMPATIBILITY_DIR / "fixtures",
                    "synthetic/basic-rule",
                    root,
                    diff.load_compiler_expectations(),
                )

    def test_compile_result_rejects_public_semantic_wir_evidence(self):
        result = copy.deepcopy(self.oracle)
        result["compile"]["semanticWIR"] = {}
        with self.assertRaises(diff.DiffError):
            diff.require_result_shape(result, "compile result")

    def test_compiler_semantic_wir_consumes_direct_evidence(self):
        fixture = "synthetic/expressions-values"
        oracle = json.loads(
            (
                COMPATIBILITY_DIR
                / "fixtures"
                / fixture
                / "oracle.json"
            ).read_text(encoding="utf-8")
        )
        result = copy.deepcopy(oracle)
        result["compatibility"] = {
            "schemaVersion": 1,
            "semanticWIR": {
                "schemaVersion": 1,
                "algorithm": "workshop-rs::roundtrip::equivalent",
                "inputSha256": oracle["input"]["sha256"],
                "referenceInputSha256": oracle["input"]["sha256"],
                "equivalent": False,
            },
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                COMPATIBILITY_DIR / "fixtures",
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "known-gap")
        self.assertEqual(
            report_result["stages"][1]["outcome"],
            "accepted-gap",
        )

        result["compatibility"]["semanticWIR"]["referenceError"] = "invalid pinned Workshop"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                COMPATIBILITY_DIR / "fixtures",
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "inconclusive")
        self.assertEqual(
            report_result["stages"][1]["outcome"],
            "inconclusive",
        )

        del result["compatibility"]["semanticWIR"]["referenceError"]
        result["compatibility"]["semanticWIR"]["equivalent"] = True
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                COMPATIBILITY_DIR / "fixtures",
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "regression")

    def test_compiler_report_declares_compiler_stages(self):
        report = diff.build_compiler_report([])
        self.assertEqual(
            report["comparison"]["stages"],
            [
                "compile-status",
                "normalized-output",
                "semantic-wir",
                "diagnostic-code",
                "compiler-contract",
            ],
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
