#!/usr/bin/env bash
# Regression: scripts/ci/check-pr-review-convergence-core accumulates large
# paged GraphQL results in two places:
#
#   1. `paginate_pr_connection` collects every GraphQL page.
#   2. The `UNREACHABLE_FIX_COMMITS` loop (around line 640) accumulates one
#      entry per disposition marker that fails the reachability check.
#
# Both were originally implemented as
#
#   jq -n --argjson a "$a" --argjson b "$b" '$a + $b'
#
# which passes both inputs through `argv` on every iteration. Once the
# accumulator grows past the platform's `argv` length cap, `execvp()` returns
# E2BIG and the page is silently dropped or the helper aborts. On Windows
# that cap is ~8-32 KB, so a PR with ~30+ review threads (each thread
# embeds `comments(first: 20) { ... }`) hits the limit at page ~3 and the
# verdict silently degrades to `state: NOT_PROVEN` /
# `classification: NOT_PROVEN / reason: github_facts_unavailable`.
#
# The paginator must also avoid reparsing the entire accumulated prefix on
# each page (#19239). Exercise its live gh seam with real jq: preserve all
# ordered nodes, literal bodies and final-page receipts, refuse late failures,
# and bound actual jq input work/processes as page counts grow. A small argv
# ceiling makes the original portability defect observable on every host.
# Keep the independent unreachable-fix accumulator's existing guard.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE="$SCRIPT_DIR/../ci/check-pr-review-convergence-core"

PASS_COUNT=0
FAIL_COUNT=0

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

command -v jq >/dev/null 2>&1 || { echo "ERROR: jq required"; exit 1; }
[[ -f "$CORE" ]] || { echo "ERROR: $CORE missing"; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "ERROR: python3 required"; exit 1; }

# ---- Data pins ---------------------------------------------------------
SMALL_A=$(jq -nc '[range(0; 150) | {page: 0, id: (. | tostring), body: "thread body for node"}]')
SMALL_B=$(jq -nc '[range(0; 50)  | {page: 1, id: (. | tostring), body: "thread body for node"}]')
SMALL_MARKER='{"page":99,"id":"u","body":"unreachable"}'

canonical() { jq -S . <<<"$1"; }

EXPECTED=$(jq -nc --argjson a "$SMALL_A" --argjson b "$SMALL_B" '$a + $b')
NEW_FORM=$(printf '%s\n%s\n' "$SMALL_A" "$SMALL_B" | jq -s 'add')
if [[ "$(canonical "$NEW_FORM")" == "$(canonical "$EXPECTED")" ]]; then
    pass "stdin-accumulator form equals direct concatenation at 200 nodes"
else
    fail "stdin-accumulator form diverged from direct concat at 200 nodes"
fi

EXPECTED2=$(jq -nc --argjson a "$SMALL_A" --argjson m "$SMALL_MARKER" '$a + [$m]')
PIPE2=$(printf '%s\n[%s]\n' "$SMALL_A" "$SMALL_MARKER" | jq -s '.[0] + .[1]')
if [[ "$(canonical "$PIPE2")" == "$(canonical "$EXPECTED2")" ]]; then
    pass "stdin-accumulator wrapper form equals '\$a + [\$m]'"
else
    fail "stdin-accumulator wrapper form diverged"
fi

# ---- Real paginated CLI ------------------------------------------------
# This deliberately leaves CONVERGENCE_TEST_FIXTURE_DIR unset for the subject:
# that shortcut bypasses the paginator. A flattened fixture is only the oracle
# for the complete report/verdict produced from the same acquired nodes.
if python3 - "$CORE" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

CORE = Path(sys.argv.pop(1)).resolve()
REAL_JQ = shutil.which("jq")
HEAD = "a" * 40
FIELDS = ("latestReviews", "reviewThreads", "comments")
META = {"headRefOid": HEAD, "reviewRequests": [], "author": {"login": "writer"},
        "reviewDecision": "", "isDraft": False}

GH = r'''#!/usr/bin/env python3
import json, os, re, sys
from pathlib import Path
case = json.loads(Path(os.environ["PAGINATION_CASE"]).read_text())
args = sys.argv[1:]
if args[:2] == ["pr", "view"]:
    print(json.dumps(case["meta"]))
elif args[:2] == ["api", "graphql"]:
    query = next(arg[6:] for arg in args if arg.startswith("query="))
    field = re.search(r"\b(latestReviews|reviewThreads|comments)\(first: 50", query)[1]
    cursor = re.search(r'after: "page-(\d+)"', query)
    index = int(cursor[1]) + 1 if cursor else 0
    with open(os.environ["PAGINATION_GH_LOG"], "a") as log:
        log.write(json.dumps([field, index]) + "\n")
    failure = case.get("failure", {})
    fails = failure.get("field") == field and failure.get("index") == index
    if fails and failure["kind"] == "fetch":
        print("late-page transport failure", file=sys.stderr)
        sys.exit(37)
    if fails and failure["kind"] == "json":
        print("{invalid-json")
        sys.exit(0)
    if fails and failure["kind"] == "empty":
        sys.exit(0)
    pages = case["pages"][field]
    assert index < len(pages), (field, index)
    value = {"nodes": pages[index], "pageInfo": {
        "hasNextPage": index + 1 < len(pages), "endCursor": "page-" + str(index)}}
    if fails and failure["kind"] == "nodes":
        value["nodes"] = None
    if fails and failure["kind"] == "pageInfo":
        del value["pageInfo"]["hasNextPage"]
    print(json.dumps({"data": {"repository": {"pullRequest": {field: value}}}}))
    if fails and failure["kind"] == "multiple":
        print(json.dumps({"data": {"repository": {"pullRequest": {field: value}}}}))
else:
    sys.exit("unexpected gh request: " + repr(args))
'''

JQ = r'''#!/usr/bin/env python3
import hashlib, json, os, subprocess, sys
args = sys.argv[1:]
data = b"" if any(arg in ("-n", "-nc") for arg in args) else sys.stdin.buffer.read()
record = {"stdin_bytes": len(data), "argv_bytes": sum(len(a.encode()) + 1 for a in args)}
try:
    value = json.loads(data)
    if isinstance(value, list):
        canonical = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
        record["array_sha256"] = hashlib.sha256(canonical).hexdigest()
except (ValueError, UnicodeError):
    pass
with open(os.environ["PAGINATION_JQ_LOG"], "a") as log:
    log.write(json.dumps(record) + "\n")
if record["argv_bytes"] > 32 * 1024:
    sys.exit("test argv ceiling exceeded: paginated JSON must travel through stdin")
result = subprocess.run([os.environ["PAGINATION_REAL_JQ"], *args], input=data)
sys.exit(result.returncode)
'''


def digest(value):
    canonical = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


def comment_pages(count):
    pages = []
    for page in range(count):
        nodes = []
        for index in range(8):
            body = f"page {page} node {index}: " + "x" * 2048
            body += '\nLiteral "quotes", \\slashes, $(not-a-command); snowman ☃\n'
            if page == count - 1 and index == 7:
                body += '<!-- review-run:v1 {"kind":"deep","status":"running"} -->'
            nodes.append({"author": {"login": "reviewer"}, "body": body})
        pages.append(nodes)
    return pages


class PaginatedCLI(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="review-pagination-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        for name, source in (("gh", GH), ("jq", JQ)):
            script = self.root / name
            script.write_text(source, encoding="utf-8")
            script.chmod(0o755)
        self.env = os.environ.copy()
        for key in ("CONVERGENCE_TEST_FIXTURE_DIR", "REVIEW_PROTOCOL_ENFORCE",
                    "CONVERGENCE_CORE_SCHEMA_VERSION"):
            self.env.pop(key, None)
        self.env.update(PATH=str(self.root) + os.pathsep + self.env["PATH"],
                        PAGINATION_REAL_JQ=REAL_JQ,
                        PAGINATION_CASE=str(self.root / "case.json"),
                        PAGINATION_GH_LOG=str(self.root / "gh.jsonl"),
                        PAGINATION_JQ_LOG=str(self.root / "jq.jsonl"))

    def run_case(self, pages, failure=None, fixture=False):
        case = {"meta": META, "pages": pages, "failure": failure or {}}
        (self.root / "case.json").write_text(json.dumps(case), encoding="utf-8")
        for name in ("gh.jsonl", "jq.jsonl"):
            (self.root / name).write_text("")
        env = self.env.copy()
        if fixture:
            fixture_dir = self.root / "fixture"
            fixture_dir.mkdir(exist_ok=True)
            (fixture_dir / "pr_view.json").write_text(json.dumps(META))
            for field, field_pages in pages.items():
                name = "pr_comments" if field == "comments" else field
                (fixture_dir / (name + ".json")).write_text(
                    json.dumps([node for page in field_pages for node in page]))
            env["CONVERGENCE_TEST_FIXTURE_DIR"] = str(fixture_dir)
        result = subprocess.run(["bash", str(CORE), "9999", "test-owner/test-repo"],
                                env=env, capture_output=True, timeout=45)
        logs = [[json.loads(line) for line in (self.root / name).read_text().splitlines()]
                for name in ("gh.jsonl", "jq.jsonl")]
        return result, *logs

    def report(self, result):
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        return json.loads(result.stdout[result.stdout.index(b"{"):])

    def test_empty_connections(self):
        result, calls, trace = self.run_case({field: [[]] for field in FIELDS})
        report = self.report(result)
        self.assertEqual(calls, [[field, 0] for field in FIELDS])
        self.assertTrue(report["converged"])
        self.assertEqual(report["human_review_count"], 0)
        self.assertEqual(report["resolved_threads"], 0)
        self.assertEqual(report["review_runs_in_flight"], 0)
        self.assertTrue(trace)

    def test_complete_ordered_nodes_and_report_match_flattened_oracle(self):
        pages = {"latestReviews": [], "reviewThreads": [], "comments": comment_pages(3)}
        for index in range(3):
            pages["latestReviews"].append([{
                "author": {"login": f"reviewer-{index}", "__typename": "User"},
                "state": "APPROVED", "commit": {"oid": HEAD}}])
            pages["reviewThreads"].append([{
                "id": f"thread-{index}", "path": f"src/file-{index}.rs",
                "isResolved": True, "isOutdated": index % 2 == 0,
                "comments": {"totalCount": 2, "nodes": [
                    {"author": {"login": "reviewer"}, "body": 'literal "body" ☃'},
                    {"author": {"login": "writer"}, "body": "disposition reply"}]}}])
        actual, calls, trace = self.run_case(pages)
        report = self.report(actual)
        self.assertEqual(calls, [[field, index] for field in FIELDS for index in range(3)])
        for field, field_pages in pages.items():
            expected = [node for page in field_pages for node in page]
            self.assertIn(digest(expected), [row.get("array_sha256") for row in trace], field)
        self.assertEqual(report["current_human_review_count"], 3)
        self.assertEqual(report["resolved_threads"], 3)
        self.assertEqual(report["review_runs_in_flight"], 1, "last-page receipt lost")
        oracle, fixture_calls, _ = self.run_case(pages, fixture=True)
        self.assertEqual(fixture_calls, [], "oracle must not query GitHub")
        self.assertEqual((actual.returncode, actual.stdout, actual.stderr),
                         (oracle.returncode, oracle.stdout, oracle.stderr))

    def test_jq_work_grows_linearly_without_json_in_argv(self):
        measurements = []
        for count in (2, 6):
            pages = {field: [[]] for field in FIELDS}
            pages["comments"] = comment_pages(count)
            result, calls, trace = self.run_case(pages)
            self.assertEqual(self.report(result)["review_runs_in_flight"], 1)
            self.assertEqual(len(calls), count + 2)
            measurements.append((len(trace), sum(row["stdin_bytes"] for row in trace)))
        small, large = measurements
        print(f"jq work at 2/6 comment pages: {small} / {large} (calls, stdin bytes)")
        # Four more pages need at most three per-page jq extractions. Folding
        # every growing prefix adds a fourth process and quadratic input work.
        self.assertLessEqual(large[0] - small[0], 3 * (6 - 2), measurements)
        self.assertLessEqual(large[1], small[1] * 3.1, measurements)

    def test_late_page_failure_never_publishes_partial_success(self):
        pages = {field: [[]] for field in FIELDS}
        pages["comments"] = comment_pages(4)
        for kind in ("fetch", "json", "empty", "multiple", "nodes", "pageInfo"):
            with self.subTest(kind=kind):
                result, calls, _ = self.run_case(
                    pages, {"field": "comments", "index": 2, "kind": kind})
                self.assertEqual(result.returncode, 2, result.stderr.decode())
                self.assertEqual(result.stdout, b"", "partial facts/verdict escaped")
                self.assertIn(b"ERROR:", result.stderr)
                self.assertEqual([index for field, index in calls if field == "comments"],
                                 [0, 1, 2])


unittest.main(verbosity=2)
PY
then
    pass "real paginated CLI preserves complete facts with linear stdin work"
else
    fail "real paginated CLI regression suite"
fi

# ---- Independent unreachable-fix guard ---------------------------------
# Keep this separate accumulator protected; changing paginator storage must not
# weaken the existing argv regression at the disposition reachability site.

OUT2=$(mktemp)
trap 'rm -f "$OUT2"' EXIT

python3 - "$CORE" "UNREACHABLE_FIX_COMMITS" "jq -s '.[0] + .[1]'" "unreachable-fix-commits accumulator" <<'PY' >"$OUT2"
import re
import sys

path, var, good_pattern, label = sys.argv[1:5]

with open(path, 'r', encoding='utf-8') as f:
    lines = f.readlines()

def strip_comment(line):
    return re.sub(r'\s*#.*$', '', line.rstrip('\n'))

code_lines = [(n, strip_comment(raw)) for n, raw in enumerate(lines, 1)
              if strip_comment(raw).strip()]

assignment = re.compile(rf'\s*{re.escape(var)}\s*=')
bad = re.compile(r'jq\s+-n\s+--argjson')

found = False
for n, code in code_lines:
    if assignment.match(code) and 'jq' in code:
        if bad.search(code):
            print(f"FAIL {label}: line {n} still uses jq -n --argjson")
        elif good_pattern not in code:
            print(f"FAIL {label}: line {n} does not contain {good_pattern!r}")
        else:
            print(f"PASS {label}: line {n} uses stdin pipe ({good_pattern!r})")
        found = True
        break

if not found:
    print(f"FAIL {label}: could not locate {var}= jq assignment in {path}")
PY

while read -r line; do
    if [[ "$line" == PASS* ]]; then
        pass "${line#PASS }"
    elif [[ "$line" == FAIL* ]]; then
        fail "${line#FAIL }"
    fi
done <"$OUT2"

echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="
[[ "$FAIL_COUNT" -eq 0 ]]
