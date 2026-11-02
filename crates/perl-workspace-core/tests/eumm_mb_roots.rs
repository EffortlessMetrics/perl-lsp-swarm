//! Discriminating proof for #2981 / #13636: EUMM / Module::Build root detection.
#![expect(
    clippy::expect_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]
#![deny(clippy::map_err_ignore)]

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use perl_workspace_core::{
    BLIB_LIB, BUILD_PL, BuildSystemKind, DetectedEummMbBuildSystem, EummMbRootDisposition,
    EummMbRootFreshness, MAKEFILE_PL, ProjectRootRole, REASON_GENERATED_ROOT_STALE,
    REASON_GENERATED_ROOT_WITHOUT_BUILD_SYSTEM, REASON_ROOT_ESCAPES_WORKSPACE, SOURCE_LIB,
    TEST_LIB, detect_eumm_mb,
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("pwc-eumm-mb-{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("fixture root");
        Self { root }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(path, contents).expect("write");
    }

    fn dir(&self, relative: &str) {
        std::fs::create_dir_all(self.root.join(relative)).expect("mkdir");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn makefile_pl_detects_eumm_without_execution() {
    let fixture = Fixture::new("eumm-marker");
    fixture.write(MAKEFILE_PL, "use ExtUtils::MakeMaker;\n");
    fixture.dir(SOURCE_LIB);

    let detection = detect_eumm_mb(&fixture.root);
    assert!(detection.makefile_pl_present);
    assert_eq!(detection.build_system, DetectedEummMbBuildSystem::ExtUtilsMakeMaker);
    assert_eq!(
        detection.build_system.as_build_system_kind(),
        Some(BuildSystemKind::ExtUtilsMakeMaker)
    );
}

#[test]
fn build_pl_detects_module_build() {
    let fixture = Fixture::new("mb-marker");
    fixture.write(BUILD_PL, "use Module::Build;\n");
    fixture.dir(SOURCE_LIB);

    let detection = detect_eumm_mb(&fixture.root);
    assert!(detection.build_pl_present);
    assert_eq!(detection.build_system, DetectedEummMbBuildSystem::ModuleBuild);
}

#[test]
fn dual_markers_prefer_module_build() {
    let fixture = Fixture::new("dual");
    fixture.write(MAKEFILE_PL, "use ExtUtils::MakeMaker;\n");
    fixture.write(BUILD_PL, "use Module::Build;\n");
    fixture.dir(SOURCE_LIB);

    let detection = detect_eumm_mb(&fixture.root);
    assert_eq!(detection.build_system, DetectedEummMbBuildSystem::ModuleBuild);
}

#[test]
fn source_lib_selected_independent_of_blib() {
    let fixture = Fixture::new("source-only");
    fixture.write(MAKEFILE_PL, "use ExtUtils::MakeMaker;\n");
    fixture.dir(SOURCE_LIB);

    let detection = detect_eumm_mb(&fixture.root);
    let lib =
        detection.roots.iter().find(|root| root.relative_path == SOURCE_LIB).expect("lib root");
    assert_eq!(lib.disposition, EummMbRootDisposition::Selected);
    assert!(lib.source_canonical_over_generated);
}

#[test]
fn fresh_blib_roots_selected_when_build_system_present() {
    let fixture = Fixture::new("fresh-blib");
    fixture.write(MAKEFILE_PL, "use ExtUtils::MakeMaker;\n");
    fixture.dir(SOURCE_LIB);
    thread::sleep(Duration::from_millis(20));
    fixture.dir(BLIB_LIB);
    fixture.dir("blib/arch");

    let detection = detect_eumm_mb(&fixture.root);
    let blib_lib =
        detection.roots.iter().find(|root| root.relative_path == BLIB_LIB).expect("blib/lib");
    assert_eq!(blib_lib.disposition, EummMbRootDisposition::Selected);
    assert_eq!(blib_lib.freshness, EummMbRootFreshness::Current);
}

#[test]
fn stale_blib_rejected_when_authoring_is_newer() {
    let fixture = Fixture::new("stale-blib");
    fixture.dir(BLIB_LIB);
    thread::sleep(Duration::from_millis(20));
    fixture.write(MAKEFILE_PL, "use ExtUtils::MakeMaker;\n");
    fixture.dir(SOURCE_LIB);

    let detection = detect_eumm_mb(&fixture.root);
    let blib_lib =
        detection.roots.iter().find(|root| root.relative_path == BLIB_LIB).expect("blib/lib");
    assert!(matches!(
        blib_lib.disposition,
        EummMbRootDisposition::Rejected { reason_code: REASON_GENERATED_ROOT_STALE, .. }
    ));
}

#[test]
fn blib_without_build_system_markers_is_rejected() {
    let fixture = Fixture::new("orphan-blib");
    fixture.dir(BLIB_LIB);

    let detection = detect_eumm_mb(&fixture.root);
    assert_eq!(detection.build_system, DetectedEummMbBuildSystem::None);
    let blib_lib =
        detection.roots.iter().find(|root| root.relative_path == BLIB_LIB).expect("blib/lib");
    assert!(matches!(
        blib_lib.disposition,
        EummMbRootDisposition::Rejected {
            reason_code: REASON_GENERATED_ROOT_WITHOUT_BUILD_SYSTEM,
            ..
        }
    ));
}

#[test]
fn test_lib_is_test_role_and_not_canonical_over_generated() {
    let fixture = Fixture::new("test-lib");
    fixture.write(BUILD_PL, "use Module::Build;\n");
    fixture.dir(TEST_LIB);

    let detection = detect_eumm_mb(&fixture.root);
    let test_lib =
        detection.roots.iter().find(|root| root.relative_path == TEST_LIB).expect("t/lib");
    assert_eq!(test_lib.project_role, ProjectRootRole::Test);
    assert!(!test_lib.source_canonical_over_generated);
    assert_eq!(test_lib.disposition, EummMbRootDisposition::Selected);
}

#[cfg(unix)]
#[test]
fn symlink_blib_outside_workspace_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("symlink-escape");
    fixture.write(MAKEFILE_PL, "use ExtUtils::MakeMaker;\n");
    fixture.dir(SOURCE_LIB);
    let outside = std::env::temp_dir().join(format!("pwc-eumm-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).expect("outside dir");
    std::fs::create_dir_all(fixture.root.join("blib")).expect("blib parent");
    symlink(&outside, fixture.root.join(BLIB_LIB)).expect("symlink");

    let detection = detect_eumm_mb(&fixture.root);
    let blib_lib =
        detection.roots.iter().find(|root| root.relative_path == BLIB_LIB).expect("blib/lib");
    assert!(matches!(
        blib_lib.disposition,
        EummMbRootDisposition::Rejected { reason_code: REASON_ROOT_ESCAPES_WORKSPACE, .. }
    ));
    let _ = std::fs::remove_dir_all(outside);
}
