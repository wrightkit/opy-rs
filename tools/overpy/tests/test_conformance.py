import copy
import sys
import tempfile
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_DIR))

import conformance  # noqa: E402


class ConformanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest = conformance.load_manifest()
        cls.audit = conformance.load_pinned_audit()

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

    def test_feature_contract_inventory_is_leaf_explicit(self):
        inventory = conformance.load_inventory()
        leaves = conformance.validate_inventory(
            inventory, self.manifest, audit=self.audit
        )
        self.assertEqual(
            {registry["id"] for registry in inventory["registries"]},
            set(inventory["requiredRegistries"]),
        )
        self.assertEqual(
            {branch["id"] for branch in inventory["branches"]},
            set(inventory["requiredBranches"]),
        )
        self.assertEqual(len(leaves), len(conformance.inventory_leaves(inventory)))
        self.assertIn("unclear", {leaf["status"] for leaf in leaves})
        self.assertIn("unsupported", {leaf["status"] for leaf in leaves})

    def test_feature_contract_inventory_requires_evidence_for_covered_claims(self):
        inventory = conformance.load_inventory()
        broken = copy.deepcopy(inventory)
        broken["branches"][0]["evidence"] = []
        with self.assertRaises(conformance.ConformanceError):
            conformance.validate_inventory(broken, self.manifest, audit=self.audit)

    def test_feature_contract_inventory_requires_every_audited_branch(self):
        inventory = conformance.load_inventory()
        broken = copy.deepcopy(inventory)
        broken["branches"].pop()
        broken["requiredBranches"].pop()
        with self.assertRaises(conformance.ConformanceError):
            conformance.validate_inventory(broken, self.manifest, audit=self.audit)

    def test_feature_contract_inventory_requires_every_audited_registry(self):
        inventory = conformance.load_inventory()
        broken = copy.deepcopy(inventory)
        broken["registries"].pop()
        broken["requiredRegistries"].pop()
        with self.assertRaises(conformance.ConformanceError):
            conformance.validate_inventory(broken, self.manifest, audit=self.audit)

    def test_feature_contract_inventory_detects_pinned_registry_drift(self):
        inventory = {
            "reference": self.manifest["reference"],
            "requiredRegistries": ["registry/test"],
            "requiredBranches": ["branch/test"],
            "registries": [
                {
                    "id": "registry/test",
                    "kind": "registry",
                    "authority": "OverPy@9.7.10:src/data/opy/keywords.ts#opyKeywords",
                    "source": {"path": "src/data/opy/keywords.ts", "objectPath": "opyKeywords"},
                    "contract": "syntax.parser-and-control-flow/statement-and-declaration-boundaries",
                    "keys": ["and", "or"],
                    "defaults": {
                        "status": "unclear",
                        "coverage": "uncovered",
                        "limits": "test inventory",
                    },
                }
            ],
            "branches": [
                {
                    "id": "branch/test",
                    "kind": "compiler-branch",
                    "authority": "OverPy@9.7.10:src/compiler/parser.ts",
                    "contract": "syntax.parser-and-control-flow/statement-and-declaration-boundaries",
                    "status": "partial",
                    "coverage": "uncovered",
                    "limits": "test branch",
                }
            ],
        }
        audit = {
            "reference": self.manifest["reference"],
            "registries": [
                {
                    "id": "registry/test",
                    "source": {
                        "path": "src/data/opy/keywords.ts",
                        "objectPath": "opyKeywords",
                    },
                }
            ],
            "branches": [{"id": "branch/test", "source": "src/compiler/parser.ts"}],
        }
        with tempfile.TemporaryDirectory(dir=TOOLS_DIR) as directory:
            source = Path(directory) / "src/data/opy/keywords.ts"
            source.parent.mkdir(parents=True)
            source.write_text('export const opyKeywords = {"and": 1, "or": 1};\n', encoding="utf-8")
            branch_source = Path(directory) / "src/compiler/parser.ts"
            branch_source.parent.mkdir(parents=True)
            branch_source.write_text("// pinned branch source\n", encoding="utf-8")
            conformance.validate_inventory(
                inventory,
                self.manifest,
                upstream_root=Path(directory),
                audit=audit,
            )
            source.write_text(
                'export const opyKeywords = {"and": 1, "or": 1, "new": 1};\n',
                encoding="utf-8",
            )
            with self.assertRaises(conformance.ConformanceError):
                conformance.validate_inventory(
                    inventory,
                    self.manifest,
                    upstream_root=Path(directory),
                    audit=audit,
                )

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

    def test_native_frontier_normalizes_lambda_context_to_parse_frontier(self):
        result = {
            "compile": {
                "status": "failure",
                "failureClass": "frontend",
                "diagnostics": [{"code": "lambda-context"}],
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

    def test_semantic_oracle_metadata_selects_the_canonical_reference(self):
        fixture = TOOLS_DIR.parents[1] / "crates/opy-rs/tests/fixtures/corpus/synthetic/switch-multiple-break"
        metadata = conformance.load_json(fixture / "fixture.json")
        self.assertEqual(
            conformance.semantic_oracle_path(fixture, metadata),
            fixture / "semantic-oracle.json",
        )
        semantic_oracle = conformance.load_json(
            conformance.semantic_oracle_path(fixture, metadata)
        )
        pinned_oracle = conformance.load_json(fixture / "oracle.json")
        self.assertNotEqual(
            semantic_oracle["compile"]["workshop"],
            pinned_oracle["compile"]["workshop"],
        )
        self.assertEqual(
            semantic_oracle["input"]["sha256"],
            pinned_oracle["input"]["sha256"],
        )

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
