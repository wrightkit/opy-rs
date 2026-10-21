import json
import subprocess
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from threading import Barrier
from unittest.mock import patch


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
        explained, unexplained, generated, stale = probe_builtins.classify(
            [
                finding("native-rejects", "default:createBeam:omit-arg4"),
                finding("different", "size:min:arg0=false"),
            ],
            self.GAPS,
        )
        self.assertEqual(len(explained["omission"]), 1)
        self.assertEqual(len(explained["folds"]), 1)
        self.assertEqual((unexplained, generated, stale), ([], [], []))

    def test_an_unmatched_finding_is_unexplained_and_an_unused_gap_is_stale(self):
        _, unexplained, _, stale = probe_builtins.classify(
            [finding("different", "default:sqrt:base")], self.GAPS
        )
        self.assertEqual([f["id"] for f in unexplained], ["default:sqrt:base"])
        self.assertEqual(stale, ["omission", "folds"])

    def test_an_unrelated_rejection_stays_unexplained(self):
        _, unexplained, _, _ = probe_builtins.classify(
            [
                finding("native-rejects", "default:sqrt:base"),
                finding("native-rejects", "default:sqrt:omit-arg0"),
            ],
            self.GAPS,
        )
        self.assertEqual(len(unexplained), 2)

    def test_native_accepts_on_a_canonical_probe_is_unexplained(self):
        _, unexplained, generated, _ = probe_builtins.classify(
            [finding("native-accepts", "default:healer:base")], self.GAPS
        )
        self.assertEqual([f["id"] for f in unexplained], ["default:healer:base"])
        self.assertEqual(generated, [])

    def test_native_accepts_on_generated_candidates_is_probe_input(self):
        _, unexplained, generated, _ = probe_builtins.classify(
            [
                finding("native-accepts", "default:wait:arg0=null"),
                finding("native-accepts", "default:createBeam:omit-arg4"),
                finding("native-accepts", "size:min:in-boolean"),
                finding("native-accepts", "default:f:in-replacement"),
            ],
            self.GAPS,
        )
        self.assertEqual(unexplained, [])
        self.assertEqual(len(generated), 4)

    def test_a_recorded_gap_can_explain_a_native_accepts_finding(self):
        gaps = self.GAPS + [
            {
                "id": "accepts",
                "status": "native-accepts",
                "functions": ["healer"],
                "variant": ":base",
            }
        ]
        explained, unexplained, generated, _ = probe_builtins.classify(
            [finding("native-accepts", "default:healer:base")], gaps
        )
        self.assertEqual(len(explained["accepts"]), 1)
        self.assertEqual(unexplained, [])
        self.assertEqual(generated, [])

    def test_a_gap_detail_can_discriminate_on_the_oracle_rejection(self):
        gaps = self.GAPS + [
            {
                "id": "accepts",
                "status": "native-accepts",
                "functions": ["createInWorldText"],
                "variant": ":base",
                "detail": "DO_NOT_CLIP",
            }
        ]
        item = finding("native-accepts", "default:createInWorldText:base")
        item["referenceError"] = "OpyError: Unknown member 'DO_NOT_CLIP' of 'Clipping'"
        explained, unexplained, _, _ = probe_builtins.classify([item], gaps)
        self.assertEqual(len(explained["accepts"]), 1)
        self.assertEqual(unexplained, [])

        item["referenceError"] = "OpyError: Unknown member 'CLIP' of 'Clipping'"
        _, unexplained, _, stale = probe_builtins.classify([item], gaps)
        self.assertEqual(len(unexplained), 1)
        self.assertEqual(stale, ["omission", "folds", "accepts"])

    def test_a_gap_must_discriminate_beyond_status_and_functions(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "gaps.json"
            path.write_text(
                json.dumps(
                    {
                        "gaps": [
                            {
                                "id": "broad",
                                "status": "native-accepts",
                                "functions": ["healer"],
                            }
                        ]
                    }
                )
            )
            with self.assertRaises(RuntimeError):
                probe_builtins.load_gaps(path)

    def test_recorded_gaps_are_well_formed(self):
        gaps = probe_builtins.load_gaps()
        for gap in gaps:
            self.assertTrue({"id", "status", "functions", "cause", "owner", "decision"} <= gap.keys())
            self.assertIsInstance(gap["functions"], list)


class ProbeExecutionTests(unittest.TestCase):
    def test_parallel_execution_preserves_every_probe_and_report_order(self):
        selected = [
            {"id": f"default:f{i}:base", "source": f"g = f{i}()"}
            for i in range(7)
        ]
        barrier = None
        seen = []

        def run(command, **kwargs):
            if command[0] == "node":
                probes = json.loads(Path(command[-2]).read_text())
                if barrier is not None:
                    barrier.wait(timeout=5)
                Path(command[-1]).write_text(json.dumps({p["id"]: p["source"] for p in probes}))
                return subprocess.CompletedProcess(command, 0, "", "")
            probes = json.loads(Path(command[-2]).read_text())
            references = json.loads(Path(command[-1]).read_text())
            self.assertEqual(references, {p["id"]: p["source"] for p in probes})
            seen.extend(p["id"] for p in probes)
            report = {
                "probes": len(probes),
                "counts": {"different": len(probes)},
                "findings": [finding("different", p["id"]) for p in probes],
            }
            return subprocess.CompletedProcess(command, 0, json.dumps(report), "")

        with patch.object(probe_builtins, "run", side_effect=run):
            serial = probe_builtins.compare_probes(Path("native"), selected, 1)
            self.assertEqual(Counter(seen), Counter(p["id"] for p in selected))
            for jobs in (2, len(selected) + 1):
                seen.clear()
                barrier = Barrier(min(jobs, len(selected)))
                parallel = probe_builtins.compare_probes(Path("native"), selected, jobs)
                self.assertEqual(parallel, serial)
                self.assertEqual(Counter(seen), Counter(p["id"] for p in selected))

    def test_worker_failure_is_not_a_partial_success(self):
        selected = [{"id": "default:f:base", "source": "g = f()"}]
        for stage in ("oracle", "native"):
            with self.subTest(stage=stage):
                results = [subprocess.CompletedProcess([], 2, "", "worker failed")]
                if stage == "native":
                    results.insert(0, subprocess.CompletedProcess([], 0, "", ""))
                with patch.object(probe_builtins, "run", side_effect=results):
                    with self.assertRaisesRegex(RuntimeError, "worker failed"):
                        probe_builtins.compare_probes(Path("native"), selected, 2)

    def test_empty_selection_has_an_empty_report(self):
        self.assertEqual(
            probe_builtins.compare_probes(Path("native"), [], 2),
            {"probes": 0, "counts": {}, "findings": []},
        )

    def test_reference_rejections_stay_attached_to_findings(self):
        def run(command, **kwargs):
            if command[0] == "node":
                Path(command[-1]).write_text(
                    json.dumps(
                        {
                            "default:f:base": {
                                "ok": False,
                                "error": "OpyError: Unknown function 'f'",
                            },
                            "default:g:base": {"ok": True, "workshop": "x"},
                        }
                    )
                )
                return subprocess.CompletedProcess(command, 0, "", "")
            report = {
                "probes": 2,
                "counts": {"native-accepts": 1, "match": 1},
                "findings": [
                    {"id": "default:f:base", "status": "native-accepts", "detail": ""},
                ],
            }
            return subprocess.CompletedProcess(command, 0, json.dumps(report), "")

        selected = [
            {"id": "default:f:base", "source": "g = f()"},
            {"id": "default:g:base", "source": "g = g()"},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(probe_builtins, "run", side_effect=run):
                report = probe_builtins.compare_batch(
                    Path("native"), selected, Path(temporary) / "batch"
                )
        self.assertEqual(
            report["findings"][0]["referenceError"],
            "OpyError: Unknown function 'f'",
        )


if __name__ == "__main__":
    unittest.main()
