"""Machine-readable support-matrix consistency checks (issue #2).

The support matrix (compatibility/support-matrix.json) is the mechanically
checkable artifact behind docs/opy/support-matrix.md. These checks enforce:

* the envelope: schemaVersion, pinned reference identity, declared states and
  categories;
* every feature has a unique id, a declared state, a declared category,
  evidence, and notes;
* every `fixtures:` evidence path exists in the corpus (relative to
  compatibility/fixtures); and
* the embedded summary counts match the feature array (self-reporting
  artifact).
"""

import json
import sys
import unittest
from pathlib import Path


COMPATIBILITY_DIR = Path(__file__).resolve().parents[1]
MATRIX_PATH = COMPATIBILITY_DIR / "support-matrix.json"
FIXTURES_DIR = COMPATIBILITY_DIR / "fixtures"

STATES = {
    "planned",
    "frontend-supported",
    "semantic-supported",
    "lowering-dependent",
    "end-to-end-supported",
}
CATEGORIES = {
    "syntax",
    "semantics",
    "preprocessing",
    "macros",
    "directives",
    "translations",
    "optimization",
    "runtime",
    "compilation",
    "decompilation",
}

REFERENCE = {
    "name": "overpy",
    "version": "9.7.10",
    "contentCommit": "889d9749d1def17f146548cbddb94ea1ab015847",
    "integrity": "sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw==",
}


class SupportMatrixTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.matrix = json.loads(MATRIX_PATH.read_text(encoding="utf-8"))
        cls.features = cls.matrix["features"]

    def test_envelope_identity(self):
        self.assertEqual(self.matrix["schemaVersion"], 1)
        self.assertEqual(self.matrix["reference"], REFERENCE)
        self.assertEqual(set(self.matrix["states"]), STATES)
        self.assertEqual(set(self.matrix["categories"]), CATEGORIES)

    def test_features_have_unique_ids_and_declared_fields(self):
        ids = [feature["id"] for feature in self.features]
        self.assertEqual(len(ids), len(set(ids)), "feature ids must be unique")
        for feature in self.features:
            self.assertIn(feature["state"], STATES, feature["id"])
            self.assertIn(feature["category"], CATEGORIES, feature["id"])
            self.assertTrue(feature["name"])
            self.assertIsInstance(feature["evidence"], list)
            self.assertTrue(feature["evidence"], feature["id"])
            self.assertTrue(feature["notes"], feature["id"])

    def test_fixture_evidence_paths_exist(self):
        referenced = set()
        for feature in self.features:
            for item in feature["evidence"]:
                if item.startswith("fixtures:"):
                    referenced.add(item.removeprefix("fixtures:"))
        for relative in sorted(referenced):
            path = FIXTURES_DIR / relative
            self.assertTrue(
                path.exists(),
                f"fixture evidence path does not exist: {relative}",
            )

    def test_summary_matches_features(self):
        by_state = {state: 0 for state in sorted(STATES)}
        by_category = {category: 0 for category in sorted(CATEGORIES)}
        for feature in self.features:
            by_state[feature["state"]] += 1
            by_category[feature["category"]] += 1
        self.assertEqual(self.matrix["summary"]["byState"], by_state)
        self.assertEqual(self.matrix["summary"]["byCategory"], by_category)
        self.assertEqual(
            sum(self.matrix["summary"]["byState"].values()),
            len(self.features),
        )

    def test_semantic_ownership_split_is_explicit(self):
        by_id = {feature["id"]: feature for feature in self.features}
        for overlay in (
            "semantics/builtin-actions-values",
            "semantics/receiver-members",
            "semantics/enum-domains",
        ):
            self.assertEqual(by_id[overlay]["state"], "semantic-supported")
        for catalog in (
            "semantics/workshop-builtin-catalog",
            "semantics/workshop-receiver-catalog",
            "semantics/workshop-enum-domains",
        ):
            self.assertEqual(by_id[catalog]["state"], "lowering-dependent")
        self.assertEqual(
            by_id["semantics/receiver-playervar"]["state"], "semantic-supported"
        )


if __name__ == "__main__":
    unittest.main()
