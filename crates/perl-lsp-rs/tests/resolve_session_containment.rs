//! Architecture containment for the resolve-envelope session authenticator
//! (#8342): the connection boundary is the only place that may construct one.
//!
//! Provider leaves adopt the substrate via the connection-owned instance; a
//! second construction site would mean a second key/identity authority, which
//! the issue's negative controls forbid. This scans the crate source and
//! fails if `SessionResolveAuthenticator::new` appears anywhere except the
//! connection-boundary module.

use std::path::Path;

/// The one sanctioned construction seam.
const CONSTRUCTION_SEAM: &str = "src/runtime/resolve_session.rs";

#[test]
fn session_authenticator_is_constructed_only_at_the_connection_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = crate_root.join("src");
    let mut offenders = Vec::new();
    collect_construction_sites(&src, &src, &mut offenders)?;

    assert!(
        offenders.is_empty(),
        "SessionResolveAuthenticator::new outside {CONSTRUCTION_SEAM}: {offenders:?}"
    );
    Ok(())
}

fn collect_construction_sites(
    root: &Path,
    dir: &Path,
    offenders: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_construction_sites(root, &path, offenders)?;
            continue;
        }
        if path.extension() != Some(std::ffi::OsStr::new("rs")) {
            continue;
        }
        let relative = path.strip_prefix(root).map_err(|err| err.to_string())?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        let source = std::fs::read_to_string(&path)?;
        if source.contains("SessionResolveAuthenticator::new")
            && format!("src/{relative}") != CONSTRUCTION_SEAM
        {
            offenders.push(format!("src/{relative}"));
        }
    }
    Ok(())
}
