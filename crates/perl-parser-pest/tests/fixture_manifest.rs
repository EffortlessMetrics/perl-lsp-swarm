//! Discriminating tests for the package-local Pest fixture manifest and runner.
//!
//! These tests own the fail-closed instrument contract. They do not assert that
//! the current parser is correct, and they do not promote `Ok` into acceptance.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

mod support;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use support::{
    Classification, CurrentObservation, DEFAULT_MANIFEST_RELATIVE, Disposition, ExecutionMode,
    FixtureError, NewlineVariant, ParseObservation, Selection, load_manifest, load_manifest_at,
    observe_resolved, observe_with_embedded_parser, package_root, run_embedded,
    run_embedded_loaded,
};

const REQUIRED_FAMILIES: &[&str] = &[
    "declaration-control-flow",
    "recovery",
    "scalar-deref",
    "bitwise-not",
    "assignment-exponentiation",
    "statement-modifier",
    "nested-call",
    "heredoc",
    "quoting-context",
    "unicode-newline",
];

struct TempPackage {
    dir: tempfile::TempDir,
}

impl TempPackage {
    fn new() -> Result<Self, Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join("tests/fixtures/sources"))?;
        Ok(Self { dir })
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write_manifest(&self, body: &str) -> Result<(), Box<dyn Error>> {
        fs::write(self.root().join("tests/fixtures/manifest.toml"), body)?;
        Ok(())
    }

    fn write_source(&self, relative: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        let path = self.root().join(relative);
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
        Ok(())
    }
}

fn valid_row(id: &str, family: &str, relative: &str) -> String {
    format!(
        r#"
[[fixtures]]
id = "{id}"
family = "{family}"
source = "{relative}"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
expected_outcome_owner = "8248"
disposition = "provisional-observation"
"#
    )
}

fn wrap_manifest(rows: &str) -> String {
    format!("schema = \"perl-parser-pest.fixture_manifest.v1\"\n{rows}")
}

#[test]
fn duplicate_fixture_ids_fail_closed() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/a.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(&format!(
        "{}{}",
        valid_row("dup", "family-a", "tests/fixtures/sources/a.pl"),
        valid_row("dup", "family-b", "tests/fixtures/sources/a.pl"),
    )))?;

    let error = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("duplicate ids must fail".into()),
    };
    assert!(matches!(error, FixtureError::DuplicateId(id) if id == "dup"));
    Ok(())
}

#[test]
fn insertion_order_is_execution_and_report_order() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/z.pl", b"my $z = 1;\n")?;
    package.write_source("tests/fixtures/sources/a.pl", b"my $a = 1;\n")?;
    package.write_source("tests/fixtures/sources/m.pl", b"my $m = 1;\n")?;
    package.write_manifest(&wrap_manifest(&format!(
        "{}{}{}",
        valid_row("z-first", "order", "tests/fixtures/sources/z.pl"),
        valid_row("a-second", "order", "tests/fixtures/sources/a.pl"),
        valid_row("m-third", "order", "tests/fixtures/sources/m.pl"),
    )))?;

    let loaded = load_manifest(package.root())?;
    let ids: Vec<&str> = loaded.fixtures.iter().map(|fixture| fixture.id.as_str()).collect();
    assert_eq!(ids, ["z-first", "a-second", "m-third"]);

    let observations = run_embedded(package.root(), &Selection::all())?;
    let observed_ids: Vec<&str> =
        observations.iter().map(|observation| observation.id.as_str()).collect();
    assert_eq!(observed_ids, ["z-first", "a-second", "m-third"]);
    Ok(())
}

#[test]
fn parent_directory_and_absolute_paths_are_rejected() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;

    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "escape"
family = "path"
source = "tests/fixtures/sources/../../Cargo.toml"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let escape = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("path escape must fail".into()),
    };
    assert!(matches!(escape, FixtureError::PathEscape(path) if path.contains("..")));

    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "absolute"
family = "path"
source = "/tmp/outside.pl"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let absolute = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("absolute path must fail".into()),
    };
    assert!(
        matches!(absolute, FixtureError::AbsolutePath(ref path) if path.contains("/tmp/outside.pl")),
        "absolute path must fail as AbsolutePath, got {absolute}"
    );
    Ok(())
}

#[test]
fn source_outside_tests_fixtures_is_rejected() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("src/lib.rs", b"not a fixture\n")?;
    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "src-lib"
family = "path"
source = "src/lib.rs"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let error = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("source outside tests/fixtures must fail".into()),
    };
    assert!(matches!(error, FixtureError::SourceNotUnderFixtures(path) if path == "src/lib.rs"));
    Ok(())
}

#[test]
fn missing_and_unreadable_sources_fail_as_instrument_errors() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "missing",
        "path",
        "tests/fixtures/sources/absent.pl",
    )))?;
    let missing = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("missing source must fail".into()),
    };
    assert!(matches!(
        missing,
        FixtureError::MissingSource { ref id, ref path }
            if id == "missing" && path == "tests/fixtures/sources/absent.pl"
    ));
    assert!(
        !missing.to_string().contains("parse-returned-err"),
        "missing fixture must not be presented as parser rejection: {missing}"
    );

    package.write_source("tests/fixtures/sources/dir-placeholder.pl", b"ignore\n")?;
    let dir_path = package.root().join("tests/fixtures/sources/not-a-file");
    fs::create_dir_all(&dir_path)?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "dir",
        "path",
        "tests/fixtures/sources/not-a-file",
    )))?;
    let unreadable = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("directory source must fail".into()),
    };
    assert!(matches!(unreadable, FixtureError::Unreadable { id, .. } if id == "dir"));
    Ok(())
}

#[test]
fn empty_manifest_and_empty_selection_fail_closed() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_manifest("schema = \"perl-parser-pest.fixture_manifest.v1\"\nfixtures = []\n")?;
    let empty_manifest = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("empty manifest must fail".into()),
    };
    assert!(matches!(empty_manifest, FixtureError::EmptyManifest));

    package.write_source("tests/fixtures/sources/one.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "one",
        "kept",
        "tests/fixtures/sources/one.pl",
    )))?;
    let loaded = load_manifest(package.root())?;
    let empty_id = match loaded.select(&Selection::id("no-such-id")) {
        Err(error) => error,
        Ok(_) => return Err("unknown id must fail".into()),
    };
    assert!(matches!(
        empty_id,
        FixtureError::EmptySelection { id: Some(id), .. } if id == "no-such-id"
    ));
    let empty_family = match loaded.select(&Selection::family("missing-family")) {
        Err(error) => error,
        Ok(_) => return Err("unknown family must fail".into()),
    };
    assert!(matches!(
        empty_family,
        FixtureError::EmptySelection { family: Some(family), .. } if family == "missing-family"
    ));
    Ok(())
}

#[test]
fn declared_digest_mismatch_and_shared_identity_byte_drift_fail() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/a.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "digest-lie"
family = "identity"
source = "tests/fixtures/sources/a.pl"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"
source_digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000"
"#,
    ))?;
    let digest = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("digest mismatch must fail".into()),
    };
    assert!(matches!(digest, FixtureError::DigestMismatch { id, .. } if id == "digest-lie"));

    package.write_source("tests/fixtures/sources/left.pl", b"left\n")?;
    package.write_source("tests/fixtures/sources/right.pl", b"right\n")?;
    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "left"
identity = "shared-case"
family = "identity"
source = "tests/fixtures/sources/left.pl"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"

[[fixtures]]
id = "right"
identity = "shared-case"
family = "identity"
source = "tests/fixtures/sources/right.pl"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let drift = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("identity byte drift must fail".into()),
    };
    assert!(matches!(
        drift,
        FixtureError::IdentityByteMismatch { identity, .. } if identity == "shared-case"
    ));
    Ok(())
}

#[test]
fn final_acceptance_without_owner_fails_and_ok_is_not_labeled_correct() -> Result<(), Box<dyn Error>>
{
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "final-no-owner"
family = "honesty"
source = "tests/fixtures/sources/ok.pl"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "final-acceptance"
"#,
    ))?;
    let error = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("final acceptance without owner must fail".into()),
    };
    assert!(matches!(
        error,
        FixtureError::FinalAcceptanceWithoutOwner { id } if id == "final-no-owner"
    ));

    package.write_manifest(&wrap_manifest(&valid_row(
        "observed",
        "honesty",
        "tests/fixtures/sources/ok.pl",
    )))?;
    let observations = run_embedded(package.root(), &Selection::id("observed"))?;
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].disposition, Disposition::ProvisionalObservation);
    assert_eq!(observations[0].parse.kind_name(), "parse-returned-ok");
    let rendered = format!("{:?}", observations[0].parse);
    assert!(
        !rendered.to_ascii_lowercase().contains("correct")
            && !rendered.to_ascii_lowercase().contains("accepted"),
        "Ok observation must not be labeled correct/accepted: {rendered}"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic,
    reason = "policy:pest-fixture-panic-probe: inject a panic so the runner cannot hide it as parser rejection"
)]
fn parser_panic_is_instrument_failure_not_parser_rejection() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "panic-case",
        "honesty",
        "tests/fixtures/sources/ok.pl",
    )))?;
    let loaded = load_manifest(package.root())?;
    let fixture = loaded.select(&Selection::id("panic-case"))?[0];
    let error = match observe_resolved(fixture, |_| std::panic::panic_any("injected parser panic"))
    {
        Err(error) => error,
        Ok(_) => return Err("parser panic must fail the run".into()),
    };
    assert!(matches!(
        error,
        FixtureError::ParserPanic { ref id, ref message }
            if id == "panic-case" && message.contains("injected parser panic")
    ));
    assert!(
        !error.to_string().contains("parse-returned-err"),
        "panic must not be rewritten as parser rejection: {error}"
    );
    Ok(())
}

#[test]
fn invalid_utf8_is_not_parser_rejection() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/binary.pl", &[0xff, 0xfe, b'x'])?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "not-utf8",
        "encoding",
        "tests/fixtures/sources/binary.pl",
    )))?;
    let observations = run_embedded(package.root(), &Selection::id("not-utf8"))?;
    assert_eq!(observations[0].parse, ParseObservation::SourceNotUtf8);
    assert_eq!(observations[0].parse.kind_name(), "source-not-utf8");
    Ok(())
}

#[test]
fn malformed_toml_and_unknown_schema_fail_as_instrument_errors() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_manifest("this is not toml [[")?;
    let invalid = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("invalid toml must fail".into()),
    };
    assert!(matches!(invalid, FixtureError::InvalidToml { .. }));

    package.write_manifest(
        &wrap_manifest(&valid_row("schema", "meta", "tests/fixtures/sources/ok.pl"))
            .replace("perl-parser-pest.fixture_manifest.v1", "not-a-schema"),
    )?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;
    let schema = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("unknown schema must fail".into()),
    };
    assert!(matches!(schema, FixtureError::InvalidSchema(value) if value == "not-a-schema"));
    Ok(())
}

#[test]
fn both_or_neither_source_fields_fail() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "both"
family = "source"
source = "tests/fixtures/sources/ok.pl"
inline_source = "my $x = 1;"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let both = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("both source fields must fail".into()),
    };
    assert!(matches!(both, FixtureError::AmbiguousSource { id } if id == "both"));

    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "neither"
family = "source"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let neither = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("neither source field must fail".into()),
    };
    assert!(matches!(neither, FixtureError::AmbiguousSource { id } if id == "neither"));
    Ok(())
}

#[test]
fn missing_id_family_and_execution_modes_fail() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
family = "source"
source = "tests/fixtures/sources/ok.pl"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let missing_id = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("missing id must fail".into()),
    };
    assert!(matches!(missing_id, FixtureError::MissingId));

    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "no-family"
source = "tests/fixtures/sources/ok.pl"
classification = "valid"
execution_modes = ["embedded"]
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let missing_family = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("missing family must fail".into()),
    };
    assert!(matches!(missing_family, FixtureError::MissingFamily { id } if id == "no-family"));

    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "no-modes"
family = "source"
source = "tests/fixtures/sources/ok.pl"
classification = "valid"
execution_modes = []
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let missing_modes = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("empty execution_modes must fail".into()),
    };
    assert!(matches!(
        missing_modes,
        FixtureError::MissingExecutionModes { id } if id == "no-modes"
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_source_is_rejected() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    let outside = tempfile::tempdir()?;
    let target = outside.path().join("outside.pl");
    fs::write(&target, b"my $x = 1;\n")?;
    let link = package.root().join("tests/fixtures/sources/link.pl");
    std::os::unix::fs::symlink(&target, &link)?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "link",
        "path",
        "tests/fixtures/sources/link.pl",
    )))?;
    let error = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("symlink source must fail".into()),
    };
    assert!(matches!(error, FixtureError::SymlinkSource { id, .. } if id == "link"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn directory_symlink_component_is_rejected() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    let outside = tempfile::tempdir()?;
    fs::write(outside.path().join("x.pl"), b"my $x = 1;\n")?;
    let redirect = package.root().join("tests/fixtures/redirect");
    std::os::unix::fs::symlink(outside.path(), &redirect)?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "dir-link",
        "path",
        "tests/fixtures/redirect/x.pl",
    )))?;
    let error = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("directory symlink component must fail".into()),
    };
    assert!(matches!(error, FixtureError::SymlinkSource { id, .. } if id == "dir-link"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_manifest_is_rejected() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "ok",
        "path",
        "tests/fixtures/sources/ok.pl",
    )))?;
    let outside = tempfile::tempdir()?;
    let target = outside.path().join("manifest.toml");
    let manifest = package.root().join("tests/fixtures/manifest.toml");
    fs::rename(&manifest, &target)?;
    std::os::unix::fs::symlink(&target, &manifest)?;
    let error = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("symlink manifest must fail".into()),
    };
    assert!(
        matches!(
            error,
            FixtureError::SymlinkSource { ref id, ref path }
                if id == "manifest" && path == "tests/fixtures/manifest.toml"
        ),
        "symlink manifest must fail closed as SymlinkSource, got {error}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn directory_symlink_component_on_manifest_is_rejected() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "ok",
        "path",
        "tests/fixtures/sources/ok.pl",
    )))?;
    let fixtures = package.root().join("tests/fixtures");
    let outside = tempfile::tempdir()?;
    let relocated = outside.path().join("fixtures");
    fs::rename(&fixtures, &relocated)?;
    std::os::unix::fs::symlink(&relocated, &fixtures)?;
    let error = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("directory symlink on the manifest path must fail".into()),
    };
    assert!(
        matches!(error, FixtureError::SymlinkSource { ref id, .. } if id == "manifest"),
        "manifest directory symlink must fail closed as SymlinkSource, got {error}"
    );
    Ok(())
}

#[test]
fn non_regular_manifest_is_rejected() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    let manifest = package.root().join("tests/fixtures/manifest.toml");
    fs::create_dir_all(&manifest)?;
    let error = match load_manifest(package.root()) {
        Err(error) => error,
        Ok(_) => return Err("directory manifest must fail".into()),
    };
    assert!(
        matches!(
            error,
            FixtureError::Unreadable { ref id, ref path, ref detail }
                if id == "manifest"
                    && path == "tests/fixtures/manifest.toml"
                    && detail.contains("not a regular file")
        ),
        "non-regular manifest must fail closed as Unreadable, got {error}"
    );
    Ok(())
}

#[test]
fn family_selection_preserves_insertion_order() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/a.pl", b"my $a = 1;\n")?;
    package.write_source("tests/fixtures/sources/b.pl", b"my $b = 1;\n")?;
    package.write_source("tests/fixtures/sources/c.pl", b"my $c = 1;\n")?;
    package.write_manifest(&wrap_manifest(&format!(
        "{}{}{}",
        valid_row("keep-1", "keep", "tests/fixtures/sources/a.pl"),
        valid_row("skip", "other", "tests/fixtures/sources/b.pl"),
        valid_row("keep-2", "keep", "tests/fixtures/sources/c.pl"),
    )))?;
    let loaded = load_manifest(package.root())?;
    let selected = loaded.select(&Selection::family("keep"))?;
    let ids: Vec<&str> = selected.iter().map(|fixture| fixture.id.as_str()).collect();
    assert_eq!(ids, ["keep-1", "keep-2"]);
    Ok(())
}

#[test]
fn dotted_crate_relative_source_is_accepted() -> Result<(), Box<dyn Error>> {
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(&valid_row(
        "dotted",
        "path",
        "./tests/fixtures/sources/ok.pl",
    )))?;
    let loaded = load_manifest(package.root())?;
    assert_eq!(loaded.select(&Selection::id("dotted"))?.len(), 1);
    Ok(())
}

#[test]
fn embedded_runner_skips_non_embedded_rows_and_fails_when_none_remain() -> Result<(), Box<dyn Error>>
{
    let package = TempPackage::new()?;
    package.write_source("tests/fixtures/sources/ok.pl", b"my $x = 1;\n")?;
    package.write_manifest(&wrap_manifest(
        r#"
[[fixtures]]
id = "packaged-only"
family = "mode"
source = "tests/fixtures/sources/ok.pl"
classification = "valid"
execution_modes = ["packaged"]
observation_owner = "8419"
disposition = "provisional-observation"
"#,
    ))?;
    let error = match run_embedded(package.root(), &Selection::all()) {
        Err(error) => error,
        Ok(_) => return Err("packaged-only catalog must not run embedded".into()),
    };
    assert!(matches!(error, FixtureError::EmptySelection { .. }));
    Ok(())
}

#[test]
fn package_root_is_crate_local_not_workspace_root() {
    let root = package_root();
    assert!(
        root.ends_with("perl-parser-pest"),
        "package_root must be the crate directory, got {}",
        root.display()
    );
    assert!(
        root.join(DEFAULT_MANIFEST_RELATIVE).is_file(),
        "crate-local manifest must exist at {DEFAULT_MANIFEST_RELATIVE}"
    );
    assert!(!root.join("Cargo.lock").is_file(), "package_root must not be the workspace root");
}

#[test]
fn crate_catalog_covers_train_families_without_claiming_acceptance() -> Result<(), Box<dyn Error>> {
    let loaded = load_manifest(&package_root())?;
    assert_eq!(loaded.schema, support::MANIFEST_SCHEMA);
    assert!(!loaded.fixtures.is_empty());

    let mut missing = Vec::new();
    for family in REQUIRED_FAMILIES {
        if loaded.select(&Selection::family(*family)).is_err() {
            missing.push(*family);
        }
    }
    assert!(missing.is_empty(), "seed catalog missing required families: {missing:?}");

    for fixture in &loaded.fixtures {
        assert_eq!(
            fixture.disposition,
            Disposition::ProvisionalObservation,
            "seed row {} must stay provisional",
            fixture.id
        );
        assert!(
            fixture.source_digest.starts_with("sha256:") && fixture.source_digest.len() == 71,
            "seed row {} must record a sha256 digest",
            fixture.id
        );
        assert!(
            fixture.execution_modes.contains(&ExecutionMode::Embedded),
            "seed row {} must declare embedded execution",
            fixture.id
        );
        if let support::SourceKind::File { relative } = &fixture.source_kind {
            assert!(
                relative.starts_with(PathBuf::from("tests/fixtures")),
                "seed source {} is not crate-local",
                relative.display()
            );
        }
    }

    let observations: Vec<CurrentObservation> = run_embedded_loaded(&loaded, &Selection::all())?;
    assert_eq!(observations.len(), loaded.fixtures.len());
    assert_eq!(observations.len(), run_embedded(&package_root(), &Selection::all())?.len());
    let first = observe_with_embedded_parser(&loaded.fixtures[0])?;
    assert_eq!(first.id, loaded.fixtures[0].id);
    for observation in &observations {
        assert_eq!(observation.disposition, Disposition::ProvisionalObservation);
        match &observation.parse {
            ParseObservation::ReturnedOk { sexp_digest } => {
                assert!(sexp_digest.starts_with("sha256:"));
            }
            ParseObservation::ReturnedErr { message } => {
                assert!(!message.is_empty(), "parser rejection must keep its error text");
            }
            ParseObservation::SourceNotUtf8 => {}
        }
        assert_ne!(observation.parse.kind_name(), "correct");
    }

    let malformed = loaded.select(&Selection::family("recovery"))?;
    assert!(
        malformed.iter().any(|fixture| fixture.classification == Classification::Malformed),
        "recovery family must include a malformed row"
    );
    Ok(())
}

#[test]
fn crate_newline_fixtures_preserve_declared_bytes() -> Result<(), Box<dyn Error>> {
    let loaded = load_manifest(&package_root())?;
    let lf = loaded.select(&Selection::id("newline.lf"))?[0];
    let crlf = loaded.select(&Selection::id("newline.crlf"))?[0];
    let cr = loaded.select(&Selection::id("newline.bare-cr"))?[0];
    assert_eq!(lf.newline, Some(NewlineVariant::Lf));
    assert_eq!(crlf.newline, Some(NewlineVariant::Crlf));
    assert_eq!(cr.newline, Some(NewlineVariant::Cr));
    assert!(lf.bytes.contains(&b'\n') && !lf.bytes.contains(&b'\r'));
    assert!(crlf.bytes.windows(2).any(|pair| pair == b"\r\n"));
    assert!(cr.bytes.contains(&b'\r') && !cr.bytes.contains(&b'\n'));
    assert_ne!(lf.source_digest, crlf.source_digest);
    assert_ne!(lf.source_digest, cr.source_digest);
    Ok(())
}

#[test]
fn load_manifest_at_rejects_escaped_manifest_path() -> Result<(), Box<dyn Error>> {
    let error = match load_manifest_at(&package_root(), Path::new("../Cargo.toml")) {
        Err(error) => error,
        Ok(_) => return Err("escaped manifest path must fail".into()),
    };
    assert!(matches!(error, FixtureError::PathEscape(_) | FixtureError::SourceNotUnderFixtures(_)));
    Ok(())
}
