import copy
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parents[1]
CORPUS_DIR = TOOLS_DIR.parents[1] / "crates/opy-rs/tests/fixtures/corpus"
sys.path.insert(0, str(TOOLS_DIR))

import diff  # noqa: E402
import run_oracle  # noqa: E402


class DiffTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        snapshot = (
            CORPUS_DIR
            / "synthetic"
            / "basic-rule"
            / "oracle.json"
        )
        cls.oracle = json.loads(snapshot.read_text(encoding="utf-8"))

    def test_source_expectations_cover_every_fixture(self):
        expectations = diff.load_expectations()
        fixtures = set(diff.fixture_ids(CORPUS_DIR))
        self.assertEqual(set(expectations), fixtures)
        for fixture, expectation in expectations.items():
            self.assertIn(expectation["relationship"], diff.EXPECTED_CLASSIFICATIONS)
            self.assertNotIn("evidence", expectation)
            self.assertNotIn("note", expectation)

    def test_compiler_expectations_cover_every_fixture(self):
        source = diff.load_expectations()
        compiler = diff.load_compiler_expectations()
        fixtures = set(diff.fixture_ids(CORPUS_DIR))
        self.assertEqual(set(compiler), fixtures)
        self.assertNotIn("comparison", source["synthetic/basic-rule"])
        for fixture, expectation in compiler.items():
            self.assertIn(expectation["comparison"], diff.EXPECTED_COMPILER_COMPARISONS)
            self.assertIn(expectation["relationship"], diff.EXPECTED_CLASSIFICATIONS)
            self.assertNotIn("evidence", expectation)
            self.assertNotIn("owner", expectation)
            self.assertNotIn("note", expectation)

    def write_result(self, root: Path, result: dict):
        path = root / result["fixture"] / "result.json"
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps(result), encoding="utf-8")

    def copy_fixture(self, root: Path, fixture: str) -> Path:
        source = CORPUS_DIR / fixture
        target = root / fixture
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(source, target)
        return target

    def test_matching_result_has_distinct_stages(self):
        result = copy.deepcopy(self.oracle)
        result["semantic"] = {"rules": 1}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_fixture(
                CORPUS_DIR,
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
                CORPUS_DIR,
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
                CORPUS_DIR,
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
                CORPUS_DIR,
                "synthetic/basic-rule",
                root,
                None,
            )
        self.assertEqual(report_result["status"], "unexpected-divergence")
        self.assertTrue(report_result["referenceGap"])

    def test_diagnostic_difference_is_regression(self):
        result = copy.deepcopy(self.oracle)
        result["compile"]["diagnostics"] = [
            {"severity": "error", "text": "Error: different"}
        ]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_fixture(
                CORPUS_DIR,
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
                    CORPUS_DIR,
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
                CORPUS_DIR,
                "synthetic/basic-rule",
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "unexpected-divergence")
        self.assertIn("compile-status", report_result["regressionStages"])

    def test_oracle_failure_frontiers_are_derived_from_snapshots(self):
        frontiers = []
        for oracle_path in sorted(CORPUS_DIR.glob("**/oracle.json")):
            oracle = json.loads(oracle_path.read_text(encoding="utf-8"))
            if oracle["compile"]["status"] == "failure":
                frontiers.append(diff.reference_failure_frontier(oracle))
        self.assertTrue(frontiers)
        self.assertTrue(all(frontiers))

    def test_compiler_failure_frontier_is_compared_independently(self):
        fixture = "synthetic/diagnostics"
        result = copy.deepcopy(
            json.loads(
                (CORPUS_DIR / fixture / "oracle.json").read_text(encoding="utf-8")
            )
        )
        result["compile"]["failureClass"] = "frontend"
        result["compile"]["diagnostics"] = [
            {"severity": "error", "code": "parse-error", "text": "native"}
        ]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                CORPUS_DIR,
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "match")
        frontier = next(
            item for item in report_result["stages"] if item["name"] == "failure-frontier"
        )
        self.assertEqual(frontier["outcome"], "match")

    def test_known_gap_frontier_difference_is_not_a_match(self):
        fixture = "real-world/ow1-emulator"
        result = copy.deepcopy(
            json.loads(
                (CORPUS_DIR / fixture / "oracle.json").read_text(encoding="utf-8")
            )
        )
        result["compile"]["failureClass"] = "integration"
        result["compile"]["diagnostics"] = [
            {
                "severity": "error",
                "code": "unsupported-integration-surface",
                "text": "native",
            }
        ]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                CORPUS_DIR,
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "known-gap")
        frontier = next(
            item for item in report_result["stages"] if item["name"] == "failure-frontier"
        )
        self.assertEqual(frontier["outcome"], "difference")

    def test_compiler_normalized_output_accepts_equivalent_numeric_spelling(self):
        fixture = "synthetic/declarations-numbers"
        oracle = json.loads(
            (CORPUS_DIR / fixture / "oracle.json").read_text(encoding="utf-8")
        )
        result = copy.deepcopy(oracle)
        result["compile"]["workshop"] = result["compile"]["workshop"].replace(
            "0.0", "0", 1
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                CORPUS_DIR,
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "match")
        self.assertEqual(report_result["stages"][1]["outcome"], "match")

    def test_compiler_normalized_output_rejects_numeric_value_change(self):
        fixture = "synthetic/declarations-numbers"
        oracle = json.loads(
            (CORPUS_DIR / fixture / "oracle.json").read_text(encoding="utf-8")
        )
        result = copy.deepcopy(oracle)
        result["compile"]["workshop"] = result["compile"]["workshop"].replace(
            "0.0", "0.5", 1
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                CORPUS_DIR,
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "regression")
        self.assertEqual(report_result["stages"][1]["outcome"], "regression")

    def test_numeric_output_comparison_rejects_non_workshop_spellings(self):
        for left, right in (
            ('Custom String("0.0")', 'Custom String("0")'),
            ("Value(1000)", "Value(1e3)"),
            ("Value(0.5)", "Value(.5)"),
            ("Value(1)", "Value(+1)"),
        ):
            with self.subTest(left=left, right=right):
                self.assertFalse(diff.numeric_output_equivalent(left, right))

    def test_compiler_input_hash_mismatch_is_rejected(self):
        result = copy.deepcopy(self.oracle)
        result["input"]["sha256"] = "different"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            with self.assertRaises(diff.DiffError):
                diff.compare_compiler_fixture(
                    CORPUS_DIR,
                    "synthetic/basic-rule",
                    root,
                    diff.load_compiler_expectations(),
                )

    def test_compile_result_rejects_public_semantic_wir_comparison(self):
        result = copy.deepcopy(self.oracle)
        result["compile"]["semanticWIR"] = {}
        with self.assertRaises(diff.DiffError):
            diff.require_result_shape(result, "compile result")

    def test_compiler_semantic_wir_rejects_stale_direct_comparison(self):
        fixture = "synthetic/expressions-values"
        oracle = json.loads(
            (
                CORPUS_DIR
                / fixture
                / "oracle.json"
            ).read_text(encoding="utf-8")
        )
        result = copy.deepcopy(oracle)
        result["compatibility"] = {
            "schemaVersion": 1,
            "semanticWIR": {
                "schemaVersion": 1,
                "algorithm": "opy-rs::structural-identity",
                "inputSha256": oracle["input"]["sha256"],
                "referenceInputSha256": oracle["input"]["sha256"],
                "equivalent": False,
            },
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                CORPUS_DIR,
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "regression")
        self.assertEqual(
            report_result["stages"][1]["outcome"],
            "regression",
        )

        result["compatibility"]["semanticWIR"]["referenceError"] = "invalid pinned Workshop"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                CORPUS_DIR,
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
                CORPUS_DIR,
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "match")

    def test_compiler_oracle_status_flip_is_not_parity(self):
        # A `match` fixture whose recorded reference rejects what the native
        # side expects to reject — or accepts what it accepts — is an
        # acceptance divergence; a wrong oracle double must not let a
        # contract stage report parity.
        fixture = "synthetic/diagnostics"
        with tempfile.TemporaryDirectory() as temporary:
            corpus = Path(temporary) / "corpus"
            target = self.copy_fixture(corpus, fixture)
            oracle_path = target / "oracle.json"
            oracle = json.loads(oracle_path.read_text(encoding="utf-8"))
            result = copy.deepcopy(oracle)
            oracle["compile"]["status"] = "success"
            oracle["compile"]["exitCode"] = 0
            oracle_path.write_text(json.dumps(oracle), encoding="utf-8")

            result["compile"]["failureClass"] = "frontend"
            result["compile"]["diagnostics"] = [
                {"severity": "error", "code": "parse-error", "text": "native"}
            ]
            results = Path(temporary) / "results"
            self.write_result(results, result)
            report_result = diff.compare_compiler_fixture(
                corpus,
                fixture,
                results,
                diff.load_compiler_expectations(corpus),
            )
        self.assertEqual(report_result["status"], "unexpected-divergence")
        self.assertIn("acceptance-status", report_result["regressionStages"])

    def test_compiler_oracle_rejection_is_not_semantic_parity(self):
        # A comparator that only trusted its own `equivalent` verdict would
        # call this a match; the reference-side failure must stay visible.
        fixture = "synthetic/expressions-values"
        with tempfile.TemporaryDirectory() as temporary:
            corpus = Path(temporary) / "corpus"
            target = self.copy_fixture(corpus, fixture)
            oracle_path = target / "oracle.json"
            oracle = json.loads(oracle_path.read_text(encoding="utf-8"))
            result = copy.deepcopy(oracle)
            result["compatibility"] = {
                "schemaVersion": 1,
                "semanticWIR": {
                    "schemaVersion": 1,
                    "algorithm": "opy-rs::structural-identity",
                    "inputSha256": oracle["input"]["sha256"],
                    "referenceInputSha256": oracle["input"]["sha256"],
                    "equivalent": True,
                    "differences": [],
                },
            }
            oracle["compile"]["status"] = "failure"
            oracle["compile"]["exitCode"] = 1
            oracle["compile"]["diagnostics"] = [
                {"severity": "error", "text": "Error: broken"}
            ]
            oracle["compile"]["workshopExact"] = ""
            oracle["compile"]["workshop"] = ""
            oracle_path.write_text(json.dumps(oracle), encoding="utf-8")

            results = Path(temporary) / "results"
            self.write_result(results, result)
            report_result = diff.compare_compiler_fixture(
                corpus,
                fixture,
                results,
                diff.load_compiler_expectations(corpus),
            )
        self.assertEqual(report_result["status"], "unexpected-divergence")
        self.assertIn("acceptance-status", report_result["regressionStages"])

    def test_semantic_wir_verdict_must_match_its_recorded_differences(self):
        # An `equivalent` verdict over a non-empty difference list is a
        # comparator claiming parity while seeing divergence; a `False`
        # verdict with no recorded difference claims a gap it cannot show.
        fixture = "synthetic/expressions-values"
        oracle = json.loads(
            (CORPUS_DIR / fixture / "oracle.json").read_text(encoding="utf-8")
        )
        base = {
            "schemaVersion": 1,
            "algorithm": "opy-rs::structural-identity",
            "inputSha256": oracle["input"]["sha256"],
            "referenceInputSha256": oracle["input"]["sha256"],
        }
        for equivalent, differences in (
            (True, [{"path": "Rule.actions.[0]"}]),
            (False, []),
        ):
            with self.subTest(equivalent=equivalent, differences=differences):
                result = copy.deepcopy(oracle)
                result["compatibility"] = {
                    "schemaVersion": 1,
                    "semanticWIR": {
                        **base,
                        "equivalent": equivalent,
                        "differences": differences,
                    },
                }
                with tempfile.TemporaryDirectory() as temporary:
                    root = Path(temporary)
                    self.write_result(root, result)
                    report_result = diff.compare_compiler_fixture(
                        CORPUS_DIR,
                        fixture,
                        root,
                        diff.load_compiler_expectations(),
                    )
                self.assertEqual(report_result["status"], "regression")

    def test_a_known_gap_still_needs_recorded_differences(self):
        # `synthetic/preprocessing` records an approved divergence; a
        # comparator that reports non-equivalence without a difference to
        # show must not satisfy the gap.
        fixture = "synthetic/preprocessing"
        oracle = json.loads(
            (CORPUS_DIR / fixture / "oracle.json").read_text(encoding="utf-8")
        )
        result = copy.deepcopy(oracle)
        result["compatibility"] = {
            "schemaVersion": 1,
            "semanticWIR": {
                "schemaVersion": 1,
                "algorithm": "opy-rs::structural-identity",
                "inputSha256": oracle["input"]["sha256"],
                "referenceInputSha256": oracle["input"]["sha256"],
                "equivalent": False,
                "differences": [],
            },
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                CORPUS_DIR,
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "regression")
        self.assertIn("semantic-wir", report_result["regressionStages"])

    def _preprocessing_result(self, oracle: dict, differences: list) -> dict:
        result = copy.deepcopy(oracle)
        result["compatibility"] = {
            "schemaVersion": 1,
            "semanticWIR": {
                "schemaVersion": 1,
                "algorithm": "opy-rs::structural-identity",
                "inputSha256": oracle["input"]["sha256"],
                "referenceInputSha256": oracle["input"]["sha256"],
                "equivalent": False,
                "differences": differences,
            },
        }
        return result

    def test_semantic_wir_gap_pin_matches_its_recorded_differences(self):
        # Re-pinning an approved gap binds it to exactly the differences it
        # recorded: the same list still satisfies the known-gap declaration.
        fixture = "synthetic/preprocessing"
        differences = [{"path": "Rule.actions.[0]", "index": 0}]
        with tempfile.TemporaryDirectory() as temporary:
            corpus = Path(temporary) / "corpus"
            target = self.copy_fixture(corpus, fixture)
            manifest_path = target / "fixture.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["tests"]["compiler"]["semanticDifferenceSha256"] = (
                diff._sha256(
                    json.dumps(differences, sort_keys=True, separators=(",", ":"))
                )
            )
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

            oracle = json.loads(
                (target / "oracle.json").read_text(encoding="utf-8")
            )
            results = Path(temporary) / "results"
            self.write_result(
                results, self._preprocessing_result(oracle, differences)
            )
            report_result = diff.compare_compiler_fixture(
                corpus,
                fixture,
                results,
                diff.load_compiler_expectations(corpus),
            )
        self.assertEqual(report_result["status"], "known-gap")

    def test_semantic_wir_gap_pin_rejects_an_added_divergence(self):
        # The recorded exception covers only the differences it pinned; a
        # second divergence in the same fixture cannot hide behind it.
        fixture = "synthetic/preprocessing"
        oracle = json.loads(
            (CORPUS_DIR / fixture / "oracle.json").read_text(encoding="utf-8")
        )
        result = self._preprocessing_result(
            oracle, [{"path": "Rule.actions.[0]", "index": 0}]
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_result(root, result)
            report_result = diff.compare_compiler_fixture(
                CORPUS_DIR,
                fixture,
                root,
                diff.load_compiler_expectations(),
            )
        self.assertEqual(report_result["status"], "regression")
        self.assertIn("semantic-wir", report_result["regressionStages"])

    def test_semantic_wir_gap_requires_a_difference_pin(self):
        # `semanticEquivalent: false` without the pinned difference list is an
        # unnamed exception and must be rejected at load time; the pin is
        # meaningless on an equivalent fixture or another comparison.
        for edit in ("drop-pin", "pin-equivalent", "pin-other-comparison"):
            with self.subTest(edit=edit):
                with tempfile.TemporaryDirectory() as temporary:
                    corpus = Path(temporary) / "corpus"
                    fixture = (
                        "synthetic/preprocessing"
                        if edit != "pin-other-comparison"
                        else "synthetic/basic-rule"
                    )
                    target = self.copy_fixture(corpus, fixture)
                    manifest_path = target / "fixture.json"
                    manifest = json.loads(
                        manifest_path.read_text(encoding="utf-8")
                    )
                    compiler = manifest["tests"]["compiler"]
                    if edit == "drop-pin":
                        del compiler["semanticDifferenceSha256"]
                    elif edit == "pin-equivalent":
                        compiler["semanticEquivalent"] = True
                    else:
                        compiler["semanticDifferenceSha256"] = "0" * 64
                    manifest_path.write_text(
                        json.dumps(manifest), encoding="utf-8"
                    )
                    with self.assertRaises(diff.DiffError):
                        diff.load_compiler_expectations(corpus)

    def test_compiler_report_declares_compiler_stages(self):
        report = diff.build_compiler_report([])
        self.assertEqual(
            report["comparison"]["stages"],
            [
                "compile-status",
                "failure-frontier",
                "normalized-output",
                "semantic-wir",
                "source-visible",
                "diagnostic-code",
                "compiler-contract",
                "acceptance-status",
            ],
        )

    def test_missing_producer_is_inconclusive(self):
        report_result = diff.compare_fixture(
            CORPUS_DIR,
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
