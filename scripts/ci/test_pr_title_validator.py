"""Execute the retained title semantics against the exact prior production body."""
import json
from pathlib import Path
import shutil
import subprocess
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
LEGACY = ROOT / ".ci/retired-workflows/pr-title-check.yml"
BRIDGE = r"""
const fs = require('node:fs');
const {evaluateOne, validateMembers} = require(process.argv[1]);
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
const AsyncFunction = Object.getPrototypeOf(async function(){}).constructor;
const legacy = new AsyncFunction('github', 'core', 'context', input.legacy);
async function one(item, historical) {
  const writes = [], failures = [], warnings = [];
  const record = operation => async args => {
    if (!historical) throw new Error('G attempted a real GitHub write: ' + operation);
    writes.push({operation, args}); return {data:{}};
  };
  const github = {rest:{issues:{
    get: async args => { if(item.unavailable) throw new Error('not found'); return {data:{number:args.issue_number}}; },
    listComments: async () => ({data:item.comments || []}),
    createComment:record('createComment'), updateComment:record('updateComment'),
    addLabels:record('addLabels'), removeLabel:record('removeLabel'),
  }}};
  const core = {info:()=>{}, warning:x=>warnings.push(x), setFailed:x=>failures.push(x)};
  const context = {actor:item.actor || 'maintainer', repo:{owner:'EffortlessMetrics',repo:'perl-lsp-swarm'},
                   payload:{pull_request:item.pr}};
  if (historical) { await legacy(github,core,context); return {failures,writes,warnings}; }
  const result = await evaluateOne({github,core,context},item.pr,{allowEventActor:input.authoring});
  return {failures,writes:result.housekeeping,warnings};
}
(async()=>{
  if (input.members) {
    const failures=[];
    const github={rest:{issues:{get:async()=>({data:{}}),listComments:async()=>({data:[]})}}};
    const core={info:()=>{},warning:()=>{},setFailed:x=>failures.push(x)};
    const context={actor:'dependabot[bot]',repo:{owner:'EffortlessMetrics',repo:'perl-lsp-swarm'}};
    const results=await validateMembers({github,core,context},input.members);
    process.stdout.write(JSON.stringify({failures,results}));return;
  }
  const result=[];
  for(const item of input.cases) result.push({legacy:await one(item,true),current:await one(item,false)});
  process.stdout.write(JSON.stringify(result));
})().catch(e=>{console.error(e);process.exit(1)});
"""


def pr(title, *, login="contributor", labels=(), number=1):
    return {"number": number, "title": title, "user": {"login": login},
            "labels": [{"name": label} for label in labels]}


@unittest.skipUnless(shutil.which("node"), "native Node is required")
class TitleValidatorSemanticsTests(unittest.TestCase):
    def run_node(self, **input):
        # The exact archived workflow ends with this sole literal script block.
        # Reading its bytes needs no newly installed parser on the native owner.
        marker = "          script: |\n"
        self.assertEqual(LEGACY.read_text().count(marker), 1)
        input["legacy"] = textwrap.dedent(LEGACY.read_text().split(marker, 1)[1])
        result = subprocess.run(["node", "-e", BRIDGE, str(ROOT / "scripts/ci/pr_title_validator.js")],
                                input=json.dumps(input), capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_prior_pass_failure_exempt_placeholder_warning_and_housekeeping_semantics_survive(self):
        titles = ["feat: proof (#42)", "no issue reference", "feat: proof (#0)", "feat: proof (#0000)",
                  "#0 then #42", "#42 then #0", "#0007", "v1.2.3", "1.2.3", "  v1.2.3  ",
                  "feat: v1.2.3#42", "feat: word#42", "feat: (#42) and (#43)", "feat: #42x", "feat: #42_",
                  "feat: #42.1", "feat: é#42", "feat: #42é", "#0000 (#00000)"]
        cases = [{"pr": pr(text)} for text in titles]
        cases += [{"pr": pr("no issue", login="dependabot[bot]")},
                  {"pr": pr("no issue", login="dependabot-preview")},
                  {"pr": pr("no issue", labels=["skip-title-check"])},
                  {"pr": pr("real (#42)", labels=["needs-issue-link"])},
                  {"pr": pr("unknown (#42)"), "unavailable": True},
                  {"pr": pr("placeholder (#0)", labels=["needs-issue-link"])},
                  {"pr": pr("no issue"), "comments": [{"id": 8, "user": {"type": "Bot"},
                                                         "body": "<!-- pr-title-check-bot --> prior"}]}]
        for case, result in zip(cases, self.run_node(cases=cases, authoring=False)):
            with self.subTest(title=case["pr"]["title"]):
                self.assertEqual(result["current"], result["legacy"])

    def test_historical_actor_exemption_remains_explicit_authoring_only(self):
        cases = [{"pr": pr("no issue"), "actor": "dependabot[bot]"}]
        authoring = self.run_node(cases=cases, authoring=True)[0]
        self.assertEqual(authoring["current"], authoring["legacy"])
        native_group = self.run_node(cases=cases, authoring=False)[0]
        self.assertFalse(native_group["legacy"]["failures"])
        self.assertTrue(native_group["current"]["failures"])

    def test_group_actor_cannot_exempt_unrelated_members_or_hide_second_failure(self):
        receipt = {"schema_id": "perl.native-queue-title.v1", "schema_version": 1, "members": [
            {"pull_number": 1, "head_sha": "a" * 40, "pull": pr("bot metadata", login="dependabot[bot]")},
            {"pull_number": 2, "head_sha": "b" * 40, "pull": pr("no issue", number=2)},
            {"pull_number": 3, "head_sha": "c" * 40, "pull": pr("valid (#42)", number=3)},
        ]}
        result = self.run_node(members=receipt)
        self.assertEqual([row["verdict"] for row in result["results"]], ["success", "failure", "success"])
        self.assertEqual(len(result["failures"]), 1)


if __name__ == "__main__":
    unittest.main()
