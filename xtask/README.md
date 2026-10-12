# Runtime worktree selection

A verified main `xtask` executable can target another worktree explicitly:
`/path/to/copied-xtask --worktree-root /path/to/worktree change-set --base HEAD^`.
The target may be a directory inside that worktree. Selection validates the Git
root, workspace repository identity and xtask package, then uses that root as the
process working directory and default task project root. Missing,
non-repository or unrelated workspace targets fail without falling back; inherited
`GIT_DIR`, `GIT_WORK_TREE` and `GIT_COMMON_DIR` must be unset. Without this option,
existing defaults remain unchanged. Per-command explicit fixture roots and
intentional compiled-source manifest/corpus consumers (including CPAN and legacy
corpus lookup) retain their contracts. This selects a runtime task target; it does
not make mutable Cargo build outputs transferable between worktrees.

The main runner accepts `--worktree-root` as a global option. Selection is
explicit; invoking a copied runner from another directory alone does not select
that worktree. Repository validation requires UTF-8 Git path output.
