// Integration test: assertion helpers (`expect`/`unwrap`/`panic!`) carry the
// failure message. The workspace-wide deny is a production-code rule.
#![allow(clippy::expect_used)]
#![allow(dead_code)]

#[allow(dead_code)]
#[path = "../../src/bin/workspace_doctor_inventory/mod.rs"]
pub mod workspace_doctor_inventory;

use std::fs;
use std::path::Path;
use tempfile::TempDir;

pub fn fixture_root() -> TempDir {
    let temp = tempfile::tempdir().expect("create fixture root");
    write(
        temp.path(),
        "justfile",
        r#"pr-fast: _check-tools-basic
    #!/usr/bin/env bash
    set -euo pipefail
    args=(--tier pr-fast --receipt)
    cargo xtask gates "${args[@]}"

_check-tools-basic:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v cargo
    command -v rustfmt
    cargo xtask check-toolchain

ready: doctor pr-fast
    @echo "✅ Workspace is ready to push (doctor + pr-fast passed)"

doctor:
    #!/usr/bin/env bash
    set -uo pipefail
    issues=0
    fixed=0
    common_dir=$(git rev-parse --git-common-dir 2>/dev/null || true)
    if [ -z "$common_dir" ]; then
        echo "❌ not inside a git repository"
        exit 1
    fi

    # Check 1: core.bare = true corruption (#3205)
    bare_value=$(git config --local --get core.bare 2>/dev/null || true)
    if [ "$bare_value" = "true" ]; then
        if git --git-dir="$main_git_dir" config --local --unset core.bare; then
            echo "Auto-fixed: unset core.bare"
            fixed=$((fixed + 1))
        fi
    fi

    # Check 2: Stale local branches (upstream gone)
    echo "git branch -D <branch>"

    main_dirty_full=$(git status --porcelain --untracked-files=no)
    declare -A main_dirty_set=()
    main_dirty_set["$path"]=1

    # Check 3: Worktree file leaks
    echo "worktree file leaks"

    # Check 4: Orphaned worktree directories
    echo "orphaned worktree directories"
    echo "git worktree prune; rm -rf <dir>"

    # Check 5: pre-push hook installed
    echo "pre-push hook installed"
    echo "pre-push hook installed but stale"
    echo "cargo xtask ci-hygiene install-githooks"

    # Check 6: Workspace clean
    echo "workspace has $dirty_count uncommitted changes"

    # Check 7: Current checkout is fast-forward-able with remote default branch.
    echo "HEAD is $behind commits behind $default_remote_ref"
    echo "Fix: git pull --ff-only"
    echo "could not resolve default remote branch"
    echo "git remote set-head origin -a && git fetch origin"

    echo "$issues issues found, $fixed auto-fixed"
    exit 0

next-recipe:
    @true
"#,
    );
    write(
        temp.path(),
        "xtask/src/tasks/writer_admission.rs",
        r#"check_shadow_ref(snapshot)
check_symbolic_head(snapshot)
check_branch_worktree_mapping(snapshot)
check_dirty_unpushed(snapshot, config)
check_disk_capacity(snapshot, config)
check_writer_collision(snapshot)
Advisory-first: `run` always returns `Ok(())`
disk-capacity
floor_gb
"#,
    );
    write(
        temp.path(),
        "xtask/src/tasks/devex_doctor.rs",
        r#"check_command("cargo", "cargo", &mut missing_required);
check_command_optional("just", "just");
check_command_optional("nix", "nix");
check_command_optional("cargo-audit", "cargo-audit");
check_pre_push_hook();
check_pre_commit_hook();
check_build_storage(&root);
pre-commit hook missing or not executable
"#,
    );
    write(
        temp.path(),
        "xtask/src/tasks/worktrees.rs",
        "Dry-run report\nargs([\"worktree\", \"prune\"])\nPrStatus::Unknown\n",
    );
    write(temp.path(), "crates/perl-ci-hygiene/src/cli.rs", "InstallGithooks\nCheckGithooks\n");
    write(
        temp.path(),
        "scripts/storage-doctor",
        "repo-local target dirs\nrepo-local target dir exceeds 1G\nsccache --show-stats\n",
    );
    temp
}

pub fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    let parent = path.parent().expect("source parent");
    fs::create_dir_all(parent).expect("create source parent");
    fs::write(path, content).expect("write fixture source");
}
