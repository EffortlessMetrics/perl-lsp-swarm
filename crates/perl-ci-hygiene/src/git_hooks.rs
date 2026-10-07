use color_eyre::eyre::{Context, Result};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A `git` command bound to the caller's `.current_dir`, never to an
/// inherited repository.
///
/// `Command` inherits the process environment, so a stray `GIT_DIR`,
/// `GIT_WORK_TREE`, or `GIT_COMMON_DIR` would redirect enumeration,
/// configuration, and resolution at another repository despite
/// `.current_dir` (#17426 review). Every git invocation in this module —
/// production and test helpers alike — goes through here.
fn git_command() -> Command {
    let mut command = Command::new("git");
    command.env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").env_remove("GIT_COMMON_DIR");
    command
}

mod install {
    /// Token-shape secret scan shared by both generated hooks (#17428).
    ///
    /// Pure git + grep: no build, no network. The rule list mirrors
    /// `TOKEN_SHAPES` in the `secret_scan` lib module, and
    /// `secret_scan_shell_matches_lib_rules` fails if the two drift. Findings
    /// name the file and rule, never the matched text.
    ///
    /// Trap-extractable: the snippet is delimited by full-line
    /// `SECRET_SCAN_SH_BEGIN` / `SECRET_SCAN_SH_END` markers, and
    /// `t03_secret_publish.sh` sources the lines between them into fixture
    /// hooks for a live refusal demo. Keep the markers unique as full lines
    /// in this file.
    pub(super) const SECRET_SCAN_SH: &str = r#"
# SECRET_SCAN_SH_BEGIN
# --- Token-shape secret scan (issue #17428) ---
# Refuses token-shaped additions before they enter history. Pure git + grep:
# no build, no network. The rule list mirrors TOKEN_SHAPES in
# crates/perl-ci-hygiene/src/secret_scan.rs; the required secret_scan gate
# re-scans the same shapes over the PR diff + body, so --no-verify bypasses
# the hook but not the gate.
SECRET_SCAN_RULE_GITHUB='gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22}_[A-Za-z0-9]{59}'
SECRET_SCAN_RULE_AWS='AKIA[0-9A-Z]{16}'
SECRET_SCAN_RULE_SLACK='xox[baprs]-[A-Za-z0-9-]{10,48}'
SECRET_SCAN_RULE_PRIVKEY='-----BEGIN (RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----'
# Double-quoted: the optional quote class needs a literal single quote, which
# cannot appear inside single quotes. The pattern holds no $, backtick, or
# backslash, so double-quoting expands nothing.
SECRET_SCAN_RULE_GENERIC="([Aa][Pp][Ii][_-]?[Kk][Ee][Yy]|[Aa][Pp][Ii][_-]?[Tt][Oo][Kk][Ee][Nn]|[Ss][Ee][Cc][Rr][Ee][Tt][_-]?[Kk][Ee][Yy]|[Aa][Cc][Cc][Ee][Ss][Ss][_-]?[Tt][Oo][Kk][Ee][Nn])[[:space:]]*[:=][[:space:]]*[\"']?[A-Za-z0-9_./+=-]{16,}($|[^A-Za-z0-9_./+()=-])"
SECRET_SCAN_HITS=""
secret_scan_allowed() {
    [ -n "${SECRET_SCAN_ALLOWLIST:-}" ] && [ -f "$SECRET_SCAN_ALLOWLIST" ] || return 1
    grep -F -x -q -- "$1" "$SECRET_SCAN_ALLOWLIST" 2>/dev/null
}
secret_scan_stream() {
    local file="$1" added name pattern hits
    added="$(grep -E '^\+[^+]' || true)"
    [ -z "$added" ] && return 0
    for name in github-token aws-access-key slack-token private-key generic-assignment; do
        case "$name" in
            github-token) pattern="$SECRET_SCAN_RULE_GITHUB" ;;
            aws-access-key) pattern="$SECRET_SCAN_RULE_AWS" ;;
            slack-token) pattern="$SECRET_SCAN_RULE_SLACK" ;;
            private-key) pattern="$SECRET_SCAN_RULE_PRIVKEY" ;;
            generic-assignment) pattern="$SECRET_SCAN_RULE_GENERIC" ;;
        esac
        # The -- matters: the private-key shape starts with dashes and would
        # otherwise parse as grep options, failing that rule open.
        hits="$(printf '%s\n' "$added" | grep -E -c -- "$pattern" || true)"
        if [ -n "$hits" ] && [ "$hits" != "0" ]; then
            SECRET_SCAN_HITS="${SECRET_SCAN_HITS}  $file: rule $name matched $hits added line(s)
"
        fi
    done
}
secret_scan_refuse_if_hits() {
    [ -z "$SECRET_SCAN_HITS" ] && return 0
    echo ""
    echo "Secret scan refused this $1: token-shaped line(s) detected"
    printf '%s' "$SECRET_SCAN_HITS"
    echo ""
    echo "   Rotate the credential if it is real; never commit secrets."
    echo "   Documented escape for inert test fixtures: list the repo-relative"
    echo "   path in .ci/secret-scan-allowlist.txt (one per line)."
    return 1
}
secret_scan_staged() {
    SECRET_SCAN_ALLOWLIST="$(git rev-parse --show-toplevel 2>/dev/null)/.ci/secret-scan-allowlist.txt"
    SECRET_SCAN_HITS=""
    local file diff
    while IFS= read -r file; do
        [ -z "$file" ] && continue
        secret_scan_allowed "$file" && continue
        diff="$(git diff --cached --unified=0 --no-color --src-prefix=a/ --dst-prefix=b/ -- "$file" 2>/dev/null || true)"
        [ -z "$diff" ] && continue
        secret_scan_stream "$file" <<< "$diff"
    done <<SECRET_SCAN_FILES
$(git diff --cached --name-only 2>/dev/null || true)
SECRET_SCAN_FILES
    secret_scan_refuse_if_hits commit
}
secret_scan_range() {
    SECRET_SCAN_ALLOWLIST="$(git rev-parse --show-toplevel 2>/dev/null)/.ci/secret-scan-allowlist.txt"
    SECRET_SCAN_HITS=""
    local base file diff
    if [ "$2" != "0000000000000000000000000000000000000000" ]; then
        base="$2"
    else
        base="$(git merge-base origin/main "$1" 2>/dev/null || git merge-base main "$1" 2>/dev/null || git merge-base origin/master "$1" 2>/dev/null || git merge-base master "$1" 2>/dev/null || echo 4b825dc642cb6eb9a060e54bf8d69288fbee4904)"
    fi
    while IFS= read -r file; do
        [ -z "$file" ] && continue
        secret_scan_allowed "$file" && continue
        diff="$(git diff --unified=0 --no-color --src-prefix=a/ --dst-prefix=b/ "$base" "$1" -- "$file" 2>/dev/null || true)"
        [ -z "$diff" ] && continue
        secret_scan_stream "$file" <<< "$diff"
    done <<SECRET_SCAN_FILES
$(git diff --name-only "$base" "$1" 2>/dev/null || true)
SECRET_SCAN_FILES
    secret_scan_refuse_if_hits push
}
# SECRET_SCAN_SH_END
"#;

    /// Pre-push call site for the shared scan: every pushed ref is scanned
    /// before all gates. (Leading blank line separates the call from the
    /// snippet above.)
    pub(super) const PRE_PUSH_SCAN_CALL: &str = r#"
# --- Token-shape secret scan over every pushed ref (issue #17428) ---
# Runs before all gates (including the doc-only fast path: secrets in docs
# refuse too). Pure git + grep; see SECRET_SCAN_SH_BEGIN above.
for line in "${PUSH_REFS[@]+"${PUSH_REFS[@]}"}"; do
    read -r _push_ref push_local _remote_ref push_remote <<< "$line"
    if [ "$push_local" != "0000000000000000000000000000000000000000" ]; then
        secret_scan_range "$push_local" "$push_remote" || exit 1
    fi
done
"#;

    pub(super) const PRE_COMMIT_HEAD: &str = r#"#!/usr/bin/env bash
set -euo pipefail

GIT_USER_NAME="$(git config user.name 2>/dev/null || true)"
GIT_USER_EMAIL="$(git config user.email 2>/dev/null || true)"

if [ "$GIT_USER_NAME" = "Codex Release Validation" ] || \
   [ "$GIT_USER_EMAIL" = "codex-release-validation@example.invalid" ] || \
   [ "$GIT_USER_NAME" = "xtask hook tests" ] || \
   [ "$GIT_USER_EMAIL" = "xtask@example.invalid" ]; then
    echo "❌ Refusing commit with placeholder git identity"
    echo "   user.name:  $GIT_USER_NAME"
    echo "   user.email: $GIT_USER_EMAIL"
    echo ""
    echo "   Fix this repo-local override first:"
    echo "   git config --local --unset-all user.name"
    echo "   git config --local --unset-all user.email"
    exit 1
fi
"#;

    /// Pre-commit tail: the secret scan runs first, then the staged formatter
    /// and the exact staged gate judge the scanned tree. (Leading blank line
    /// separates the scan call from the snippet above.)
    pub(super) const PRE_COMMIT_TAIL: &str = r#"
secret_scan_staged || exit 1

# Format the staged Rust diff before the gate inspects it.
#
# `rustfmt_staged` in the commit gate below blocks a commit whose staged Rust
# would be reformatted. Formatting the diff first turns that block into a
# self-heal: the common case (a few unformatted lines in the files you are
# already committing) is fixed and re-staged instead of bouncing you out to
# run a workspace-wide `cargo xtask fmt` by hand.
#
# Only fully staged files are rewritten. A file that is staged *and*
# separately modified in the worktree is reported and left alone, so this can
# never sweep unstaged work into the commit.
#
# Non-fatal on its own: if rustfmt is unavailable the gate below still blocks,
# so a missing formatter cannot turn into a silently unformatted commit.
#
# A failed run leaves nothing half-done. Files are formatted in memory first,
# and any write or re-stage failure restores the original bytes, so the
# worktree and the index stay in step and the gate below judges the same tree
# you started with. The one exception — a rollback that itself fails — is
# reported by name in the command's own output above this warning.
echo "Formatting staged Rust diff: cargo xtask fmt --staged"
cargo xtask fmt --staged || echo "⚠️  staged formatting did not run; the commit gate below still applies"

echo "Running exact staged commit gate: cargo xtask precommit"
cargo xtask precommit
"#;

    pub(super) fn print_install_summary() {
        println!("✅ Installed pre-commit and pre-push hooks");
        println!(
            "   The pre-commit hook blocks placeholder identities, refuses token-shaped staged \
             additions, formats the staged Rust diff ('cargo xtask fmt --staged'), then runs \
             'cargo xtask precommit'"
        );
        println!(
            "   The pre-push hook refuses token-shaped pushed ranges, then runs 'nix develop -c \
             just pr-fast' before each push"
        );
        println!("   The pre-push hook runs 'nix develop -c just pr-fast' before each push");
        println!(
            "   The pre-push hook also refuses non-fast-forward updates and protected-ref rewrites \
             (escape hatch: PERL_LSP_ALLOW_HISTORY_REWRITE=\"<remote-ref>\")"
        );
        println!("   Skip with: git commit --no-verify / git push --no-verify");
    }
}

pub(crate) fn pre_commit_hook_script() -> String {
    let head = install::PRE_COMMIT_HEAD;
    let scan = install::SECRET_SCAN_SH;
    let tail = install::PRE_COMMIT_TAIL;
    format!("{head}{scan}{tail}")
}

pub(crate) fn pre_push_hook_script() -> String {
    let head = r#"#!/usr/bin/env bash
# ============================================================================
# perl-lsp pre-push hook (generated by `cargo xtask ci-hygiene install-githooks`)
# ============================================================================
#
# Bypass policy
# -------------
# OK to bypass with `git push --no-verify`:
#   * Deletion-only push on a non-protected branch (the hook already
#     auto-skips, but if it doesn't, bypassing is safe — there's nothing
#     to validate). Deleting a protected branch (main, master) is refused
#     by the hook's own ref-update check, not bypassable this way in spirit.
#   * A non-fast-forward push you have proven safe (your own rebased branch,
#     no teammate work): prefer the admit-list escape hatch
#     (PERL_LSP_ALLOW_HISTORY_REWRITE="<remote-ref>") over --no-verify,
#     so the remaining gates still run — and push with the printed
#     --force-with-lease=<ref>:<inspected-sha>, never plain --force.
#   * Urgent fixes during incident response, when the gate has a known bug
#     being tracked (see hint output below for issue numbers).
#   * The hook is failing for an environmental reason that is out of band of
#     your change (e.g., nix store cache miss, transient toolchain issue,
#     `cargo xtask change-set` reporting NOT PROVEN because origin/main is
#     unreachable — see issue #3985).
#
# NOT OK to bypass:
#   * "I just don't want to wait."
#   * "I just want to push something quick."
#   * Code-touching changes where you haven't actually run the gate locally.
#   * A non-fast-forward refusal you have not investigated (fetch and
#     rebase/merge instead — see the refusal's recovery order).
#
# If you find yourself bypassing repeatedly, file an issue and link it here.
# ============================================================================
#
# Hook logic version (#17431 review wave 3): bump the integer below on EVERY
# guard-logic change. Self-heal only upgrades (checkout > installed) and
# never downgrades; unmarked legacy hooks count as version 0.
# pre-push-hook-version: 1

set -euo pipefail

# --- Self-heal core.bare corruption (issue #3205) ---
# Some sequences of `git worktree add`/`remove` silently flip core.bare=true
# on the main checkout, breaking every worktree-aware git command including
# this hook. Auto-unset before doing anything else.
if [ "$(git config --get core.bare 2>/dev/null || true)" = "true" ]; then
    # Confirm this is actually a non-bare repo (has a working tree).
    if git rev-parse --show-toplevel >/dev/null 2>&1; then
        echo "⚠️  Detected core.bare=true corruption (issue #3205) — auto-fixing"
        git config --local --unset core.bare || true
    fi
fi

# --- Self-heal stale hook installation (issue #4220) ---
# When hooks/pre-push is updated in master, .git/hooks/pre-push is only
# updated when install-githooks is re-run. On drift, UPGRADE only: copy the
# checkout file over the installed hook solely when its version marker is
# strictly newer, then REFUSE this push: exec "$0" "$@" does NOT work here —
# git stdin is already consumed before the hook executes — so continuing
# would guard this push with stale logic (#17431 review: upgrade barrier).
# Exactly one aborted push per hook upgrade; re-push runs the fresh guards.
# When the checkout copy is NOT newer (stale branch/worktree), the installed
# hook is never downgraded: the push proceeds under the installed guards
# with a warning (#17431 review wave 3: downgrade barrier).
hook_version() {
    # Print the numeric hook-version marker of $1; prints nothing when absent.
    sed -n 's/^# pre-push-hook-version: *\([0-9][0-9]*\).*/\1/p' "$1" 2>/dev/null | head -n 1
}
REPO_ROOT_FOR_HOOK="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [ -n "$REPO_ROOT_FOR_HOOK" ] && [ -f "$REPO_ROOT_FOR_HOOK/hooks/pre-push" ]; then
    if ! diff -q "$0" "$REPO_ROOT_FOR_HOOK/hooks/pre-push" >/dev/null 2>&1; then
        installed_version="$(hook_version "$0")"
        case "$installed_version" in ''|*[!0-9]*) installed_version=0 ;; esac
        checkout_version="$(hook_version "$REPO_ROOT_FOR_HOOK/hooks/pre-push")"
        case "$checkout_version" in ''|*[!0-9]*) checkout_version=0 ;; esac
        if [ "$checkout_version" -gt "$installed_version" ]; then
            # Atomic replace (#17431 review wave 4): a failed cp must never
            # leave $0 truncated, or later pushes run a broken hook.
            heal_tmp="$0.tmp.$$"
            if cp "$REPO_ROOT_FOR_HOOK/hooks/pre-push" "$heal_tmp" && chmod +x "$heal_tmp" && mv "$heal_tmp" "$0"; then
                echo "pre-push hook updated itself from hooks/pre-push (v$installed_version -> v$checkout_version)."
                echo "This push was refused so stale logic never guards it — re-push to run the fresh guards."
            else
                rm -f "$heal_tmp"
                echo "pre-push hook is stale and could not be self-updated (check permissions on $0); re-run install-githooks to refresh it." >&2
            fi
            exit 1
        fi
        echo "warning: hooks/pre-push in this checkout (v$checkout_version) is not newer than the installed hook (v$installed_version); keeping the installed hook. Update this branch/worktree to get the latest hook." >&2
    fi
fi

# stdin provides: <local ref> <local sha> <remote ref> <remote sha>
# Git sends all-zero SHA for deletions. Read all refs into an array first
# so stdin is available for both the delete-check and the test-file scan.
PUSH_REFS=()
while IFS= read -r line; do
    PUSH_REFS+=("$line")
done

# --- Refuse non-fast-forward updates and protected-ref rewrites (issue #17427) ---
# A push that would discard remote commits — force-push, reset+push, or
# deleting a protected branch (main, master) — is refused here, before any
# gate runs, with a recovery order. Fast-forward updates, new branches, and
# deletions of non-protected branches are unaffected.
#
# Escape hatch: when you have proven the discarded commits are safe to drop
# (your own rebased branch, no teammate work), name the full remote ref(s)
# in PERL_LSP_ALLOW_HISTORY_REWRITE (space-separated, as git reports them,
# e.g. "refs/heads/my-rebased-branch") and push with
# --force-with-lease=<ref>:<inspected-sha> (printed in the refusal) so a
# concurrent push after your inspection fails instead of being erased.
# Prefer this over --no-verify so the remaining gates still run.
# Protected branches should ~never need it.
#
# Implementation note: the protected list holds short branch names on purpose
# (compared against ${remote_ref#refs/heads/}), so no executable line names a
# full protected ref — see the T8 trap's hook-gate pin.
ZERO_SHA="0000000000000000000000000000000000000000"
PROTECTED_BRANCHES="main master"
ALLOW_HISTORY_REWRITE="${PERL_LSP_ALLOW_HISTORY_REWRITE:-}"

ref_update_admitted() {
    case " $ALLOW_HISTORY_REWRITE " in
        *" $1 "*) return 0 ;;
        *) return 1 ;;
    esac
}

for line in "${PUSH_REFS[@]+"${PUSH_REFS[@]}"}"; do
    [ -z "$line" ] && continue
    read -r _local_ref local_sha remote_ref remote_sha <<< "$line"
    if ref_update_admitted "$remote_ref"; then
        continue
    fi
    # Pastable forms, computed for every refused ref: a ref name may
    # legally contain shell syntax (e.g. refs/heads/$(id)), so anything
    # the refusal prints for copy-paste goes through %q (#17431 review).
    printf -v RECOVERY_REMOTE '%q' "${1:-origin}"
    printf -v RECOVERY_REF '%q' "$remote_ref"
    printf -v RECOVERY_ADMIT '%q' "$remote_ref"
    printf -v RECOVERY_LEASE '%q' "$remote_ref:$remote_sha"
    if [ "$local_sha" = "$ZERO_SHA" ]; then
        remote_short="${remote_ref#refs/heads/}"
        case " $PROTECTED_BRANCHES " in
            *" $remote_short "*)
                echo ""
                echo "❌ Refusing deletion of protected ref $remote_ref."
                echo "   Deleting shared branches destroys history other people build on."
                echo "   If this is genuinely intended (repository decommission, never"
                echo "   routine work), admit this ref explicitly and re-push:"
                echo "   PERL_LSP_ALLOW_HISTORY_REWRITE=$RECOVERY_ADMIT git push <remote> --delete <branch>"
                exit 1
                ;;
        esac
        continue
    fi
    if [ "$remote_sha" = "$ZERO_SHA" ]; then
        continue
    fi
    if git merge-base --is-ancestor "$remote_sha" "$local_sha" 2>/dev/null; then
        continue
    fi
    if git cat-file -e "$remote_sha" 2>/dev/null; then
        UNKNOWN_TIP=""
    else
        UNKNOWN_TIP=" (tip $remote_sha is not in your local object store — fetch first)"
    fi
    echo ""
    echo "❌ Refusing non-fast-forward push to $remote_ref$UNKNOWN_TIP."
    echo "   Your push would discard remote commit(s)."
    echo "   Recover with: git fetch $RECOVERY_REMOTE $RECOVERY_REF && git rebase FETCH_HEAD (or git merge FETCH_HEAD), then push again."
    echo "   Only when you have proven the remote commits are safe to discard"
    echo "   (your own rebased branch, no teammate work), admit this ref explicitly"
    echo "   and bind the push to the tip you inspected — plain --force could erase"
    echo "   concurrent work pushed after your inspection:"
    echo "   PERL_LSP_ALLOW_HISTORY_REWRITE=$RECOVERY_ADMIT git push --force-with-lease=$RECOVERY_LEASE <remote> <branch>"
    exit 1
done

# --- Skip CI gate when all refs are being deleted ---
IS_DELETE_ONLY=true
for line in "${PUSH_REFS[@]+"${PUSH_REFS[@]}"}"; do
    local_sha="$(echo "$line" | awk '{print $2}')"
    if [ "$local_sha" != "0000000000000000000000000000000000000000" ]; then
        IS_DELETE_ONLY=false
        break
    fi
done

if [ "$IS_DELETE_ONLY" = true ]; then
    echo "Branch deletion — skipping CI gate"
    exit 0
fi
"#;
    let tail = r#"
# --- Detect doc-only changes for the fast-path gate ---
# A push is doc-only if every changed file matches one of:
#   *.md, *.txt, LICENSE*, CHANGELOG*, docs/**, .github/ISSUE_TEMPLATE/**,
#   or */LICENSE* (crate-subdir license files, e.g. crates/*/LICENSE-APACHE)
# Doc-only pushes skip code gates entirely instead of running the full
# ci-gate, since the test suite and workspace-wide rustfmt check are
# pointless for prose-only changes and can fail spuriously on Windows (#4047).
REPO_ROOT="$(git rev-parse --show-toplevel)"
DOC_ONLY=true
TEST_FILES_CHANGED=false
HAS_DIFFABLE_REF=false
# Track unique crate names for the single-crate tier.
# We use a newline-separated string rather than an array for POSIX compat.
SINGLE_CRATE_NAMES=""
SINGLE_CRATE_ALL_UNDER_CRATES=true
for line in "${PUSH_REFS[@]+"${PUSH_REFS[@]}"}"; do
    read -r _local_ref local_sha _remote_ref remote_sha <<< "$line"
    if [ "$remote_sha" = "0000000000000000000000000000000000000000" ]; then
        # New branch: there is no known remote SHA to diff against. Resolve
        # the changed-path set through the single shared #3985 change_set
        # resolver (`cargo xtask change-set` — main-first: origin/main,
        # main, HEAD~1; never origin/master, which does not exist on this
        # remote) instead of a second shell base-resolution algorithm.
        #
        # The former fallback here was
        #   git merge-base "$local_sha" origin/master 2>/dev/null || echo "$local_sha"
        # which always failed (origin/master is absent on this remote) and
        # silently fell back to comparing "$local_sha" against itself —
        # an empty self-diff for every new-branch push (issue #3985
        # Slice 3A: the pre-push affected-proof was silently degrading to
        # the broadest/full gate for every new branch instead of running
        # the intended targeted proof against the real diff).
        if ! CHANGED_FILES="$(cargo xtask change-set --base auto --head "$local_sha" --format paths)"; then
            echo ""
            echo "❌ NOT PROVEN: could not resolve the change set for this new-branch push."
            echo "   'cargo xtask change-set' failed to resolve a canonical base ref"
            echo "   (see the error above — e.g. origin/main unreachable, no network, or"
            echo "   another git error). Refusing to silently treat this as an empty"
            echo "   change set and skip the affected-scope proof."
            echo "   Fix the underlying git/network issue, or bypass with:"
            echo "   git push --no-verify (only for a genuinely environmental failure —"
            echo "   see the bypass policy at the top of this file)."
            exit 1
        fi
    else
        CHANGED_FILES="$(git diff --name-only "$remote_sha" "$local_sha" 2>/dev/null || true)"
    fi
    if [ -n "$CHANGED_FILES" ]; then
        HAS_DIFFABLE_REF=true
    fi
    while IFS= read -r changed; do
        [ -z "$changed" ] && continue
        case "$changed" in
            *.md|*.txt|LICENSE*|CHANGELOG*|docs/*|.github/ISSUE_TEMPLATE/*|*/LICENSE*)
                ;;
            *)
                DOC_ONLY=false
                # Track whether this code file lives under crates/<name>/
                case "$changed" in
                    crates/*/*)
                        # Extract the crate directory name: crates/<name>/...
                        crate_name="${changed#crates/}"
                        crate_name="${crate_name%%/*}"
                        # Append to list if not already present
                        if ! printf '%s\n' "$SINGLE_CRATE_NAMES" | grep -qxF "$crate_name" 2>/dev/null; then
                            SINGLE_CRATE_NAMES="${SINGLE_CRATE_NAMES}${crate_name}
"
                        fi
                        ;;
                    *)
                        # File is outside crates/ — can't be single-crate
                        SINGLE_CRATE_ALL_UNDER_CRATES=false
                        ;;
                esac
                ;;
        esac
    done <<< "$CHANGED_FILES"
    if echo "$CHANGED_FILES" | grep -qE '^crates/.*/tests/.*\.rs$'; then
        TEST_FILES_CHANGED=true
    fi
done

# If we couldn't compute a diff for any ref (e.g., shallow clone, missing
# remote), assume code changes and run the full gate to be safe.
if [ "$HAS_DIFFABLE_REF" != true ]; then
    DOC_ONLY=false
fi

if [ "$DOC_ONLY" = true ]; then
    echo "📝 Doc-only push — skipping code gates"
    echo "   (Skip with: git push --no-verify)"
    echo "✅ Doc-only fast-path gate passed"
    exit 0
fi

# --- Single-crate proportional gate ---
# If every code change is under a single crates/<name>/ directory, run a
# targeted cargo fmt/clippy/test -p <name> instead of the full workspace gate.
# Falls back to the full gate if classification is ambiguous.
SINGLE_CRATE_COUNT="$(printf '%s' "$SINGLE_CRATE_NAMES" | grep -c . 2>/dev/null || echo 0)"
if [ "$SINGLE_CRATE_ALL_UNDER_CRATES" = true ] && [ "$SINGLE_CRATE_COUNT" = "1" ]; then
    SINGLE_CRATE_DIR="$(printf '%s' "$SINGLE_CRATE_NAMES" | tr -d '[:space:]')"
    # Resolve the Cargo package name from Cargo.toml, not the directory basename.
    # This fixes the perl-lsp -> perl-lsp-rs mismatch (issue #4512).
    # Falls back to directory name if cargo xtask is unavailable.
    SINGLE_CRATE_NAME="$(cargo xtask resolve-package-name "crates/${SINGLE_CRATE_DIR}" 2>/dev/null || printf '%s' "$SINGLE_CRATE_DIR")"
    echo "Single-crate push (${SINGLE_CRATE_DIR} -> ${SINGLE_CRATE_NAME}) — running targeted gate"
    echo "   (Skip with: git push --no-verify)"
    echo ""
    # Clippy target selection follows the crate's CI cohort, because the tier
    # is meant to narrow SCOPE (one crate instead of the workspace), not to
    # apply a different contract than CI does.
    #
    # Cargo's default selection is lib + bins, so a bare `cargo clippy -p X`
    # cannot see tests/, benches/ or examples/ at all. For a crate in the
    # clippy_tests_kernel cohort that is a real hole: the #14549 regression was
    # a duplicated #![deny(clippy::map_err_ignore)] in
    # crates/perl-workspace-core/tests/, a file this tier never compiled, so it
    # reached main and reddened the kernel gate for every PR.
    #
    # But --all-targets is NOT universally right here. The crates in
    # CLIPPY_TESTS_KERNEL_RESIDUAL_PACKAGES (#15677, #15613) are deliberately
    # outside that cohort because they retain measured test-target lint debt;
    # CI lints them through clippy_strict, which is lib + bins. Applying
    # --all-targets to those would fail every otherwise valid push on
    # pre-existing findings that CI does not gate on.
    #
    # So read the cohort from the gate policy rather than assuming one answer,
    # and match its exact lint flags. The list is parsed out of the
    # clippy_tests_kernel command itself, so it cannot drift from what CI runs;
    # unparseable or missing policy falls back to the narrower command.
    #
    # --locked on every command so a gate run cannot quietly rewrite Cargo.lock
    # and have CI reject the push it just approved. Deliberately NOT
    # --all-targets on cargo test: that flag drops doctests, which the current
    # invocation does run.
    clippy_tests_kernel_cohort() {
        awk '
            /^  - name: clippy_tests_kernel$/ { in_gate = 1; next }
            in_gate && /^  - name: / { in_gate = 0 }
            in_gate {
                line = $0
                sub(/^[[:space:]]+/, "", line)
                sub(/[[:space:]]+$/, "", line)
                # Whole-line match only: prose in this block mentions "-p <crate>"
                # when narrating past admissions, and those must not count.
                if (line ~ /^-p [A-Za-z0-9_-]+$/) { print substr(line, 4) }
            }
        ' "$1"
    }

    GATE_POLICY_FILE="$REPO_ROOT/.ci/gate-policy.yaml"
    CLIPPY_ALL_TARGETS=false
    if [ -f "$GATE_POLICY_FILE" ]; then
        if clippy_tests_kernel_cohort "$GATE_POLICY_FILE" \
            | grep -qxF "$SINGLE_CRATE_NAME" 2>/dev/null; then
            CLIPPY_ALL_TARGETS=true
        fi
    fi

    if [ "$CLIPPY_ALL_TARGETS" = true ]; then
        echo "   clippy: --all-targets (crate is in the clippy_tests_kernel cohort)"
    else
        echo "   clippy: lib + bins (crate is outside the clippy_tests_kernel cohort)"
    fi
    echo ""

    run_single_crate_gate() {
        cargo fmt -p "$SINGLE_CRATE_NAME" -- --check || return
        if [ "$CLIPPY_ALL_TARGETS" = true ]; then
            cargo clippy -p "$SINGLE_CRATE_NAME" --all-targets --locked \
                -- -D warnings -A missing_docs || return
        else
            cargo clippy -p "$SINGLE_CRATE_NAME" --locked -- -D warnings || return
        fi
        cargo test -p "$SINGLE_CRATE_NAME" --locked
    }
    GATE_LOG="$(mktemp -t perl-lsp-prepush.XXXXXX.log 2>/dev/null || mktemp)"
    trap 'rm -f "$GATE_LOG"' EXIT
    set +e
    run_single_crate_gate 2>&1 | tee "$GATE_LOG"
    GATE_STATUS=${PIPESTATUS[0]}
    set -e
    if [ "$GATE_STATUS" -ne 0 ]; then
        echo ""
        echo "❌ Single-crate gate failed (exit $GATE_STATUS)"
        echo "   See bypass policy at the top of .git/hooks/pre-push for when"
        echo "   --no-verify is appropriate."
        exit "$GATE_STATUS"
    fi
    echo "✅ Single-crate gate passed"
    exit 0
fi

echo "Running local fast gate before push: nix develop -c just pr-fast"
echo "   (Skip with: git push --no-verify)"
echo ""

# --- Check if test files changed and CURRENT_STATUS.md needs updating ---
if [ "$TEST_FILES_CHANGED" = true ] && [ -f "$REPO_ROOT/scripts/update-current-status.py" ]; then
    echo "📊 Test files changed — checking if docs/project/status/ is up to date..."
    if command -v python3 &>/dev/null; then
        python3 "$REPO_ROOT/scripts/update-current-status.py" 2>/dev/null || true
        if ! git diff --quiet -- docs/project/status/ 2>/dev/null; then
            echo ""
            echo "⚠️  docs/project/status/ has stale test counts!"
            echo "   Run: python3 scripts/update-current-status.py"
            echo "   Then commit the updated files before pushing."
            echo "   git add docs/project/status/"
            echo ""
            exit 1
        fi
    fi
fi

# --- Run the full gate, capturing output for failure-mode hints ---
GATE_LOG="$(mktemp -t perl-lsp-prepush.XXXXXX.log 2>/dev/null || mktemp)"
trap 'rm -f "$GATE_LOG"' EXIT

run_gate() {
    if command -v nix &>/dev/null && [ -f flake.nix ]; then
        nix develop -c just pr-fast
    elif command -v just &>/dev/null; then
        just pr-fast
    else
        echo "⚠️  Neither 'nix develop' nor 'just' available, skipping pre-push gate"
        echo "   Install just: cargo install just"
        return 0
    fi
}

set +e
run_gate 2>&1 | tee "$GATE_LOG"
GATE_STATUS=${PIPESTATUS[0]}
set -e

if [ "$GATE_STATUS" -ne 0 ]; then
    echo ""
    echo "❌ pr-fast failed (exit $GATE_STATUS) — checking for known issues..."
    HINTED=false
    if grep -q 'ci-parser-features-check' "$GATE_LOG" 2>/dev/null && \
       grep -qE 'os error 5|Access is denied' "$GATE_LOG" 2>/dev/null; then
        echo "   • Known: Windows file-lock race in ci-parser-features-check (#3202)"
        echo "     Workaround: re-run the gate, or use --no-verify if you're confident"
        echo "     your change is unrelated to xtask/parser-features."
        HINTED=true
    fi
    if grep -qE 'cargo (xtask )?fmt.*--check' "$GATE_LOG" 2>/dev/null && \
       grep -qE 'Diff in|rustfmt' "$GATE_LOG" 2>/dev/null; then
        echo "   • Formatting drift — run \`cargo xtask fmt\` to auto-fix"
        HINTED=true
    fi
    if grep -qE 'clippy::|warning: .*-> .*\.rs' "$GATE_LOG" 2>/dev/null; then
        echo "   • Clippy warnings — look for the error above and fix the warnings"
        HINTED=true
    fi
    if grep -qE 'os error 206|filename.*too long|ERROR_FILENAME_EXCED' "$GATE_LOG" 2>/dev/null; then
        echo "   • Windows CreateProcess command-line length limit (os error 206)"
        echo "     'cargo fmt --all' passes all 1200+ source files in one command,"
        echo "     exceeding the ~32K-char CreateProcess limit on Windows."
        echo "     Fix: bash scripts/install-githooks.sh"
        echo "     This installs the current hook which uses 'cargo xtask fmt' (per-crate)."
        echo "     See: docs/contributing/FIRST_PR.md"
        HINTED=true
    fi
    if [ "$HINTED" = false ]; then
        echo "   No known-issue patterns matched. Read the gate output above."
    fi
    echo ""
    echo "   See bypass policy at the top of .git/hooks/pre-push for when"
    echo "   --no-verify is appropriate."
    exit "$GATE_STATUS"
fi
"#;
    let scan = install::SECRET_SCAN_SH;
    let call = install::PRE_PUSH_SCAN_CALL;
    format!("{head}{scan}{call}{tail}")
}

/// Worktree-relative hooks directory managed by the installer (#17414 rule C).
///
/// Stored as a repo-local relative `core.hooksPath`, so every linked worktree
/// resolves it against its own top level (`<tree>/.githooks`) while sharing
/// the one config value in the common `.git/config`. Must stay relative: an
/// absolute value would fork all trees onto one shared directory again.
pub(crate) const INSTALLER_HOOKS_PATH: &str = ".githooks";

pub(crate) fn cmd_install_githooks(repo_root: &Path) -> Result<i32> {
    // The shared config flip takes effect in every tree at once, so every
    // existing tree is provisioned in the same run: otherwise siblings
    // without a .githooks dir yet would run unguarded until their own first
    // provision (#17426 review). Writes land before the flip, so a failed
    // flip leaves trees on their old (guarded) common-dir hooks.
    for tree in worktree_roots(repo_root)? {
        let hooks_dir = tree.join(INSTALLER_HOOKS_PATH);
        fs::create_dir_all(&hooks_dir)?;
        write_git_hook(&hooks_dir.join("pre-commit"), &pre_commit_hook_script())?;
        write_git_hook(&hooks_dir.join("pre-push"), &pre_push_hook_script())?;
    }
    set_installer_hooks_path(repo_root)?;

    install::print_install_summary();
    Ok(0)
}

/// Every worktree root (main checkout plus linked trees) sharing this repo.
///
/// Stale entries whose directories are gone are skipped: there is nothing to
/// guard there. An enumeration failure is loud — installing into one tree
/// while siblings stay unknown would silently unguard them at the flip.
fn worktree_roots(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let output = git_command()
        .current_dir(repo_root)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .with_context(|| format!("listing worktrees from {}", repo_root.display()))?;
    if !output.status.success() {
        return Err(color_eyre::eyre::eyre!(
            "git worktree list failed in {}: {}",
            repo_root.display(),
            String::from_utf8_lossy(&output.stderr).trim_end()
        ));
    }
    // NUL-delimited: a worktree path may itself contain a newline, which
    // line-based parsing cannot recover (#17426 review).
    let stdout =
        String::from_utf8(output.stdout).context("git worktree list emitted non-UTF8 output")?;
    let roots = parse_worktree_roots_porcelain_z(&stdout);
    if roots.is_empty() {
        return Err(color_eyre::eyre::eyre!(
            "git worktree list reported no live trees in {}",
            repo_root.display()
        ));
    }
    Ok(roots)
}

/// Live worktree roots from `git worktree list --porcelain -z` output.
///
/// Fields split on NUL, so paths containing newlines survive; stale
/// entries whose directories are gone are dropped.
fn parse_worktree_roots_porcelain_z(output: &str) -> Vec<PathBuf> {
    output
        .split('\0')
        .filter_map(|field| field.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|root| root.is_dir())
        .collect()
}

/// Check that installed hooks match the repository-generated authorities.
pub(crate) fn check_githooks(repo_root: &Path) -> Result<i32> {
    let hooks_dir = resolve_git_hooks_dir(repo_root)?;
    let expected = [("pre-commit", pre_commit_hook_script()), ("pre-push", pre_push_hook_script())];
    let mut status = 0;
    for (name, expected_script) in expected {
        let path = hooks_dir.join(name);
        let actual = match fs::read_to_string(&path) {
            Ok(actual) => actual,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                println!("NOT_PROVEN: installed hook is missing: {}", path.display());
                status = 1;
                continue;
            }
            Err(error) => {
                println!("NOT_PROVEN: cannot read installed hook {}: {error}", path.display());
                status = 1;
                continue;
            }
        };
        if normalize_hook(&actual) == normalize_hook(&expected_script) && is_executable(&path) {
            println!("current: {name}");
        } else {
            println!("stale: {name} ({})", path.display());
            status = 1;
        }
    }
    Ok(status)
}

fn normalize_hook(script: &str) -> String {
    script.replace("\r\n", "\n").trim_end().to_string()
}

/// Point this repository at the installer-managed per-worktree hooks dir.
///
/// `--local` from a linked worktree lands in the shared common `.git/config`,
/// which is exactly what rule C wants: one relative value, resolved per tree.
/// The toolchain never writes the common hooks dir again; whatever remains
/// there is inert (git ignores it while `core.hooksPath` is set).
fn set_installer_hooks_path(repo_root: &Path) -> Result<()> {
    let output = git_command()
        .current_dir(repo_root)
        .args(["config", "--local", "core.hooksPath", INSTALLER_HOOKS_PATH])
        .output()
        .with_context(|| format!("setting core.hooksPath from {}", repo_root.display()))?;

    if !output.status.success() {
        return Err(color_eyre::eyre::eyre!(
            "git config --local core.hooksPath failed in {}: {}",
            repo_root.display(),
            String::from_utf8_lossy(&output.stderr).trim_end()
        ));
    }
    Ok(())
}

fn resolve_git_hooks_dir(repo_root: &Path) -> Result<PathBuf> {
    let output = git_command()
        .current_dir(repo_root)
        .args(["rev-parse", "--git-path", "hooks"])
        .output()
        .with_context(|| format!("resolving git hooks dir from {}", repo_root.display()))?;

    if !output.status.success() {
        return Err(color_eyre::eyre::eyre!(
            "git rev-parse --git-path hooks failed in {}: {}",
            repo_root.display(),
            String::from_utf8_lossy(&output.stderr).trim_end()
        ));
    }

    let hooks_path = String::from_utf8(output.stdout)
        .context("git rev-parse --git-path hooks emitted non-UTF8 output")?;
    let hooks_path = hooks_path.trim();
    let hooks_dir = PathBuf::from(hooks_path);

    Ok(if hooks_dir.is_absolute() { hooks_dir } else { repo_root.join(hooks_dir) })
}

fn write_git_hook(hook_path: &Path, hook: &str) -> Result<()> {
    if fs::symlink_metadata(hook_path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        fs::remove_file(hook_path)
            .with_context(|| format!("removing legacy symlink {:?}", hook_path))?;
    }
    fs::write(hook_path, format!("{hook}\n"))
        .with_context(|| format!("writing {:?}", hook_path))?;
    #[cfg(unix)]
    {
        fs::set_permissions(hook_path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("setting executable bit for {:?}", hook_path))?;
    }
    Ok(())
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> Result<PathBuf> {
        let path = std::env::temp_dir().join(format!(
            "perl-ci-hygiene-hooks-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&path)?;
        let status = git_command().args(["init", "--quiet"]).current_dir(&path).status()?;
        if !status.success() {
            return Err(color_eyre::eyre::eyre!("git init failed"));
        }
        Ok(path)
    }

    #[test]
    fn pre_commit_guard_precedes_exact_staged_gate() -> Result<()> {
        let hook = pre_commit_hook_script();
        let guard = hook
            .find("Refusing commit with placeholder git identity")
            .ok_or_else(|| color_eyre::eyre::eyre!("placeholder identity guard missing"))?;
        let gate = hook
            .find("cargo xtask precommit")
            .ok_or_else(|| color_eyre::eyre::eyre!("staged gate missing"))?;
        assert!(guard < gate);
        // Note this asserts the absence of a bare workspace-wide `cargo fmt`.
        // The staged formatter is `cargo xtask fmt --staged`, which does not
        // match — see `pre_commit_formats_staged_diff_before_the_gate`.
        assert!(!hook.contains("cargo fmt"));
        assert!(!hook.contains("cargo clippy"));
        assert!(!hook.contains("cargo test"));
        assert!(!hook.contains("ripr"));
        Ok(())
    }

    #[test]
    fn pre_commit_never_publishes_the_non_rust_inventory_reference() {
        // #14688: the tracked inventory Markdown is a default-branch
        // publication, not branch merge authority. A hook that regenerates
        // and stages it makes every independently based branch write the
        // same whole-repository snapshot, which is exactly the conflict
        // topology `xtask/tests/non_rust_inventory_conflict_topology.rs`
        // rejects end to end.
        let hook = pre_commit_hook_script();
        assert!(
            !hook.contains("non-rust inventory --write"),
            "generated pre-commit hook must not publish the non-Rust inventory reference"
        );
        assert!(
            !hook.contains("docs/policy/NON_RUST_INVENTORY.md"),
            "generated pre-commit hook must not stage the published inventory reference"
        );
    }

    #[test]
    fn pre_commit_formats_staged_diff_before_the_gate() -> Result<()> {
        // Ordering is the hook's contract: format the staged diff, then let the
        // gate judge the result. Reversed, the gate would reject an index the
        // very next step was about to fix. Nothing else asserted that the
        // formatting step is present at all, so a reorder or a deletion would
        // have gone unnoticed.
        let hook = pre_commit_hook_script();
        let guard = hook
            .find("Refusing commit with placeholder git identity")
            .ok_or_else(|| color_eyre::eyre::eyre!("placeholder identity guard missing"))?;
        let format = hook
            .find("cargo xtask fmt --staged")
            .ok_or_else(|| color_eyre::eyre::eyre!("staged formatting step missing"))?;
        let gate = hook
            .find("cargo xtask precommit")
            .ok_or_else(|| color_eyre::eyre::eyre!("staged gate missing"))?;
        assert!(guard < format, "identity guard must run before staged formatting");
        assert!(format < gate, "staged formatting must run before the commit gate");
        Ok(())
    }

    #[test]
    fn pre_push_hook_refuses_non_fast_forward_updates() -> Result<()> {
        // Issue #17427 (traps T1/T5): the generated hook must deny any ref
        // update that discards remote commits, before any gate runs, and the
        // refusal must carry a recovery order plus the documented admit-list
        // escape hatch. The hatch binds the re-push to the inspected tip
        // (--force-with-lease), never plain --force, so a concurrent push
        // after inspection fails instead of being erased (#17431 review).
        let hook = pre_push_hook_script();
        for marker in [
            "merge-base --is-ancestor",
            "Refusing non-fast-forward",
            "Recover with: git fetch $RECOVERY_REMOTE $RECOVERY_REF",
            "git rebase FETCH_HEAD",
            "PERL_LSP_ALLOW_HISTORY_REWRITE",
            "printf -v RECOVERY_LEASE '%q'",
            "--force-with-lease=$RECOVERY_LEASE",
        ] {
            assert!(hook.contains(marker), "hook must contain push-path refusal marker {marker:?}");
        }
        let refusal = hook
            .find("Refusing non-fast-forward")
            .ok_or_else(|| color_eyre::eyre::eyre!("refusal headline must exist in hook script"))?;
        let gate = hook.find("just pr-fast").ok_or_else(|| {
            color_eyre::eyre::eyre!("fast gate invocation must exist in hook script")
        })?;
        assert!(
            refusal < gate,
            "refusal must precede the gates so doomed pushes fail fast without running them"
        );
        Ok(())
    }

    #[test]
    fn secret_scan_precedes_staged_format_and_gate() -> Result<()> {
        // A token-shaped addition must never reach the formatter or the gate:
        // the scan refuses it before either runs.
        let hook = pre_commit_hook_script();
        let guard = hook
            .find("Refusing commit with placeholder git identity")
            .ok_or_else(|| color_eyre::eyre::eyre!("placeholder identity guard missing"))?;
        let scan = hook
            .find("secret_scan_staged || exit 1")
            .ok_or_else(|| color_eyre::eyre::eyre!("staged secret scan call missing"))?;
        let format = hook
            .find("cargo xtask fmt --staged")
            .ok_or_else(|| color_eyre::eyre::eyre!("staged formatting step missing"))?;
        let gate = hook
            .find("cargo xtask precommit")
            .ok_or_else(|| color_eyre::eyre::eyre!("staged gate missing"))?;
        assert!(guard < scan, "identity guard must run before the secret scan");
        assert!(scan < format, "secret scan must run before staged formatting");
        assert!(format < gate, "staged formatting must run before the commit gate");
        Ok(())
    }

    #[test]
    fn pre_push_hook_refuses_protected_ref_deletion() {
        // Issue #17427: deleting a protected branch must be refused even
        // though deletions of ordinary branches still skip the gate below.
        let hook = pre_push_hook_script();
        assert!(
            hook.contains("PROTECTED_BRANCHES=\"main master\""),
            "hook must name the protected short branch names"
        );
        assert!(
            hook.contains("Refusing deletion of protected ref"),
            "hook must refuse protected-ref deletion with a recovery order"
        );
    }

    #[test]
    fn pre_push_hook_aborts_after_self_heal() {
        // #17431 review (upgrade barrier): when the installed hook detects
        // drift and heals itself, it must refuse the current push instead
        // of guarding it with stale logic — the fresh guards run on re-push.
        let hook = pre_push_hook_script();
        assert!(
            hook.contains("This push was refused so stale logic never guards it"),
            "hook must abort the push it healed during"
        );
        assert!(
            hook.contains("could not be self-updated"),
            "hook must name the failed-heal path honestly instead of claiming an update"
        );
    }

    #[test]
    fn pre_push_hook_self_heal_never_downgrades() {
        // #17431 review wave 3 (downgrade barrier): when the checkout copy
        // predates the installed hook, self-heal must keep the installed
        // hook and continue under its guards — never copy the older file
        // over the newer installation. Only a strictly newer checkout copy
        // upgrades (and refuses that one push).
        let hook = pre_push_hook_script();
        for marker in [
            "# pre-push-hook-version: 1",
            "hook_version()",
            "\"$checkout_version\" -gt \"$installed_version\"",
            "keeping the installed hook. Update this branch/worktree",
        ] {
            assert!(hook.contains(marker), "hook must contain downgrade-barrier marker {marker:?}");
        }
    }

    #[test]
    fn pre_push_hook_self_heal_replaces_atomically() {
        // #17431 review wave 4: the upgrade copy must stage to a temp file
        // and rename over the installed hook — a failed cp must never leave
        // $0 truncated, or later pushes run a broken hook.
        let hook = pre_push_hook_script();
        for marker in ["heal_tmp=\"$0.tmp.$$\"", "mv \"$heal_tmp\" \"$0\"", "rm -f \"$heal_tmp\""] {
            assert!(hook.contains(marker), "hook must contain atomic-replace marker {marker:?}");
        }
        assert!(
            !hook.contains("cp \"$REPO_ROOT_FOR_HOOK/hooks/pre-push\" \"$0\""),
            "hook must not copy directly over the installed hook"
        );
    }

    #[test]
    fn embedded_pre_push_matches_checked_in_hook() -> Result<()> {
        // The installer writes the embedded bytes; the traps and the
        // drift detectors compare against hooks/pre-push. If the two
        // drift apart, installed hooks silently stop matching the
        // authority they are verified against.
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let checked_in = fs::read_to_string(manifest.join("../../hooks/pre-push"))?;
        assert_eq!(
            normalize_hook(&pre_push_hook_script()),
            normalize_hook(&checked_in),
            "embedded pre-push bytes must match hooks/pre-push"
        );
        Ok(())
    }

    #[test]
    fn secret_scan_precedes_push_gates() -> Result<()> {
        // The pushed-range scan runs before every push gate, including the
        // doc-only fast path: secrets in docs refuse too.
        let hook = pre_push_hook_script();
        let scan = hook
            .find("secret_scan_range \"$push_local\" \"$push_remote\" || exit 1")
            .ok_or_else(|| color_eyre::eyre::eyre!("pushed-range secret scan call missing"))?;
        let doc_only = hook
            .find("# --- Detect doc-only changes")
            .ok_or_else(|| color_eyre::eyre::eyre!("doc-only detection missing"))?;
        let fast_gate = hook
            .find("Running local fast gate")
            .ok_or_else(|| color_eyre::eyre::eyre!("fast-gate invocation missing"))?;
        assert!(scan < doc_only, "secret scan must run before the doc-only fast path");
        assert!(doc_only < fast_gate, "doc-only detection must precede the fast gate");
        Ok(())
    }

    #[test]
    fn installed_pre_push_hook_carries_push_path_refusal() -> Result<()> {
        // The installer must write the refusal into the installed hook, and
        // `check_githooks` must verify those installed bytes as current.
        let repo = temp_repo()?;
        cmd_install_githooks(&repo)?;
        assert_eq!(check_githooks(&repo)?, 0);

        let hooks_dir = resolve_git_hooks_dir(&repo)?;
        let installed = fs::read_to_string(hooks_dir.join("pre-push"))?;
        for marker in [
            "merge-base --is-ancestor",
            "Refusing non-fast-forward",
            "Refusing deletion of protected ref",
            "PERL_LSP_ALLOW_HISTORY_REWRITE",
        ] {
            assert!(
                installed.contains(marker),
                "installed pre-push hook must contain push-path refusal marker {marker:?}"
            );
        }
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn secret_scan_shell_matches_lib_rules() -> Result<()> {
        // The lib TOKEN_SHAPES is the rule authority; the shell copy embedded
        // in both hooks must contain every shape exactly. The generic rule is
        // double-quoted in shell (it holds a literal single quote), so its \"
        // escape normalizes back before comparing.
        let shell = install::SECRET_SCAN_SH.replace("\\\"", "\"");
        for (name, ere) in perl_ci_hygiene::secret_scan::TOKEN_SHAPES {
            assert!(shell.contains(ere), "shell scan must embed the lib {name} shape exactly");
        }
        for var in [
            "SECRET_SCAN_RULE_GITHUB",
            "SECRET_SCAN_RULE_AWS",
            "SECRET_SCAN_RULE_SLACK",
            "SECRET_SCAN_RULE_PRIVKEY",
            "SECRET_SCAN_RULE_GENERIC",
        ] {
            assert!(shell.contains(var), "shell scan must define {var}");
        }
        Ok(())
    }

    #[test]
    fn secret_scan_refusal_names_rule_and_allowlist() {
        // The refusal must name where and what without reproducing the
        // credential, and must point at the documented fixture escape.
        for hook in [pre_commit_hook_script(), pre_push_hook_script()] {
            assert!(hook.contains("Secret scan refused"), "refusal message missing");
            assert!(hook.contains("rule $name matched"), "refusal must name the matched rule");
            assert!(
                hook.contains(perl_ci_hygiene::secret_scan::ALLOWLIST_PATH),
                "refusal must name the allowlist escape path"
            );
        }
    }

    #[test]
    fn secret_scan_grep_invocations_are_option_terminated() {
        // The private-key shape starts with dashes; without `--` grep parses
        // it as options and that rule fails open (found by dogfooding the
        // generated hook over this worktree's own staged tree).
        assert!(
            install::SECRET_SCAN_SH.contains("grep -E -c -- "),
            "rule greps must terminate options before the pattern"
        );
        assert!(
            install::SECRET_SCAN_SH.contains("grep -F -x -q -- "),
            "allowlist greps must terminate options before the path"
        );
    }

    #[test]
    fn secret_scan_snippet_is_embedded_once_per_hook() {
        // Single embedding keeps the trap extraction (first BEGIN..END range)
        // unambiguous and proves both hooks share the const.
        for hook in [pre_commit_hook_script(), pre_push_hook_script()] {
            assert_eq!(
                hook.matches("\n# SECRET_SCAN_SH_BEGIN\n").count(),
                1,
                "each hook must embed the scan snippet exactly once"
            );
            assert_eq!(
                hook.matches("\n# SECRET_SCAN_SH_END\n").count(),
                1,
                "each hook must embed the scan snippet exactly once"
            );
        }
    }

    #[test]
    fn installed_hook_check_detects_current_and_stale_versions() -> Result<()> {
        let repo = temp_repo()?;
        cmd_install_githooks(&repo)?;
        assert_eq!(check_githooks(&repo)?, 0);

        let hooks_dir = resolve_git_hooks_dir(&repo)?;
        fs::write(hooks_dir.join("pre-commit"), "stale\n")?;
        assert_eq!(check_githooks(&repo)?, 1);
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn installer_sets_relative_hookspath_and_leaves_common_dir_alone() -> Result<()> {
        // #17414 rule C: the installer owns a repo-local relative
        // core.hooksPath and writes only the per-tree dir it resolves to.
        let repo = temp_repo()?;
        cmd_install_githooks(&repo)?;

        let output = git_command()
            .current_dir(&repo)
            .args(["config", "--get", "core.hooksPath"])
            .output()?;
        assert!(output.status.success());
        let value = String::from_utf8(output.stdout)?;
        assert_eq!(value.trim(), INSTALLER_HOOKS_PATH);
        assert!(
            Path::new(value.trim()).is_relative(),
            "core.hooksPath must stay worktree-relative"
        );

        assert!(resolve_git_hooks_dir(&repo)? == repo.join(INSTALLER_HOOKS_PATH));
        assert!(repo.join(INSTALLER_HOOKS_PATH).join("pre-push").is_file());
        assert!(repo.join(INSTALLER_HOOKS_PATH).join("pre-commit").is_file());
        assert!(!repo.join(".git").join("hooks").join("pre-push").exists());
        assert!(!repo.join(".git").join("hooks").join("pre-commit").exists());
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn installer_provisions_sibling_worktrees_in_the_same_run() -> Result<()> {
        // #17426 review: the shared config flip takes effect in every tree
        // at once, so installing from one tree must guard its siblings too.
        let repo = temp_repo()?;
        let seed = git_command()
            .current_dir(&repo)
            .args(["-c", "user.email=t@t", "-c", "user.name=t"])
            .arg("commit")
            .args(["--quiet", "--allow-empty", "-m", "seed"])
            .status()?;
        assert!(seed.success());
        let sib = repo.with_extension("sib");
        let added = git_command()
            .current_dir(&repo)
            .args(["worktree", "add", "--quiet"])
            .arg(&sib)
            .status()?;
        assert!(added.success());

        cmd_install_githooks(&repo)?;

        for tree in [&repo, &sib] {
            assert!(tree.join(INSTALLER_HOOKS_PATH).join("pre-push").is_file());
            assert!(tree.join(INSTALLER_HOOKS_PATH).join("pre-commit").is_file());
        }
        let output =
            git_command().current_dir(&sib).args(["config", "--get", "core.hooksPath"]).output()?;
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout)?.trim(), INSTALLER_HOOKS_PATH);

        fs::remove_dir_all(&sib)?;
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn worktree_parser_keeps_stale_entries_out() {
        // Missing directories are dropped, never provisioned.
        let output = "worktree /definitely/not/here\0HEAD abc\0worktree /also/missing\0";
        assert!(parse_worktree_roots_porcelain_z(output).is_empty());
    }

    // Newlines are illegal in Windows path names, so the newline-path
    // case can only be built on Unix; the -z listing itself still runs
    // on every platform via the installer tests below.
    #[cfg(unix)]
    #[test]
    fn worktree_parser_survives_newline_paths() -> Result<()> {
        // #17426 review: NUL-delimited parsing must recover a path a
        // line-based parser would shred (and then drop as non-dir,
        // leaving that tree unguarded at the flip).
        let base = std::env::temp_dir().join(format!(
            "perl-ci-hygiene-hooks-nl-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let odd = base.join("wt\nwith\nnewlines");
        fs::create_dir_all(&odd)?;
        let output = format!(
            "worktree {}\0HEAD abc123\0branch refs/heads/x\0worktree /also/missing\0",
            odd.display()
        );
        assert_eq!(parse_worktree_roots_porcelain_z(&output), vec![odd]);
        fs::remove_dir_all(base)?;
        Ok(())
    }

    /// Child-process probe: runs inside a child whose *inherited* environment
    /// carries hostile GIT_DIR/GIT_WORK_TREE/GIT_COMMON_DIR, and reports the
    /// installer outcome with a stdout marker. No-op under the normal harness
    /// (marker env absent), mirroring the `process::tests` child pattern.
    const INHERITED_GIT_ENV_FILTER: &str = "git_hooks::tests::inherited_git_env_child";
    const INHERITED_GIT_ENV_REPO: &str = "PERL_CI_HYGIENE_GIT_ENV_TEST_REPO";
    const INHERITED_GIT_ENV_DECOY: &str = "PERL_CI_HYGIENE_GIT_ENV_TEST_DECOY";
    const INHERITED_GIT_ENV_MARKER: &str = "INHERITED-GIT-ENV-OK";

    #[test]
    fn inherited_git_env_child() -> Result<()> {
        let repo = match std::env::var_os(INHERITED_GIT_ENV_REPO) {
            None => return Ok(()),
            Some(path) => PathBuf::from(path),
        };
        let decoy = PathBuf::from(std::env::var_os(INHERITED_GIT_ENV_DECOY).ok_or_else(|| {
            color_eyre::eyre::eyre!("child decoy path missing ({INHERITED_GIT_ENV_DECOY} unset)")
        })?);
        cmd_install_githooks(&repo)?;
        assert!(repo.join(INSTALLER_HOOKS_PATH).join("pre-push").is_file());
        let flipped = git_command()
            .current_dir(&decoy)
            .args(["config", "--get", "core.hooksPath"])
            .output()?;
        assert!(!flipped.status.success(), "decoy repo must not gain core.hooksPath");
        println!("{INHERITED_GIT_ENV_MARKER}");
        Ok(())
    }

    #[test]
    fn installer_ignores_inherited_git_repository_env() -> Result<()> {
        // #17426 review: a stray GIT_DIR/GIT_WORK_TREE/GIT_COMMON_DIR must
        // not redirect enumeration or the hooksPath flip at another repo.
        // #17426 review wave 2: the hostile variables are inherited by a
        // child probe process — the parent never mutates process env, so no
        // concurrently running test can observe the pollution (#1269).
        let repo = temp_repo()?;
        let decoy = temp_repo()?;
        let child = Command::new(std::env::current_exe()?)
            .args([INHERITED_GIT_ENV_FILTER, "--exact", "--nocapture"])
            .env("GIT_DIR", decoy.join(".git"))
            .env("GIT_WORK_TREE", decoy.as_os_str())
            .env("GIT_COMMON_DIR", decoy.join(".git"))
            .env(INHERITED_GIT_ENV_REPO, repo.as_os_str())
            .env(INHERITED_GIT_ENV_DECOY, decoy.as_os_str())
            .output()?;
        let stdout = String::from_utf8_lossy(&child.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&child.stderr).into_owned();
        fs::remove_dir_all(repo)?;
        fs::remove_dir_all(decoy)?;
        assert!(
            child.status.success() && stdout.contains(INHERITED_GIT_ENV_MARKER),
            "child probe must install under hostile inherited git env and report {INHERITED_GIT_ENV_MARKER}:\n{stdout}\n{stderr}"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn installed_hook_check_rejects_non_executable_current_bytes() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let repo = temp_repo()?;
        cmd_install_githooks(&repo)?;
        let hooks_dir = resolve_git_hooks_dir(&repo)?;
        fs::set_permissions(hooks_dir.join("pre-commit"), fs::Permissions::from_mode(0o644))?;
        assert_eq!(check_githooks(&repo)?, 1);
        fs::remove_dir_all(repo)?;
        Ok(())
    }
}
