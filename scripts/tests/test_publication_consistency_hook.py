"""Real Git/Bash publication transport controls; Cargo is a recorded fake.

These tests prove hook dispatch and failure propagation, not Rust gate execution.
All Git fixtures strip inherited selectors and use isolated configuration.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
ZERO = "0" * 40


class PublicationHookTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="publication-hook-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        self.tools = Path(self.temp.name) / "tools"
        self.tools.mkdir()
        self.log = Path(self.temp.name) / "commands.jsonl"
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                        GIT_CONFIG_SYSTEM=os.devnull, GIT_AUTHOR_NAME="Publication fixture",
                        GIT_COMMITTER_NAME="Publication fixture",
                        GIT_AUTHOR_EMAIL="fixture@example.invalid",
                        GIT_COMMITTER_EMAIL="fixture@example.invalid")
        self.git("init", "-q", "--initial-branch=main")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "core.autocrlf", "false")
        self.git("config", "core.hooksPath", str(self.tools / "no-hooks"))
        (self.root / "hooks").mkdir()
        self.hook = self.root / "hooks/pre-push"
        self.hook.write_bytes((ROOT / "hooks/pre-push").read_bytes())
        (self.root / "seed").write_text("base\n")
        self.base = self.commit()
        self.git("update-ref", "refs/remotes/origin/main", self.base)
        driver = self.tools / "driver.py"
        driver.write_text(
            "import json,os,sys\n"
            "args=sys.argv[1:]\n"
            "if args[0]=='cargo':\n"
            "    with open(os.environ['PUBLICATION_TEST_ENV_LOG'],'w') as f: json.dump({k:v for k,v in os.environ.items() if k in ('GIT_DIR','GIT_WORK_TREE','GIT_COMMON_DIR','GIT_INDEX_FILE','GIT_OBJECT_DIRECTORY','GIT_ALTERNATE_OBJECT_DIRECTORIES')},f)\n"
            "with open(os.environ['PUBLICATION_TEST_LOG'],'a') as f: f.write(json.dumps(args)+'\\n')\n"
            "if args[0]=='cargo' and '--tier' in args and args[args.index('--tier')+1]=='publication':\n"
            "    sys.exit(int(os.environ.get('PUBLICATION_TEST_EXIT','0')))\n"
            "if args[0]=='cargo' and 'change-set' in args:\n"
            "    print('scripts/cargo_admitted.py')\n"
            "sys.exit(0)\n", encoding="utf-8")
        for name in ("cargo", "just"):
            script = self.tools / name
            script.write_text('#!/bin/sh\nexec "$PUBLICATION_TEST_PYTHON" "$PUBLICATION_TEST_DRIVER" '
                              + name + ' "$@"\n', encoding="utf-8", newline="\n")
            script.chmod(0o755)
        # Declared fake admission forwarder: it records Cargo argv, without
        # executing production admission or compiling any crate.
        admission = self.root / "scripts/cargo-admitted"
        admission.parent.mkdir(exist_ok=True)
        admission.write_text('#!/bin/sh\nexec cargo "$@"\n', encoding="utf-8", newline="\n")
        admission.chmod(0o755)
        self.bash = shutil.which("bash")
        if os.name == "nt":
            self.bash = "C:/Program Files/Git/bin/bash.exe"
            system_paths = ["C:/Program Files/Git/bin", "C:/Program Files/Git/usr/bin"]
        else:
            system_paths = ["/usr/bin", "/bin"]
        self.env.update(PATH=os.pathsep.join([str(self.tools), *system_paths]),
                        PUBLICATION_TEST_PYTHON=Path(sys.executable).as_posix(),
                        PUBLICATION_TEST_DRIVER=driver.as_posix(),
                        PUBLICATION_TEST_LOG=self.log.as_posix(),
                        PUBLICATION_TEST_ENV_LOG=(Path(self.temp.name) / "child-env.json").as_posix())

    def git(self, *args):
        result = subprocess.run(["git", *args], cwd=self.root, env=self.env,
                                capture_output=True, text=True, encoding="utf-8")
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def commit(self):
        self.git("add", "-A")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def change(self, path):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text("fixture change\n")
        return self.commit()

    def invoke(self, refs, code=0):
        self.env["PUBLICATION_TEST_EXIT"] = str(code)
        # Git's hook protocol is LF-delimited even on Windows. Python text
        # stdin would translate it to CRLF and corrupt the final SHA token.
        result = subprocess.run([self.bash, self.hook.as_posix(), "origin", "fixture"],
                                input="".join(f"refs/heads/topic {head} {remote} {base}\n"
                                              for head, remote, base in refs).encode("utf-8"),
                                cwd=self.root, env=self.env, capture_output=True)
        result.stdout = result.stdout.decode("utf-8")
        result.stderr = result.stderr.decode("utf-8")
        calls = [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        return result, calls

    def assert_publication_only(self, calls, count=1):
        publications = [c for c in calls if "--tier" in c and c[c.index("--tier") + 1] == "publication"]
        self.assertEqual(len(publications), count, calls)
        self.assertFalse(any(c[0] == "just" or any(x in c for x in ("fmt", "clippy", "test"))
                             for c in calls), calls)
        for call in publications:
            self.assertEqual(call[1:8], ["run", "--package", "xtask", "--bin", "xtask", "--locked", "--"])
        return publications

    def test_python_and_rust_pushes_use_same_publication_role(self):
        for path in ("scripts/cargo_admitted.py", "crates/example/src/lib.rs"):
            with self.subTest(path=path):
                head = self.change(path)
                self.log.unlink(missing_ok=True)
                result, calls = self.invoke([(head, "refs/heads/topic", self.base)])
                self.assertEqual(result.returncode, 0, result.stderr)
                publication = self.assert_publication_only(calls)[0]
                self.assertEqual(publication[publication.index("--head") + 1], head)
                self.assertEqual(publication[publication.index("--base") + 1], self.base)

    def test_each_non_deletion_ref_is_checked_and_deletion_is_noop(self):
        head = self.change("scripts/example.py")
        result, calls = self.invoke([(head, "refs/heads/one", self.base),
                                     (head, "refs/heads/two", self.base),
                                     (ZERO, "refs/heads/old", self.base)])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_publication_only(calls, count=2)

    def test_publication_instrument_failure_blocks_push(self):
        head = self.change("scripts/example.py")
        result, calls = self.invoke([(head, "refs/heads/topic", self.base)], code=75)
        self.assertNotEqual(result.returncode, 0, calls)
        self.assert_publication_only(calls)

    def test_new_branch_uses_auto_base_and_expected_pushed_head(self):
        head = self.change("scripts/example.py")
        result, calls = self.invoke([(head, "refs/heads/new", ZERO)])
        self.assertEqual(result.returncode, 0, result.stderr)
        publication = self.assert_publication_only(calls)[0]
        self.assertEqual(publication[publication.index("--base") + 1], "auto")
        self.assertEqual(publication[publication.index("--head") + 1], head)

    def test_protected_deletion_and_non_fast_forward_still_refuse(self):
        head = self.change("scripts/example.py")
        for refs in ([(ZERO, "refs/heads/main", head)],
                     [(self.base, "refs/heads/topic", head)]):
            with self.subTest(refs=refs):
                result, calls = self.invoke(refs)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, [])

    def test_pushed_commit_is_not_silently_replaced_by_checked_out_head(self):
        pushed = self.change("scripts/example.py")
        self.change("scripts/later.py")
        result, calls = self.invoke([(pushed, "refs/heads/topic", self.base)])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [], "mismatching checkout must refuse before Cargo")
        self.assertIn("matching checkout", result.stderr)

    def test_later_nonhead_ref_prevents_all_cargo(self):
        earlier = self.change("scripts/example.py")
        head = self.change("scripts/later.py")
        result, calls = self.invoke([(head, "refs/heads/current", self.base),
                                     (earlier, "refs/heads/other", self.base)])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [], "whole batch admission must precede the first Cargo call")
        self.assertIn(earlier, result.stderr)
        self.assertIn("refs/heads/topic", result.stderr)

    def test_annotated_tag_at_head_keeps_literal_pushed_object(self):
        self.change("scripts/example.py")
        self.git("tag", "-a", "fixture-tag", "-m", "fixture")
        tag = self.git("rev-parse", "fixture-tag")
        result, calls = self.invoke([(tag, "refs/tags/fixture-tag", self.base)])
        self.assertEqual(result.returncode, 0, result.stderr)
        publication = self.assert_publication_only(calls)[0]
        self.assertEqual(publication[publication.index("--head") + 1], tag)

    def test_validation_child_isolates_hook_git_selectors(self):
        head = self.change("scripts/example.py")
        gitdir = self.root / ".git"
        self.env.update(GIT_DIR=gitdir.as_posix(), GIT_WORK_TREE=self.root.as_posix(),
                        GIT_COMMON_DIR=gitdir.as_posix(), GIT_INDEX_FILE=(gitdir / "index").as_posix(),
                        GIT_OBJECT_DIRECTORY=(gitdir / "objects").as_posix(),
                        GIT_ALTERNATE_OBJECT_DIRECTORIES="")
        result, calls = self.invoke([(head, "refs/heads/topic", self.base)])
        self.assertEqual(result.returncode, 0, result.stderr)
        publication = self.assert_publication_only(calls)[0]
        self.assertEqual(Path(publication[publication.index("--worktree-root") + 1]), self.root)
        observed = json.loads(Path(self.env["PUBLICATION_TEST_ENV_LOG"]).read_text())
        self.assertEqual(observed, {}, "validation child inherited hook repository selectors")

    def test_missing_admission_tool_does_not_pass_publication(self):
        head = self.change("scripts/example.py")
        (self.root / "scripts/cargo-admitted").unlink()
        result, calls = self.invoke([(head, "refs/heads/topic", self.base)])
        self.assertNotEqual(result.returncode, 0, calls)
        self.assertEqual(calls, [])

    def test_missing_cargo_does_not_pass_publication(self):
        head = self.change("scripts/example.py")
        (self.tools / "cargo").unlink()
        result, calls = self.invoke([(head, "refs/heads/topic", self.base)])
        self.assertNotEqual(result.returncode, 0, calls)


class PublicationReceiptSchemaTests(unittest.TestCase):
    def test_generated_hook_matches_tracked_hook(self):
        source = (ROOT / "crates/perl-ci-hygiene/src/git_hooks.rs").read_text(encoding="utf-8")
        body = source.split("pub(crate) fn pre_push_hook_script()", 1)[1]
        embedded = body.split('r#"', 1)[1].split('\n"#\n}', 1)[0] + "\n"
        self.assertEqual(embedded, (ROOT / "hooks/pre-push").read_text(encoding="utf-8"))

    def test_native_publication_receipt_validates_and_unknown_tier_does_not(self):
        from jsonschema import Draft7Validator
        schema = json.loads((ROOT / ".ci/receipt.schema.json").read_text(encoding="utf-8"))
        Draft7Validator.check_schema(schema)
        validator = Draft7Validator(schema)
        receipt = {
            "schema_version": 1,
            "metadata": {"timestamp": "2026-10-17T00:00:00Z", "git_sha": "a" * 40,
                         "git_branch": "fixture", "toolchain": {"rustc_version": "rustc 1.95.0"},
                         "platform": {"os": "windows", "arch": "x86_64"},
                         "environment": {"type": "local"}},
            "gates": [{"gate_name": "whitespace_check", "tier": "publication",
                       "status": "pass", "duration_ms": 1, "required": True,
                       "command": "cargo xtask commit-check whitespace_check"}],
            "summary": {"total_gates": 1, "passed": 1, "failed": 0, "skipped": 0,
                        "total_duration_ms": 1, "overall_status": "pass",
                        "tier_results": {"publication": {"passed": 1, "failed": 0, "skipped": 0}}},
            "agent_receipt": {"sha": "a" * 40, "is_latest": True, "tier": "publication",
                              "scope": {"direct_crates": [], "reverse_deps": [], "risk_tags": []},
                              "selected_lanes": [], "failures": [], "suggested_next_actions": [],
                              "staged_tree_oid": "b" * 40}}
        validator.validate(receipt)
        receipt["gates"][0]["tier"] = "unreviewed-role"
        self.assertTrue(list(validator.iter_errors(receipt)), "unknown role must not validate")


if __name__ == "__main__":
    unittest.main()
