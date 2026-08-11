mod authority;
mod classify;
mod model;
mod path;

#[cfg(test)]
mod authorization_tests;
#[cfg(test)]
mod schema_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

use authority::load_authority;
use clap::Parser;
use classify::classify;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use model::{Observation, Receipt, Verdict};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Parser)]
#[command(about = "Classify an exact-SHA publication drift observation")]
struct Args {
    /// Comparison observation JSON.
    #[arg(long)]
    input: PathBuf,

    /// Repository root used to resolve the authority manifest's repository-relative path.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,

    /// Receipt JSON written even when the verdict blocks promotion.
    #[arg(long, default_value = "target/receipts/publication-drift.json")]
    out: PathBuf,
}

pub fn run_from_env() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    run_with_paths(args.input, args.repo_root, args.out)
}

pub fn run_with_paths(input: PathBuf, repo_root: PathBuf, out: PathBuf) -> Result<()> {
    let observation = load_observation(&input)?;
    let authority_path = observation
        .manifest
        .as_ref()
        .map(|manifest| repo_root.join(&manifest.path));
    prepare_output_parent(&out)?;
    ensure_safe_output(&out, &input, authority_path.as_deref())?;

    let authority = load_authority(&repo_root, observation.manifest.as_ref());
    let receipt = classify(observation, authority);
    write_receipt(&out, &receipt)?;

    match receipt.verdict {
        Verdict::Clean => {
            println!(
                "publication-drift: clean comparison {} -> {} at version {}",
                receipt.swarm.sha,
                receipt.public.sha,
                receipt.comparison_version.as_deref().unwrap_or("not-proven")
            );
            Ok(())
        }
        Verdict::Drift => bail!("publication-drift: product drift detected; see {}", out.display()),
        Verdict::NotProven => {
            bail!("publication-drift: comparison not proven; see {}", out.display())
        }
    }
}

fn load_observation(path: &Path) -> Result<Observation> {
    let raw = fs::read_to_string(path)
        .wrap_err_with(|| format!("reading publication drift observation {}", path.display()))?;
    serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing publication drift observation {}", path.display()))
}

fn prepare_output_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .wrap_err_with(|| format!("creating publication drift output {}", parent.display()))
}

fn ensure_safe_output(out: &Path, input: &Path, authority: Option<&Path>) -> Result<()> {
    let protected = [Some(input), authority];
    let output_identity = resolved_candidate_path(out)?;

    for source in protected.into_iter().flatten() {
        let source_identity = resolved_candidate_path(source)?;
        if output_identity == source_identity {
            bail!(
                "publication drift output {} aliases protected evidence source {}",
                out.display(),
                source.display()
            );
        }
    }

    match fs::symlink_metadata(out) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                bail!(
                    "publication drift output {} must be a regular file and must not be a symlink",
                    out.display()
                );
            }
            for source in protected.into_iter().flatten() {
                if same_file_identity(&metadata, source)? {
                    bail!(
                        "publication drift output {} is a hard-link alias of protected evidence source {}",
                        out.display(),
                        source.display()
                    );
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .wrap_err_with(|| format!("inspecting publication drift output {}", out.display()));
        }
    }
    Ok(())
}

fn resolved_candidate_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(eyre!("publication drift path must not be empty"));
    }
    if path.exists() {
        return fs::canonicalize(path)
            .wrap_err_with(|| format!("canonicalizing publication drift path {}", path.display()));
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| eyre!("publication drift path has no file name: {}", path.display()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let canonical_parent = fs::canonicalize(parent)
        .wrap_err_with(|| format!("canonicalizing publication drift parent {}", parent.display()))?;
    normalize_lexically(&canonical_parent.join(file_name))
}

fn normalize_lexically(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!("publication drift path escapes its filesystem root: {}", path.display());
                }
            }
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    Ok(normalized)
}

#[cfg(unix)]
fn same_file_identity(output: &fs::Metadata, source: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let source = match fs::metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .wrap_err_with(|| format!("reading protected evidence metadata {}", source.display()));
        }
    };
    Ok(output.dev() == source.dev() && output.ino() == source.ino())
}

#[cfg(not(unix))]
fn same_file_identity(_output: &fs::Metadata, _source: &Path) -> Result<bool> {
    // Canonical path equality above covers direct paths and symlink aliases on every platform.
    // Standard library metadata exposes stable hard-link identities only on Unix.
    Ok(false)
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let raw = serde_json::to_string_pretty(receipt).wrap_err("serializing drift receipt")?;
    let mut temporary = NamedTempFile::new_in(parent)
        .wrap_err_with(|| format!("creating atomic publication drift receipt in {}", parent.display()))?;
    temporary
        .write_all(format!("{raw}\n").as_bytes())
        .wrap_err_with(|| format!("writing temporary publication drift receipt for {}", path.display()))?;
    temporary
        .as_file_mut()
        .sync_all()
        .wrap_err_with(|| format!("syncing temporary publication drift receipt for {}", path.display()))?;
    temporary.persist(path).map_err(|error| {
        eyre!(
            "atomically persisting publication drift receipt {}: {}",
            path.display(),
            error.error
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod output_tests {
    use super::ensure_safe_output;
    use color_eyre::eyre::{Result, bail};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn output_cannot_equal_the_observation() -> Result<()> {
        let temp = TempDir::new()?;
        let input = temp.path().join("observation.json");
        fs::write(&input, "{}")?;
        expect_rejection(&input, &input, None, "aliases protected evidence source")
    }

    #[cfg(unix)]
    #[test]
    fn output_symlink_cannot_alias_the_observation() -> Result<()> {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new()?;
        let input = temp.path().join("observation.json");
        let out = temp.path().join("receipt.json");
        fs::write(&input, "{}")?;
        symlink(&input, &out)?;
        expect_rejection(&out, &input, None, "aliases protected evidence source")
    }

    #[cfg(unix)]
    #[test]
    fn output_hard_link_cannot_alias_the_authority() -> Result<()> {
        let temp = TempDir::new()?;
        let input = temp.path().join("observation.json");
        let authority = temp.path().join("authority.json");
        let out = temp.path().join("receipt.json");
        fs::write(&input, "{}")?;
        fs::write(&authority, "{}")?;
        fs::hard_link(&authority, &out)?;
        expect_rejection(&out, &input, Some(&authority), "hard-link alias")
    }

    fn expect_rejection(
        out: &std::path::Path,
        input: &std::path::Path,
        authority: Option<&std::path::Path>,
        expected: &str,
    ) -> Result<()> {
        let error = match ensure_safe_output(out, input, authority) {
            Ok(()) => bail!("unsafe output alias should be rejected"),
            Err(error) => error,
        };
        if !format!("{error:#}").contains(expected) {
            bail!("unexpected output safety error: {error:#}");
        }
        Ok(())
    }
}
