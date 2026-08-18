#[path = "support/emacs_projects.rs"]
mod emacs_projects;

use emacs_projects::{
    bind_stock_project_probe, stock_project_probe_driver, EmacsProjectCaseKind,
    EmacsProjectFixtureMatrix, REQUIRED_CASES,
};
use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::process::Command;

#[test]
fn project_matrix_covers_every_required_layout_once() -> Result<(), Box<dyn Error>> {
    let matrix = EmacsProjectFixtureMatrix::new()?;
    let kinds = matrix.fixtures().iter().map(|fixture| fixture.kind).collect::<BTreeSet<_>>();
    let required = REQUIRED_CASES.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(kinds, required);
    assert_eq!(matrix.fixtures().len(), REQUIRED_CASES.len());
    for fixture in matrix.fixtures() {
        assert!(fixture.open_file.is_file());
        assert!(fixture.open_file.starts_with(matrix.root()));
        if let Some(intended_root) = &fixture.intended_root {
            assert!(intended_root.starts_with(matrix.root()));
        }
    }
    Ok(())
}

#[test]
fn non_vcs_cpan_layouts_do_not_receive_a_fake_git_project() -> Result<(), Box<dyn Error>> {
    let matrix = EmacsProjectFixtureMatrix::new()?;
    for kind in [
        EmacsProjectCaseKind::MakefilePlNoVcs,
        EmacsProjectCaseKind::BuildPlNoVcs,
        EmacsProjectCaseKind::CpanfileNoVcs,
        EmacsProjectCaseKind::DistIniNoVcs,
        EmacsProjectCaseKind::PerlLspConfigRoot,
    ] {
        let fixture = matrix.fixture(kind).ok_or("required fixture missing")?;
        assert!(!fixture.case_root.join(".git").exists());
    }
    Ok(())
}

#[test]
fn nested_projects_have_behavior_bearing_outer_and_inner_module_facts() -> Result<(), Box<dyn Error>>
{
    let matrix = EmacsProjectFixtureMatrix::new()?;
    for kind in [
        EmacsProjectCaseKind::NestedMakefileUnderGit,
        EmacsProjectCaseKind::NestedCpanfileUnderGit,
        EmacsProjectCaseKind::OuterConfigNestedDistribution,
    ] {
        let fixture = matrix.fixture(kind).ok_or("required fixture missing")?;
        let intended =
            fixture.intended_root.as_ref().ok_or("nested fixture missing intended root")?;
        let outer = fixture.outer_root.as_ref().ok_or("nested fixture missing outer root")?;

        let inner_module = fs::read_to_string(intended.join("lib/My/Thing.pm"))?;
        let outer_module = fs::read_to_string(outer.join("lib/My/Thing.pm"))?;
        assert!(inner_module.contains(fixture.expected_module_sentinel));
        assert!(outer_module.contains("OUTER"));
        assert_ne!(inner_module, outer_module);
    }
    Ok(())
}

#[test]
fn sibling_projects_cannot_pass_from_the_other_distribution() -> Result<(), Box<dyn Error>> {
    let matrix = EmacsProjectFixtureMatrix::new()?;
    let fixture = matrix
        .fixture(EmacsProjectCaseKind::SiblingDistributions)
        .ok_or("sibling fixture missing")?;
    let intended = fixture.intended_root.as_ref().ok_or("sibling fixture missing intended root")?;
    let sibling = fixture.sibling_root.as_ref().ok_or("sibling fixture missing sibling root")?;

    let intended_module = fs::read_to_string(intended.join("lib/My/Thing.pm"))?;
    let sibling_module = fs::read_to_string(sibling.join("lib/My/Thing.pm"))?;
    assert!(intended_module.contains("ALPHA"));
    assert!(sibling_module.contains("BETA"));
    assert_ne!(intended_module, sibling_module);
    Ok(())
}

#[test]
fn worktree_and_single_file_boundaries_are_explicit() -> Result<(), Box<dyn Error>> {
    let matrix = EmacsProjectFixtureMatrix::new()?;
    let worktree =
        matrix.fixture(EmacsProjectCaseKind::GitWorktreeShape).ok_or("worktree fixture missing")?;
    let worktree_root =
        worktree.intended_root.as_ref().ok_or("worktree fixture missing intended root")?;
    let dot_git = worktree_root.join(".git");
    assert!(dot_git.is_file());
    assert!(fs::read_to_string(dot_git)?.starts_with("gitdir: "));
    // The linked-worktree case only discriminates while `.git` is the sole root
    // evidence: any competing distribution marker would let plain marker
    // discovery report the intended root with worktree recognition broken.
    for marker in ["Makefile.PL", "Build.PL", "cpanfile", "dist.ini", ".perl-lsp.toml"] {
        assert!(!worktree_root.join(marker).exists(), "competing marker {marker}");
    }

    let standalone =
        matrix.fixture(EmacsProjectCaseKind::StandaloneFile).ok_or("standalone fixture missing")?;
    assert!(standalone.intended_root.is_none());
    assert!(standalone.open_file.is_file());
    Ok(())
}

#[test]
fn stock_project_probe_observes_project_el_without_prebinding_a_root() {
    let driver = stock_project_probe_driver();

    assert!(driver.contains("(project-current nil)"));
    assert!(driver.contains("(project-root project)"));
    assert!(driver.contains("(find-file probe-file)"));
    assert!(!driver.contains("project-remember-project"));
    assert!(!driver.contains("project-known-project-roots"));
    assert!(!driver.contains("PERL_LSP_PROJECT_ROOT"));
}

#[test]
fn stock_project_probe_serializes_negatives_with_native_json_sentinels() {
    let driver = stock_project_probe_driver();

    // The negative cases are the point of this probe: `project-current' returns
    // nil for the standalone and stock non-VCS layouts. `json-serialize' only
    // accepts its own `:false'/`:null' objects, so the legacy json.el sentinel
    // or a bare nil would make the probe error out instead of recording the
    // negative fact.
    assert!(!driver.contains(":json-false"));
    assert!(driver.contains("(if project t :false)"));
    assert!(driver.contains("(or root :null)"));
    assert!(driver.contains("(or (buffer-file-name) :null)"));
}

#[test]
fn stock_project_probe_is_bound_to_the_exact_open_file_and_receipt() -> Result<(), Box<dyn Error>> {
    let matrix = EmacsProjectFixtureMatrix::new()?;
    let fixture =
        matrix.fixture(EmacsProjectCaseKind::MakefilePlNoVcs).ok_or("probe fixture missing")?;
    let receipt = matrix.root().join("receipt.json");
    let mut command = Command::new("emacs");
    bind_stock_project_probe(&mut command, fixture, &receipt);

    let file_env = command
        .get_envs()
        .find(|(name, _)| *name == OsStr::new("PERL_LSP_PROJECT_PROBE_FILE"))
        .and_then(|(_, value)| value);
    let receipt_env = command
        .get_envs()
        .find(|(name, _)| *name == OsStr::new("PERL_LSP_PROJECT_PROBE_RECEIPT"))
        .and_then(|(_, value)| value);

    assert_eq!(file_env, Some(fixture.open_file.as_os_str()));
    assert_eq!(receipt_env, Some(receipt.as_os_str()));
    Ok(())
}
