#!/usr/bin/env python3
"""Source-routing guards for the cutover runbook (#17080/#17903).

These assertions prevent the demonstrated retired instructions from becoming
current again. They do not establish native execution, safety of an apply,
protection completeness, or the semantics of arbitrary natural-language prose.
"""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
RUNBOOK = "docs/ci/qualification-cutover.md"
HISTORY = "docs/ci/qualification-workflow-composition.md"
WORKFLOW = ".github/workflows/gate-enforcement-contract.yml"


def text(path):
    return (ROOT / path).read_text(encoding="utf-8")


def compact(value):
    return " ".join(value.split())


class CutoverDocumentationTests(unittest.TestCase):
    def test_current_route_is_review_then_native_group_not_manual_bootstrap(self):
        source = text(RUNBOOK)
        self.assertIn("current transition plan; deployment NOT_PROVEN", source)
        self.assertIn("October 21", source)
        self.assertIn("October 22–23, 2026", source)
        stages = (
            "focused/affected proof, substantive review and joined repairs in-session",
            "conditional landing authorization",
            "verified native enqueue",
            "selected integration qualification on actual merge-group G",
            "protected native landing",
            "cheap landed M reconciliation",
        )
        positions = []
        for stage in stages:
            self.assertIn(stage, source)
            positions.append(source.index(stage))
        self.assertEqual(positions, sorted(positions))

    def test_retired_execution_instructions_cannot_reenter_current_runbook(self):
        source = compact(text(RUNBOOK))
        for retired in (
            "Existing main/PR validation still runs normally.",
            "select the qualified public central SHA in an explicit manual-only native rehearsal route",
            "No protection setting changes are part of cutover or rollback.",
            "verify ordinary draft/Ready/update and generated-PR behavior through the restored generation's selected proof",
        ):
            with self.subTest(retired=retired):
                self.assertNotIn(retired, source)

    def test_legacy_composition_has_a_preceding_historical_boundary(self):
        source = text(HISTORY)
        prefix = source.split("##", 1)[0]
        self.assertIn("historical operating sequence; reusable source/evidence record", prefix)
        self.assertIn("[qualification-cutover.md](qualification-cutover.md)", prefix)
        self.assertIn("do not authorize pre-queue hosted qualification", compact(prefix))
        self.assertIn("Dated source and protection observations", prefix)
        # Keep the old source/diagnostic record, rather than erase its evidence.
        self.assertIn("## Operator-admitted actual local rehearsal", source)
        self.assertIn("rehearse_central_qualification.py", source)

    def test_first_installation_names_missing_operation_and_three_generations(self):
        source = text(RUNBOOK)
        self.assertIn("## First-installation decision", source)
        section = source.partition("## First-installation decision")[2].partition("## Authorized canary")[0]
        self.assertIn("first source-to-protected-main", section)
        for generation in ("Source", "Enforcement", "Workflow enablement"):
            self.assertIn("| " + generation + " |", section)
        self.assertIn("not an accepted live sequence", compact(section))
        self.assertIn("Zero jobs or an empty filtered CLI listing is not zero workflow runs", compact(section))

    def test_rollback_does_not_restore_a_prohibited_execution_path(self):
        source = text(RUNBOOK)
        self.assertIn("## Rollback", source)
        section = compact(source.partition("## Rollback")[2].partition("## Retained historical evidence")[0])
        self.assertIn("source, enforcement and workflow enablement", section)
        self.assertIn("does not authorize executing those subscriptions", section)
        self.assertIn("no temporary ordinary-lifecycle bootstrap or rollback exception", section)
        self.assertIn("never leave cheap admission protecting direct merges", section)

    def test_historical_proof_subjects_and_limits_are_retained(self):
        source = text(RUNBOOK)
        self.assertIn("## Retained historical evidence", source)
        history = source.partition("## Retained historical evidence")[2]
        for identity in (
            "b77041f5a8fc7513a39e423673a35968a11f6b5b",
            "877349ea9c4fb17d8bc8e5c903b152f556adde8c",
            "fff84023cf0b25ccb4872c5a8f7cdd77fec0cb0f",
            "ea5cf42e896f436cddc8d96707d67ee21ef83c5b11bf635999d29a8f134efcde",
            "4.312 seconds", "15m52s", "NOT_PROVEN",
        ):
            self.assertIn(identity, history)
        self.assertIn("does not qualify this plan", compact(history))

    def test_existing_advisory_command_invokes_the_source_guards(self):
        source = text(WORKFLOW)
        self.assertIn("python3 -m unittest", source)
        command = source.partition("python3 -m unittest")[2].partition("test_status=$?")[0]
        self.assertIn("scripts/ci/test_qualification_cutover_docs.py", command)
        for existing in ("test_queue_qualification_source.py", "test_required_checks_doc_contract.py"):
            self.assertIn(existing, command)
        self.assertIn("merge_group:\n    types: [checks_requested]", source)
        self.assertIn("permissions:\n  contents: read", source)

    def test_current_perimeter_and_required_fanin_keep_missing_native_joins_explicit(self):
        source = text(RUNBOOK)
        for current in ('## Applied source perimeter and required integration fan-in',
                        '`ci.yml:integration-required`', '--no-development-events',
                        '.ci/retired-workflows/main-history-event.yml',
                        'zero-run status `NOT_PROVEN`', 'complete G membership',
                        '**applied in this source candidate**', 'Runtime activation remains'):
            self.assertIn(current, source)
        self.assertNotIn('replacement is still **unapplied**', source)
        self.assertNotIn('Do not apply `.ci/staged/17903-root-triggers.patch` as the selected queue migration:', source)

    def test_agent_entrypoints_and_finishing_routes_follow_authorized_native_queue(self):
        method = text('docs/agents/DEVELOPMENT_METHOD.md')
        self.assertIn('exact-H product/scope judgment and conditional landing authorization', method)
        self.assertIn('→ verified native enqueue', method)
        self.assertIn('→ selected integration qualification on actual merge-group G', method)
        self.assertIn('→ cheap landed M read-back and reconciliation', method)
        self.assertLess(method.index('conditional landing authorization'), method.index('→ verified native enqueue'))
        for path in ('AGENTS.md', 'CLAUDE.md'):
            source = compact(text(path))
            self.assertIn('Ordinary Draft, Ready and publication start no Actions', source)
            self.assertIn('Authorization precedes enqueue', source)
            self.assertNotIn('Never invent the missing request command or arm routine auto-merge/queue.', source)
        for provider in ('.agents', '.claude'):
            for name in ('finish-pr', 'verify-live-ci', 'merge-reconcile'):
                source = compact(text(f'{provider}/skills/{name}/SKILL.md'))
                self.assertIn('native', source)
                self.assertIn('NOT_PROVEN', source)
                for retired in ('Do not arm routine auto-merge or enqueue.',
                                'pending qualification does not authorize auto-merge, enqueueing',
                                'one verified-clean empty commit',
                                'ci: re-request fresh merge-tree checks'):
                    self.assertNotIn(retired, source)


if __name__ == "__main__":
    unittest.main()
