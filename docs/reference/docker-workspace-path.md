# Docker workspace path and URI namespace (#8987)

perl-lsp must not silently translate host file URIs into a different container
path namespace. Supported topologies are explicit and checked by
`policy/docker-workspace-path.v1.toml` plus `cargo run -p xtask --bin docker-workspace-path -- check`.

## Topologies

| Topology | When | Mount rule |
| --- | --- | --- |
| `host_editor_same_path_bind` | Editor on Linux, macOS, or WSL talking to a containerized `perllsp` | Each workspace root bind-mounts to the **same absolute path** inside the container |
| `editor_inside_container` | Dev Containers, remote containers, in-container editors | Project may mount at `/workspace` **only when the client also uses `/workspace`** |

Native Windows host editors (outside WSL) are **not supported** unless a separate
translation layer is designed and proven.

## Host editor launcher

From the repository root, with an absolute workspace path:

```bash
WORKSPACE_ROOT="$(pwd)" PERLLSP_IMAGE=ghcr.io/effortlessmetrics/perl-lsp-perl:<version> \
  ./scripts/docker/run-perllsp-host-editor.sh
```

Equivalent manual invocation:

```bash
docker run --rm -i --init \
  --mount "type=bind,src=$PWD,dst=$PWD,readonly" \
  --workdir "$PWD" \
  ghcr.io/effortlessmetrics/perl-lsp-perl:<version> \
  --stdio
```

Do **not** use `-v "${PWD}:/workspace"` for host editors unless the client's
`rootUri` is literally `file:///workspace`.

## Editor inside container

Use `.devcontainer/devcontainer.json` or `docker compose run --rm perl-lsp` for
the `/workspace` namespace. The editor and server share one filesystem view.

## Multi-root

The host wrapper refuses more than one workspace folder until the launcher can
obtain same-path mounts for every root. It does not pretend a single `$PWD`
mount proves multi-root.

## Proof

Contract tests live in `xtask/src/docker_workspace_path/plan.rs` and
`xtask/tests/docker_workspace_path_contract.rs`. Path fixtures for acceptance
journeys live under `fixtures/docker_workspace_path/`.

Run:

```bash
cargo test -p xtask docker_workspace_path -- --nocapture
cargo run -p xtask --bin docker-workspace-path -- check
```

## Non-goals

No general URI rewriting proxy. URI/source identity types remain authoritative
in `perl-uri` / `perl-source-identity`.
