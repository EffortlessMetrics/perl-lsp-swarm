# Toolchain and proof routes

Use root `AGENTS.md`, `CONTRIBUTING.md`, and package-local guidance as the current
contract. Read dependency versions, workspace membership, lint policy, and formatting
settings from their manifests/configuration rather than copying them into steering.

## Tool selection

Keep the workspace `Cargo.toml` MSRV separate from the default selected by
`rust-toolchain.toml`. An explicit `RUSTUP_TOOLCHAIN` can select a different compiler;
compatibility and exact-pin qualification are different claims. Follow the active
setup/doctor owner's limitations rather than changing the pin to make diagnosis green.
Node/npm acceptance belongs to the extension manifest and its doctor.

Provisioning tools, activating an independent task shell, inspecting repository layout,
and admitting a build are separate steps. Follow the contributor quick start and
`docs/how-to/AGENT_CONTRIBUTING.md`; do not install utilities or compile the whole
workspace merely to inspect a source change.

## Focused proof

`just agent-preflight` is layout inspection, not writer ownership or Cargo capacity.
Before compiled work, read `docs/agents/CARGO_STORAGE.md`. Select the package and target
required by the claim. From the same shell and worktree, preflight and execute matching
requests, for example:

```bash
scripts/cargo-admitted --preflight check -p <package> --profile agent --locked --lib
just agent-check <package> --lib
scripts/cargo-admitted --preflight test -p <package> --profile agent --locked --test <target>
just agent-test <package> --test <target>
```

These two Just recipes use `cargo-admitted`; the test recipe requires a target selector.
A test-name filter alone does not restrict compilation targets. Keep extra arguments
identical in both requests. Use the direct admitted route for wrapper-specific options.

Clippy follows the separately qualified staged native route in `CARGO_STORAGE.md`.
Raw Cargo aliases, `cargo-safe`, and the remaining legacy Just recipes are not admission.
Broad gates run only for claims that need them on a host admitted for their actual work;
there is no universal every-push workspace gate or timing promise in this adapter.

Formatting stays separate:

```bash
cargo fmt -p <package> -- --check
```

Read current `AGENTS.md` and the owning test package for assertion-helper policy. Keep
explanatory assertion context; do not replace a diagnostic assertion with a bare helper.
Do not weaken production-code rules, tests, or support claims to obtain green results.
