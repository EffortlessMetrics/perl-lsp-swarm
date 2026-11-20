#!/usr/bin/env bash
set -euo pipefail

# Only the artifact download's HTTP 503 is infrastructure evidence. Keep the
# job failed on exhaustion so generation and its dependent writer stay closed.
dest="$RUNNER_TEMP/ripr-badge"
stage=$(mktemp -d "$RUNNER_TEMP/badge-download.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT
for attempt in 1 2; do
  mkdir "$stage/$attempt"
  if gh run download "$PRODUCER_RUN_ID" --repo "$GITHUB_REPOSITORY" \
      --name ripr-pr-evidence --dir "$stage/$attempt" 2>"$stage/error"; then
    for file in ripr-plus.json ripr-badge-producer.json; do
      if [[ ! -f "$stage/$attempt/receipts/quality/$file" ]]; then
        echo "::error::Invalid RIPR artifact: missing $file" >&2
        exit 1
      fi
    done
    mv -T "$stage/$attempt" "$dest"
    exit 0
  fi
  # gh errors can contain signed URLs. Report only the classification.
  if ! grep -Eq '^error downloading ripr-pr-evidence: HTTP 503([: ]|$)' "$stage/error"; then
    echo "::error::RIPR artifact download failed without a recognized backend 503" >&2
    exit 1
  fi
  if [[ $attempt == 2 ]] || grep -Eiq 'egress is over the account limit|retry-after|rate limit' "$stage/error"; then
    echo "::error::RIPR artifact backend unavailable (HTTP 503)" >&2
    exit 1
  fi
  echo "::warning::RIPR artifact backend returned HTTP 503; retrying once in 30 seconds" >&2
  sleep 30
done
