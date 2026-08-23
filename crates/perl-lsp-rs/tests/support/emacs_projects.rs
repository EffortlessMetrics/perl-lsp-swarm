#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EmacsProjectCaseKind {
    GitRoot,
    MakefilePlNoVcs,
    BuildPlNoVcs,
    CpanfileNoVcs,
    DistIniNoVcs,
    PerlLspConfigRoot,
    NestedMakefileUnderGit,
    NestedCpanfileUnderGit,
    OuterConfigNestedDistribution,
    SiblingDistributions,
    GitWorktreeShape,
    StandaloneFile,
}

impl EmacsProjectCaseKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::GitRoot => "git_root",
            Self::MakefilePlNoVcs => "makefile_pl_no_vcs",
            Self::BuildPlNoVcs => "build_pl_no_vcs",
            Self::CpanfileNoVcs => "cpanfile_no_vcs",
            Self::DistIniNoVcs => "dist_ini_no_vcs",
            Self::PerlLspConfigRoot => "perl_lsp_config_root",
            Self::NestedMakefileUnderGit => "nested_makefile_under_git",
            Self::NestedCpanfileUnderGit => "nested_cpanfile_under_git",
            Self::OuterConfigNestedDistribution => "outer_config_nested_distribution",
            Self::SiblingDistributions => "sibling_distributions",
            Self::GitWorktreeShape => "git_worktree_shape",
            Self::StandaloneFile => "standalone_file",
        }
    }
}

pub const REQUIRED_CASES: &[EmacsProjectCaseKind] = &[
    EmacsProjectCaseKind::GitRoot,
    EmacsProjectCaseKind::MakefilePlNoVcs,
    EmacsProjectCaseKind::BuildPlNoVcs,
    EmacsProjectCaseKind::CpanfileNoVcs,
    EmacsProjectCaseKind::DistIniNoVcs,
    EmacsProjectCaseKind::PerlLspConfigRoot,
    EmacsProjectCaseKind::NestedMakefileUnderGit,
    EmacsProjectCaseKind::NestedCpanfileUnderGit,
    EmacsProjectCaseKind::OuterConfigNestedDistribution,
    EmacsProjectCaseKind::SiblingDistributions,
    EmacsProjectCaseKind::GitWorktreeShape,
    EmacsProjectCaseKind::StandaloneFile,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmacsProjectFixture {
    pub kind: EmacsProjectCaseKind,
    pub case_root: PathBuf,
    pub open_file: PathBuf,
    pub intended_root: Option<PathBuf>,
    pub outer_root: Option<PathBuf>,
    pub sibling_root: Option<PathBuf>,
    pub expected_module_sentinel: &'static str,
}

#[derive(Debug)]
pub struct EmacsProjectFixtureMatrix {
    root: TempDir,
    fixtures: Vec<EmacsProjectFixture>,
}

impl EmacsProjectFixtureMatrix {
    pub fn new() -> io::Result<Self> {
        let root = tempfile::tempdir()?;
        let mut fixtures = Vec::with_capacity(REQUIRED_CASES.len());
        for kind in REQUIRED_CASES {
            fixtures.push(create_fixture(root.path(), *kind)?);
        }
        Ok(Self { root, fixtures })
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn fixtures(&self) -> &[EmacsProjectFixture] {
        &self.fixtures
    }

    pub fn fixture(&self, kind: EmacsProjectCaseKind) -> Option<&EmacsProjectFixture> {
        self.fixtures.iter().find(|fixture| fixture.kind == kind)
    }
}

pub fn stock_project_probe_driver() -> &'static str {
    r#"
(require 'project)
(require 'json)
(let* ((probe-file (getenv "PERL_LSP_PROJECT_PROBE_FILE"))
       (receipt-path (getenv "PERL_LSP_PROJECT_PROBE_RECEIPT")))
  (unless (and probe-file receipt-path)
    (error "missing project probe environment"))
  (find-file probe-file)
  ;; `json-serialize' is the native serializer: its default false object is
  ;; `:false' and its default null object is `:null'.  The legacy json.el
  ;; false sentinel and a bare nil are not accepted here, and the negative
  ;; cases (standalone file, stock non-VCS layouts) are exactly the ones that
  ;; reach them, so the negative receipt must use the native sentinels or the
  ;; probe errors instead of recording the negative fact.
  (let* ((project (project-current nil))
         (root (and project (project-root project)))
         (payload (list :projectRecognized (if project t :false)
                        :projectRoot (or root :null)
                        :openedFile (or (buffer-file-name) :null)
                        :majorMode (symbol-name major-mode))))
    (with-temp-file receipt-path
      (insert (json-serialize payload)))))
"#
}

pub fn bind_stock_project_probe(
    command: &mut Command,
    fixture: &EmacsProjectFixture,
    receipt_path: &Path,
) {
    command
        .env("PERL_LSP_PROJECT_PROBE_FILE", &fixture.open_file)
        .env("PERL_LSP_PROJECT_PROBE_RECEIPT", receipt_path);
}

fn create_fixture(root: &Path, kind: EmacsProjectCaseKind) -> io::Result<EmacsProjectFixture> {
    let case_root = root.join(kind.id());
    fs::create_dir_all(&case_root)?;

    match kind {
        EmacsProjectCaseKind::GitRoot => {
            fs::create_dir_all(case_root.join(".git"))?;
            write_distribution(&case_root, None, "GIT_ROOT")?;
            Ok(simple_fixture(kind, case_root, "GIT_ROOT"))
        }
        EmacsProjectCaseKind::MakefilePlNoVcs => {
            fs::write(case_root.join("Makefile.PL"), "use ExtUtils::MakeMaker;\n")?;
            write_distribution(&case_root, None, "MAKEFILE_PL")?;
            Ok(simple_fixture(kind, case_root, "MAKEFILE_PL"))
        }
        EmacsProjectCaseKind::BuildPlNoVcs => {
            fs::write(case_root.join("Build.PL"), "use Module::Build;\n")?;
            write_distribution(&case_root, None, "BUILD_PL")?;
            Ok(simple_fixture(kind, case_root, "BUILD_PL"))
        }
        EmacsProjectCaseKind::CpanfileNoVcs => {
            fs::write(case_root.join("cpanfile"), "requires 'strict';\n")?;
            write_distribution(&case_root, None, "CPANFILE")?;
            Ok(simple_fixture(kind, case_root, "CPANFILE"))
        }
        EmacsProjectCaseKind::DistIniNoVcs => {
            fs::write(case_root.join("dist.ini"), "name = Fixture\n")?;
            write_distribution(&case_root, None, "DIST_INI")?;
            Ok(simple_fixture(kind, case_root, "DIST_INI"))
        }
        EmacsProjectCaseKind::PerlLspConfigRoot => {
            fs::write(case_root.join(".perl-lsp.toml"), "[perl]\ninclude_paths = [\"lib\"]\n")?;
            write_distribution(&case_root, None, "PERL_LSP_CONFIG")?;
            Ok(simple_fixture(kind, case_root, "PERL_LSP_CONFIG"))
        }
        EmacsProjectCaseKind::NestedMakefileUnderGit => {
            fs::create_dir_all(case_root.join(".git"))?;
            write_distribution(&case_root, None, "OUTER")?;
            let nested = case_root.join("packages/inner");
            fs::create_dir_all(&nested)?;
            fs::write(nested.join("Makefile.PL"), "use ExtUtils::MakeMaker;\n")?;
            write_distribution(&nested, None, "INNER_MAKEFILE")?;
            Ok(nested_fixture(kind, case_root, nested, "INNER_MAKEFILE"))
        }
        EmacsProjectCaseKind::NestedCpanfileUnderGit => {
            fs::create_dir_all(case_root.join(".git"))?;
            write_distribution(&case_root, None, "OUTER")?;
            let nested = case_root.join("services/inner");
            fs::create_dir_all(&nested)?;
            fs::write(nested.join("cpanfile"), "requires 'strict';\n")?;
            write_distribution(&nested, None, "INNER_CPANFILE")?;
            Ok(nested_fixture(kind, case_root, nested, "INNER_CPANFILE"))
        }
        EmacsProjectCaseKind::OuterConfigNestedDistribution => {
            fs::write(
                case_root.join(".perl-lsp.toml"),
                "[perl]\ninclude_paths = [\"outer-lib\"]\n",
            )?;
            write_distribution(&case_root, None, "OUTER_CONFIG")?;
            let nested = case_root.join("dist/inner");
            fs::create_dir_all(&nested)?;
            fs::write(nested.join("Makefile.PL"), "use ExtUtils::MakeMaker;\n")?;
            write_distribution(&nested, None, "INNER_DISTRIBUTION")?;
            Ok(nested_fixture(kind, case_root, nested, "INNER_DISTRIBUTION"))
        }
        EmacsProjectCaseKind::SiblingDistributions => {
            let alpha = case_root.join("alpha");
            let beta = case_root.join("beta");
            fs::create_dir_all(&alpha)?;
            fs::create_dir_all(&beta)?;
            fs::write(alpha.join("Makefile.PL"), "use ExtUtils::MakeMaker;\n")?;
            fs::write(beta.join("Makefile.PL"), "use ExtUtils::MakeMaker;\n")?;
            write_distribution(&alpha, None, "ALPHA")?;
            write_distribution(&beta, None, "BETA")?;
            Ok(EmacsProjectFixture {
                kind,
                case_root,
                open_file: alpha.join("script/probe.pl"),
                intended_root: Some(alpha),
                outer_root: None,
                sibling_root: Some(beta),
                expected_module_sentinel: "ALPHA",
            })
        }
        EmacsProjectCaseKind::GitWorktreeShape => {
            let git_common = case_root.join("common/.git/worktrees/fixture");
            fs::create_dir_all(&git_common)?;
            let worktree = case_root.join("worktree");
            fs::create_dir_all(&worktree)?;
            // Normalize separators: `Path::display` renders backslashes on
            // Windows, and both Git and the Emacs Lisp readers of this file
            // expect forward slashes in `gitdir:` pointers.
            let gitdir_path = git_common.to_string_lossy().replace('\\', "/");
            fs::write(worktree.join(".git"), format!("gitdir: {}\n", gitdir_path))?;
            // No Perl distribution marker here on purpose. The receipt records
            // only recognition and root, not which project backend answered, so
            // a competing `Makefile.PL` at the same directory would let plain
            // marker discovery report the intended root while linked-worktree
            // recognition is entirely broken. Leaving `.git` as the only root
            // evidence is what makes this case discriminate.
            write_distribution(&worktree, None, "WORKTREE")?;
            Ok(EmacsProjectFixture {
                kind,
                case_root,
                open_file: worktree.join("script/probe.pl"),
                intended_root: Some(worktree),
                outer_root: None,
                sibling_root: None,
                expected_module_sentinel: "WORKTREE",
            })
        }
        EmacsProjectCaseKind::StandaloneFile => {
            let file = case_root.join("standalone.pl");
            fs::write(&file, "use strict;\nuse warnings;\nprint \"STANDALONE\\n\";\n")?;
            Ok(EmacsProjectFixture {
                kind,
                case_root,
                open_file: file,
                intended_root: None,
                outer_root: None,
                sibling_root: None,
                expected_module_sentinel: "STANDALONE",
            })
        }
    }
}

fn simple_fixture(
    kind: EmacsProjectCaseKind,
    root: PathBuf,
    sentinel: &'static str,
) -> EmacsProjectFixture {
    EmacsProjectFixture {
        kind,
        open_file: root.join("script/probe.pl"),
        intended_root: Some(root.clone()),
        outer_root: None,
        sibling_root: None,
        case_root: root,
        expected_module_sentinel: sentinel,
    }
}

fn nested_fixture(
    kind: EmacsProjectCaseKind,
    outer: PathBuf,
    nested: PathBuf,
    sentinel: &'static str,
) -> EmacsProjectFixture {
    EmacsProjectFixture {
        kind,
        case_root: outer.clone(),
        open_file: nested.join("script/probe.pl"),
        intended_root: Some(nested),
        outer_root: Some(outer),
        sibling_root: None,
        expected_module_sentinel: sentinel,
    }
}

fn write_distribution(root: &Path, config: Option<&str>, sentinel: &'static str) -> io::Result<()> {
    let lib = root.join("lib/My");
    let script = root.join("script");
    fs::create_dir_all(&lib)?;
    fs::create_dir_all(&script)?;
    fs::write(
        lib.join("Thing.pm"),
        format!(
            "package My::Thing;\nuse strict;\nuse warnings;\nsub marker {{ '{sentinel}' }}\n1;\n"
        ),
    )?;
    fs::write(
        script.join("probe.pl"),
        "use strict;\nuse warnings;\nuse lib 'lib';\nuse My::Thing;\nprint My::Thing::marker();\n",
    )?;
    if let Some(config) = config {
        fs::write(root.join(".perl-lsp.toml"), config)?;
    }
    Ok(())
}
