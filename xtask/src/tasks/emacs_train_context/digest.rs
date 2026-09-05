//! Digest helpers for the exact-tree context engine.
//!
//! Every digest is a plain SHA-256 over exact bytes (hex lowercase) so a
//! packet consumer can recompute it with any tool. The node title
//! fingerprint reproduces the train's own rule: the first 16 uppercase hex
//! characters of the SHA-256 of the exact title text.

use std::path::Path;

use color_eyre::eyre::{Context, Result, bail};

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("reading {} for digest", path.display()))?;
    Ok(sha256_hex(&bytes))
}

/// Recompute a node title fingerprint: first 16 uppercase hex characters of
/// the SHA-256 of the exact title text.
pub fn title_fingerprint(title: &str) -> String {
    sha256_hex(title.as_bytes())[..16].to_ascii_uppercase()
}

/// Deterministic composite digest over a set of named inputs. Inputs are
/// sorted by name, then hashed as `name\0sha256(name-content)\0` pairs over
/// the raw bytes, so the digest is order-invariant with respect to input
/// iteration order while remaining sensitive to every input byte.
pub fn composite_digest(inputs: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = inputs.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut buffer = String::new();
    for (name, digest) in sorted {
        buffer.push_str(name);
        buffer.push('\0');
        buffer.push_str(digest);
        buffer.push('\0');
    }
    sha256_hex(buffer.as_bytes())
}

/// Read the current `HEAD` commit and tree SHAs from local git. This is the
/// only git usage in the engine: offline, read-only, and deterministic for a
/// given checkout. Missing git metadata fails closed instead of emitting an
/// unbound packet.
pub fn git_identity(root: &Path) -> Result<(String, String)> {
    let run = |args: &[&str]| -> Result<String> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .with_context(|| format!("running git {} for tree binding", args.join(" ")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "git {} failed with status {}: {} (cannot bind the context packet to an exact \
                 tree)",
                args.join(" "),
                output.status,
                stderr.trim()
            );
        }
        let text = String::from_utf8(output.stdout)
            .with_context(|| format!("git {} produced non-UTF-8 output", args.join(" ")))?;
        Ok(text.trim().to_owned())
    };
    let commit = run(&["rev-parse", "HEAD"])?;
    let tree = run(&["rev-parse", "HEAD^{tree}"])?;
    Ok((commit, tree))
}
