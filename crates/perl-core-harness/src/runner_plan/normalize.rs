//! Discovery-path normalization and target-selector matching.

use crate::model::{ManifestPopulation, TargetScriptForm, TargetSelector};
use crate::runner_model::{InvocationContextClass, RunnerSourceItem, SourceForm, SourcePathClass};

pub(crate) fn normalize_source_item(raw: &str) -> Result<RunnerSourceItem, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("discovery path cannot be empty".to_string());
    }
    if trimmed.contains('\0') {
        return Err("discovery path contains NUL".to_string());
    }
    let mut path = trimmed.replace('\\', "/");
    while let Some(rest) = path.strip_prefix("./") {
        path = rest.to_string();
    }
    if let Some(rest) = path.strip_prefix("../") {
        path = rest.to_string();
    }
    if path.starts_with('/')
        || path.get(1..2) == Some(":")
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(format!("invalid discovery path {trimmed}"));
    }

    let canonical_path = if path.starts_with("t/")
        || path.starts_with("lib/")
        || path.starts_with("dist/")
        || path.starts_with("ext/")
        || path.starts_with("cpan/")
    {
        path
    } else {
        format!("t/{path}")
    };
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
        canonical_path,
        source_form,
        path_class,
        invocation_context,
    })
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
        let local = normalize_source_item("base/if.t")?;
        assert_eq!(local.canonical_path, "t/base/if.t");
        let external = normalize_source_item("../ext/re/t/basic.t")?;
        assert_eq!(external.canonical_path, "ext/re/t/basic.t");
        let root_lib = normalize_source_item("lib/Foo/test.pl")?;
        assert_eq!(root_lib.source_form, SourceForm::TestPl);
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
