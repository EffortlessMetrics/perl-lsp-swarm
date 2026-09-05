# Non-Rust File Inventory

This file is a frozen pointer. It carries no counts and no rows, and it never
changes on `main`, so it can never conflict on merge (#14688).

The inventory is generated evidence, not a tracked publication:

- `cargo xtask non-rust inventory` writes the current-tree projection to
  `target/policy/non-rust-inventory.md` and `target/policy/non-rust-inventory.json`
  (both git-ignored).
- `cargo xtask non-rust inventory --check` is the required merge gate. It
  validates `policy/non-rust-allowlist.toml`, classifies the current tracked
  tree, writes the same two files, requires this pointer to be byte-identical
  to `main`, and rejects newly added unclassified paths against the merge base.
- The `policy` CI shard uploads both projections as the
  `non-rust-inventory-<sha>` artifact for every run, including runs on `main`.
  That artifact is the default-branch reference.

If a branch has regenerated this file, restore it with:

```text
git checkout origin/main -- docs/policy/NON_RUST_INVENTORY.md
```

Policy and authority boundaries are documented in
[`docs/FILE_POLICY.md`](../FILE_POLICY.md) and
[`docs/policy/NON_RUST_POLICY.md`](NON_RUST_POLICY.md).
