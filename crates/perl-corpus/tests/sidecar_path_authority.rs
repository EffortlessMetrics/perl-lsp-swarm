use perl_corpus::fixture_expectations;
use perl_corpus::sidecar::SidecarValidationContext;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn sidecar_toml() -> &'static str {
    r#"
[concept]
id = "parser.example"
tier = "pr"

[expect]
panic = false
timeout = false
mode = "parse_clean"
"#
}

fn require_error<T>(
    result: anyhow::Result<T>,
    message: &'static str,
) -> Result<anyhow::Error, Box<dyn Error>> {
    result
        .err()
        .ok_or_else(|| std::io::Error::other(message).into())
}

fn write_pair(root: &Path, relative: &str) -> Result<PathBuf, Box<dyn Error>> {
    let sidecar = root.join(format!("{relative}.meta.toml"));
    let fixture = root.join(format!("{relative}.pl"));
    if let Some(parent) = sidecar.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&sidecar, sidecar_toml())?;
    fs::write(&fixture, "1;")?;
    Ok(sidecar)
}

#[test]
fn contained_regular_pair_validates_and_rebinds() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    write_pair(root.path(), "nested/case")?;
    let context = SidecarValidationContext::discover(root.path())?;
    let pair = context.resolve_pair(Path::new("nested/case.meta.toml"))?;
    let identity = pair.identity().clone();
    assert_eq!(identity.fixture_path, Path::new("nested/case.pl"));
    assert_eq!(context.rebind_pair(&identity)?.identity(), &identity);
    Ok(())
}

#[test]
fn traversal_absolute_and_late_nonmember_paths_fail_closed() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    write_pair(root.path(), "first")?;
    let context = SidecarValidationContext::discover(root.path())?;

    assert!(
        context
            .resolve_pair(Path::new("../outside.meta.toml"))
            .is_err()
    );
    let outside = tempfile::tempdir()?;
    let outside_sidecar = write_pair(outside.path(), "outside")?;
    assert!(context.resolve_pair(&outside_sidecar).is_err());

    write_pair(root.path(), "late")?;
    let error = require_error(
        context.resolve_pair(Path::new("late.meta.toml")),
        "late pair is not in discovered population",
    )?;
    assert!(error.to_string().contains("not a member"));
    Ok(())
}

#[test]
fn serialized_identity_is_revalidated_after_rebinding() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    write_pair(root.path(), "case")?;
    let context = SidecarValidationContext::discover(root.path())?;
    let identity = context
        .resolve_pair(Path::new("case.meta.toml"))?
        .identity()
        .clone();
    fs::remove_file(root.path().join("case.pl"))?;
    assert!(context.rebind_pair(&identity).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn fixture_leaf_symlinks_inside_and_outside_root_are_rejected() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    for target_inside in [false, true] {
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        fs::write(root.path().join("case.meta.toml"), sidecar_toml())?;
        let target = if target_inside {
            root.path().join("target.pl")
        } else {
            outside.path().join("target.pl")
        };
        fs::write(&target, "1;")?;
        symlink(&target, root.path().join("case.pl"))?;

        let context = SidecarValidationContext::bind(root.path())?;
        let error = require_error(
            context.resolve_pair(Path::new("case.meta.toml")),
            "fixture symlink must be rejected",
        )?;
        assert!(error.to_string().contains("crosses a symlink"));
        assert!(
            !error
                .to_string()
                .contains(outside.path().to_string_lossy().as_ref())
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn intermediate_symlink_and_socket_fixture_are_rejected() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;

    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    write_pair(outside.path(), "case")?;
    symlink(outside.path(), root.path().join("linked"))?;
    let context = SidecarValidationContext::bind(root.path())?;
    assert!(
        context
            .resolve_pair(Path::new("linked/case.meta.toml"))
            .is_err()
    );

    fs::write(root.path().join("socket.meta.toml"), sidecar_toml())?;
    let _listener = UnixListener::bind(root.path().join("socket.pl"))?;
    let error = require_error(
        context.resolve_pair(Path::new("socket.meta.toml")),
        "socket is not a regular fixture",
    )?;
    assert!(error.to_string().contains("not a regular file"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn excluded_metadata_symlink_does_not_enter_authority_population() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    write_pair(root.path(), "case")?;
    let target = outside.path().join("notes.json");
    fs::write(&target, "{}")?;
    symlink(&target, root.path().join("notes.json"))?;

    let context = SidecarValidationContext::discover(root.path())?;
    assert_eq!(context.sidecars().count(), 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn compatibility_adapter_reports_the_same_symlink_boundary() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    fs::write(root.path().join("case.meta.toml"), sidecar_toml())?;
    let target = outside.path().join("case.pl");
    fs::write(&target, "1;")?;
    symlink(&target, root.path().join("case.pl"))?;

    let context = SidecarValidationContext::bind(root.path())?;
    let canonical = require_error(
        context.resolve_pair(Path::new("case.meta.toml")),
        "canonical pair must reject symlink",
    )?
    .to_string();
    let compatibility = fixture_expectations::validate_sidecar(
        &context,
        Path::new("case.meta.toml"),
        None,
    );
    assert!(
        compatibility
            .errors
            .iter()
            .any(|error| error == &canonical)
    );
    assert!(compatibility.fixture_path.is_none());
    Ok(())
}
