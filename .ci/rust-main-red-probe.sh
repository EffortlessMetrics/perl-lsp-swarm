#!/usr/bin/env bash
# Extracted without policy changes from PR16134; transport is read-only Python.
set -u
probe_dir="$(mktemp -d)"
trap 'rm -rf "$probe_dir"' EXIT

api_ok=true
TRUSTED_SCRIPT_AVAILABLE=false
read_main_sha() {
  python3 scripts/ci/github_read.py \
        "repos/${REPOSITORY}/git/ref/heads/main" \
    --field '.object.sha' 2>"$probe_dir/main-ref.err"
}
read_workflow_sha() {
  python3 scripts/ci/github_read.py \
        "repos/${REPOSITORY}/contents/.github/workflows/ci.yml?ref=$1" \
    --field '.sha' 2>"$probe_dir/workflow-$2.err"
}

# Read the branch immediately before and immediately after the
# check-run lookup. A main movement makes all observations stale;
# the classifier deliberately turns that into a warning.
MAIN_SHA_BEFORE="$(read_main_sha)" || {
  MAIN_SHA_BEFORE=""
  api_ok=false
}
MAIN_WORKFLOW_SHA="$(read_workflow_sha "${MAIN_SHA_BEFORE:-}" main)" || {
  MAIN_WORKFLOW_SHA=""
  api_ok=false
}
if [ -n "${CANDIDATE_SHA:-}" ]; then
  trusted_response="$probe_dir/trusted-script.json"
  TRUSTED_SCRIPT="$probe_dir/main_red_refusal.py"
  if python3 scripts/ci/github_read.py \
        "repos/${REPOSITORY}/contents/scripts/ci/main_red_refusal.py?ref=${MAIN_SHA_BEFORE}" \
    >"$trusted_response" 2>"$probe_dir/trusted-script.err" &&
    jq -e '.content | type == "string"' "$trusted_response" >/dev/null &&
    jq -r '.content' "$trusted_response" | tr -d '\n' | base64 --decode >"$TRUSTED_SCRIPT"
  then
    TRUSTED_SCRIPT_AVAILABLE=true
  else
    echo "::warning::trusted main-red classifier is not available at ${MAIN_SHA_BEFORE}; refusal probe is non-blocking during bootstrap"
  fi
  python3 scripts/ci/github_read.py --paginate --slurp \
        "repos/${REPOSITORY}/commits/${MAIN_SHA_BEFORE}/check-runs?per_page=100" \
    >"$probe_dir/main-runs.json" 2>"$probe_dir/main-runs.err" || {
      : >"$probe_dir/main-runs.json"
      api_ok=false
    }
  python3 scripts/ci/github_read.py --paginate --slurp \
        "repos/${REPOSITORY}/actions/workflows/ci.yml/runs?head_sha=${MAIN_SHA_BEFORE}&per_page=100" \
    >"$probe_dir/main-workflow-runs.json" 2>"$probe_dir/main-workflow-runs.err" || {
      : >"$probe_dir/main-workflow-runs.json"
      api_ok=false
    }
  python3 scripts/ci/github_read.py --paginate --slurp \
        "repos/${REPOSITORY}/commits/${CANDIDATE_SHA}/check-runs?per_page=100" \
    >"$probe_dir/candidate-runs.json" 2>"$probe_dir/candidate-runs.err" || {
      : >"$probe_dir/candidate-runs.json"
      api_ok=false
    }
  python3 scripts/ci/github_read.py --paginate --slurp \
        "repos/${REPOSITORY}/actions/workflows/ci.yml/runs?head_sha=${CANDIDATE_SHA}&per_page=100" \
    >"$probe_dir/candidate-workflow-runs.json" 2>"$probe_dir/candidate-workflow-runs.err" || {
      : >"$probe_dir/candidate-workflow-runs.json"
      api_ok=false
    }
  CANDIDATE_WORKFLOW_SHA=""
  if [ -n "${EFFECTIVE_WORKFLOW_TREE:-}" ]; then
    CANDIDATE_WORKFLOW_SHA="$(read_workflow_sha "$EFFECTIVE_WORKFLOW_TREE" candidate)" || {
      CANDIDATE_WORKFLOW_SHA=""
      api_ok=false
    }
  fi
else
  echo "::warning::${EVENT_NAME} has no PR/merge-group subject SHA; main-red refusal probe is non-applicable"
  : >"$probe_dir/main-runs.json"
  : >"$probe_dir/candidate-runs.json"
  : >"$probe_dir/main-workflow-runs.json"
  : >"$probe_dir/candidate-workflow-runs.json"
  MAIN_WORKFLOW_SHA=""
  CANDIDATE_WORKFLOW_SHA=""
  TRUSTED_SCRIPT=""
fi
MAIN_SHA_AFTER="$(read_main_sha)" || {
  MAIN_SHA_AFTER=""
  api_ok=false
}

if [ "$api_ok" = "false" ]; then
  echo "::warning::main-red refusal API probe was incomplete; treating probe data as non-blocking"
fi
run_refusal() {
  if [ "$TRUSTED_SCRIPT_AVAILABLE" != "true" ]; then
    return 0
  fi
  python3 "$TRUSTED_SCRIPT" \
    --main-runs "$probe_dir/main-runs.json" \
    --candidate-runs "$probe_dir/candidate-runs.json" \
    --main-workflow-runs "$probe_dir/main-workflow-runs.json" \
    --candidate-workflow-runs "$probe_dir/candidate-workflow-runs.json" \
    --main-sha-before "${MAIN_SHA_BEFORE:-}" \
    --main-sha-after "${MAIN_SHA_AFTER:-}" \
    --candidate-sha "${CANDIDATE_SHA:-}" \
    --main-workflow-sha "${MAIN_WORKFLOW_SHA:-}" \
    --candidate-workflow-sha "${CANDIDATE_WORKFLOW_SHA:-}" \
    "$@"
}

set +e
run_refusal
refusal_status=$?
set -u
poll_attempt=0
while [ "$refusal_status" -eq 2 ] && [ "$poll_attempt" -lt 50 ]; do
  poll_attempt=$((poll_attempt + 1))
  echo "::notice::main-red refusal is waiting for candidate shard evidence (poll ${poll_attempt}/50)"
  sleep 30
  MAIN_SHA_AFTER="$(read_main_sha)" || {
    MAIN_SHA_AFTER=""
  }
  python3 scripts/ci/github_read.py --paginate --slurp \
        "repos/${REPOSITORY}/commits/${CANDIDATE_SHA}/check-runs?per_page=100" \
    >"$probe_dir/candidate-runs.json" 2>"$probe_dir/candidate-runs.err" || {
      : >"$probe_dir/candidate-runs.json"
    }
  python3 scripts/ci/github_read.py --paginate --slurp \
        "repos/${REPOSITORY}/actions/workflows/ci.yml/runs?head_sha=${CANDIDATE_SHA}&per_page=100" \
    >"$probe_dir/candidate-workflow-runs.json" 2>"$probe_dir/candidate-workflow-runs.err" || {
      : >"$probe_dir/candidate-workflow-runs.json"
    }
  CANDIDATE_WORKFLOW_SHA=""
  if [ -n "${EFFECTIVE_WORKFLOW_TREE:-}" ]; then
    CANDIDATE_WORKFLOW_SHA="$(read_workflow_sha "$EFFECTIVE_WORKFLOW_TREE" candidate)" || {
      CANDIDATE_WORKFLOW_SHA=""
    }
  fi
  run_refusal
  refusal_status=$?
done
if [ "$refusal_status" -eq 2 ]; then
  echo "::warning::candidate shard evidence did not become terminal before the bounded wait; applying final fail-closed refusal"
  run_refusal --final
  refusal_status=$?
fi
if [ "$refusal_status" -eq 1 ]; then
  exit 1
fi
