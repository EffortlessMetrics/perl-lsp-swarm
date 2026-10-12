// Shared build/runtime source evidence. The inventory is the release identity
// protocol's `git ls-tree -r -z --full-tree` format; a private index overlays
// tracked edits and non-ignored additions without touching the user's index.
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn source_identity(root: &Path) -> Result<(String, String), Box<dyn std::error::Error>> {
    for selector in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        if std::env::var_os(selector).is_some() {
            return Err(format!("source identity refuses inherited {selector}").into());
        }
    }
    let scratch = tempfile::tempdir()?;
    let index = scratch.path().join("index");
    let git = |args: &[&str]| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let output = Command::new("git")
            .current_dir(root)
            .env("GIT_INDEX_FILE", &index)
            .args(args)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "source identity git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(output.stdout)
    };
    let top = String::from_utf8(git(&["rev-parse", "--show-toplevel"])?)?;
    if Path::new(top.trim_end_matches('\n')).canonicalize()? != root.canonicalize()? {
        return Err("source identity Git checkout differs from requested root".into());
    }
    let revision = String::from_utf8(git(&["rev-parse", "HEAD"])?)?.trim().to_owned();
    git(&["read-tree", "HEAD"])?;
    let ignored = git(&[
        "ls-files",
        "--others",
        "--ignored",
        "--exclude-standard",
        "-z",
        "--",
        "crates",
        ".cargo",
    ])?;
    for path in ignored.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
        let path = std::str::from_utf8(path)?;
        if !path.split('/').any(|part| part == "target")
            && (path.ends_with(".rs") || path.ends_with(".toml"))
        {
            return Err(
                format!("ignored source/build configuration is not provable: {path}").into()
            );
        }
    }
    git(&["add", "-A", "--", "."])?;
    // Git's text/clean filters are appropriate for a committed release tree,
    // but local compiler inputs must retain their exact bytes.
    let staged = git(&["ls-files", "--stage", "-z"])?;
    let mut files = Vec::new();
    for entry in staged.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
        let text = std::str::from_utf8(entry)?;
        let (header, path) = text.split_once('\t').ok_or("invalid source index entry")?;
        let mode = header.split(' ').next().ok_or("missing source mode")?;
        if mode == "160000" {
            return Err("submodule source identity is unsupported".into());
        }
        // --stdin-paths interprets a leading quote as Git C quoting. Refuse
        // that unsupported spelling rather than hashing a different file.
        if path.starts_with('"') {
            return Err("leading-quote source paths are unsupported".into());
        }
        if path.contains(['\n', '\r']) {
            return Err("newline source paths are unsupported".into());
        }
        if mode == "120000" {
            let target = root.join(path).canonicalize()?;
            let canonical_root = root.canonicalize()?;
            let relative = target
                .strip_prefix(&canonical_root)
                .map_err(|_| format!("external symlink source is not provable: {path}"))?;
            let relative = relative.to_str().ok_or("non-UTF8 symlink target")?;
            if git(&["ls-files", "-z", "--", relative])?.is_empty() {
                return Err(format!("uninventoried symlink target is not provable: {path}").into());
            }
            let ignored = git(&[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
                "--",
                relative,
            ])?;
            for target_path in ignored.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
                let target_path = std::str::from_utf8(target_path)?;
                if target_path.ends_with(".rs") || target_path.ends_with(".toml") {
                    return Err(
                        format!("ignored symlink source is not provable: {target_path}").into()
                    );
                }
            }
        }
        if mode != "120000" {
            files.push((mode, path));
        }
    }
    let run_input = |args: &[&str], input: &[u8]| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut input_file = tempfile::tempfile()?;
        input_file.write_all(input)?;
        use std::io::{Seek, SeekFrom};
        input_file.seek(SeekFrom::Start(0))?;
        let output = Command::new("git")
            .current_dir(root)
            .env("GIT_INDEX_FILE", &index)
            .args(args)
            .stdin(Stdio::from(input_file))
            .output()?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
        }
        Ok(output.stdout)
    };
    let paths = files.iter().map(|(_, path)| format!("{path}\n")).collect::<String>();
    let hashes = String::from_utf8(run_input(
        &["hash-object", "-w", "--no-filters", "--stdin-paths"],
        paths.as_bytes(),
    )?)?;
    if hashes.lines().count() != files.len() {
        return Err("incomplete raw source inventory".into());
    }
    let mut updates = Vec::new();
    for ((mode, path), hash) in files.iter().zip(hashes.lines()) {
        updates.extend_from_slice(format!("{mode} {hash}\t{path}\0").as_bytes());
    }
    run_input(&["update-index", "-z", "--index-info"], &updates)?;
    let tree = String::from_utf8(git(&["write-tree"])?)?;
    let inventory = git(&["ls-tree", "-r", "-z", "--full-tree", tree.trim()])?;
    Ok((
        revision,
        Sha256::digest(inventory).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
    ))
}
