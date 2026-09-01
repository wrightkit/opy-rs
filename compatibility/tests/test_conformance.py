import copy
import sys
import unittest
from pathlib import Path


COMPATIBILITY_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(COMPATIBILITY_DIR))

import conformance  # noqa: E402


class ConformanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest = conformance.load_manifest()

    def test_inventory_covers_the_corpus_and_audits_failures(self):
        conformance.validate_manifest(self.manifest)
        self.assertTrue(
            all(category["owner"] for category in self.manifest["categories"])
        )
        declared = [
            fixture
            for category in self.manifest["categories"]
            for fixture in category["probeFixtures"]
        ]
        self.assertEqual(set(declared), conformance.fixture_ids())

    def test_contract_inventory_requires_executable_evidence(self):
        conformance.validate_manifest(self.manifest)
        kinds = {
            kind
            for category in self.manifest["categories"]
            for contract in category["contracts"]
            for kind in contract["probeKinds"]
        }
        self.assertEqual(kinds, conformance.PROBE_KINDS)

        broken = copy.deepcopy(self.manifest)
        broken["categories"][0]["contracts"][0]["probes"] = []
        with self.assertRaises(conformance.ConformanceError):
            conformance.validate_manifest(broken)

    def test_native_frontier_uses_failure_class_without_hiding_stage(self):
        result = {
            "compile": {
                "status": "failure",
                "failureClass": "integration",
                "diagnostics": [{"code": "unsupported-integration-surface"}],
            }
        }
        self.assertEqual(
            conformance.native_frontier(result),
            {"stage": "lowering", "construct": "unsupported-integration-surface"},
        )

    def test_native_frontier_skips_warnings_before_the_first_error(self):
        result = {
            "compile": {
                "status": "failure",
                "failureClass": "frontend",
                "diagnostics": [
                    {"severity": "warning", "code": "w_already_imported"},
                    {"severity": "error", "code": "parse-error"},
                ],
            }
        }
        self.assertEqual(
            conformance.native_frontier(result),
            {"stage": "parse", "construct": "parse-error"},
        )

    def test_reference_success_native_failure_is_divergence(self):
        oracle = {
            "compile": {"status": "success", "diagnostics": []},
        }
        native = {
            "fixture": "synthetic/basic-rule",
            "compile": {
                "status": "failure",
                "failureClass": "frontend",
                "diagnostics": [{"code": "parse-error"}],
            },
        }
        result = conformance.compare_case(
            oracle, native, None, conformance.native_frontier(native), None
        )
        self.assertEqual(result["status"], "divergence")

    def test_success_requires_canonical_wir_match(self):
        oracle = {"compile": {"status": "success", "diagnostics": []}}
        native = {
            "fixture": "synthetic/basic-rule",
            "compile": {"status": "success", "diagnostics": []},
        }
        semantic = {"status": "divergence", "evidence": {"equivalent": False}}
        result = conformance.compare_case(oracle, native, None, None, semantic)
        self.assertEqual(result["status"], "divergence")

    def test_reference_failure_frontier_difference_is_divergence(self):
        oracle = {"compile": {"status": "failure", "diagnostics": []}}
        native = {
            "fixture": "synthetic/diagnostics",
            "compile": {
                "status": "failure",
                "failureClass": "frontend",
                "diagnostics": [{"code": "parse-error"}],
            },
        }
        result = conformance.compare_case(
            oracle,
            native,
            {"stage": "semantic", "construct": "unknown-member"},
            conformance.native_frontier(native),
            None,
        )
        self.assertEqual(result["status"], "divergence")


if __name__ == "__main__":
    unittest.main()
