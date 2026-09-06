#!/usr/bin/env python3
"""Discriminating falsifiers for the Dependabot source contract (#13478).

Each negative mutates one realistic wrong candidate. Tests are the claim
owner: the validator is split around the cases they actually catch, not
the other way around.
"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from validate_dependabot_contract import (  # noqa: E402
    BOOK_POINTER,
    CANONICAL_GUIDES,
    CONFIG_PATH,
    CONTRIBUTING_GUIDE,
    GOVERNED_SURFACES,
    MANAGEMENT_GUIDE,
    POLICY_WORKFLOW,
    POPULATE_BOOK,
    QUICK_REFERENCE,
    Finding,
    ParseError,
    VALIDATOR_SCRIPT,
    inspect_workflow_wiring,
    parse_yaml_subset,
    validate,
)

ROOT = Path(__file__).resolve().parents[2]


def _ids(findings: list[Finding]) -> set[str]:
    return {finding.finding_id for finding in findings}


def _clone_surfaces(tmp: Path) -> None:
    for rel in (*GOVERNED_SURFACES, POLICY_WORKFLOW):
        src = ROOT / rel
        dest = tmp / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(src.read_bytes())


def _rewrite(tmp: Path, rel: Path, transform) -> None:
    path = tmp / rel
    path.write_text(transform(path.read_text(encoding="utf-8")), encoding="utf-8")


def _findings(tmp: Path) -> list[Finding]:
    return validate(tmp, check_wiring=False)


class YamlSubsetTests(unittest.TestCase):
    def test_committed_dependabot_yml_parses_three_rows(self) -> None:
        doc = parse_yaml_subset((ROOT / CONFIG_PATH).read_text(encoding="utf-8"))
        self.assertEqual(doc["version"], 2)
        updates = doc["updates"]
        assert isinstance(updates, list)
        self.assertEqual(len(updates), 3)
        cargo = updates[0]
        assert isinstance(cargo, dict)
        self.assertEqual(cargo["labels"], [])
        self.assertEqual(cargo["commit-message"]["prefix"], "chore")
        self.assertEqual(cargo["commit-message"]["include"], "scope")
        self.assertIn("serde", cargo["groups"])
        npm = updates[2]
        assert isinstance(npm, dict)
        self.assertEqual(npm["directory"], "/vscode-extension")
        ignores = npm["ignore"]
        assert isinstance(ignores, list)
        self.assertEqual(ignores[1]["dependency-name"], "@types/vscode")
        self.assertEqual(
            ignores[1]["update-types"],
            ["version-update:semver-major", "version-update:semver-minor"],
        )

    def test_empty_flow_sequence_is_distinct_from_omitted_key(self) -> None:
        present = parse_yaml_subset("labels: []\n")
        omitted = parse_yaml_subset("name: x\n")
        nullish = parse_yaml_subset("labels:\nname: x\n")
        self.assertEqual(present["labels"], [])
        self.assertNotIn("labels", omitted)
        self.assertIsNone(nullish["labels"])

    def test_duplicate_keys_and_unsupported_constructs_fail_closed(self) -> None:
        with self.assertRaises(ParseError):
            parse_yaml_subset("a: 1\na: 2\n")
        with self.assertRaises(ParseError):
            parse_yaml_subset("row: {a: 1}\n")
        with self.assertRaises(ParseError):
            parse_yaml_subset("body: |\n  extra\n")
        with self.assertRaises(ParseError):
            parse_yaml_subset("anchor: &foo bar\n")
        self.assertIsNone(parse_yaml_subset("updates:\n")["updates"])

    def test_malformed_flow_sequence_is_parse_error(self) -> None:
        with self.assertRaises(ParseError):
            parse_yaml_subset('labels: ["a"\n')


class CurrentRepoTests(unittest.TestCase):
    def test_current_main_surfaces_pass_together(self) -> None:
        self.assertEqual(validate(ROOT), [])

    def test_book_surface_stays_in_the_default_inventory(self) -> None:
        self.assertIn(BOOK_POINTER, GOVERNED_SURFACES)
        for guide in CANONICAL_GUIDES:
            self.assertIn(guide, GOVERNED_SURFACES)

    def test_validator_does_not_reimplement_github_scope_composition(self) -> None:
        source = (ROOT / VALIDATOR_SCRIPT).read_text(encoding="utf-8")
        self.assertNotIn("def rendered_title_prefix", source)
        self.assertNotIn("chore(deps)(deps)", source)
        self.assertNotIn("chore(deps)(deps-dev)", source)

    def test_policy_validators_wiring_is_load_bearing(self) -> None:
        workflow = (ROOT / POLICY_WORKFLOW).read_text(encoding="utf-8")
        self.assertEqual(inspect_workflow_wiring(workflow), [])


class NegativeControlTests(unittest.TestCase):
    def test_omitting_one_labels_entry_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(tmp, CONFIG_PATH, lambda text: text.replace("    labels: []\n", "", 1))
            self.assertIn("labels-omitted", _ids(_findings(tmp)))

    def test_labels_null_is_not_empty_list(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                CONFIG_PATH,
                lambda text: text.replace("    labels: []\n", "    labels:\n", 1),
            )
            self.assertIn("custom-label-without-disposition", _ids(_findings(tmp)))

    def test_restoring_chore_deps_prefix_fails_without_composition_oracle(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                CONFIG_PATH,
                lambda text: text.replace(
                    '      prefix: "chore"\n', '      prefix: "chore(deps)"\n', 1
                ),
            )
            findings = _findings(tmp)
            self.assertIn("commit-message-scope", _ids(findings))
            self.assertTrue(
                any("does not re-derive GitHub composition" in f.message for f in findings)
            )

    def test_tower_lsp_guide_claim_without_config_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                MANAGEMENT_GUIDE,
                lambda text: text.replace(
                    "- `tokio` - Async runtime\n",
                    "- `tokio` - Async runtime\n- `tower-lsp` - claimed without config\n",
                ),
            )
            findings = _findings(tmp)
            self.assertIn("ignore-guide-mismatch", _ids(findings))
            self.assertTrue(any("tower-lsp" in f.message for f in findings))

    def test_retired_label_filter_fails_in_each_canonical_guide(self) -> None:
        for guide in CANONICAL_GUIDES:
            with self.subTest(guide=guide.as_posix()), tempfile.TemporaryDirectory() as temp:
                tmp = Path(temp)
                _clone_surfaces(tmp)
                _rewrite(
                    tmp,
                    guide,
                    lambda text: text + '\n\ngh pr list --label "dependencies"\n',
                )
                self.assertIn("retired-label-filter", _ids(_findings(tmp)))

    def test_pipe_into_gh_pr_merge_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            snippet = (
                "\n```bash\n"
                'gh pr list --author "app/dependabot" --search "status:success" '
                "| gh pr merge --auto --squash\n"
                "```\n"
            )
            _rewrite(tmp, QUICK_REFERENCE, lambda text: text + snippet)
            self.assertIn("pipe-into-merge", _ids(_findings(tmp)))

    def test_looped_gh_pr_list_into_merge_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            snippet = (
                "\n```bash\n"
                'for n in $(gh pr list --author "app/dependabot" '
                '--search "status:success"); '
                'do gh pr merge "$n" --auto --squash; done\n'
                "```\n"
            )
            _rewrite(tmp, QUICK_REFERENCE, lambda text: text + snippet)
            self.assertIn("pipe-into-merge", _ids(_findings(tmp)))

    def test_status_success_called_patch_updates_without_version_delta_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)

            def mutate(text: str) -> str:
                return text.replace(
                    "# CI-passing candidates (discovery only; inspect the version delta before merge)\n"
                    'gh pr list --author "app/dependabot" --search "status:success"',
                    "# These status:success results are patch updates\n"
                    'gh pr list --author "app/dependabot" --search "status:success"',
                )

            _rewrite(tmp, QUICK_REFERENCE, mutate)
            self.assertIn("patch-query-misclassified", _ids(_findings(tmp)))

    def test_config_group_rename_while_docs_claim_old_name_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                CONFIG_PATH,
                lambda text: text.replace("      serde:\n", "      serde2:\n", 1),
            )
            findings = _findings(tmp)
            self.assertIn("group-guide-mismatch", _ids(findings))
            self.assertTrue(any("serde" in f.message for f in findings))

    def test_directory_change_while_docs_claim_old_value_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                CONFIG_PATH,
                lambda text: text.replace(
                    '    directory: "/vscode-extension"',
                    '    directory: "/ext"',
                    1,
                ),
            )
            ids = _ids(_findings(tmp))
            self.assertIn("missing-ecosystem-row", ids)
            self.assertIn("extra-ecosystem-row", ids)
            self.assertIn("directory-guide-drift", ids)

    def test_dropping_book_surface_from_inventory_fails(self) -> None:
        selected = [rel for rel in GOVERNED_SURFACES if rel != BOOK_POINTER]
        findings = validate(ROOT, surfaces=selected, check_wiring=False)
        self.assertIn("book-surface-missing", _ids(findings))

    def test_malformed_yaml_is_a_finding_not_an_empty_pass(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            (tmp / CONFIG_PATH).write_text("updates: [\n", encoding="utf-8")
            findings = _findings(tmp)
            self.assertIn("malformed-yaml", _ids(findings))
            self.assertNotEqual(findings, [])

    def test_empty_inventory_fails(self) -> None:
        findings = validate(ROOT, surfaces=[], check_wiring=False)
        self.assertEqual(
            findings,
            [
                Finding(
                    "empty-inventory",
                    ".",
                    "zero selected surfaces is failure",
                )
            ],
        )

    def test_unknown_parse_shape_is_not_no_findings(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            (tmp / CONFIG_PATH).write_text("version: 2\nupdates:\n  foo: bar\n", encoding="utf-8")
            findings = _findings(tmp)
            self.assertTrue(_ids(findings) & {"malformed-yaml", "unsupported-shape"})

    def test_duplicate_cargo_row_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)

            def mutate(text: str) -> str:
                cargo_start = text.index('  - package-ecosystem: "cargo"')
                gha_start = text.index('  - package-ecosystem: "github-actions"')
                cargo_block = text[cargo_start:gha_start]
                return text[:gha_start] + cargo_block + text[gha_start:]

            _rewrite(tmp, CONFIG_PATH, mutate)
            self.assertIn("duplicate-ecosystem-row", _ids(_findings(tmp)))

    def test_custom_label_without_disposition_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                CONFIG_PATH,
                lambda text: text.replace("    labels: []\n", '    labels: ["dependencies"]\n', 1),
            )
            self.assertIn("custom-label-without-disposition", _ids(_findings(tmp)))

    def test_missing_app_dependabot_discovery_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                CONTRIBUTING_GUIDE,
                lambda text: text.replace('gh pr list --author "app/dependabot"', "gh pr list"),
            )
            self.assertIn("app-dependabot-discovery-missing", _ids(_findings(tmp)))

    def test_book_stub_losing_contributing_pointer_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                BOOK_POINTER,
                lambda text: text.replace("CONTRIBUTING.md", "README.md"),
            )
            self.assertIn("book-pointer-missing", _ids(_findings(tmp)))

    def test_populate_book_overwrite_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                POPULATE_BOOK,
                lambda text: text
                + '\ncopy_doc "$ROOT/CONTRIBUTING.md" "$BOOK_SRC/developer/contributing.md"\n',
            )
            self.assertIn("populate-book-overwrite", _ids(_findings(tmp)))

    def test_cargo_ignore_type_flip_is_not_names_only(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                CONFIG_PATH,
                lambda text: text.replace(
                    '- dependency-name: "tree-sitter"\n'
                    '        update-types: ["version-update:semver-major"]',
                    '- dependency-name: "tree-sitter"\n'
                    '        update-types: ["version-update:semver-minor"]',
                    1,
                ),
            )
            self.assertIn("ignore-type-mismatch", _ids(_findings(tmp)))

    def test_npm_schedule_drift_is_not_masked_by_cargo_monday(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            marker = (
                "### npm Dependencies (VS Code Extension)\n\n"
                "**Schedule**: Weekly on Monday at 09:00 UTC"
            )
            replacement = (
                "### npm Dependencies (VS Code Extension)\n\n"
                "**Schedule**: Weekly on Tuesday at 09:00 UTC"
            )

            def mutate(text: str) -> str:
                if marker not in text:
                    raise AssertionError("npm schedule marker missing from fixture")
                return text.replace(marker, replacement, 1)

            _rewrite(tmp, MANAGEMENT_GUIDE, mutate)
            self.assertIn("schedule-guide-drift", _ids(_findings(tmp)))

    def test_run_commands_do_not_satisfy_path_wiring(self) -> None:
        workflow = (ROOT / POLICY_WORKFLOW).read_text(encoding="utf-8")
        mutated = workflow.replace(
            "      - 'scripts/ci/validate_dependabot_contract.py'\n",
            "",
            1,
        ).replace(
            "      - 'scripts/ci/test_validate_dependabot_contract.py'\n",
            "",
            1,
        )
        ids = _ids(inspect_workflow_wiring(mutated))
        self.assertIn("workflow-path-unwired", ids)
        self.assertNotIn("workflow-tests-unwired", ids)
        self.assertNotIn("workflow-validator-unwired", ids)

    def test_comment_only_workflow_mention_is_not_wiring(self) -> None:
        workflow = (
            "name: Policy Validators\n"
            "# python3 scripts/ci/test_validate_dependabot_contract.py\n"
            "# python3 scripts/ci/validate_dependabot_contract.py --repo-root .\n"
            "# .github/dependabot.yml\n"
        )
        ids = _ids(inspect_workflow_wiring(workflow))
        self.assertIn("workflow-tests-unwired", ids)
        self.assertIn("workflow-validator-unwired", ids)
        self.assertIn("workflow-path-unwired", ids)

    def test_omitting_include_scope_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                CONFIG_PATH,
                lambda text: text.replace('      include: "scope"\n', "", 1),
            )
            self.assertIn("commit-message-scope", _ids(_findings(tmp)))

    def test_unreadable_config_is_a_finding(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            (tmp / CONFIG_PATH).unlink()
            self.assertIn("unreadable-source", _ids(_findings(tmp)))

    def test_master_as_this_repo_default_branch_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone_surfaces(tmp)
            _rewrite(
                tmp,
                MANAGEMENT_GUIDE,
                lambda text: text + "\n\nTarget pull requests to `master`.\n",
            )
            self.assertIn("master-as-default-branch", _ids(_findings(tmp)))


if __name__ == "__main__":
    unittest.main()
