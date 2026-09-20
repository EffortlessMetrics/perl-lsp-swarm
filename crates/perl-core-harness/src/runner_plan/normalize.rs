//! Discovery-path normalization and target-selector matching.

use crate::model::{ManifestPopulation, TargetScriptForm, TargetSelector};
use crate::runner_model::{
    DiscoveryFrame, InvocationContextClass, RunnerSourceItem, SourceForm, SourcePathClass,
};

pub(crate) fn normalize_source_item(
    raw: &str,
    discovery_frame: DiscoveryFrame,
) -> Result<RunnerSourceItem, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("discovery path cannot be empty".to_string());
    }
    if trimmed.contains('\0') {
        return Err("discovery path contains NUL".to_string());
    }
    let path = trimmed.replace('\\', "/");
    let canonical_path = resolve_path(&path, discovery_frame)?;
    let source_form = if canonical_path.ends_with("/test.pl") {
        SourceForm::TestPl
    } else if canonical_path.ends_with(".t") {
        SourceForm::DotT
    } else {
        return Err(format!(
            "unsupported discovery source form for {canonical_path}; expected .t or test.pl"
        ));
    };
    let (path_class, invocation_context) = if let Some(local) = canonical_path.strip_prefix("t/") {
        let root = local.split('/').next().unwrap_or_default();
        let context = if matches!(root, "base" | "comp" | "run") {
            InvocationContextClass::BaseCompRun
        } else {
            InvocationContextClass::LocalTestInit
        };
        (SourcePathClass::LocalT, context)
    } else if canonical_path.starts_with("lib/") {
        (SourcePathClass::RootLib, InvocationContextClass::RootLibU1)
    } else if canonical_path.starts_with("dist/") {
        (SourcePathClass::Dist, InvocationContextClass::DistributionU2T)
    } else if canonical_path.starts_with("ext/") {
        (SourcePathClass::Ext, InvocationContextClass::DistributionU2T)
    } else if canonical_path.starts_with("cpan/") {
        (SourcePathClass::Cpan, InvocationContextClass::DistributionU2T)
    } else {
        return Err(format!("unsupported discovery root for {canonical_path}"));
    };

    Ok(RunnerSourceItem {
        raw_path: trimmed.to_string(),
        discovery_frame,
        canonical_path,
        source_form,
        path_class,
        invocation_context,
    })
}

fn resolve_path(raw: &str, frame: DiscoveryFrame) -> Result<String, String> {
    if raw.starts_with('/') || raw.get(1..2) == Some(":") {
        return Err(format!("invalid absolute discovery path {raw}"));
    }
    let mut components = Vec::new();
    match frame {
        DiscoveryFrame::RunnerTDirectoryRelative => components.push("t"),
        DiscoveryFrame::RepositoryRootRelative | DiscoveryFrame::CanonicalRepositoryPath => {}
    }
    for component in raw.split('/') {
        match component {
            "" => return Err(format!("invalid discovery path {raw}")),
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(format!("discovery path escapes repository root: {raw}"));
                }
            }
            value => components.push(value),
        }
    }
    if matches!(frame, DiscoveryFrame::CanonicalRepositoryPath)
        && raw.split('/').any(|component| component == "." || component == "..")
    {
        return Err(format!("canonical discovery path contains unresolved components: {raw}"));
    }
    let path = components.join("/");
    if path.is_empty() {
        return Err(format!("discovery path resolves to repository root: {raw}"));
    }
    Ok(path)
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
    fn normalizes_local_and_root_external_paths() -> Result<(), String> {
        let local = normalize_source_item("base/if.t", DiscoveryFrame::RunnerTDirectoryRelative)?;
        assert_eq!(local.canonical_path, "t/base/if.t");
        let external =
            normalize_source_item("../ext/re/t/basic.t", DiscoveryFrame::RunnerTDirectoryRelative)?;
        assert_eq!(external.canonical_path, "ext/re/t/basic.t");
        let root_lib =
            normalize_source_item("lib/Foo/test.pl", DiscoveryFrame::RepositoryRootRelative)?;
        assert_eq!(root_lib.source_form, SourceForm::TestPl);
        Ok(())
    }

    #[test]
    fn frame_is_load_bearing_and_traversal_is_lexical() {
        let from_t =
            normalize_source_item("lib/Foo/test.pl", DiscoveryFrame::RunnerTDirectoryRelative)
                .unwrap();
        let from_root =
            normalize_source_item("lib/Foo/test.pl", DiscoveryFrame::RepositoryRootRelative)
                .unwrap();
        assert_eq!(from_t.canonical_path, "t/lib/Foo/test.pl");
        assert_eq!(from_root.canonical_path, "lib/Foo/test.pl");
        assert_ne!(from_t, from_root);
        assert_eq!(
            normalize_source_item("../lib/Foo/test.pl", DiscoveryFrame::RunnerTDirectoryRelative)
                .unwrap()
                .canonical_path,
            "lib/Foo/test.pl"
        );
        assert!(
            normalize_source_item("../../escape.t", DiscoveryFrame::RunnerTDirectoryRelative)
                .is_err()
        );
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

    #[test]
    fn unsupported_discovery_source_form_is_named_with_expected_forms() {
        let Err(error) =
            normalize_source_item("t/base/readme.txt", DiscoveryFrame::CanonicalRepositoryPath)
        else {
            panic!("non-.t and non-test.pl discovery must be rejected");
        };
        assert_eq!(
            error,
            "unsupported discovery source form for t/base/readme.txt; expected .t or test.pl"
        );

        let Err(error) =
            normalize_source_item("lib/x/notes.md", DiscoveryFrame::CanonicalRepositoryPath)
        else {
            panic!("root-lib markdown discovery must be rejected");
        };
        assert_eq!(
            error,
            "unsupported discovery source form for lib/x/notes.md; expected .t or test.pl"
        );
    }
}
