#!/usr/bin/env bash
# Reject a RIPR document whose own producer marks its evidence unusable.
# Optional fields remain optional on complete 0.11.0 output; every field the
# producer does emit must explicitly support downstream consumption (#19499).
set -euo pipefail

if [ "$#" -ne 1 ]; then
    printf 'usage: %s RAW_CHECK_JSON\n' "$0" >&2
    exit 2
fi
raw_check=$1

if jq -e '
    (if .run_limitations == null then true else
        ((.run_limitations | type == "array") and
         (.run_limitations | all(.downstream_consumable == true)))
    end) and
    (.analysis_scope.downstream_consumable == null or
        .analysis_scope.downstream_consumable == true) and
    (.analysis_scope.gate_eligibility == null or
        .analysis_scope.gate_eligibility == "eligible") and
    (.analysis_outcome.document_complete == null or
        .analysis_outcome.document_complete == true)
' "$raw_check" >/dev/null; then
    exit 0
fi

printf 'RIPR raw-check evidence is incomplete, ineligible, or unconsumable: %s\n' "$raw_check" >&2
jq '{run_limitations, analysis_scope, analysis_outcome}' "$raw_check" >&2 || true
exit 1
