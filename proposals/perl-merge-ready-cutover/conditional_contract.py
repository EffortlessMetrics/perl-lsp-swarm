"""Inactive, closed-shape proposal for a future canonical validator extension.

This does not wrap, exempt, suppress, or modify the current validator. Only RIPR
is modeled. Accepting this shape still needs an owner-reviewed schema/parser/
producer-index change in validate_gate_enforcement_contract.py before cutover.
"""

CONTRACT = "pr-material-required-name-v1"
WORKFLOW = ".github/workflows/ripr.yml"
JOB = "ripr"
NAME = "ripr+ New Gap Gate"
IGNORED = "Ignored metadata / " + NAME
M = ("github.event_name == 'pull_request' && ("
     "contains(fromJSON('[\"opened\",\"synchronize\",\"reopened\",\"ready_for_review\",\"converted_to_draft\"]'), github.event.action) || "
     "(contains(fromJSON('[\"labeled\",\"unlabeled\"]'), github.event.action) && github.event.label.name == 'merge-ready') || "
     "(github.event.action == 'edited' && github.event.changes.base != null))")
NAME_EXPRESSION = "${{ (" + M + ") && '" + NAME + "' || '" + IGNORED + "' }}"
CONDITION = "${{ always() && (" + M + ") }}"
GROUP = ("${{ github.workflow }}-${{ (" + M + ") && "
         "format('pr-{0}', github.event.pull_request.number) || format('ignored-{0}', github.run_id) }}")


def example():
    return dict(contract=CONTRACT, workflow=WORKFLOW, job=JOB,
                required_name=NAME, ignored_name=IGNORED,
                name_expression=NAME_EXPRESSION, condition=CONDITION,
                concurrency_group=GROUP, cancel_in_progress=True,
                continue_on_error=False, path_filters=False)


def recognizes(row, *, protected_names, other_emitted_names=()):
    """Exact finite grammar, not arbitrary expression evaluation or an exemption."""
    if row != example():
        return False
    if NAME not in protected_names or IGNORED in protected_names:
        return False
    # Unique required producer remains mandatory across static/conditional jobs.
    return NAME not in other_emitted_names and IGNORED not in other_emitted_names
