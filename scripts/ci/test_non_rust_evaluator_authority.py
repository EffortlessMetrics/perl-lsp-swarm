#!/usr/bin/env python3
"""Execute production workflow JS with read-only API doubles and real local Git.

No GitHub calls or Cargo builds. These tests prove source wiring and authority
refusal, not deployed workflow integrity, protection installation or Rust policy.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/non-rust-policy.yml"


def bash_executable():
    return os.environ.get("NON_RUST_POLICY_BASH", "bash")


def script(name):
    # Exact workflow literal, not a fixture reimplementation of the checks.
    lines = WORKFLOW.read_text(encoding="utf-8").splitlines()
    start = lines.index("      - name: " + name)
    end = next((i for i in range(start + 1, len(lines))
                if lines[i].startswith("      - name:")), len(lines))
    block = lines[start:end]
    offset = block.index("          script: |") + 1
    return "\n".join(line[12:] for line in block[offset:] if line.strip())


class Authority(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="nonrust-\u00e9-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        self.git("init", "-q", "--template=")
        (self.root / "source").write_text("trusted evaluator \u00e9", encoding="utf-8")
        self.git("add", "source")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "-c", "commit.gpgsign=false", "-c", "core.hooksPath=", "commit", "-qm", "T")
        self.t = self.git("rev-parse", "HEAD").strip()
        self.b, self.g = "b" * 40, "c" * 40
        self.repo = {"full_name": "EffortlessMetrics/perl-lsp-swarm", "id": 42,
                     "node_id": "R_fixture", "default_branch": "main"}
        self.context = {"eventName": "merge_group", "repo": {"owner": "EffortlessMetrics", "repo": "perl-lsp-swarm"},
                        "payload": {"repository": self.repo, "action": "checks_requested",
                                    "merge_group": {"base_ref": "refs/heads/main", "base_sha": self.b, "head_sha": self.g}}}
        self.branch = {"name": "main", "protected": True, "commit": {"sha": self.t}}
        self.ref = {"ref": "refs/heads/main", "object": {"type": "commit", "sha": self.t}}

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, env=self.env,
                              check=True, capture_output=True, text=True, encoding="utf-8").stdout

    def execute(self, name, expected=None):
        fixture = json.dumps({"repository": self.repo, "branch": self.branch,
                              "ref": self.ref, "context": self.context})
        harness = "const f=" + fixture + "; const context=f.context; const calls=[];" + """
const read = key => async args => {
  if (args.owner !== 'EffortlessMetrics' || args.repo !== 'perl-lsp-swarm') throw Error('scope');
  calls.push(key); return {data: f[key]};
};
const github={rest:{repos:{get:read('repository'),getBranch:read('branch')},git:{getRef:read('ref')}}};
const core={setOutput:(key,value)=>console.log(key+'='+value)};
(async()=>{
""" + script(name) + """
if (calls.join(',') !== 'repository,branch,ref') throw Error('read custody');
})().catch(e=>{console.error(e.message);process.exitCode=1});
"""
        env = dict(self.env)
        if expected is not None:
            env["EVALUATOR_SHA"] = expected
        else:
            env.pop("EVALUATOR_SHA", None)
        return subprocess.run(["node", "-e", harness], cwd=self.root, env=env,
                              capture_output=True, text=True, encoding="utf-8")

    def test_distinct_t_bootstrap_and_before_after_execution(self):
        out = self.execute("Resolve protected main evaluator generation")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(out.stdout.strip(), "sha=" + self.t)
        self.assertNotEqual(self.t, self.b)
        for name in ("Revalidate protected evaluator before execution", "Revalidate protected evaluator after execution"):
            out = self.execute(name, self.t)
            self.assertEqual(out.returncode, 0, out.stderr)

    def test_wrong_missing_stale_and_unprotected_t_refuse(self):
        for expected in (None, self.b, "a" * 40):
            out = self.execute("Revalidate protected evaluator before execution", expected)
            self.assertNotEqual(out.returncode, 0)
            self.assertIn("generation changed", out.stderr)
        self.branch["commit"]["sha"] = self.b
        self.ref["object"]["sha"] = self.b
        out = self.execute("Revalidate protected evaluator after execution", self.t)
        self.assertNotEqual(out.returncode, 0)
        self.assertIn("generation changed", out.stderr)
        self.branch["commit"]["sha"] = self.t
        self.ref["object"]["sha"] = self.b
        out = self.execute("Resolve protected main evaluator generation")
        self.assertNotEqual(out.returncode, 0)
        self.assertIn("unavailable", out.stderr)
        self.ref["object"]["sha"] = self.t
        self.branch["commit"].pop("sha")
        self.assertNotEqual(self.execute("Resolve protected main evaluator generation").returncode, 0)
        self.branch["commit"]["sha"] = self.t
        self.branch["protected"] = False
        self.assertNotEqual(self.execute("Resolve protected main evaluator generation").returncode, 0)

    def test_checkout_custody_refuses_modified_source_and_wrong_head(self):
        (self.root / "source").write_text("modified evaluator", encoding="utf-8")
        out = self.execute("Revalidate protected evaluator before execution", self.t)
        self.assertNotEqual(out.returncode, 0)
        self.assertIn("custody", out.stderr)
        # An independently verified main T cannot authorize a different checkout.
        self.branch["commit"]["sha"] = self.b
        self.ref["object"]["sha"] = self.b
        out = self.execute("Revalidate protected evaluator before execution", self.b)
        self.assertNotEqual(out.returncode, 0)
        self.assertIn("custody", out.stderr)

    def test_speculative_base_and_group_remain_exact_subjects(self):
        # Real local B/G objects differ from T; production binding must not
        # check out either object or mutate the independently verified source.
        for label in ("B", "G"):
            (self.root / "source").write_text(label, encoding="utf-8")
            self.git("add", "source")
            self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                     "-c", "commit.gpgsign=false", "-c", "core.hooksPath=", "commit", "-qm", label)
            setattr(self, label.lower(), self.git("rev-parse", "HEAD").strip())
        self.git("checkout", "--detach", self.t)
        self.git("remote", "add", "origin", str(self.root))
        lines = WORKFLOW.read_text(encoding="utf-8").splitlines()
        start = lines.index("      - name: Fetch and bind subject Git object")
        end = lines.index("      - name: Verify trusted workflow contract")
        block = lines[start:end]
        offset = block.index("        run: |") + 1
        body = "\n".join(line[10:] for line in block[offset:])
        output, exported = self.root / "output", self.root / "env"
        env = dict(self.env, BASE_SHA=self.b, SUBJECT_SHA=self.g, PR_HEAD_SHA="",
                   PR_NUMBER="", GITHUB_OUTPUT=str(output), GITHUB_ENV=str(exported))
        result = subprocess.run([bash_executable(), "--noprofile", "--norc", "-c", body],
                                env=env, cwd=self.root, capture_output=True, text=True, encoding="utf-8")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("subject_sha=" + self.g, output.read_text(encoding="utf-8"))
        self.assertEqual(self.git("rev-parse", "HEAD").strip(), self.t)
        self.assertNotEqual(self.t, self.b)
        self.assertEqual(self.git("rev-parse", "non-rust-policy-subject").strip(), self.g)
        out = self.execute("Revalidate protected evaluator before execution", self.t)
        self.assertEqual(out.returncode, 0, out.stderr)

    def test_utf8_subprocess_output_does_not_use_platform_default(self):
        # A real Unicode checkout path and output exercise the bytes boundary.
        # Refuse fallback decoding even on hosts whose default is already UTF-8.
        with patch("subprocess._text_encoding", side_effect=AssertionError("implicit decoder")):
            self.assertEqual(self.git("show", "HEAD:source"), "trusted evaluator \u00e9")
            out = self.execute("Revalidate protected evaluator before execution", self.t)
            self.assertEqual(out.returncode, 0, out.stderr)

    def test_explicit_bash_selection_is_preserved(self):
        with patch.dict(os.environ, {"NON_RUST_POLICY_BASH": "/fixture/Git Bash/bash.exe"}):
            self.assertEqual(bash_executable(), "/fixture/Git Bash/bash.exe")
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(bash_executable(), "bash")

    def test_dispatch_is_main_only(self):
        self.context.update(eventName="workflow_dispatch", ref="refs/heads/main")
        self.assertEqual(self.execute("Resolve protected main evaluator generation").returncode, 0)
        self.context["ref"] = "refs/heads/candidate"
        self.assertNotEqual(self.execute("Resolve protected main evaluator generation").returncode, 0)

    def test_production_order_and_subject_separation(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("ref: ${{ steps.evaluator.outputs.sha }}", text)
        self.assertNotIn("ref: ${{ env.EVALUATOR_SHA }}", text)
        self.assertIn('git fetch --no-tags origin "$BASE_SHA" "$SUBJECT_SHA"', text)
        self.assertIn('--base-sha "$BASE_SHA" --subject-sha "$SUBJECT_SHA"', text)
        names = ["Resolve protected main evaluator generation", "Checkout trusted evaluator",
                 "Fetch and bind subject Git object", "Revalidate protected evaluator before execution",
                 "Run trusted exact-tree evaluator", "Revalidate protected evaluator after execution"]
        positions = [text.index("      - name: " + name) for name in names]
        self.assertEqual(positions, sorted(positions))
        self.assertNotIn("contents: write", text)
        self.assertIn("persist-credentials: false", text)


if __name__ == "__main__":
    unittest.main()
