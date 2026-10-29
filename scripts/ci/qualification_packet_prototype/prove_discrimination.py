"""Execute a controlled always-pass mutant against the regression oracle."""
import io
import json
import unittest

import test_evaluator
from evaluator import evaluate
from fixtures import fixture


def main():
    positive = evaluate(fixture())
    if positive['status'] != 'SYNTHETIC_PASS':
        raise RuntimeError('positive fixture did not execute successfully')
    original = test_evaluator.evaluate
    # This controlled always-pass mutant ignores input and returns a cached
    # positive result. It demonstrates oracle discrimination, not a replay of
    # the original aggregate-field consumer implementation.
    test_evaluator.evaluate = lambda document: positive
    names = ('test_ordinary_outcome_and_footer_contradictions',
             'test_storage_summary_and_redundant_inventory_consistency',
             'test_optional_retained_attribution',
             'test_consistently_rehashed_nonfinite_and_noncanonical_units',
             'test_nonfinite_json_rejected_even_in_advisory_fields')
    try:
        suite = unittest.TestSuite(test_evaluator.Controls(name) for name in names)
        result = unittest.TextTestRunner(stream=io.StringIO()).run(suite)
    finally:
        test_evaluator.evaluate = original
    summary = {'mutant': 'always-pass', 'positive_control': positive['status'],
               'groups': result.testsRun, 'assertion_failures': len(result.failures),
               'instrument_errors': len(result.errors),
               'production_qualification': positive['production_qualification']}
    print(json.dumps(summary, sort_keys=True))
    return 0 if len(result.failures) == 30 and not result.errors else 1


if __name__ == '__main__':
    raise SystemExit(main())
