//! Network-assisted observation for the #11411 vim-lsp subject refresh.
//!
//! The transport side (git `ls-remote` plus two depth-1 fetches) is gated
//! behind an explicit `--allow-network` flag and is never exercised by
//! tests or CI. Everything downstream of transport — parsing `ls-remote`
//! output, extracting the floor sentence, plugin defaults, maintenance
//! markers, and needle lookups — lives in pure functions covered by the
//! offline discriminating fixtures.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use regex::Regex;

use crate::vim_lsp_subject_refresh::classify::PinnedSubject;
use crate::vim_lsp_subject_refresh::model::{
    FloorObservation, HeadTreeProbe, OBSERVATION_SCHEMA_VERSION, ObservationPacket, ObservedFile,
    PinnedCommitProbe, ProbeStatus, RefsProbe, SurfaceFinding,
};
use crate::vim_lsp_subject_refresh::probe_table::{
    EXPECTED_PLUGIN_DEFAULTS, FILE_DOC, FILE_PLUGIN, FILE_README, LOAD_GUARD_NEEDLE,
    MAINTENANCE_MARKERS, SNIPPET_NOTE_NEEDLE, SURFACE_PROBES,
};

/// Upper bound on any single upstream file read by the observer.
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

/// Parse `git ls-remote` output into (head, master, tags). Pure.
pub fn parse_ls_remote(output: &str) -> (Option<String>, Option<String>, Vec<String>) {
    let mut head = None;
    let mut master = None;
    let mut tags = Vec::new();
    for line in output.lines() {
        let Some((sha, refname)) = line.split_once('\t') else { continue };
        let sha = sha.trim();
        let refname = refname.trim();
        match refname {
            "HEAD" => head = Some(sha.to_string()),
            "refs/heads/master" => master = Some(sha.to_string()),
            other => {
                if let Some(tag) = other.strip_prefix("refs/tags/")
                    && !tag.ends_with("^{}")
                {
                    tags.push(tag.to_string());
                }
            }
        }
    }
    tags.sort();
    (head, master, tags)
}

/// Extract the theoretical floor sentence from `doc/vim-lsp.txt`. Pure.
/// The pinned bytes state it as
/// `Requires NeoVim with version 0.3 or Vim 8.1.1035 or newer.`
/// (possibly line-wrapped between `or` and `Vim`).
pub fn extract_floor(doc: &str) -> FloorObservation {
    // The pattern is static; a compilation failure (impossible for this
    // literal) degrades to `parsed: false`, which classifies fail-closed.
    let regex = Regex::new(r"Requires NeoVim with version (\S+) or\s+Vim (\S+) or newer").ok();
    match regex.as_ref().and_then(|regex| regex.captures(doc)) {
        Some(captures) => FloorObservation {
            parsed: true,
            neovim_minimum: captures.get(1).map(|m| m.as_str().to_string()),
            vim_minimum: captures.get(2).map(|m| m.as_str().to_string()),
        },
        None => FloorObservation { parsed: false, neovim_minimum: None, vim_minimum: None },
    }
}

/// Extract the default expression of `let g:<name> = get(g:, '<name>', <expr>)`
/// with balanced-paren scanning so nested calls survive. Pure.
pub fn extract_global_default(content: &str, name: &str) -> Option<String> {
    // `name` arrives with its `g:` prefix (the manifest spelling).
    let prefix = format!("let {name} =");
    let start = content.find(&prefix)?;
    let rest = &content[start + prefix.len()..];
    let get_keyword = rest.find("get(")?;
    // Index of the '(' that opens the `get(` call.
    let get_open = start + prefix.len() + get_keyword + "get(".len() - 1;
    let bytes = content.as_bytes();
    let mut depth: i32 = 0;
    let mut commas: usize = 0;
    let mut argument_start: Option<usize> = None;
    let mut index = get_open;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let argument_start = argument_start?;
                    return Some(content[argument_start..index].trim().to_string());
                }
            }
            b',' if depth == 1 => {
                commas += 1;
                if commas == 2 {
                    argument_start = Some(index + 1);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// Case-insensitive closed-vocabulary maintenance marker scan. Pure.
pub fn find_maintenance_markers(readme: &str) -> Vec<String> {
    let lowered = readme.to_lowercase();
    MAINTENANCE_MARKERS
        .iter()
        .filter(|marker| lowered.contains(*marker))
        .map(|marker| (*marker).to_string())
        .collect()
}

/// Needle lookup helper. Pure.
pub fn contains_needle(content: &str, needle: &str) -> bool {
    content.contains(needle)
}

/// All pure file-content probes for the observed head tree. Pure; the
/// network observer shells the bytes in and tests shell fixtures in.
pub fn probe_file_contents(
    _commit: &str,
    files: &BTreeMap<String, String>,
) -> (
    Vec<SurfaceFinding>,
    Option<FloorObservation>,
    BTreeMap<String, String>,
    Option<bool>,
    Vec<String>,
    Option<bool>,
) {
    let content = |path: &str| files.get(path).map(String::as_str);

    let mut findings = Vec::new();
    for probe in SURFACE_PROBES {
        let found =
            content(probe.file).map(|text| contains_needle(text, probe.needle)).unwrap_or(false);
        findings.push(SurfaceFinding {
            surface: probe.surface.to_string(),
            file: probe.file.to_string(),
            needle: probe.needle.to_string(),
            found,
        });
    }
    let floor = content(FILE_DOC).map(extract_floor);
    let mut defaults = BTreeMap::new();
    for (name, _) in EXPECTED_PLUGIN_DEFAULTS {
        if let Some(value) =
            content(FILE_PLUGIN).and_then(|text| extract_global_default(text, name))
        {
            defaults.insert((*name).to_string(), value);
        }
    }
    let load_guard = content(FILE_PLUGIN).map(|text| contains_needle(text, LOAD_GUARD_NEEDLE));
    let markers = content(FILE_README).map(find_maintenance_markers).unwrap_or_default();
    let snippet_note = content(FILE_README).map(|text| contains_needle(text, SNIPPET_NOTE_NEEDLE));
    (findings, floor, defaults, load_guard, markers, snippet_note)
}

/// Run the read-only network observation. Requires the explicit
/// `--allow-network` gate; there is no accidental-network path.
pub fn observe(
    repository: &str,
    pinned: &PinnedSubject,
    allow_network: bool,
) -> Result<ObservationPacket> {
    ensure!(
        repository == pinned.repository,
        "observer must target the pinned subject repository {repo}, got {repository}",
        repo = pinned.repository
    );
    if !allow_network {
        bail!(
            "live observation is network-assisted and opt-in: pass --allow-network, or classify a retained packet with --observation <path>"
        );
    }
    let observed_at_utc = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // Probe 1: refs.
    let refs_method = format!("git ls-remote {repository} HEAD refs/heads/master 'refs/tags/*'");
    let refs_probe = match run_git(
        None,
        ["ls-remote", repository, "HEAD", "refs/heads/master", "refs/tags/*"],
    ) {
        Ok(output) => {
            let (head, master, tags) = parse_ls_remote(&output);
            ensure!(head.is_some() && master.is_some(), "ls-remote returned no HEAD/master");
            RefsProbe {
                method: refs_method,
                status: ProbeStatus::Ok,
                head,
                master,
                tags,
                error: None,
            }
        }
        Err(error) => RefsProbe {
            method: refs_method,
            status: ProbeStatus::Failed,
            head: None,
            master: None,
            tags: Vec::new(),
            error: Some(truncate(&format!("{error:#}"), 280)),
        },
    };

    let scratch = tempfile::tempdir().context("creating observation scratch directory")?;
    run_git(Some(scratch.path()), ["init", "--quiet"])?;

    // Probe 2: pinned commit identity.
    let pinned_method = format!("git fetch --depth 1 {repository} {}", pinned.selected_commit);
    let pinned_commit_probe =
        match fetch_commit(scratch.path(), repository, &pinned.selected_commit) {
            Ok(commit) => {
                let resolved_tree = git_output(
                    scratch.path(),
                    ["rev-parse", format!("{commit}^{{tree}}").as_str()],
                )
                .ok();
                let commit_subject =
                    git_output(scratch.path(), ["show", "-s", "--format=%s", &commit]).ok();
                let commit_author_date =
                    git_output(scratch.path(), ["show", "-s", "--format=%aI", &commit]).ok();
                let mut entry_file_blobs = Vec::new();
                for (path, _) in &pinned.entry_files {
                    let blob = git_output(
                        scratch.path(),
                        ["rev-parse", format!("{commit}:{path}").as_str()],
                    )
                    .ok();
                    entry_file_blobs.push(ObservedFile {
                        commit: commit.clone(),
                        path: path.clone(),
                        present: blob.is_some(),
                        git_blob_sha1: blob,
                    });
                }
                PinnedCommitProbe {
                    method: pinned_method,
                    status: ProbeStatus::Ok,
                    requested_commit: pinned.selected_commit.clone(),
                    resolved_commit: Some(commit),
                    resolved_tree,
                    commit_subject: commit_subject.map(|value| truncate(&value, 200)),
                    commit_author_date,
                    entry_file_blobs,
                    error: None,
                }
            }
            Err(error) => PinnedCommitProbe {
                method: pinned_method,
                status: ProbeStatus::Failed,
                requested_commit: pinned.selected_commit.clone(),
                resolved_commit: None,
                resolved_tree: None,
                commit_subject: None,
                commit_author_date: None,
                entry_file_blobs: Vec::new(),
                error: Some(truncate(&format!("{error:#}"), 280)),
            },
        };

    // Probe 3: observed head tree.
    let head_method = format!("git fetch --depth 1 {repository} <observed-master>");
    let head_probe = match refs_probe.master.clone() {
        Some(master) => match fetch_commit(scratch.path(), repository, &master) {
            Ok(commit) => {
                let mut probed_paths: Vec<&str> =
                    SURFACE_PROBES.iter().map(|probe| probe.file).collect();
                for (path, _) in &pinned.entry_files {
                    probed_paths.push(path);
                }
                probed_paths.push(FILE_DOC);
                probed_paths.push(FILE_README);
                probed_paths.sort_unstable();
                probed_paths.dedup();
                let mut files = Vec::new();
                let mut contents: BTreeMap<String, String> = BTreeMap::new();
                for path in probed_paths {
                    match read_file_from(scratch.path(), &commit, path) {
                        Some(text) => {
                            contents.insert(path.to_string(), text);
                            let blob = git_output(
                                scratch.path(),
                                ["rev-parse", format!("{commit}:{path}").as_str()],
                            )
                            .ok();
                            files.push(ObservedFile {
                                commit: commit.clone(),
                                path: path.to_string(),
                                present: true,
                                git_blob_sha1: blob,
                            });
                        }
                        None => files.push(ObservedFile {
                            commit: commit.clone(),
                            path: path.to_string(),
                            present: false,
                            git_blob_sha1: None,
                        }),
                    }
                }
                let (
                    surface_findings,
                    floor,
                    plugin_defaults,
                    load_guard_present,
                    maintenance_markers,
                    snippet_note_present,
                ) = probe_file_contents(&commit, &contents);
                HeadTreeProbe {
                    method: head_method,
                    status: ProbeStatus::Ok,
                    commit: Some(commit),
                    files,
                    floor,
                    plugin_defaults,
                    load_guard_present,
                    maintenance_markers,
                    snippet_note_present,
                    surface_findings,
                    error: None,
                }
            }
            Err(error) => HeadTreeProbe {
                method: head_method,
                status: ProbeStatus::Failed,
                commit: None,
                files: Vec::new(),
                floor: None,
                plugin_defaults: BTreeMap::new(),
                load_guard_present: None,
                maintenance_markers: Vec::new(),
                snippet_note_present: None,
                surface_findings: Vec::new(),
                error: Some(truncate(&format!("{error:#}"), 280)),
            },
        },
        None => HeadTreeProbe {
            method: head_method,
            status: ProbeStatus::Failed,
            commit: None,
            files: Vec::new(),
            floor: None,
            plugin_defaults: BTreeMap::new(),
            load_guard_present: None,
            maintenance_markers: Vec::new(),
            snippet_note_present: None,
            surface_findings: Vec::new(),
            error: Some("refs probe carried no master head".to_string()),
        },
    };

    Ok(ObservationPacket {
        schema_version: OBSERVATION_SCHEMA_VERSION.to_string(),
        observed_at_utc,
        upstream_repository: repository.to_string(),
        refs_probe,
        pinned_commit_probe,
        head_tree_probe: head_probe,
    })
}

fn truncate(value: &str, cap: usize) -> String {
    if value.chars().count() <= cap {
        value.to_string()
    } else {
        let mut truncated: String = value.chars().take(cap).collect();
        truncated.push('…');
        truncated
    }
}

fn run_git(cwd: Option<&Path>, args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<String> {
    let rendered: Vec<String> = args.into_iter().map(|arg| arg.as_ref().to_string()).collect();
    let mut command = std::process::Command::new("git");
    command.args(&rendered);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output =
        command.output().with_context(|| format!("spawning git {}", rendered.join(" ")))?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        rendered.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_output(cwd: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<String> {
    let rendered: Vec<String> = args.into_iter().map(|arg| arg.as_ref().to_string()).collect();
    let mut command = std::process::Command::new("git");
    command.args(&rendered).current_dir(cwd);
    let output =
        command.output().with_context(|| format!("spawning git {}", rendered.join(" ")))?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        rendered.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Fetch one commit shallowly and return the resolved commit id.
fn fetch_commit(cwd: &Path, repository: &str, commit: &str) -> Result<String> {
    run_git(Some(cwd), ["fetch", "--quiet", "--depth", "1", repository, commit])?;
    git_output(cwd, ["rev-parse", "FETCH_HEAD"])
}

/// Read one file out of a fetched commit, bounded to [`MAX_FILE_BYTES`].
fn read_file_from(cwd: &Path, commit: &str, path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["cat-file", "-p", &format!("{commit}:{path}")])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}
