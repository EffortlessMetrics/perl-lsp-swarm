"""Small mock-command controls; never invoke a Rust compiler or Cargo build."""
from pathlib import Path
import json
import os
import shutil
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
OBSERVER = ROOT / "scripts/ci/observe-xtask-build.sh"
BASH = shutil.which("bash")
if os.name == "nt":
    BASH = r"C:\Program Files\Git\bin\bash.exe"


def posix(path):
    value = str(Path(path).resolve()).replace("\\", "/")
    return f"/{value[0].lower()}{value[2:]}" if os.name == "nt" else value


class ObservationTests(unittest.TestCase):
    def setUp(self):
        # Retain bounded fixtures as evidence; no recursive cleanup of user paths.
        base = Path(os.environ.get("XTASK_OBSERVATION_TEST_ARTIFACTS", tempfile.gettempdir()))
        base.mkdir(parents=True, exist_ok=True)
        self.work = Path(tempfile.mkdtemp(prefix="xtask-observation-test-", dir=base))
        self.bin = self.work / "bin"
        self.bin.mkdir()
        self.state = self.work / "state"
        self.state.mkdir()
        (self.state / "head").write_text("a" * 40)
        (self.state / "rustc").write_text("rustc 1.95.0 (test)")
        for name in ["Cargo.lock", "Cargo.toml", "xtask/Cargo.toml", "rust-toolchain.toml"]:
            path = self.work / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("controlled fixture\n")
        self.stub("git", '''case "$*" in
"rev-parse HEAD") cat "$FAKE_STATE/head" ;;
"rev-parse HEAD^{tree}") printf '%040d' 1 ;;
*) exit 2 ;; esac
''')
        self.stub("rustc", '''if [[ "$*" == "--print sysroot" ]]; then echo /test/toolchain; else cat "$FAKE_STATE/rustc"; fi
''')
        self.stub("cargo", '''if [[ "$*" == "-vV" ]]; then echo 'cargo 1.95.0 (test)'; exit 0; fi
printf '%s\\0' "$@" > "$FAKE_STATE/args"
if [[ ${FAKE_TRACE:-yes} == yes ]]; then
  if [[ ${FAKE_WARM:-no} == yes ]]; then
    echo '0.01 TRACE cargo::core::compiler::fingerprint: fingerprint at: target/debug/.fingerprint/serde' >&2
  else
    echo '0.01 INFO cargo::core::compiler::fingerprint: fingerprint dirty for serde v1.0.229' >&2
    echo '0.01 INFO cargo::core::compiler::fingerprint: dirty: PathChanged { old: 1, new: 2 }' >&2
  fi
fi
if [[ ${FAKE_FINGERPRINT_ERROR:-no} == yes ]]; then
  echo '0.01 INFO cargo::core::compiler::fingerprint: fingerprint error for serde v1.0.229' >&2
  echo '0.01 INFO cargo::core::compiler::fingerprint: err: failed to read fingerprint' >&2
fi
if [[ ${FAKE_DETACHED_STDERR:-no} == yes ]]; then sleep 8 >/dev/null & fi
if [[ ${FAKE_LARGE_TRACE:-no} == yes ]]; then
  for ((i=0;i<12000;i++)); do printf '0.01 TRACE cargo::core::compiler::fingerprint: fingerprint at: %090d\\n' "$i" >&2; done
fi
if [[ ${FAKE_WARM:-no} != yes ]]; then echo 'Compiling xtask v0.17.0' >&2; fi
echo 'Finished `dev` profile target(s) in 1.00s' >&2
echo 'ordinary command stderr' >&2
case ${FAKE_CHANGE:-none} in
head) printf '%040d' 2 > "$FAKE_STATE/head" ;;
rustc) echo 'rustc 1.96.0 (changed)' > "$FAKE_STATE/rustc" ;;
missing-head) : > "$FAKE_STATE/head" ;;
lock) echo 'changed dependency identity' > Cargo.lock ;;
fingerprint) printf '{"path":18446744073709551614}\\n' > target/debug/.fingerprint/serde-test/lib-serde.json ;;
esac
exit "${FAKE_EXIT:-0}"
''')

    def stub(self, name, body):
        path = self.bin / name
        path.write_text("#!/usr/bin/env bash\n" + body, encoding="utf-8", newline="\n")
        path.chmod(0o755)

    def observe(self, **extra):
        env = dict(os.environ, FAKE_STATE=posix(self.state), HOME=posix(self.work),
                   CARGO_HOME=posix(self.work / "cargo-home"), CARGO_TARGET_DIR="target",
                   GITHUB_RUN_ID="123", GITHUB_RUN_ATTEMPT="2")
        env.update(extra)
        command = ["cargo", "xtask", "ripr-plus", "--receipt", "target/path with space.json"]
        result = subprocess.run(
            [BASH, "--noprofile", "--norc", "-c", 'export PATH="$1:$PATH"; shift; exec bash "$@"',
             "observation-control", posix(self.bin), posix(OBSERVER), "host-test", "out", "--", *command],
            cwd=self.work, env=env, capture_output=True, text=True, encoding="utf-8", timeout=25,
        )
        records = list((self.work / "out").glob("*.observation.json"))
        self.assertEqual(len(records), 1, result.stderr[-2500:])
        observation = json.loads(records[0].read_text())
        actual_args = (self.state / "args").read_bytes().decode().split("\0")[:-1]
        self.assertEqual(actual_args, command[1:])
        self.assertEqual(observation["command"], command)
        self.assertIn("ordinary command stderr", result.stderr)
        return result, observation

    def test_original_arguments_verdict_and_identity_are_preserved(self):
        result, record = self.observe()
        self.assertEqual(result.returncode, 0)
        self.assertTrue(record["identity_stable"])
        self.assertEqual(record["before"]["target_dir_hint"], "target")
        self.assertTrue(record["selected_trace_complete"])
        self.assertEqual(record["stderr"]["compiling_messages"], 1)
        self.assertEqual(record["stderr"]["dirty_lines"], 2)
        self.assertEqual((record["run_id"], record["run_attempt"]), ("123", "2"))

    def test_failed_cargo_is_not_rescued(self):
        result, record = self.observe(FAKE_EXIT="37")
        self.assertEqual(result.returncode, 37)
        self.assertEqual(record["exit_code"], 37)

    def test_changed_source_is_not_comparable_identity(self):
        _, record = self.observe(FAKE_CHANGE="head")
        self.assertFalse(record["identity_stable"])

    def test_changed_compiler_is_not_comparable_identity(self):
        _, record = self.observe(FAKE_CHANGE="rustc")
        self.assertFalse(record["identity_stable"])

    def test_changed_lockfile_is_not_comparable_identity(self):
        _, record = self.observe(FAKE_CHANGE="lock")
        self.assertFalse(record["identity_stable"])

    def test_warm_command_can_report_no_compile_work(self):
        _, record = self.observe(FAKE_WARM="yes")
        self.assertTrue(record["identity_stable"])
        self.assertTrue(record["selected_trace_complete"])
        self.assertEqual(record["stderr"]["compiling_messages"], 0)
        self.assertEqual(record["stderr"]["dirty_lines"], 0)

    def test_u64_fingerprint_differences_are_not_rounded_away(self):
        unit = self.work / "target/debug/.fingerprint/serde-test/lib-serde.json"
        unit.parent.mkdir(parents=True)
        unit.write_text('{"path":18446744073709551613}\n')
        _, record = self.observe(FAKE_CHANGE="fingerprint")
        before = record["before"]["fingerprints"][0]
        after = record["after"]["fingerprints"][0]
        self.assertEqual(before["hashes"]["path"], "18446744073709551613")
        self.assertEqual(after["hashes"]["path"], "18446744073709551614")
        self.assertNotEqual(before["sha256"], after["sha256"])

    def test_environment_and_flags_values_are_not_retained(self):
        unit = self.work / "target/debug/.fingerprint/serde-test/run-build-script.json"
        unit.parent.mkdir(parents=True)
        secret = "CONTROL-SENSITIVE-VALUE"
        unit.write_text(json.dumps({"path": 42, "local": [{"RerunIfEnvChanged": {"val": secret}}],
                                   "rustflags": [secret]}))
        cargo = self.bin / "cargo"
        body = cargo.read_text()
        body = body.replace("case ${FAKE_CHANGE:-none} in",
                            'echo \'0.01 INFO cargo::core::compiler::fingerprint: dirty: EnvVarChanged { old_value: "' + secret +
                            '", new_value: None }\' >&2\n' +
                            'echo \'0.01 TRACE cargo::core::compiler::fingerprint: env changed: ' + secret + '\' >&2\n' +
                            "case ${FAKE_CHANGE:-none} in")
        cargo.write_text(body, encoding="utf-8", newline="\n")
        result, record = self.observe()
        self.assertNotIn(secret, result.stdout + result.stderr)
        for path in (self.work / "out").iterdir():
            self.assertNotIn(secret, path.read_text(), str(path))
        reasons = next((self.work / "out").glob("*.dirty.log")).read_text()
        self.assertIn("EnvVarChanged (details omitted)", reasons)
        self.assertGreater(record["stderr"]["filtered_fingerprint_lines"], 0)

    def test_missing_source_identity_is_not_accepted(self):
        _, record = self.observe(FAKE_CHANGE="missing-head")
        self.assertFalse(record["identity_stable"])

    def test_missing_fingerprint_trace_is_not_complete_evidence(self):
        _, record = self.observe(FAKE_TRACE="no")
        self.assertFalse(record["selected_trace_complete"])

    def test_fingerprint_read_errors_are_not_complete_evidence(self):
        _, record = self.observe(FAKE_FINGERPRINT_ERROR="yes")
        self.assertFalse(record["selected_trace_complete"])
        self.assertEqual(record["stderr"]["fingerprint_errors"], 1)
        reasons = next((self.work / "out").glob("*.dirty.log")).read_text()
        self.assertIn("fingerprint error for serde", reasons)
        self.assertIn("err: details omitted", reasons)

    def test_detached_stderr_cannot_hold_the_native_verdict_indefinitely(self):
        start = time.monotonic()
        result, record = self.observe(FAKE_DETACHED_STDERR="yes", FAKE_EXIT="37")
        self.assertLess(time.monotonic() - start, 7)
        self.assertEqual(result.returncode, 37)
        self.assertTrue(record["reader_timed_out"])
        self.assertFalse(record["selected_trace_complete"])

    def test_trace_is_bounded_and_truncation_is_explicit(self):
        _, record = self.observe(FAKE_LARGE_TRACE="yes")
        self.assertFalse(record["selected_trace_complete"])
        self.assertTrue(record["stderr"]["trace_clipped"])
        for path in (self.work / "out").glob("*.trace.log"):
            self.assertLessEqual(path.stat().st_size, 1048576)

    def workflow_run(self, name):
        lines = (ROOT / ".github/workflows/ripr.yml").read_text(encoding="utf-8").splitlines()
        start = lines.index("      - name: " + name)
        end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("      - name:")), len(lines))
        run = lines.index("        run: |", start, end)
        return "\n".join(line[10:] for line in lines[run + 1:end] if line.startswith("          "))

    def receipt_path(self, exact, fail=False):
        # Execute the workflow's actual adjudicator and generation blocks, using
        # fake Cargo. This catches accidental widening of exact-head memoization.
        path = self.work / "target/receipts/quality/ripr-plus.json"
        path.parent.mkdir(parents=True)
        path.write_text("cached other-head receipt\n")
        marker = path.parent / ".ripr-plus-fresh"
        if exact:
            marker.write_text("old marker\n")
        helper = self.work / "scripts/ci/observe-xtask-build.sh"
        helper.parent.mkdir(parents=True)
        shutil.copyfile(OBSERVER, helper)
        env = dict(os.environ, FAKE_STATE=posix(self.state), HOME=posix(self.work),
                   CARGO_HOME=posix(self.work / "cargo-home"), CARGO_TARGET_DIR="target",
                   GITHUB_ENV="action-env", RIPR_FRESHNESS_HANDOFF="handoff",
                   RIPR_RECEIPT_CACHE_MATCHED="true" if exact else "false",
                   RIPR_RECEIPT_CACHE_MATCHED_KEY="different-head-prefix-key", FAKE_EXIT="37" if fail else "0")
        prefix = 'export PATH="$1:$PATH"; shift; '
        adjudicator = subprocess.run([BASH, "--noprofile", "--norc", "-e", "-o", "pipefail", "-c",
                                      prefix + self.workflow_run("Adjudicate receipt cache exactness"),
                                      "cache-control", posix(self.bin)], cwd=self.work, env=env,
                                     capture_output=True, text=True, encoding="utf-8", timeout=10)
        self.assertEqual(adjudicator.returncode, 0, adjudicator.stderr[-1000:])
        hit = (self.work / "action-env").read_text().strip().split("=")[1]
        env["RIPR_RECEIPT_CACHE_HIT"] = hit
        producer = subprocess.run([BASH, "--noprofile", "--norc", "-e", "-o", "pipefail", "-c",
                                   prefix + self.workflow_run("Generate repo-wide RIPR+ baseline receipt"),
                                   "cache-control", posix(self.bin)], cwd=self.work, env=env,
                                  capture_output=True, text=True, encoding="utf-8", timeout=15)
        return producer, marker

    def test_wrong_head_prefix_hit_still_produces_fresh_receipt(self):
        result, marker = self.receipt_path(exact=False)
        self.assertEqual(result.returncode, 0, result.stderr[-1000:])
        self.assertIn("produced-fresh head=", marker.read_text())
        self.assertTrue((self.state / "args").exists())

    def test_exact_hit_does_not_publish_a_freshness_marker(self):
        result, marker = self.receipt_path(exact=True)
        self.assertEqual(result.returncode, 0, result.stderr[-1000:])
        self.assertFalse(marker.exists())
        self.assertFalse((self.state / "args").exists())

    def test_failed_fresh_generation_does_not_publish_a_marker(self):
        result, marker = self.receipt_path(exact=False, fail=True)
        self.assertEqual(result.returncode, 37, result.stderr[-1000:])
        self.assertFalse(marker.exists())


if __name__ == "__main__":
    unittest.main()
