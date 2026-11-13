//! Clean, immutable Git input selection. No source filters or target code runs.

use super::{SourceIdentity, SourceState};
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_BLOB: u64 = 8 * 1024 * 1024;
const MAX_GIT_OUTPUT: u64 = 16 * 1024 * 1024;

pub struct Blob {
    pub id: String,
    pub mode: String,
    pub bytes: Vec<u8>,
}

pub struct Snapshot {
    root: PathBuf,
    commit: String,
    tree: String,
}

pub fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.starts_with("archive/")
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.eq_ignore_ascii_case(".git")
        })
        && path.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
}

struct QueryChild {
    child: Child,
    settled: bool,
}

impl Drop for QueryChild {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn read_capture(file: &mut File) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(MAX_GIT_OUTPUT + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_GIT_OUTPUT, "Git output exceeds ceiling");
    Ok(bytes)
}

/// Only local Git queries use this bound. This is not the native verifier runner.
pub fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    let mut command = xtask::git_environment::isolate(Command::new("git"));
    command
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .args(["--no-replace-objects", "--literal-pathspecs", "-c", "core.fsmonitor=false"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    let mut child =
        QueryChild { child: command.spawn().context("start local Git query")?, settled: false };
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.child.try_wait()? {
            child.settled = true;
            break status;
        }
        if started.elapsed() > Duration::from_secs(15)
            || stdout.metadata()?.len() > MAX_GIT_OUTPUT
            || stderr.metadata()?.len() > MAX_GIT_OUTPUT
        {
            anyhow::bail!("local Git query exceeded deadline/output budget");
        }
        thread::sleep(Duration::from_millis(10));
    };
    // Avoid leaking repository paths/configuration/credentials from diagnostics.
    ensure!(status.success(), "local Git query failed");
    ensure!(stderr.metadata()?.len() <= MAX_GIT_OUTPUT, "Git stderr exceeds ceiling");
    read_capture(&mut stdout)
}

fn text(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(git(root, args)?)?.trim_end_matches(['\n', '\r']).to_owned())
}

fn clean(root: &Path) -> Result<()> {
    ensure!(
        git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty(),
        "dirty or untracked source is not admitted by clean_only"
    );
    Ok(())
}

impl Snapshot {
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let top = PathBuf::from(text(&root, &["rev-parse", "--show-toplevel"])?).canonicalize()?;
        ensure!(top == root, "select the exact repository root");
        clean(&root)?;
        let commit = text(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
        ensure!(
            commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "unsupported Git object identity"
        );
        let tree = text(&root, &["rev-parse", &format!("{commit}^{{tree}}")])?;
        Ok(Self { root, commit, tree })
    }

    pub fn read(&self, path: &str) -> Result<Blob> {
        ensure!(valid_path(path), "noncanonical repository input path");
        let listing = git(&self.root, &["ls-tree", "-z", &self.tree, "--", path])?;
        let listing = std::str::from_utf8(&listing)?;
        let entries: Vec<_> = listing.split('\0').filter(|line| !line.is_empty()).collect();
        ensure!(entries.len() == 1, "missing or ambiguous committed input");
        let (metadata, actual) = entries[0].split_once('\t').context("invalid Git tree entry")?;
        let fields: Vec<_> = metadata.split_whitespace().collect();
        ensure!(
            actual == path
                && fields.len() == 3
                && fields[1] == "blob"
                && matches!(fields[0], "100644" | "100755"),
            "input is not a regular committed file"
        );
        let size: u64 = text(&self.root, &["cat-file", "-s", fields[2]])?.parse()?;
        ensure!(size <= MAX_BLOB, "input exceeds per-blob ceiling");
        let bytes = git(&self.root, &["cat-file", "blob", fields[2]])?;
        ensure!(bytes.len() as u64 == size, "incomplete committed blob");
        Ok(Blob { id: fields[2].to_owned(), mode: fields[0].to_owned(), bytes })
    }

    pub fn identity(&self) -> SourceIdentity {
        SourceIdentity {
            commit: self.commit.clone(),
            tree: self.tree.clone(),
            state: SourceState::Clean,
            evaluated_content_digest: None,
        }
    }

    pub fn finish(&self) -> Result<()> {
        clean(&self.root)?;
        ensure!(
            text(&self.root, &["rev-parse", "--verify", "HEAD^{commit}"])? == self.commit,
            "HEAD changed during closure evaluation"
        );
        Ok(())
    }
}
