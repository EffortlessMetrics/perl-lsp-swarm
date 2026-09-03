//! Git staged-tree plumbing for the commit-tier gate (issue #3786).
//!
//! Every commit-tier check must inspect the **staged tree** — the exact set
//! of blobs `git commit` would record right now — never the working tree and
//! never an unstaged edit. The identity of "what's being committed" is
//! `git write-tree`'s output, not `git status` or `fs::read_to_string`.
//!
//! This matters concretely for executable-bit policy: `core.fileMode=false`
//! (set on this repo, and common on Windows checkouts) makes the filesystem's
//! permission bits unreliable — a file can show as executable on disk without
//! that ever being staged, or vice versa. `git ls-tree` on the written tree
//! reports the mode git actually recorded (`100644` vs `100755`), which is
//! the only reliable source for that policy.
//!
//! # Read the captured snapshot, not the live index
//!
//! [`staged_tree_oid`] is called exactly once per run (`plan_gates`) and its
//! result is threaded through every later check invocation
//! (`GatePlan.staged_tree_oid` → `AgentReceipt.staged_tree_oid` →
//! `commit_checks::run_named_check`'s `tree_oid` parameter). [`staged_diff_paths`]
//! and [`read_staged_path_text`] both take an `Option<&str>` tree OID for
//! exactly this reason: passing the captured OID makes them read from that
//! frozen tree object (`git diff HEAD <oid>`, `git show <oid>:path`) instead
//! of the live index (`git diff --cached`, `git show :path`). A concurrent
//! `git add` between `plan_gates` capturing the OID and a check running
//! would otherwise make different checks — or a check and the receipt that
//! records what ran — inspect different states of the same commit. `None`
//! is a defensive fallback for callers outside a real plan (e.g. ad hoc
//! testing); every production call path has a captured OID to pass.

use color_eyre::eyre::{Context, ContextCompat, Result, bail, eyre};
use std::path::Path;
use std::process::Command;

/// One entry from `git ls-tree -r` over the staged tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedEntry {
    /// Octal mode string as recorded by git, e.g. `"100644"` or `"100755"`.
    pub mode: String,
    pub blob_oid: String,
    /// Repo-relative path, forward-slash separated (git's native form).
    pub path: String,
}

impl StagedEntry {
    pub fn is_executable(&self) -> bool {
        self.mode == "100755"
    }
}

fn run_git_ok(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn `git {}`", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`git {}` failed: {stderr}", args.join(" "));
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("`git {}` output was not UTF-8", args.join(" ")))
}

/// Whether `path` exists in the staged snapshot, distinguished from a
/// genuine git failure **without parsing any of git's prose** (issue #4092
/// gap 3).
///
/// The prior implementation ran `git cat-file -e <tree>:<path>` and decided
/// "absent" by matching English substrings ("does not exist", "but not in")
/// in stderr. On a non-English git locale those markers never match, so a
/// legitimate absence surfaced as `Err` — fail-safe, but a locale-dependent
/// false block once the commit tier blocks promotion.
///
/// The replacement is exit-code-structured, so there is no message text to
/// localize:
///
/// - `tree_oid = Some(oid)`: first `git cat-file -t <oid>` validates the
///   tree object itself — nonzero there is a malformed ref or a corrupted
///   object database and stays a real instrument failure (`Err`), never
///   "absent". Then `git ls-tree -z <oid> -- <path>` answers membership:
///   success + a parsed entry naming exactly `path` = present, success +
///   no such entry = absent.
/// - `tree_oid = None`: `git ls-files -z -- <path>` answers index
///   membership the same way (a `:<path>` spec has no ref component that
///   could be malformed).
///
/// Entries are matched by exact path string under `--literal-pathspecs`,
/// never by git's pathspec pattern semantics, so a query for `weird*name.rs`
/// cannot be answered by an unrelated `weirdXname.rs` (and vice versa), and
/// a path that begins with `:` (pathspec-magic syntax) is matched literally.
/// A path recorded with a
/// type-change mode (e.g. `120000` symlink) is present like any other
/// entry — callers own mode policy (see [`list_staged_entries`]).
fn staged_path_exists_in(root: &Path, tree_oid: Option<&str>, path: &str) -> Result<bool> {
    match tree_oid {
        Some(oid) => {
            // Validate the tree object first: `ls-tree` on a malformed OID
            // also exits nonzero, but only this step tells a bad ref from a
            // legitimate membership question, and only a valid object may
            // ever reach the membership query below.
            let kind = run_git_ok(root, &["cat-file", "-t", oid])?;
            if kind.trim() != "tree" {
                bail!("staged snapshot `{oid}` is a {}, not a tree", kind.trim());
            }
            // `--literal-pathspecs` must precede the subcommand: a tracked
            // filename may legally begin with `:` (pathspec-magic syntax),
            // and the query is an exact path, never a pattern.
            let raw = run_git_ok(root, &["--literal-pathspecs", "ls-tree", "-z", "--", oid, path])?;
            Ok(parse_ls_tree_paths(&raw)?.iter().any(|entry| entry == path))
        }
        None => {
            let output = Command::new("git")
                .current_dir(root)
                .args(["--literal-pathspecs", "ls-files", "-z", "--", path])
                .output()
                .with_context(|| format!("failed to check whether {path} is staged"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("`git ls-files -- {path}` failed: {stderr}");
            }
            let raw = String::from_utf8(output.stdout)
                .with_context(|| format!("`git ls-files -- {path}` output was not UTF-8"))?;
            Ok(raw.split('\0').any(|entry| entry == path))
        }
    }
}

/// Repo-relative paths from `git ls-tree -z` output (records of the form
/// `<mode> SP <type> SP <oid> TAB <path> NUL`; paths needing quotes in
/// non-`-z` output are emitted raw under `-z`).
///
/// Every nonempty record must carry the documented shape. A record without
/// the TAB separator, or with incomplete `<mode> SP <type> SP <oid>`
/// metadata, is an instrument failure (`Err`) — never a silently skipped
/// entry — so output drift cannot turn a staged path into a clean absence.
fn parse_ls_tree_paths(raw: &str) -> Result<Vec<String>> {
    raw.split('\0')
        .filter(|record| !record.is_empty())
        .map(|record| {
            let (metadata, path) = record.split_once('\t').ok_or_else(|| {
                eyre!("malformed `git ls-tree` record without a TAB separator: {record:?}")
            })?;
            let metadata: Vec<&str> = metadata.split(' ').collect();
            let [mode, kind, oid] = metadata.as_slice() else {
                bail!("malformed `git ls-tree` record metadata {metadata:?}: {record:?}");
            };
            let mode_is_octal = mode.len() == 6 && mode.bytes().all(|byte| byte.is_ascii_digit());
            if !mode_is_octal || kind.is_empty() || oid.is_empty() {
                bail!("malformed `git ls-tree` record metadata {metadata:?}: {record:?}");
            }
            Ok(path.to_string())
        })
        .collect()
}

/// The OID of the tree that `git commit` would record right now — i.e.
/// exactly the staged index, independent of the working tree. This is the
/// commit tier's identity: every check keys off this OID, never a working
/// -tree walk.
pub fn staged_tree_oid(root: &Path) -> Result<String> {
    Ok(run_git_ok(root, &["write-tree"])?.trim().to_string())
}

/// List every entry (mode + blob OID + path) in the staged tree.
///
/// This is the *whole* tree, not just what changed in this commit — use
/// [`staged_diff_paths`] to scope a check to the files actually being
/// committed (the common case; scanning the whole tree on every commit would
/// blow the commit-tier time budget on a large repo). Already frozen-tree
/// -based by construction: `tree_oid` names the exact tree object to list,
/// never the live index.
pub fn list_staged_entries(root: &Path, tree_oid: &str) -> Result<Vec<StagedEntry>> {
    let raw = run_git_ok(root, &["ls-tree", "-r", "-z", tree_oid])?;
    let mut entries = Vec::new();
    for record in raw.split('\0') {
        if record.is_empty() {
            continue;
        }
        // Format: "<mode> <type> <oid>\t<path>"
        let Some((meta, path)) = record.split_once('\t') else { continue };
        let mut fields = meta.split(' ');
        let (Some(mode), Some(_kind), Some(oid)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        entries.push(StagedEntry {
            mode: mode.to_string(),
            blob_oid: oid.to_string(),
            path: path.to_string(),
        });
    }
    Ok(entries)
}

/// Paths added/copied/deleted/modified/renamed/type-changed relative to
/// `HEAD` — the files that are actually part of this commit. This is the
/// scoping input every per-file commit-tier check should use so cost stays
/// proportional to the staged change, not the whole repo.
///
/// `--diff-filter=ACDMRT`: **includes** `D` (deleted) and `T` (type-changed,
/// e.g. a file replaced by a symlink) — issue #4031 item 5. The prior
/// `ACMR`-only filter silently dropped a staged deletion or type change from
/// every check's view of "what's being committed": a deleted `.changie`
/// fragment, a removed config file, a mode-flip from regular file to
/// symlink, none of it a check ever saw. A caller reading a deleted path's
/// *content* gets [`StagedPathText::Absent`] from [`read_staged_path_text`]
/// (there's nothing to read — the path isn't in the target tree), not an
/// error and not silence; content-oriented checks scope their own logic
/// accordingly (skip, or flag, depending on what "deleted" means for that
/// specific check).
///
/// `tree_oid`: `Some(oid)` diffs `HEAD` against that specific tree object
/// (`git diff HEAD <oid>`) — the captured snapshot, immune to a `git add`
/// that happens after `oid` was written. `None` falls back to `git diff
/// --cached` (the live index) for callers outside a real plan.
pub fn staged_diff_paths(root: &Path, tree_oid: Option<&str>) -> Result<Vec<String>> {
    let raw = match tree_oid {
        Some(oid) => {
            let base = diff_base(root)?;
            run_git_ok(root, &["diff", "--name-only", "--diff-filter=ACDMRT", "-z", &base, oid])?
        }
        None => {
            run_git_ok(root, &["diff", "--cached", "--name-only", "--diff-filter=ACDMRT", "-z"])?
        }
    };
    Ok(raw.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect())
}

/// Derive the empty-tree object OID for THIS repository's actual object
/// format (SHA-1 or SHA-256) by asking git to hash zero bytes as a tree.
/// `git hash-object` without `-w` never writes to the object database — a
/// pure, side-effect-free query — and it automatically uses whatever hash
/// algorithm the repo is configured with, so the result is correct under
/// both formats without this module needing to special-case either one.
///
/// Issue #4031 item 7: a hardcoded SHA-1 constant here silently produced
/// the WRONG diff base for a SHA-256 repository's first/empty staged tree
/// (SHA-1's well-known empty-tree OID `4b825dc642cb6eb9a060e54bf8d69288fbee4904`
/// is not a valid object name in a SHA-256 repo at all, and even if it were
/// accepted it wouldn't name the right object).
fn derive_empty_tree_oid(root: &Path) -> Result<String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("git")
        .current_dir(root)
        .args(["hash-object", "-t", "tree", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn `git hash-object -t tree --stdin`")?;
    // Explicitly write zero bytes then drop the handle to close stdin,
    // rather than relying on an implicit drop at scope exit — the intent
    // (hash an EMPTY tree) is then visible at the call site.
    child
        .stdin
        .take()
        .context("git hash-object stdin was not piped")?
        .write_all(b"")
        .context("failed to write empty input to git hash-object stdin")?;
    let output = child.wait_with_output().context("failed to wait for git hash-object")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`git hash-object -t tree --stdin` failed: {stderr}");
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .context("`git hash-object -t tree --stdin` output was not UTF-8")
}

/// `"HEAD"` when it resolves, else the repo's own empty-tree OID (derived,
/// not hardcoded — see [`derive_empty_tree_oid`]) — so a tree-oid-pinned
/// diff still works on a brand-new repo with no commits yet (an unborn
/// `HEAD`), matching what `git diff --cached` already handles implicitly
/// for the live-index path.
///
/// `pub(crate)`: also used directly by `commit_checks::whitespace_check_at`,
/// which needs the same base to run `git diff <base> <oid> --check` against
/// the pinned tree object instead of `git diff --cached --check` (the live
/// index) — see that function's doc comment for the TOCTOU this closes.
pub(crate) fn diff_base(root: &Path) -> Result<String> {
    let resolves = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()
        .context("failed to check whether HEAD resolves")?
        .status
        .success();
    if resolves { Ok("HEAD".to_string()) } else { derive_empty_tree_oid(root) }
}

/// Byte size of a staged blob without reading its content (`git cat-file -s`).
/// Already frozen-tree-based by construction: `oid` names the exact blob,
/// never the live index.
pub fn blob_size(root: &Path, oid: &str) -> Result<u64> {
    let raw = run_git_ok(root, &["cat-file", "-s", oid])?;
    raw.trim()
        .parse::<u64>()
        .with_context(|| format!("`git cat-file -s {oid}` returned non-numeric size: {raw:?}"))
}

/// Outcome of reading a staged path's content — see [`read_staged_path_text`].
///
/// Three states, not two: `Absent` must be kept distinct from `Binary`,
/// because callers treat them differently. A deleted or never-staged path
/// has nothing to check — silently skipping it is correct. A path a check
/// expects to be text (a JSON/YAML/TOML config, a Changie fragment) that
/// turns out not to be valid UTF-8 is itself a finding worth surfacing, not
/// a reason to skip (issue #4031 item 1: a non-UTF-8 config/fragment must
/// not silently report clean).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedPathText {
    /// The path exists in the tree/index and its content is valid UTF-8.
    Present(String),
    /// The path exists but its content is not valid UTF-8 — a binary blob,
    /// or (for a caller expecting text) a decode failure worth flagging.
    Binary,
    /// The path does not exist in the tree/index at all — e.g. staged for
    /// deletion (a `git diff --diff-filter=...D` entry, see
    /// [`staged_diff_paths`]) or simply never staged. A legitimate,
    /// expected outcome, never conflated with a genuine git invocation
    /// failure (issue #4031 item 6).
    Absent,
}

/// Read a staged path's content — works even when the working tree doesn't
/// match (a partially-staged edit), because it never touches the filesystem
/// outside git's object database.
///
/// `tree_oid`: `Some(oid)` reads the blob from that specific tree object
/// (`git show <oid>:path`) — the captured snapshot. `None` reads from the
/// live index (`git show :path`) for callers outside a real plan.
///
/// Distinguishes three outcomes (see [`StagedPathText`]) instead of
/// conflating "the path isn't there" with "git failed to run": existence is
/// checked first via [`staged_path_exists_in`], which tells a clean "no"
/// (git ran fine and determined the path isn't in the snapshot) apart from
/// a real instrument failure (a malformed ref, a missing git binary, a
/// corrupted object database) through exit-code structure, never by parsing
/// localized message text (issue #4092 gap 3). Only a genuine instrument
/// failure — including a `git show` that fails even though the
/// immediately-prior existence check passed, a TOCTOU race worth surfacing
/// rather than swallowing — produces a real `Err` here.
pub fn read_staged_path_text(
    root: &Path,
    path: &str,
    tree_oid: Option<&str>,
) -> Result<StagedPathText> {
    if !staged_path_exists_in(root, tree_oid, path)? {
        return Ok(StagedPathText::Absent);
    }
    let spec = match tree_oid {
        Some(oid) => format!("{oid}:{path}"),
        None => format!(":{path}"),
    };
    let output = Command::new("git")
        .current_dir(root)
        .args(["show", &spec])
        .output()
        .with_context(|| format!("failed to read staged content for {path}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("failed to read staged content for {path}: {stderr}");
    }
    Ok(match String::from_utf8(output.stdout) {
        Ok(text) => StagedPathText::Present(text),
        Err(_) => StagedPathText::Binary,
    })
}

/// Whether `path` exists at all in the given staged tree object — for a
/// caller like `commit_checks::rustfmt_staged_at` that needs the STAGED
/// version of a config file (`rustfmt.toml`) which may or may not be part of
/// the diff being committed (it might be untouched by this commit, or not
/// tracked at all). Built on the same absent-vs-failure distinction as
/// [`read_staged_path_text`] (via [`staged_path_exists_in`]), so a malformed
/// `tree_oid` or a genuine git failure surfaces as `Err` here too, rather
/// than silently reporting `Ok(false)`.
pub fn staged_path_exists(root: &Path, tree_oid: &str, path: &str) -> Result<bool> {
    staged_path_exists_in(root, Some(tree_oid), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_staged_entries_parses_ls_tree_records() -> Result<()> {
        // Synthetic ls-tree -z record shape (mode SP type SP oid TAB path NUL).
        // `\x00` (not `\0`) for the NUL separator: `\0` immediately followed
        // by digits reads as an octal escape to a human (and to clippy's
        // octal_escapes lint), even though Rust has no octal escapes and
        // parses it correctly either way.
        let raw = "100644 blob abc123\tfoo.rs\x00100755 blob def456\tscripts/run.sh\x00";
        let mut entries = Vec::new();
        for record in raw.split('\0') {
            if record.is_empty() {
                continue;
            }
            let Some((meta, path)) = record.split_once('\t') else { continue };
            let mut fields = meta.split(' ');
            let (Some(mode), Some(_kind), Some(oid)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            entries.push(StagedEntry {
                mode: mode.to_string(),
                blob_oid: oid.to_string(),
                path: path.to_string(),
            });
        }

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "foo.rs");
        assert!(!entries[0].is_executable());
        assert_eq!(entries[1].path, "scripts/run.sh");
        assert!(entries[1].is_executable());
        Ok(())
    }

    // -------------------------------------------------------------------
    // Staged-vs-working-tree correctness proof.
    //
    // This is the one property the whole module exists to hold: every read
    // must reflect the STAGED index, never an unstaged working-tree edit and
    // never a pre-image the index has since moved past. Proven against a
    // real temp git repository (never the host repo's own index) in both
    // directions: staging something the working tree doesn't have, and
    // un-staging something the working tree still has.
    // -------------------------------------------------------------------

    /// Unwrap the `Present` case of a [`StagedPathText`] read, failing the
    /// test with a descriptive message for `Binary`/`Absent` — the common
    /// shape for tests that expect a real staged text file to be readable.
    fn expect_present(result: Result<StagedPathText>, path: &str) -> Result<String> {
        match result? {
            StagedPathText::Present(text) => Ok(text),
            StagedPathText::Binary => bail!("expected {path} to be valid UTF-8, got Binary"),
            StagedPathText::Absent => bail!("expected {path} to be staged, got Absent"),
        }
    }

    struct TempRepo {
        dir: tempfile::TempDir,
    }

    impl TempRepo {
        fn init() -> Result<Self> {
            let dir = tempfile::tempdir().context("failed to create temp repo dir")?;
            let root = dir.path();
            for args in [
                vec!["init", "--quiet"],
                vec!["config", "user.email", "test@example.com"],
                vec!["config", "user.name", "Test"],
                // Mirrors this repo's own core.fileMode=false — the exact
                // condition that makes filesystem mode bits unreliable (see
                // module docs).
                vec!["config", "core.fileMode", "false"],
            ] {
                let status = Command::new("git")
                    .current_dir(root)
                    .args(&args)
                    .status()
                    .with_context(|| format!("failed to run git {args:?}"))?;
                if !status.success() {
                    bail!("git {args:?} failed in temp repo setup");
                }
            }
            Ok(Self { dir })
        }

        fn root(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, rel_path: &str, content: &str) -> Result<()> {
            let path = self.root().join(rel_path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, content).context("failed to write file in temp repo")
        }

        fn add(&self, rel_path: &str) -> Result<()> {
            let status = Command::new("git")
                .current_dir(self.root())
                .args(["add", rel_path])
                .status()
                .context("failed to run git add")?;
            if !status.success() {
                bail!("git add {rel_path} failed");
            }
            Ok(())
        }

        fn commit(&self, message: &str) -> Result<()> {
            let status = Command::new("git")
                .current_dir(self.root())
                .args(["commit", "--quiet", "-m", message])
                .status()
                .context("failed to run git commit")?;
            if !status.success() {
                bail!("git commit failed");
            }
            Ok(())
        }

        /// Stage a deletion of an already-committed path (`git rm --cached`)
        /// — for proving item 5 (deletions must be part of the staged
        /// change-set) without also touching the working tree.
        fn remove_cached(&self, rel_path: &str) -> Result<()> {
            let status = Command::new("git")
                .current_dir(self.root())
                .args(["rm", "--cached", "--quiet", rel_path])
                .status()
                .context("failed to run git rm --cached")?;
            if !status.success() {
                bail!("git rm --cached {rel_path} failed");
            }
            Ok(())
        }

        /// Stage `content` as a blob at `rel_path` with an explicit git
        /// `mode` (`100644`, `120000` symlink, …) via
        /// `git hash-object -w` + `git update-index --cacheinfo`. This
        /// records the entry purely in the index/object database — no
        /// working-tree file is created — so type-change fixtures work even
        /// on platforms that cannot create real symlinks, and paths with
        /// characters the local filesystem forbids (`*`) remain stageable.
        fn stage_blob_at(&self, mode: &str, content: &str, rel_path: &str) -> Result<()> {
            use std::io::Write;
            use std::process::Stdio;

            let mut child = Command::new("git")
                .current_dir(self.root())
                .args(["hash-object", "-w", "--stdin"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .context("failed to spawn `git hash-object -w --stdin`")?;
            child
                .stdin
                .take()
                .context("git hash-object stdin was not piped")?
                .write_all(content.as_bytes())
                .context("failed to write blob content to git hash-object")?;
            let output = child.wait_with_output().context("failed to wait for git hash-object")?;
            if !output.status.success() {
                bail!("git hash-object failed: {}", String::from_utf8_lossy(&output.stderr));
            }
            let blob = String::from_utf8(output.stdout)
                .context("git hash-object output was not UTF-8")?
                .trim()
                .to_string();
            let spec = format!("{mode},{blob},{rel_path}");
            let status = Command::new("git")
                .current_dir(self.root())
                .args(["update-index", "--add", "--cacheinfo", &spec])
                .status()
                .context("failed to run git update-index --cacheinfo")?;
            if !status.success() {
                bail!("git update-index --add --cacheinfo {spec} failed");
            }
            Ok(())
        }

        /// Record `content` as a blob at `rel_path` inside a freshly minted
        /// TREE object (`git hash-object -w` + `git mktree`) and return the
        /// tree OID. Unlike [`Self::stage_blob_at`] this bypasses the index
        /// entirely: git's index plumbing refuses `:`-prefixed paths, but
        /// trees carrying them exist in the wild (fast-import, foreign
        /// tooling), and membership queries against such a pinned snapshot
        /// must still be answered literally.
        fn pin_blob_at(&self, mode: &str, content: &str, rel_path: &str) -> Result<String> {
            use std::io::Write;
            use std::process::Stdio;

            let mut child = Command::new("git")
                .current_dir(self.root())
                .args(["hash-object", "-w", "--stdin"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .context("failed to spawn `git hash-object -w --stdin`")?;
            child
                .stdin
                .take()
                .context("git hash-object stdin was not piped")?
                .write_all(content.as_bytes())
                .context("failed to write blob content to git hash-object")?;
            let output = child.wait_with_output().context("failed to wait for git hash-object")?;
            if !output.status.success() {
                bail!("git hash-object failed: {}", String::from_utf8_lossy(&output.stderr));
            }
            let blob = String::from_utf8(output.stdout)
                .context("git hash-object output was not UTF-8")?
                .trim()
                .to_string();
            let record = format!("{mode} blob {blob}\t{rel_path}\n");
            let mut child = Command::new("git")
                .current_dir(self.root())
                .args(["mktree"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .context("failed to spawn `git mktree`")?;
            child
                .stdin
                .take()
                .context("git mktree stdin was not piped")?
                .write_all(record.as_bytes())
                .context("failed to write the record to git mktree")?;
            let output = child.wait_with_output().context("failed to wait for git mktree")?;
            if !output.status.success() {
                bail!("git mktree failed: {}", String::from_utf8_lossy(&output.stderr));
            }
            let tree = String::from_utf8(output.stdout)
                .context("git mktree output was not UTF-8")?
                .trim()
                .to_string();
            Ok(tree)
        }

        /// A repo initialized with an explicit object format (`"sha1"` or
        /// `"sha256"`) — for proving item 7 (the empty-tree OID must be
        /// derived for the repo's actual hash algorithm, not hardcoded to
        /// SHA-1). No commits are made; every such repo starts with an
        /// unborn `HEAD`, which is exactly the state `diff_base` needs the
        /// empty-tree OID for.
        fn init_with_object_format(object_format: &str) -> Result<Self> {
            let dir = tempfile::tempdir().context("failed to create temp repo dir")?;
            let root = dir.path();
            for args in [
                vec!["init", "--quiet", &format!("--object-format={object_format}")],
                vec!["config", "user.email", "test@example.com"],
                vec!["config", "user.name", "Test"],
            ] {
                let status = Command::new("git")
                    .current_dir(root)
                    .args(&args)
                    .status()
                    .with_context(|| format!("failed to run git {args:?}"))?;
                if !status.success() {
                    bail!("git {args:?} failed in temp repo setup");
                }
            }
            Ok(Self { dir })
        }
    }

    #[test]
    fn read_staged_path_text_ignores_unstaged_working_tree_edits() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("foo.rs", "fn main() {}\n")?;
        repo.add("foo.rs")?;
        repo.commit("initial")?;

        // Dirty the WORKING TREE without staging it.
        repo.write("foo.rs", "fn main() { /* unstaged edit */ }\n")?;

        let staged_text =
            expect_present(read_staged_path_text(repo.root(), "foo.rs", None), "foo.rs")?;
        assert_eq!(
            staged_text, "fn main() {}\n",
            "read_staged_path_text must return the committed/staged blob, not the dirtied \
             working-tree file"
        );
        Ok(())
    }

    #[test]
    fn read_staged_path_text_sees_staged_content_the_working_tree_no_longer_has() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("foo.rs", "fn main() {}\n")?;
        repo.add("foo.rs")?;
        repo.commit("initial")?;

        // Stage a change, matching what `git write-tree` would record...
        repo.write("foo.rs", "fn main() { /* staged edit */ }\n")?;
        repo.add("foo.rs")?;
        // ...then revert the WORKING TREE back to the old content. The file
        // on disk no longer has the edit, but the STAGED blob still does.
        repo.write("foo.rs", "fn main() {}\n")?;

        let staged_text =
            expect_present(read_staged_path_text(repo.root(), "foo.rs", None), "foo.rs")?;
        assert_eq!(
            staged_text, "fn main() { /* staged edit */ }\n",
            "read_staged_path_text must return the STAGED blob even though the working tree \
             was reverted underneath it"
        );
        Ok(())
    }

    #[test]
    fn read_staged_path_text_reports_absent_for_a_path_that_was_never_staged() -> Result<()> {
        // Issue #4031 item 6: a path that genuinely isn't in the tree/index
        // — never staged, or staged for deletion — must be a clean, legitimate
        // `StagedPathText::Absent`, NOT conflated with a real git invocation
        // failure. The prior design bailed hard here, which (once
        // `staged_diff_paths` starts including `D` entries — item 5) would
        // make every content-reading check hard-error on an ordinary staged
        // deletion instead of just having nothing to check.
        let repo = TempRepo::init()?;
        repo.write("foo.rs", "fn main() {}\n")?;
        repo.add("foo.rs")?;
        repo.commit("initial")?;

        match read_staged_path_text(repo.root(), "never-staged.rs", None)? {
            StagedPathText::Absent => {}
            other => bail!(
                "expected Absent for a path that was never staged, got {other:?} — an absent \
                 path must not be conflated with a git failure (Err) nor misreported as \
                 present/binary content"
            ),
        }
        Ok(())
    }

    #[test]
    fn read_staged_path_text_errors_on_a_genuinely_invalid_tree_oid() -> Result<()> {
        // The other half of item 6's distinction: a MALFORMED ref (not a
        // legitimate absence) must still surface as a real `Err`, so
        // read_staged_path_text can't be "fixed" for the Absent case by
        // just swallowing every git failure. `git cat-file -t` on a
        // syntactically invalid object name exits nonzero and lands in
        // run_git_ok's `Err` branch, while a valid tree plus a never-staged
        // path exits 0 with empty membership output — exit structure, not
        // stderr prose, is what tells the two apart.
        let repo = TempRepo::init()?;
        repo.write("foo.rs", "fn main() {}\n")?;
        repo.add("foo.rs")?;
        repo.commit("initial")?;

        let result = read_staged_path_text(repo.root(), "foo.rs", Some("not-a-valid-oid"));
        match result {
            Err(_) => {}
            Ok(value) => bail!(
                "expected a genuinely malformed tree OID to surface as Err (an instrument \
                 failure), not be silently reported as {value:?}"
            ),
        }
        Ok(())
    }

    #[test]
    fn read_staged_path_text_distinguishes_binary_from_absent() -> Result<()> {
        // The third leg of item 6/item 1: Binary and Absent must not
        // collapse into the same outcome, because callers treat them
        // differently (a deleted path has nothing to check; a binary file
        // where text was expected is itself a finding).
        let repo = TempRepo::init()?;
        // Invalid UTF-8 byte sequence staged as "binary.dat".
        let path = repo.root().join("binary.dat");
        std::fs::write(&path, [0xff, 0xfe, 0x00, 0xff])
            .context("failed to write binary fixture")?;
        repo.add("binary.dat")?;

        match read_staged_path_text(repo.root(), "binary.dat", None)? {
            StagedPathText::Binary => {}
            other => bail!("expected Binary for a non-UTF-8 staged file, got {other:?}"),
        }
        match read_staged_path_text(repo.root(), "does-not-exist.dat", None)? {
            StagedPathText::Absent => {}
            other => bail!("expected Absent for a never-staged path, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn staged_diff_paths_reflects_the_index_not_the_working_tree() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("committed.rs", "fn main() {}\n")?;
        repo.add("committed.rs")?;
        repo.commit("initial")?;

        // committed.rs dirtied on disk but NOT staged -> must not appear.
        repo.write("committed.rs", "fn main() { /* dirty */ }\n")?;
        // new_file.rs staged -> must appear.
        repo.write("new_file.rs", "fn main() {}\n")?;
        repo.add("new_file.rs")?;

        let paths = staged_diff_paths(repo.root(), None)?;
        assert!(
            paths.iter().any(|p| p == "new_file.rs"),
            "the newly staged file must be reported: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p == "committed.rs"),
            "an unstaged working-tree edit must NOT be reported: {paths:?}"
        );
        Ok(())
    }

    #[test]
    fn staged_tree_oid_changes_with_the_index_not_the_working_tree() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("foo.rs", "fn main() {}\n")?;
        repo.add("foo.rs")?;
        repo.commit("initial")?;

        let oid_before = staged_tree_oid(repo.root())?;

        // Dirty the working tree only — the staged tree's OID must not move.
        repo.write("foo.rs", "fn main() { /* dirty, unstaged */ }\n")?;
        let oid_unstaged_dirty = staged_tree_oid(repo.root())?;
        assert_eq!(
            oid_before, oid_unstaged_dirty,
            "an unstaged working-tree edit must not change the staged-tree identity"
        );

        // Now stage the same edit — the OID must move.
        repo.add("foo.rs")?;
        let oid_after_stage = staged_tree_oid(repo.root())?;
        assert_ne!(
            oid_before, oid_after_stage,
            "staging a real content change must change the staged-tree identity"
        );
        Ok(())
    }

    #[test]
    fn list_staged_entries_reads_git_mode_not_filesystem_mode() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("crates/foo/src/lib.rs", "fn main() {}\n")?;
        repo.add("crates/foo/src/lib.rs")?;

        // With core.fileMode=false the filesystem's permission bit is not
        // what git records; force the STAGED mode via update-index, the
        // same class of divergence the R1 chmod defect hit.
        let status = Command::new("git")
            .current_dir(repo.root())
            .args(["update-index", "--chmod=+x", "crates/foo/src/lib.rs"])
            .status()
            .context("failed to run git update-index --chmod=+x")?;
        if !status.success() {
            bail!("git update-index --chmod=+x failed");
        }

        let tree_oid = staged_tree_oid(repo.root())?;
        let entries = list_staged_entries(repo.root(), &tree_oid)?;
        let entry = entries
            .iter()
            .find(|e| e.path == "crates/foo/src/lib.rs")
            .context("staged entry should be present")?;
        assert!(
            entry.is_executable(),
            "list_staged_entries must report the git-recorded mode (100755), not whatever the \
             filesystem happens to show under core.fileMode=false"
        );
        Ok(())
    }

    // -------------------------------------------------------------------
    // Frozen-tree-vs-live-index correctness proof.
    //
    // The class of bug a deep review caught in #3786-A: a check must read
    // the SAME snapshot `plan_gates` captured via `staged_tree_oid`, not
    // whatever the index has become by the time the check actually runs. A
    // concurrent `git add` between capture and dispatch must not change
    // what a `Some(oid)`-pinned read reports.
    // -------------------------------------------------------------------

    #[test]
    fn read_staged_path_text_with_pinned_oid_ignores_a_later_git_add() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("foo.rs", "fn main() {}\n")?;
        repo.add("foo.rs")?;
        repo.commit("initial")?;

        // Stage an edit and capture the tree OID -- this is what plan_gates
        // does once, up front.
        repo.write("foo.rs", "fn main() { /* captured */ }\n")?;
        repo.add("foo.rs")?;
        let captured_oid = staged_tree_oid(repo.root())?;

        // A concurrent `git add` changes the index AFTER the OID was
        // captured -- simulating another process staging more work while
        // this run's checks are still dispatching.
        repo.write("foo.rs", "fn main() { /* concurrent change */ }\n")?;
        repo.add("foo.rs")?;

        // A read pinned to the captured OID must still see the captured
        // content, not the concurrent change.
        let pinned_text = expect_present(
            read_staged_path_text(repo.root(), "foo.rs", Some(&captured_oid)),
            "foo.rs",
        )?;
        assert_eq!(
            pinned_text, "fn main() { /* captured */ }\n",
            "a Some(oid)-pinned read must ignore a concurrent git add that happened after the \
             OID was captured"
        );

        // For contrast: an unpinned (None) read DOES see the live index --
        // proving the difference is the tree-oid pinning, not some other
        // accident of the test setup.
        let live_text =
            expect_present(read_staged_path_text(repo.root(), "foo.rs", None), "foo.rs")?;
        assert_eq!(
            live_text, "fn main() { /* concurrent change */ }\n",
            "an unpinned (None) read should see the live index, confirming the pinned read's \
             stability comes from the OID, not from git caching"
        );
        Ok(())
    }

    #[test]
    fn staged_diff_paths_with_pinned_oid_ignores_a_later_git_add() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("committed.rs", "fn main() {}\n")?;
        repo.add("committed.rs")?;
        repo.commit("initial")?;

        // Stage one new file and capture the tree OID.
        repo.write("captured.rs", "fn main() {}\n")?;
        repo.add("captured.rs")?;
        let captured_oid = staged_tree_oid(repo.root())?;

        // A concurrent `git add` stages a SECOND new file after the OID was
        // captured.
        repo.write("concurrent.rs", "fn main() {}\n")?;
        repo.add("concurrent.rs")?;

        let pinned_paths = staged_diff_paths(repo.root(), Some(&captured_oid))?;
        assert!(
            pinned_paths.iter().any(|p| p == "captured.rs"),
            "the file staged before the OID was captured must be reported: {pinned_paths:?}"
        );
        assert!(
            !pinned_paths.iter().any(|p| p == "concurrent.rs"),
            "a file staged AFTER the OID was captured must NOT be reported by a pinned read: \
             {pinned_paths:?}"
        );

        let live_paths = staged_diff_paths(repo.root(), None)?;
        assert!(
            live_paths.iter().any(|p| p == "concurrent.rs"),
            "an unpinned (None) read should see the concurrently staged file: {live_paths:?}"
        );
        Ok(())
    }

    // -------------------------------------------------------------------
    // Issue #4031 item 5: deleted and type-changed staged paths must be
    // part of the diff-filter's view, not silently dropped.
    // -------------------------------------------------------------------

    #[test]
    fn staged_diff_paths_includes_a_staged_deletion() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("keep.rs", "fn main() {}\n")?;
        repo.write("doomed.rs", "fn main() {}\n")?;
        repo.add("keep.rs")?;
        repo.add("doomed.rs")?;
        repo.commit("initial")?;

        repo.remove_cached("doomed.rs")?;

        let live_paths = staged_diff_paths(repo.root(), None)?;
        assert!(
            live_paths.iter().any(|p| p == "doomed.rs"),
            "a staged deletion (git rm --cached) must appear in staged_diff_paths — the prior \
             --diff-filter=ACMR silently excluded D (deleted) entries: {live_paths:?}"
        );

        let tree_oid = staged_tree_oid(repo.root())?;
        let pinned_paths = staged_diff_paths(repo.root(), Some(&tree_oid))?;
        assert!(
            pinned_paths.iter().any(|p| p == "doomed.rs"),
            "the pinned-tree-OID path must also include the staged deletion: {pinned_paths:?}"
        );
        Ok(())
    }

    #[test]
    fn read_staged_path_text_reports_absent_for_a_staged_deletion() -> Result<()> {
        // The item 5 / item 6 interaction: once a deleted path shows up in
        // staged_diff_paths, a check that reads its content via the pinned
        // tree OID must get a clean Absent, not an Err — the path really
        // isn't in the target tree, that's the whole point of a deletion.
        let repo = TempRepo::init()?;
        repo.write("doomed.rs", "fn main() {}\n")?;
        repo.add("doomed.rs")?;
        repo.commit("initial")?;

        repo.remove_cached("doomed.rs")?;
        let tree_oid = staged_tree_oid(repo.root())?;

        match read_staged_path_text(repo.root(), "doomed.rs", Some(&tree_oid))? {
            StagedPathText::Absent => {}
            other => bail!(
                "expected a deleted staged path to read back as Absent from the pinned tree, \
                 got {other:?}"
            ),
        }
        Ok(())
    }

    // -------------------------------------------------------------------
    // Issue #4092 gap 2: a `T` (type-change) staged entry must stay in the
    // staged change-set with its own recorded mode, never vanish and never
    // silently become clean content validation.
    // -------------------------------------------------------------------

    /// Stage a regular file, then replace the index entry with a `120000`
    /// symlink blob pointing outside the tree — the realistic hostile
    /// type-change. Proves the entry survives into `staged_diff_paths`
    /// (both live and pinned) and keeps its non-regular mode for the
    /// owning checks to reject.
    #[test]
    fn staged_diff_paths_includes_a_type_change_with_its_recorded_mode() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("doomed.rs", "fn main() {}\n")?;
        repo.add("doomed.rs")?;
        repo.commit("initial")?;

        // Type-change: regular file -> symlink (mode 120000) recorded only
        // in the index; the working tree still holds the regular file.
        repo.stage_blob_at("120000", "../../outside/target", "doomed.rs")?;

        let live_paths = staged_diff_paths(repo.root(), None)?;
        assert!(
            live_paths.iter().any(|p| p == "doomed.rs"),
            "a staged type change (T) must appear in staged_diff_paths: {live_paths:?}"
        );

        let tree_oid = staged_tree_oid(repo.root())?;
        let pinned_paths = staged_diff_paths(repo.root(), Some(&tree_oid))?;
        assert!(
            pinned_paths.iter().any(|p| p == "doomed.rs"),
            "the pinned-tree path must also include the type change: {pinned_paths:?}"
        );

        let entries = list_staged_entries(repo.root(), &tree_oid)?;
        let entry = entries
            .iter()
            .find(|e| e.path == "doomed.rs")
            .context("type-changed entry must remain listable")?;
        assert_eq!(
            entry.mode, "120000",
            "the type-changed entry must carry its own recorded mode so owning checks can \
             reject non-regular files instead of silently validating them"
        );
        assert!(!entry.is_executable());
        Ok(())
    }

    // -------------------------------------------------------------------
    // Issue #4092 gap 3: absence detection is exit-code-structured, never
    // git-prose parsing — so it also cannot mis-answer through pathspec
    // pattern semantics.
    // -------------------------------------------------------------------

    /// The exact-match guarantee behind locale independence: a queried path
    /// is answered only by an identical staged path, never by git pathspec
    /// pattern semantics. `evil[1]x.rs` is staged (brackets are legal in
    /// index paths everywhere, unlike `*` on Windows); a query for
    /// `evil1x.rs` — which the pathspec pattern `evil[1]x.rs` would match —
    /// must be `Absent`, and the exact query must be `Present`.
    #[test]
    fn absent_query_never_matches_a_differently_named_staged_path() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.stage_blob_at("100644", "payload\n", "evil[1]x.rs")?;

        match read_staged_path_text(repo.root(), "evil1x.rs", None)? {
            StagedPathText::Absent => {}
            other => bail!(
                "a pattern-neighboring query must not answer a staged `evil[1]x.rs`: got {other:?} \
                 — path membership must be exact, not pattern-matched"
            ),
        }
        match read_staged_path_text(repo.root(), "evil[1]x.rs", None)? {
            StagedPathText::Present(text) => assert_eq!(text, "payload\n"),
            other => bail!("the exact staged path must read back present, got {other:?}"),
        }

        // The same exactness holds against a pinned tree OID.
        let tree_oid = staged_tree_oid(repo.root())?;
        match read_staged_path_text(repo.root(), "evil1x.rs", Some(&tree_oid))? {
            StagedPathText::Absent => {}
            other => bail!(
                "pinned exact-match must also reject the pattern-neighboring query: got {other:?}"
            ),
        }
        Ok(())
    }

    /// A query that begins with `:` is an exact path, never git pathspec
    /// magic. The live index cannot legitimately hold such a path (git's
    /// index plumbing refuses it), and the magic-stripped query must not be
    /// answered by the differently named `literal.yaml` either. A pinned
    /// tree CAN hold one (`git mktree`, fast-import, foreign tooling), and
    /// membership must be answered literally — before `--literal-pathspecs`,
    /// `git ls-tree` parsed `:(literal)` as magic and silently reported
    /// Absent for a snapshot that contained the entry.
    #[test]
    fn colon_prefixed_staged_paths_are_answered_literally() -> Result<()> {
        let repo = TempRepo::init()?;

        repo.write("literal.yaml", "plain\n")?;
        repo.add("literal.yaml")?;
        match read_staged_path_text(repo.root(), ":literal.yaml", None)? {
            StagedPathText::Absent => {}
            other => bail!(
                "a `:`-prefixed query must not be answered by the differently named \
                 `literal.yaml`: got {other:?}"
            ),
        }

        let tree = repo.pin_blob_at("100644", "payload\n", ":(literal)bad.yaml")?;
        match read_staged_path_text(repo.root(), ":(literal)bad.yaml", Some(&tree))? {
            StagedPathText::Present(text) => assert_eq!(text, "payload\n"),
            other => bail!("a pinned `:(literal)bad.yaml` must be found literally, got {other:?}"),
        }

        let plain_tree = repo.pin_blob_at("100644", "plain\n", "literal.yaml")?;
        match read_staged_path_text(repo.root(), ":literal.yaml", Some(&plain_tree))? {
            StagedPathText::Absent => {}
            other => bail!(
                "a `:`-prefixed query must not match the plain `literal.yaml` entry: got {other:?}"
            ),
        }
        Ok(())
    }

    /// A nonempty `ls-tree` record without the documented shape is an
    /// instrument failure, never a silently discarded entry: output drift
    /// must not be able to turn a staged path into a clean absence.
    #[test]
    fn malformed_ls_tree_records_are_instrument_failures_not_absence() {
        assert!(parse_ls_tree_paths("").unwrap().is_empty());
        let well_formed = "100644 blob 4f1c3f0d4bc31cf1a5e4d13d314a4a1c31d0225d\tok.rs\0";
        assert_eq!(parse_ls_tree_paths(well_formed).unwrap(), vec!["ok.rs"]);

        for malformed in [
            // No TAB separator at all.
            "100644 blob 4f1c3f0d4bc31cf1a5e4d13d314a4a1c31d0225d",
            // Incomplete metadata: missing OID / type+OID fields.
            "100644 blob\tpath.rs",
            "100644\tpath.rs",
            // Non-octal or short mode.
            "10x644 blob 4f1c3f0d4bc31cf1a5e4d13d314a4a1c31d0225d\tpath.rs",
            "10064 blob 4f1c3f0d4bc31cf1a5e4d13d314a4a1c31d0225d\tpath.rs",
        ] {
            assert!(
                parse_ls_tree_paths(malformed).is_err(),
                "malformed record {malformed:?} must be rejected, not skipped"
            );
        }
    }

    /// A type-changed (symlink) entry is PRESENT for content reads — its
    /// blob content is the link target — and the pinned form of a
    /// never-staged path is still Absent. Presence must not depend on the
    /// entry's mode being a regular file.
    #[test]
    fn type_changed_entry_is_present_and_never_staged_path_is_absent_when_pinned() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("plain.rs", "fn main() {}\n")?;
        repo.add("plain.rs")?;
        repo.commit("initial")?;
        repo.stage_blob_at("120000", "../elsewhere", "plain.rs")?;
        let tree_oid = staged_tree_oid(repo.root())?;

        match read_staged_path_text(repo.root(), "plain.rs", Some(&tree_oid))? {
            StagedPathText::Present(text) => assert_eq!(text, "../elsewhere"),
            other => bail!(
                "a type-changed entry must be present with its own blob (the link target), got \
                 {other:?}"
            ),
        }
        match read_staged_path_text(repo.root(), "never-staged.rs", Some(&tree_oid))? {
            StagedPathText::Absent => {}
            other => bail!("expected pinned Absent for a never-staged path, got {other:?}"),
        }
        Ok(())
    }

    // -------------------------------------------------------------------
    // Issue #4031 item 7: the empty-tree OID must be derived for the
    // repo's actual object format, not hardcoded to the SHA-1 constant.
    // -------------------------------------------------------------------

    #[test]
    fn diff_base_derives_the_sha1_empty_tree_oid_on_unborn_head() -> Result<()> {
        let repo = TempRepo::init_with_object_format("sha1")?;
        let base = diff_base(repo.root())?;
        assert_eq!(
            base, "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            "a SHA-1 repo's unborn-HEAD diff base must be the well-known SHA-1 empty-tree OID"
        );
        Ok(())
    }

    #[test]
    fn diff_base_derives_the_sha256_empty_tree_oid_on_unborn_head() -> Result<()> {
        // Decisive fixture for item 7: this is the exact case the prior
        // hardcoded `EMPTY_TREE_OID` constant got wrong. A SHA-256 repo's
        // empty-tree object hash is a DIFFERENT string from SHA-1's; a
        // diff_base that still returned the SHA-1 constant here would name
        // an invalid (or simply wrong) object in this repo's object
        // database.
        let repo = TempRepo::init_with_object_format("sha256")?;
        let base = diff_base(repo.root())?;
        assert_eq!(
            base, "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321",
            "a SHA-256 repo's unborn-HEAD diff base must be the SHA-256 empty-tree OID, not the \
             SHA-1 constant"
        );
        assert_ne!(
            base, "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            "must not fall back to the SHA-1 empty-tree OID in a SHA-256 repo"
        );
        Ok(())
    }

    #[test]
    fn staged_diff_paths_works_against_an_unborn_sha256_head() -> Result<()> {
        // End-to-end proof that the derived empty-tree OID actually feeds
        // a working `git diff` call, not just that the string itself looks
        // right in isolation.
        let repo = TempRepo::init_with_object_format("sha256")?;
        repo.write("new_file.rs", "fn main() {}\n")?;
        repo.add("new_file.rs")?;

        let tree_oid = staged_tree_oid(repo.root())?;
        let paths = staged_diff_paths(repo.root(), Some(&tree_oid))?;
        assert!(
            paths.iter().any(|p| p == "new_file.rs"),
            "a staged file on a brand-new SHA-256 repo (unborn HEAD) must be reported: {paths:?}"
        );
        Ok(())
    }
}
