use std::path::{Component, Path, PathBuf};

use super::FixtureError;

/// Join a crate-local relative path onto `package_root`.
///
/// Rejects absolute paths and any `..` / prefix component so a fixture cannot
/// walk out of the package tree by normalization. `CurDir` (`.`) is ignored.
pub fn resolve_crate_relative(
    package_root: &Path,
    relative: &Path,
) -> Result<PathBuf, FixtureError> {
    if relative.is_absolute() {
        return Err(FixtureError::AbsolutePath(relative.display().to_string()));
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(FixtureError::PathEscape(relative.display().to_string())),
        }
    }
    Ok(package_root.join(relative))
}

/// File sources must live under `tests/fixtures/` inside the package.
pub fn require_under_fixtures(relative: &Path) -> Result<(), FixtureError> {
    let mut components =
        relative.components().filter(|component| !matches!(component, Component::CurDir));
    let tests = components.next();
    let fixtures = components.next();
    match (tests, fixtures) {
        (Some(Component::Normal(tests)), Some(Component::Normal(fixtures)))
            if tests == "tests" && fixtures == "fixtures" =>
        {
            Ok(())
        }
        _ => Err(FixtureError::SourceNotUnderFixtures(relative.display().to_string())),
    }
}
