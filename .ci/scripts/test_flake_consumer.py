#!/usr/bin/env python3
"""Execute the production flake workflow's Bash with scripted Cargo (#19084).

No Rust, network, Git mutations, or third-party Python packages are used.
"""

import ast
import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def workflow_steps():
    # Read only this workflow's block-style steps; fail if its shape changes.
    steps = []
    for line in (ROOT / ".github/workflows/flake-detection.yml").read_text().splitlines():
        if line.startswith("      - "):
            steps.append({"lines": []})
        if steps:
            steps[-1]["lines"].append(line)
    for step in steps:
        lines = step.pop("lines")
        for line in lines:
            for key in ("name", "id", "if"):
                match = re.fullmatch(r"\s+(?:- )?" + key + r": (.*)", line)
                if match:
                    step[key] = match[1]
        if "        run: |" in lines:
            start = lines.index("        run: |") + 1
            body = []
            for line in lines[start:]:
                if line and not line.startswith("          "):
                    break
                body.append(line[10:] if line else "")
            step["run"] = "\n".join(body) + "\n"
    return steps


def condition(expression, values):
    """Evaluate the small actual Actions condition vocabulary used by this job."""
    expression = expression.removeprefix("${{").removesuffix("}}").strip()
    if expression in ("true", "false"):
        return expression == "true"
    expression = expression.replace("&&", " and ").replace("||", " or ")
    expression = expression.replace("always()", "True")
    expression = expression.replace("cancelled()", repr(values.get("cancelled", False)))
    expression = re.sub(r"!(?!=)", "not ", expression).strip()
    expression = re.sub(
        r"steps\.\w+\.(?:outputs\.\w+|outcome)",
        lambda match: repr(values.get(match[0], "")),
        expression,
    )

    def evaluate(node):
        if isinstance(node, ast.Constant):
            return node.value
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
            return not evaluate(node.operand)
        if isinstance(node, ast.BoolOp):
            items = [bool(evaluate(item)) for item in node.values]
            return all(items) if isinstance(node.op, ast.And) else any(items)
        if isinstance(node, ast.Compare) and len(node.ops) == 1:
            left, right = evaluate(node.left), evaluate(node.comparators[0])
            if isinstance(node.ops[0], ast.Eq):
                return left == right
            if isinstance(node.ops[0], ast.Gt):
                return float(left or 0) > float(right or 0)
        raise AssertionError(f"unsupported workflow condition: {ast.dump(node)}")

    return bool(evaluate(ast.parse(expression, mode="eval").body))


def events(name="real::failure", failed=True, corrupt=False):
    records = [
        {"type": "suite", "event": "started", "test_count": 1},
        {"type": "test", "event": "started", "name": name},
        {"type": "test", "event": "failed" if failed else "ok", "name": name},
        {"type": "suite", "event": "failed" if failed else "ok",
         "passed": 0 if failed else 1, "failed": 1 if failed else 0, "ignored": 0},
    ]
    return [json.dumps(record) for record in records] + (["{broken"] if corrupt else [])


FAKE_CARGO = r'''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
config = json.loads(Path("scenario.json").read_text())
args = sys.argv[1:]
with open("cargo-calls.jsonl", "a") as calls:
    calls.write(json.dumps(args) + "\n")
if args == ["--version"]:
    print("cargo fixture-toolchain")
elif args[0] == "metadata":
    print(json.dumps({"packages": [{"name": run["pkg"], "targets": [{"kind": ["lib"]}]} for run in config["runs"]]}))
elif args[0] == "test":
    if "--exact" in args:
        name = args[args.index("--exact") + 1]
        mode = config.get("replay", "pass")
        if mode == "wrong":
            name = "wrong::identity"
        failed = mode == "failed"
        records = [
            {"type":"suite","event":"started","test_count":1},
            {"type":"test","event":"started","name":name},
            {"type":"test","event":"failed" if failed else "ok","name":name},
            {"type":"suite","event":"failed" if failed else "ok","passed":0 if failed else 1,"failed":1 if failed else 0,"ignored":0},
        ]
        for record in records:
            print(json.dumps(record))
        sys.exit(101 if failed else 0)
    counter = Path("initial-count")
    index = int(counter.read_text()) if counter.exists() else 0
    counter.write_text(str(index + 1))
    run = config["runs"][index]
    assert args[args.index("-p") + 1] == run["pkg"]
    print("\n".join(run["events"]))
    sys.exit(run["status"])
else:
    raise SystemExit("unexpected fixture Cargo command")
'''


class FlakeConsumerTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.work = Path(temporary.name)
        scripts = self.work / ".ci/scripts"
        scripts.mkdir(parents=True)
        for name in ("extract-failed-tests.py", "prepare-flake-replay.py"):
            source = ROOT / ".ci/scripts" / name
            if source.exists():
                shutil.copy2(source, scripts / name)
        bin_dir = self.work / "bin"
        bin_dir.mkdir()
        cargo = bin_dir / "cargo"
        cargo.write_text(FAKE_CARGO)
        cargo.chmod(0o755)
        self.env = dict(os.environ, PATH=str(bin_dir) + os.pathsep + os.environ["PATH"],
                        GITHUB_SHA="fixture-head", TEST_FILTER_INPUT="", PYTHONDONTWRITEBYTECODE="1")
        self.steps = workflow_steps()

    def step(self, **selector):
        matches = [step for step in self.steps if all(step.get(key) == value for key, value in selector.items())]
        self.assertEqual(len(matches), 1, selector)
        return matches[0]

    def run_step(self, step, **extra):
        output = self.work / "outputs"
        output.write_text("")
        result = subprocess.run(["bash", "-c", step["run"]], cwd=self.work,
                                env=dict(self.env, GITHUB_OUTPUT=str(output), **extra),
                                text=True, capture_output=True, timeout=20)
        outputs = dict(line.split("=", 1) for line in output.read_text().splitlines())
        return result, outputs

    def initial(self, runs, replay="pass"):
        (self.work / "initial-count").unlink(missing_ok=True)
        (self.work / "cargo-calls.jsonl").unlink(missing_ok=True)
        (self.work / "scenario.json").write_text(json.dumps({"runs": runs, "replay": replay}))
        return self.run_step(self.step(id="initial_run"))

    def values(self, initial, retry=None):
        values = {"steps.initial_run.outcome": "success"}
        for step, outputs in (("initial_run", initial), ("retry_results", retry or {})):
            values.update({f"steps.{step}.outputs.{key}": value for key, value in outputs.items()})
        values["steps.quarantine.outcome"] = "success"
        return values

    def test_mixed_stream_replays_valid_failure_but_remains_red_without_remote_actions(self):
        result, outputs = self.initial([
            {"pkg": "good", "events": events(), "status": 101},
            {"pkg": "bad", "events": ["{broken"], "status": 0},
        ])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual((self.work / "failed-tests.tsv").read_text(), "good\treal::failure\n")
        self.assertEqual(outputs["aggregate_trusted"], "false")
        self.assertEqual(outputs["observation_exit_code"], "3")
        retry = self.step(id="retry_results")
        self.assertTrue(condition(retry["if"], self.values(outputs)))
        result, retries = self.run_step(retry)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(retries["flake_count"], "1")
        for name in ("Auto-quarantine flaky tests", "Create PR for quarantine changes",
                     "Comment on #2093 with flake report", "Create issue for stable failures"):
            self.assertFalse(condition(self.step(name=name)["if"], self.values(outputs, retries)), name)
        final = self.step(name="Enforce initial observation result")
        self.assertTrue(condition(final["if"], self.values(outputs, retries)))
        result, _ = self.run_step(final,
                                  OBSERVATION_EXIT_CODE=outputs["observation_exit_code"])
        self.assertEqual(result.returncode, 3, result.stdout + result.stderr)

    def test_duplicate_envelopes_bind_failures_by_exact_start_in_both_orders(self):
        good = {"pkg": "same", "events": events("valid::failure") + [
            json.dumps({"message": '{"type":"run"}'}),
            'noise mentioning {"type":"run"}',
        ], "status": 101}
        bad = {"pkg": "same", "events": events("invalid::failure", corrupt=True), "status": 101}
        for runs in ([good, bad], [bad, good]):
            with self.subTest(invalid_first=runs[0] is bad):
                result, outputs = self.initial(runs)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual((self.work / "failed-tests.tsv").read_text(), "same\tvalid::failure\n")
                self.assertEqual(outputs["replay_count"], "1")
                result, retries = self.run_step(self.step(id="retry_results"))
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(retries["flake_count"], "1")
                calls = [json.loads(line) for line in (self.work / "cargo-calls.jsonl").read_text().splitlines()]
                replays = [args for args in calls if "--exact" in args]
                self.assertEqual(len(replays), 1)
                self.assertEqual(replays[0][replays[0].index("--exact") + 1], "valid::failure")

    def test_invalid_block_failure_is_not_replay_eligible(self):
        result, outputs = self.initial([
            {"pkg": "bad", "events": events(corrupt=True), "status": 101},
        ])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(outputs["flake_count"], "1")
        self.assertEqual(outputs["replay_count"], "0")
        self.assertEqual((self.work / "failed-tests.tsv").read_text(), "")
        self.assertFalse(condition(self.step(id="retry_results")["if"], self.values(outputs)))

    def test_other_instrument_failures_retain_valid_sibling_failure_and_original_exit(self):
        for bad_events, status, expected_exit in [
            (["error: could not compile"], 101, "2"),
            ([json.dumps({"type": "suite", "event": "started", "test_count": 1})], 0, "4"),
        ]:
            with self.subTest(exit=expected_exit):
                result, outputs = self.initial([
                    {"pkg": "good", "events": events(), "status": 101},
                    {"pkg": "bad", "events": bad_events, "status": status},
                ])
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(outputs["replay_count"], "1")
                self.assertEqual(outputs["aggregate_trusted"], "false")
                self.assertEqual(outputs["observation_exit_code"], expected_exit)
                result, _ = self.run_step(self.step(name="Enforce initial observation result"),
                                          OBSERVATION_EXIT_CODE=expected_exit)
                self.assertEqual(result.returncode, int(expected_exit))

    def test_complete_pass_and_failure_keep_trusted_behavior(self):
        for failed in (False, True):
            with self.subTest(failed=failed):
                result, outputs = self.initial([
                    {"pkg": "good", "events": events(failed=failed), "status": 101 if failed else 0},
                ])
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(outputs["aggregate_trusted"], "true")
                self.assertEqual(outputs["observation_exit_code"], "0")
                self.assertEqual(outputs["replay_count"], "1" if failed else "0")
                self.assertEqual((self.work / "test-filter-args.txt").read_text(), "")
                if failed:
                    result, retries = self.run_step(self.step(id="retry_results"))
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertTrue(condition(self.step(name="Auto-quarantine flaky tests")["if"],
                                              self.values(outputs, retries)))
                result, _ = self.run_step(self.step(name="Enforce initial observation result"),
                                          OBSERVATION_EXIT_CODE="0")
                self.assertEqual(result.returncode, 0)

    def test_same_test_name_in_different_packages_remains_distinct(self):
        result, outputs = self.initial([
            {"pkg": package, "events": events(), "status": 101} for package in ("one", "two")
        ])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(outputs["replay_count"], "2")
        self.assertEqual((self.work / "failed-tests.tsv").read_text(),
                         "one\treal::failure\ntwo\treal::failure\n")

    def test_wrong_replay_remains_not_proven(self):
        for mode in ("wrong",):
            with self.subTest(mode=mode):
                result, outputs = self.initial([
                    {"pkg": "good", "events": events(), "status": 101},
                ], replay=mode)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                result, retries = self.run_step(self.step(id="retry_results"))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("NOT_PROVEN", result.stdout)
                self.assertNotIn("flake_count", retries)

    def test_three_exact_failed_replays_produce_a_stable_failure(self):
        self.env["TEST_FILTER_INPUT"] = '--features "example-feature"'
        result, outputs = self.initial([
            {"pkg": "good", "events": events(), "status": 101},
        ], replay="failed")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        result, retries = self.run_step(self.step(id="retry_results"))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(retries["stable_failure_count"], "1")
        self.assertEqual(retries["flake_count"], "0")
        self.assertEqual(retries["instrument_failure"], "0")
        self.assertEqual((self.work / "stable-failures.txt").read_text(), "real::failure\n")
        calls = [json.loads(line) for line in (self.work / "cargo-calls.jsonl").read_text().splitlines()]
        tests = [args for args in calls if args[0] == "test"]
        self.assertEqual(len(tests), 4)
        subject = tests[0][:tests[0].index("--")]
        self.assertEqual(subject[-2:], ["--features", "example-feature"])
        for replay in tests[1:]:
            self.assertEqual(replay[:replay.index("--")], subject)
            self.assertEqual(replay[-3:], ["--exact", "real::failure", "--test-threads=1"])
        self.assertTrue(condition(self.step(name="Create issue for stable failures")["if"],
                                  self.values(outputs, retries)))

    def test_stable_failure_in_mixed_stream_still_cannot_mutate_remote_state(self):
        result, outputs = self.initial([
            {"pkg": "good", "events": events(), "status": 101},
            {"pkg": "bad", "events": ["{broken"], "status": 0},
        ], replay="failed")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        result, retries = self.run_step(self.step(id="retry_results"))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(retries["stable_failure_count"], "1")
        self.assertFalse(condition(self.step(name="Create issue for stable failures")["if"],
                                   self.values(outputs, retries)))
        final = self.step(name="Enforce initial observation result")
        self.assertTrue(condition(final["if"], self.values(outputs, retries)))
        result, _ = self.run_step(final, OBSERVATION_EXIT_CODE=outputs["observation_exit_code"])
        self.assertEqual(result.returncode, 3)

    def test_replay_nonzero_extractor_exit_cannot_be_overridden_by_passing_receipt(self):
        extractor = self.work / ".ci/scripts/extract-failed-tests.py"
        source = extractor.read_text()
        source = source.replace("sys.exit(main())", 'code = main()\n    sys.exit(7 if "--expect-replay" in sys.argv else code)')
        extractor.write_text(source)
        result, _ = self.initial([{"pkg": "good", "events": events(), "status": 101}])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        result, retries = self.run_step(self.step(id="retry_results"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Instrument failure", result.stdout)
        self.assertNotIn("flake_count", retries)

    def test_missing_final_result_and_failed_initial_step_cannot_authorize_actions(self):
        final = self.step(name="Enforce initial observation result")
        self.assertTrue(condition(final["if"], self.values({})))
        result, _ = self.run_step(final,
                                  OBSERVATION_EXIT_CODE="")
        self.assertEqual(result.returncode, 1)
        self.assertIn("no valid consumer result", result.stdout)
        values = self.values({"replay_count": "1", "aggregate_trusted": "false"},
                             {"flake_count": "1", "instrument_failure": "0", "stable_failure_count": "1"})
        values["steps.initial_run.outcome"] = "failure"
        self.assertFalse(condition(self.step(id="retry_results")["if"], values))
        for name in ("Auto-quarantine flaky tests", "Create PR for quarantine changes",
                     "Comment on #2093 with flake report", "Create issue for stable failures"):
            self.assertFalse(condition(self.step(name=name)["if"], values), name)

    def test_cancellation_does_not_launch_replays(self):
        values = self.values({"replay_count": "1"})
        self.assertTrue(condition(self.step(id="retry_results")["if"], values))
        values["cancelled"] = True
        self.assertFalse(condition(self.step(id="retry_results")["if"], values))

    def test_consumer_rejects_missing_unreadable_stale_receipts_and_status_mismatch(self):
        for damage in ("missing", "unreadable", "malformed", "stale", "head", "status"):
            with self.subTest(damage=damage):
                receipt = self.work / "flake-receipt.json"
                if receipt.is_dir():
                    receipt.rmdir()
                result, _ = self.initial([
                    {"pkg": "good", "events": events(), "status": 101},
                ])
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                if damage == "missing":
                    receipt.unlink()
                elif damage == "unreadable":
                    receipt.unlink()
                    receipt.mkdir()
                elif damage == "malformed":
                    receipt.write_text("{broken")
                elif damage == "stale":
                    with (self.work / "test-observations.jsonl").open("a") as stream:
                        stream.write("extra diagnostic\n")
                output = self.work / "consumer-outputs"
                output.write_text("")
                result = subprocess.run([
                    "python3", ".ci/scripts/prepare-flake-replay.py", "test-observations.jsonl", "flake-receipt.json",
                    "--extractor-exit", "3" if damage == "status" else "0",
                    "--expected-head", "wrong-head" if damage == "head" else "fixture-head",
                    "--expected-toolchain", "cargo fixture-toolchain", "--filter-count", "0",
                ], cwd=self.work, env=dict(self.env, GITHUB_OUTPUT=str(output)), text=True, capture_output=True)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("flake receipt consumer failed", result.stdout)
                self.assertFalse((self.work / "failed-tests.tsv").exists())
                self.assertEqual(output.read_text(), "")

    def test_zero_discovery_preserves_explicit_filter_boundary(self):
        empty = [json.dumps({"type": "suite", "event": "started", "test_count": 0}),
                 json.dumps({"type": "suite", "event": "ok", "passed": 0, "failed": 0, "ignored": 0})]
        for filtered in (False, True):
            with self.subTest(filtered=filtered):
                self.env["TEST_FILTER_INPUT"] = "nonexistent" if filtered else ""
                result, outputs = self.initial([{"pkg": "empty", "events": empty, "status": 0}])
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(outputs["aggregate_trusted"], "true" if filtered else "false")
                self.assertEqual(outputs["observation_exit_code"], "0" if filtered else "1")
                self.assertEqual((self.work / "test-filter-args.txt").read_text(), "nonexistent\n" if filtered else "")


if __name__ == "__main__":
    unittest.main(verbosity=2)
