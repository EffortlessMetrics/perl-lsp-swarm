//! Discovery-frame source-identity normalization and target-selector matching.
//!
//! The resolution law is purely lexical against a logical base; it never uses
//! checkout state, symlink resolution, file existence, current cwd, or ambient
//! path spelling as semantic authority:
//!
//! ```text
//! RunnerTDirectoryRelative base = <repo>/t
//! RepositoryRootRelative base   = <repo>
//! CanonicalRepositoryPath       = already repo-relative, independently admitted
//! ```
//!
//! `.` and `..` components are normalized without filesystem access, escapes
//! above the logical repository root fail closed, and no raw-string-only API
//! may construct a current authoritative runner-plan source row: this module's
//! [`normalize_source_item`] requires the [`DiscoveryFrame`].

use crate::model::{ManifestPopulation, TargetScriptForm, TargetSelector};
use crate::runner_model::{
    DiscoveryFrame, InvocationContextClass, NormalizeError, RunnerSourceIdentityV2,
    SOURCE_NORMALIZATION_SCHEMA_VERSION, SourceForm, SourcePathClass,
};

pub(crate) fn normalize_source_item(
    raw: &str,
    frame: DiscoveryFrame,
) -> Result<RunnerSourceIdentityV2, NormalizeError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(NormalizeError::Empty);
    }
    if trimmed.contains('\0') {
        return Err(NormalizeError::ContainsNul);
    }
    if trimmed.contains('\\') {
        return Err(NormalizeError::BackslashSeparator { raw: trimmed.to_string() });
    }
    let components = resolve_components(trimmed, frame)?;
    let canonical_repo_path = components.join("/");
    let source_form = if canonical_repo_path.ends_with("/test.pl") {
        SourceForm::TestPl
    } else if canonical_repo_path.ends_with(".t") {
        SourceForm::DotT
    } else {
        return Err(NormalizeError::UnsupportedForm {
            canonical_repo_path: canonical_repo_path.clone(),
        });
    };
    let (path_class, invocation_context) = classify_namespace(&canonical_repo_path)?;

    Ok(RunnerSourceIdentityV2 {
        raw_path: trimmed.to_string(),
        discovery_frame: frame,
        canonical_repo_path,
        source_form,
        path_class,
        invocation_context,
        normalization_version: SOURCE_NORMALIZATION_SCHEMA_VERSION.to_string(),
    })
}

/// Lexically resolve one raw spelling against its declared logical base and
/// return the repository-relative component stack.
fn resolve_components(raw: &str, frame: DiscoveryFrame) -> Result<Vec<String>, NormalizeError> {
    if raw.starts_with('/') {
        return Err(NormalizeError::AbsolutePath { raw: raw.to_string() });
    }
    let mut stack = match frame {
        DiscoveryFrame::RunnerTDirectoryRelative => vec!["t".to_string()],
        DiscoveryFrame::RepositoryRootRelative | DiscoveryFrame::CanonicalRepositoryPath => {
            Vec::new()
        }
    };
    for component in raw.split('/') {
        match component {
            "" => {
                return Err(NormalizeError::InvalidComponent {
                    raw: raw.to_string(),
                    component: component.to_string(),
                });
            }
            "." | ".." => {
                // A canonical spelling is admitted as-is; `.` and `..` mean it
                // was not canonical after all.
                if matches!(frame, DiscoveryFrame::CanonicalRepositoryPath) {
                    return Err(NormalizeError::InvalidComponent {
                        raw: raw.to_string(),
                        component: component.to_string(),
                    });
                }
                if component == "." {
                    continue;
                }
                if stack.pop().is_none() {
                    return Err(NormalizeError::EscapeAboveRoot { raw: raw.to_string() });
                }
            }
            other => {
                if other.contains(':')
                    || other.chars().any(char::is_control)
                    || other.starts_with(' ')
                    || other.ends_with(' ')
                {
                    return Err(NormalizeError::InvalidComponent {
                        raw: raw.to_string(),
                        component: other.to_string(),
                    });
                }
                stack.push(other.to_string());
            }
        }
    }
    if stack.is_empty() {
        return Err(NormalizeError::ResolvesToEmpty { raw: raw.to_string() });
    }
    Ok(stack)
}

fn classify_namespace(
    canonical_repo_path: &str,
) -> Result<(SourcePathClass, InvocationContextClass), NormalizeError> {
    if let Some(local) = canonical_repo_path.strip_prefix("t/") {
        let root = local.split('/').next().unwrap_or_default();
        let context = if matches!(root, "base" | "comp" | "run") {
            InvocationContextClass::BaseCompRun
        } else {
            InvocationContextClass::LocalTestInit
        };
        Ok((SourcePathClass::LocalT, context))
    } else if canonical_repo_path.starts_with("lib/") {
        Ok((SourcePathClass::RootLib, InvocationContextClass::RootLibU1))
    } else if canonical_repo_path.starts_with("dist/") {
        Ok((SourcePathClass::Dist, InvocationContextClass::DistributionU2T))
    } else if canonical_repo_path.starts_with("ext/") {
        Ok((SourcePathClass::Ext, InvocationContextClass::DistributionU2T))
    } else if canonical_repo_path.starts_with("cpan/") {
        Ok((SourcePathClass::Cpan, InvocationContextClass::DistributionU2T))
    } else {
        Err(NormalizeError::UnsupportedNamespace {
            canonical_repo_path: canonical_repo_path.to_string(),
        })
    }
}

impl std::fmt::Display for NormalizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "discovery path cannot be empty"),
            Self::ContainsNul => write!(f, "discovery path contains NUL"),
            Self::BackslashSeparator { raw } => write!(
                f,
                "discovery path {raw} contains a backslash separator; discovery frames accept \
                 only '/'-separated spellings"
            ),
            Self::AbsolutePath { raw } => {
                write!(f, "absolute discovery path {raw} is not admitted under any frame")
            }
            Self::InvalidComponent { raw, component } => write!(
                f,
                "invalid discovery path component {component:?} in {raw}; empty components, \
                 drive-like or control spellings, and non-canonical '.'/'..' under the \
                 canonical-repository-path frame are rejected"
            ),
            Self::EscapeAboveRoot { raw } => {
                write!(f, "discovery path {raw} escapes above its declared logical repository root")
            }
            Self::ResolvesToEmpty { raw } => {
                write!(f, "discovery path {raw} resolves to an empty repository path")
            }
            Self::UnsupportedForm { canonical_repo_path } => write!(
                f,
                "unsupported discovery source form for {canonical_repo_path}; expected .t or \
                 test.pl"
            ),
            Self::UnsupportedNamespace { canonical_repo_path } => {
                write!(f, "unsupported discovery root for {canonical_repo_path}")
            }
        }
    }
}

pub(crate) fn source_form_allowed(source_form: SourceForm, allowed: &[TargetScriptForm]) -> bool {
    allowed.iter().any(|form| {
        matches!(
            (source_form, form),
            (SourceForm::DotT, TargetScriptForm::DotT)
                | (SourceForm::TestPl, TargetScriptForm::TestPl)
                | (SourceForm::DotT, TargetScriptForm::GeneratedPerl)
                | (SourceForm::TestPl, TargetScriptForm::GeneratedPerl)
        )
    })
}

pub(crate) fn matches_any_selector(path: &str, selectors: &[TargetSelector]) -> bool {
    selectors.iter().any(|selector| matches_selector(path, selector))
}

fn matches_selector(path: &str, selector: &TargetSelector) -> bool {
    match selector {
        TargetSelector::RecursiveRoot { path: root } => {
            let prefix = format!("t/{root}/");
            path.starts_with(&prefix)
        }
        TargetSelector::NonRecursiveGlob { pattern } => {
            path.strip_prefix("t/").is_some_and(|relative| glob_matches(pattern, relative))
        }
        TargetSelector::ExactFile { path: exact } => path == format!("t/{exact}"),
        TargetSelector::ExternalGlob { pattern } => {
            pattern.strip_prefix("../").is_some_and(|relative| glob_matches(relative, path))
        }
        TargetSelector::ManifestPopulation { component } => match component {
            ManifestPopulation::RootLib => path.starts_with("lib/"),
            ManifestPopulation::CoreRootLib => path
                .strip_prefix("lib/")
                .and_then(|rest| rest.as_bytes().first().copied())
                .is_some_and(|byte| byte.is_ascii_lowercase()),
            ManifestPopulation::Dist => path.starts_with("dist/"),
            ManifestPopulation::Ext => path.starts_with("ext/"),
            ManifestPopulation::Cpan => path.starts_with("cpan/"),
        },
    }
}

fn glob_matches(pattern: &str, candidate: &str) -> bool {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return pattern == candidate;
    };
    if suffix.contains('*') || !candidate.starts_with(prefix) || !candidate.ends_with(suffix) {
        return false;
    }
    let wildcard_start = prefix.len();
    let wildcard_end = candidate.len().saturating_sub(suffix.len());
    wildcard_start <= wildcard_end && !candidate[wildcard_start..wildcard_end].contains('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_discriminators_resolve_distinct_canonical_paths() -> Result<(), String> {
        let t_relative =
            normalize_source_item("lib/Foo/test.pl", DiscoveryFrame::RunnerTDirectoryRelative)
                .map_err(|error| error.to_string())?;
        assert_eq!(t_relative.canonical_repo_path, "t/lib/Foo/test.pl");

        let t_parent =
            normalize_source_item("../lib/Foo/test.pl", DiscoveryFrame::RunnerTDirectoryRelative)
                .map_err(|error| error.to_string())?;
        assert_eq!(t_parent.canonical_repo_path, "lib/Foo/test.pl");
        assert_eq!(t_parent.raw_path, "../lib/Foo/test.pl");

        let repo_root =
            normalize_source_item("lib/Foo/test.pl", DiscoveryFrame::RepositoryRootRelative)
                .map_err(|error| error.to_string())?;
        assert_eq!(repo_root.canonical_repo_path, "lib/Foo/test.pl");

        assert_ne!(t_relative.canonical_repo_path, t_parent.canonical_repo_path);
        assert_eq!(t_parent.canonical_repo_path, repo_root.canonical_repo_path);
        assert_ne!(
            t_relative.path_class, repo_root.path_class,
            "t/lib/** must classify LocalT while root lib/** classifies RootLib"
        );
        Ok(())
    }

    #[test]
    fn local_t_member_normalizes_under_runner_t_frame() -> Result<(), String> {
        let local = normalize_source_item("base/if.t", DiscoveryFrame::RunnerTDirectoryRelative)
            .map_err(|error| error.to_string())?;
        assert_eq!(local.canonical_repo_path, "t/base/if.t");
        assert_eq!(local.invocation_context, InvocationContextClass::BaseCompRun);

        let external =
            normalize_source_item("../ext/re/t/basic.t", DiscoveryFrame::RunnerTDirectoryRelative)
                .map_err(|error| error.to_string())?;
        assert_eq!(external.canonical_repo_path, "ext/re/t/basic.t");
        Ok(())
    }

    #[test]
    fn escape_above_the_logical_repository_root_fails_closed() {
        for (raw, frame) in [
            ("../../escape.t", DiscoveryFrame::RunnerTDirectoryRelative),
            ("../escape.t", DiscoveryFrame::RepositoryRootRelative),
        ] {
            let rejected = matches!(
                normalize_source_item(raw, frame),
                Err(NormalizeError::EscapeAboveRoot { .. })
            );
            assert!(rejected, "escape above the logical root must be rejected: {raw}");
        }
    }

    #[test]
    fn canonical_frame_rejects_non_canonical_spellings() -> Result<(), String> {
        for raw in ["../lib/x.t", "/lib/x.t", "./lib/x.t", "C:/lib/x.t"] {
            let error = match normalize_source_item(raw, DiscoveryFrame::CanonicalRepositoryPath) {
                Ok(item) => {
                    return Err(format!(
                        "canonical frame must reject non-canonical spelling {raw}: {item:?}"
                    ));
                }
                Err(error) => error,
            };
            assert!(
                matches!(
                    error,
                    NormalizeError::InvalidComponent { .. } | NormalizeError::AbsolutePath { .. }
                ),
                "unexpected error for {raw}: {error}"
            );
        }
        Ok(())
    }

    #[test]
    fn windows_drive_and_absolute_spellings_fail_closed() {
        for raw in ["t\\base\\if.t", "C:/t/base/if.t", "/t/base/if.t"] {
            for frame in [
                DiscoveryFrame::RunnerTDirectoryRelative,
                DiscoveryFrame::RepositoryRootRelative,
                DiscoveryFrame::CanonicalRepositoryPath,
            ] {
                assert!(
                    normalize_source_item(raw, frame).is_err(),
                    "{raw:?} under frame {frame:?} must be rejected"
                );
            }
        }
    }

    #[test]
    fn unsupported_frames_and_forms_are_named() -> Result<(), String> {
        assert!(DiscoveryFrame::parse("teleport").is_err());

        let form_error = match normalize_source_item(
            "base/readme.txt",
            DiscoveryFrame::RunnerTDirectoryRelative,
        ) {
            Ok(item) => return Err(format!("expected form rejection, got {item:?}")),
            Err(error) => error.to_string(),
        };
        assert_eq!(
            form_error,
            "unsupported discovery source form for t/base/readme.txt; expected .t or test.pl"
        );

        let namespace_error =
            match normalize_source_item("docs/notes.t", DiscoveryFrame::RepositoryRootRelative) {
                Ok(item) => return Err(format!("expected namespace rejection, got {item:?}")),
                Err(error) => error.to_string(),
            };
        assert_eq!(namespace_error, "unsupported discovery root for docs/notes.t");
        Ok(())
    }

    #[test]
    fn alias_collapses_to_one_canonical_member_for_explicit_duplicate_detection()
    -> Result<(), String> {
        let first = normalize_source_item("lib/x/basic.t", DiscoveryFrame::RepositoryRootRelative)
            .map_err(|error| error.to_string())?;
        let alias =
            normalize_source_item("./lib/x/basic.t", DiscoveryFrame::RepositoryRootRelative)
                .map_err(|error| error.to_string())?;
        assert_eq!(
            first.canonical_repo_path, alias.canonical_repo_path,
            "aliases must collapse so duplicate detection can name the conflict"
        );
        assert_ne!(first.raw_path, alias.raw_path);
        Ok(())
    }

    #[test]
    fn non_recursive_glob_does_not_absorb_nested_members() {
        let selector = TargetSelector::NonRecursiveGlob { pattern: "op/*.t".to_string() };
        assert!(matches_selector("t/op/basic.t", &selector));
        assert!(!matches_selector("t/op/hook/hook.t", &selector));
    }

    #[test]
    fn core_root_lib_requires_lowercase_first_component() {
        let selector =
            TargetSelector::ManifestPopulation { component: ManifestPopulation::CoreRootLib };
        assert!(matches_selector("lib/attributes.t", &selector));
        assert!(!matches_selector("lib/Config/Perl/V.t", &selector));
    }
}
