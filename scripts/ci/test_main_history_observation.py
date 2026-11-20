import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class HistoryObservation(unittest.TestCase):
    def setUp(self):
        self.run_data = dict(id=43944532917, head_sha="a" * 40, event="push",
                             head_branch="main", path=".github/workflows/main-history-event.yml",
                             run_attempt=1)
        self.steps = [
            dict(name=name, conclusion="success") for name in (
                "Classify the delivered push event", "Publish the history-event receipt",
                "Enforce the detector verdict")
        ]
        self.job = dict(run_id=self.run_data["id"], run_attempt=1, name="Classify main push movement",
                        status="completed", conclusion="success", steps=self.steps)
        self.artifacts = dict(artifacts=[dict(
            name=f"main-history-event-receipt-{'a' * 40}-43944532917", expired=False,
            workflow_run=dict(id=self.run_data["id"], head_sha=self.run_data["head_sha"]))])

    def verdict(self, code, label):
        with tempfile.TemporaryDirectory() as directory:
            paths = []
            for name, data in zip(("run", "job", "artifacts"), (self.run_data, self.job, self.artifacts)):
                path = Path(directory, name)
                path.write_text(json.dumps(data))
                paths.append(path)
            result = subprocess.run([sys.executable, Path(__file__).with_name("main_history_observation.py"),
                                     *paths], text=True, capture_output=True)
            self.assertEqual(result.returncode, code, result.stderr)
            self.assertIn(label, result.stdout)

    def test_observed_even_when_detector_reports_red(self):
        self.verdict(0, "observed")
        self.steps[-1]["conclusion"] = self.job["conclusion"] = "failure"
        self.verdict(0, "observed")

    def test_pre_classifier_cancellation_and_runner_never_started(self):
        self.job["conclusion"] = "cancelled"
        self.artifacts["artifacts"].clear()
        for step in self.steps:
            step["conclusion"] = "skipped"
        self.verdict(1, "missing_observation")
        self.job["steps"] = []
        self.verdict(1, "missing_observation")

    def test_absent_or_foreign_receipt(self):
        self.artifacts["artifacts"][0]["workflow_run"]["id"] += 1
        self.verdict(1, "missing_observation")
        self.artifacts["artifacts"].clear()
        self.verdict(1, "missing_observation")

    def test_inapplicable_and_incomplete_metadata(self):
        self.run_data["event"] = "workflow_dispatch"
        self.verdict(0, "inapplicable")
        del self.run_data["event"]
        self.verdict(2, "invalid_evidence")

    def test_cancellation_after_publication(self):
        self.job["conclusion"] = self.steps[-1]["conclusion"] = "cancelled"
        self.verdict(1, "missing_observation")
