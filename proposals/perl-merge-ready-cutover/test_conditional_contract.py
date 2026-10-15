import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

from conditional_contract import (NAME, IGNORED, NAME_EXPRESSION, CONDITION, GROUP,
                                  example, recognizes)
from model import Event, material, result_name, group

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "canonical_gate_fixture", ROOT / "scripts/ci/test_validate_gate_enforcement_contract.py")
canonical = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = canonical
SPEC.loader.exec_module(canonical)


class ConditionalRequiredNameContract(unittest.TestCase):
    def test_current_canonical_accepts_static_control_and_rejects_dynamic_name(self):
        for dynamic in (False, True):
            workflow = canonical.WORKFLOW.replace("name: Compile All Targets\n",
                                                   "name: " + NAME + "\n")
            if dynamic:
                workflow = workflow.replace("name: " + NAME + "\n",
                                            "name: " + NAME_EXPRESSION + "\n")
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                checks = canonical.REQUIRED.replace('name = "Compile All Targets"',
                                                     'name = "' + NAME + '"')
                policy = canonical.fixture(root, checks, workflow=workflow)
                result = canonical.contract.validate(root, policy)
            codes = {finding["code"] for finding in result["findings"]}
            if dynamic:
                self.assertIn("job_name_not_static", codes)
                self.assertIn("mapped_producer_not_indexed", codes)
                self.assertIn("duplicate_emitted_context", codes)
            else:
                self.assertEqual(result["status"], "SUCCESS")

    def test_only_explicit_ripr_closed_shape_can_be_recognized(self):
        self.assertTrue(recognizes(example(), protected_names={NAME}))
        for key in example():
            value = example()[key]
            bad = not value if isinstance(value, bool) else value + " unknown"
            with self.subTest(field=key):
                self.assertFalse(recognizes(example() | {key: bad}, protected_names={NAME}))
        self.assertFalse(recognizes(example() | {"exemption": True}, protected_names={NAME}))

    def test_no_fallback_collision_or_second_producer(self):
        self.assertFalse(recognizes(example(), protected_names={NAME, IGNORED}))
        self.assertFalse(recognizes(example(), protected_names=set()))
        self.assertFalse(recognizes(example(), protected_names={NAME},
                                    other_emitted_names={NAME}))
        self.assertFalse(recognizes(example(), protected_names={NAME},
                                    other_emitted_names={IGNORED}))

    def test_inverted_or_sticky_or_user_supplied_names_are_not_supported(self):
        for expression in (NAME_EXPRESSION.replace("&&", "||", 1),
                           "${{ github.event.label.name }}", NAME,
                           NAME_EXPRESSION.replace("github.event.label.name", "github.event.pull_request.labels")):
            self.assertFalse(recognizes(example() | {"name_expression": expression},
                                        protected_names={NAME}))
        self.assertFalse(recognizes(example() | {"condition": "${{ false }}"},
                                    protected_names={NAME}))

    def test_material_partition_never_skips_a_protected_name(self):
        # Independent truth-table oracle for the finite event partition. The model
        # remains illustrative; hosted expression evaluation is a separate canary.
        for action in ("opened", "synchronize", "reopened", "ready_for_review",
                       "converted_to_draft", "labeled", "unlabeled", "edited"):
            for label in ("merge-ready", "bug", ""):
                event = Event(action, label)
                relevant = action in {"opened", "synchronize", "reopened",
                                      "ready_for_review", "converted_to_draft"} or (
                    action in {"labeled", "unlabeled"} and label == "merge-ready")
                self.assertEqual(material(event), relevant)
                self.assertEqual(result_name(event, NAME), NAME if relevant else IGNORED)
                self.assertEqual(group(event, "ripr"),
                                 "ripr-pr-1" if relevant else "ripr-ignored-1")


if __name__ == "__main__":
    unittest.main()
