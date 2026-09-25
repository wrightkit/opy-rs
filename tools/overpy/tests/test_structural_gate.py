import sys
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_DIR))

import structural_gate  # noqa: E402


def difference(name: str, path: str) -> dict:
    return {
        "section": "rules", "index": 3, "name": name, "path": path,
        "native": "a", "reference": "b",
    }


EXCEPTION = {
    "id": "known", "project": "p", "entry": "src/main.opy", "section": "rules",
    "name": '"Rule"', "path": "Rule.actions",
    "upstream": "u", "opyRs": "n", "decision": "d", "pinningTest": "t",
}


class ClassifyTests(unittest.TestCase):
    def test_a_recorded_difference_passes_and_marks_its_exception_used(self):
        recorded, unrecorded, used = structural_gate.classify(
            "p", "src/main.opy", [difference('"Rule"', "Rule.actions")], [EXCEPTION]
        )
        self.assertEqual((len(recorded), unrecorded, used), (1, [], {"known"}))

    def test_a_difference_outside_the_exceptions_is_unrecorded(self):
        for project, entry, item in (
            ("p", "src/main.opy", difference('"Rule"', "Rule.conditions")),
            ("p", "src/main.opy", difference('"Other"', "Rule.actions")),
            ("q", "src/main.opy", difference('"Rule"', "Rule.actions")),
            ("p", "src/other.opy", difference('"Rule"', "Rule.actions")),
        ):
            _, unrecorded, used = structural_gate.classify(project, entry, [item], [EXCEPTION])
            self.assertEqual((unrecorded, used), ([item], set()))

    def test_the_report_names_the_rule_and_path(self):
        text = structural_gate.describe(difference('"Rule"', "Rule.conditions.[1]Condition"))
        self.assertIn('rules[3] "Rule": Rule.conditions.[1]Condition', text)


class ConfigTests(unittest.TestCase):
    def test_the_shipped_config_pins_full_commits_and_complete_exceptions(self):
        config = structural_gate.load_config(structural_gate.CONFIG)
        self.assertTrue(config["projects"])


if __name__ == "__main__":
    unittest.main()
