# Acceptance Criteria: #4056 - Agent capability gate runner routing

## Behavior

| Condition | Expected result |
|---|---|
| Trusted same-repository PR/merge_group/main push and idle nano capacity | Self-hosted `workflow-nano` job runs the unchanged capability command |
| Fork pull request | Hosted `ubuntu-24.04` job runs; no self-hosted job is eligible |
| Same-repository bot pull request | Hosted `ubuntu-24.04` job runs; no self-hosted job is eligible |
| Missing `EM_RUNNER_READ_TOKEN` | Hosted fallback runs with reason `runner_token_missing` |
| Runner API non-200 response | Hosted fallback runs with reason `runner_api_failed` |
| Runner-group API non-200 response or missing `em-ci-nano` group | Hosted fallback runs with an explicit runner-group reason |
| No online idle matching runner | Hosted fallback runs with reason `no_idle_runner` |
| Capability checker returns non-zero | Selected execution job fails; routing does not convert a policy failure to success |
| Trigger, permissions, concurrency, pinned actions, and command | Existing contract remains intact |

## Test grid

| Scenario | Kind | Proof |
|---|---|---|
| Self-hosted group and labels are exact | structural | YAML policy test |
| Hosted fallback uses pinned `ubuntu-24.04` | structural | YAML policy test |
| Fork/bot guards select hosted | structural | YAML policy test and route-body inspection |
| Token/API/capacity fallback reasons are explicit | structural | YAML policy test |
| Capability command remains unchanged in both jobs | structural | YAML policy test |
| Existing trigger/permission/concurrency contract remains | regression | YAML policy test |
| Policy and lane mapping remain valid | integration | validator scripts and workflow-policy lint |
| Live self-hosted route | external | exact-head CI receipt, `NOT_PROVEN` locally |

## Required proof and non-claims

The repository proof must establish the static workflow contract and governed
mapping. It does not establish that a live `workflow-nano` runner currently has
Rust/Cargo or that `EM_RUNNER_READ_TOKEN` can read the organization runner API.
