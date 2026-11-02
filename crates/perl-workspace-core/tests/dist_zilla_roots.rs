//! Discriminating proof for #2980: Dist::Zilla root and tool-candidate detection.
#![expect(
    clippy::expect_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]
#![deny(clippy::map_err_ignore)]

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use perl_workspace_core::id::{Digest, FileId};
use perl_workspace_core::{
    classify_plugin_heading, detect_dist_zilla, detect_dist_zilla_with_plugins, parse_dist_ini,
    plugin_notes_from_authoring, BuildSystemKind, DetectedDistZillaBuildSystem,
    DistZillaDetectOptions, DistZillaRootDisposition, DistZillaRootFreshness,
    DistZillaToolDisposition, BLIB_LIB, DIST_INI, DIST_ZILLA_BLIB_LIB,
    DIST_ZILLA_REASON_GENERATED_ROOT_STALE, DIST_ZILLA_REASON_GENERATED_ROOT_WITHOUT_BUILD_SYSTEM,
    DIST_ZILLA_SOURCE_LIB, REASON_CONFIGURED_DZIL_MISSING,
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("pwc-dist-zilla-{tag}"));
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
fn dist_ini_detects_dist_zilla_without_execution() {
    let fixture = Fixture::new("marker");
    fixture.write(DIST_INI, "name = App-Dist\nversion = 0.01\n");
    fixture.dir(DIST_ZILLA_SOURCE_LIB);

    let detection = detect_dist_zilla(&fixture.root, DistZillaDetectOptions::default());
    assert!(detection.dist_ini_present);
    assert_eq!(detection.build_system, DetectedDistZillaBuildSystem::DistZilla);
    assert_eq!(detection.build_system.as_build_system_kind(), Some(BuildSystemKind::DistZilla));
}

#[test]
fn source_lib_selected_independent_of_blib() {
    let fixture = Fixture::new("source-only");
    fixture.write(DIST_INI, "name = App-Dist\n");
    fixture.dir(DIST_ZILLA_SOURCE_LIB);

    let detection = detect_dist_zilla(&fixture.root, DistZillaDetectOptions::default());
    let lib = detection
        .roots
        .iter()
        .find(|root| root.relative_path == DIST_ZILLA_SOURCE_LIB)
        .expect("lib root");
    assert_eq!(lib.disposition, DistZillaRootDisposition::Selected);
    assert!(lib.source_canonical_over_generated);
}

#[test]
fn fresh_blib_roots_selected_when_dist_ini_present() {
    let fixture = Fixture::new("fresh-blib");
    fixture.write(DIST_INI, "name = App-Dist\n");
    fixture.dir(DIST_ZILLA_SOURCE_LIB);
    thread::sleep(Duration::from_millis(20));
    fixture.dir(BLIB_LIB);

    let detection = detect_dist_zilla(&fixture.root, DistZillaDetectOptions::default());
    let blib_lib = detection
        .roots
        .iter()
        .find(|root| root.relative_path == DIST_ZILLA_BLIB_LIB)
        .expect("blib/lib");
    assert_eq!(blib_lib.disposition, DistZillaRootDisposition::Selected);
    assert_eq!(blib_lib.freshness, DistZillaRootFreshness::Current);
}

#[test]
fn stale_blib_rejected_when_dist_ini_is_newer() {
    let fixture = Fixture::new("stale-blib");
    fixture.dir(BLIB_LIB);
    thread::sleep(Duration::from_millis(20));
    fixture.write(DIST_INI, "name = App-Dist\n");
    fixture.dir(DIST_ZILLA_SOURCE_LIB);

    let detection = detect_dist_zilla(&fixture.root, DistZillaDetectOptions::default());
    let blib_lib = detection
        .roots
        .iter()
        .find(|root| root.relative_path == DIST_ZILLA_BLIB_LIB)
        .expect("blib/lib");
    assert!(matches!(
        blib_lib.disposition,
        DistZillaRootDisposition::Rejected {
            reason_code: DIST_ZILLA_REASON_GENERATED_ROOT_STALE,
            ..
        }
    ));
}

#[test]
fn blib_without_dist_ini_is_rejected() {
    let fixture = Fixture::new("orphan-blib");
    fixture.dir(BLIB_LIB);

    let detection = detect_dist_zilla(&fixture.root, DistZillaDetectOptions::default());
    assert_eq!(detection.build_system, DetectedDistZillaBuildSystem::None);
    let blib_lib = detection
        .roots
        .iter()
        .find(|root| root.relative_path == DIST_ZILLA_BLIB_LIB)
        .expect("blib/lib");
    assert!(matches!(
        blib_lib.disposition,
        DistZillaRootDisposition::Rejected {
            reason_code: DIST_ZILLA_REASON_GENERATED_ROOT_WITHOUT_BUILD_SYSTEM,
            ..
        }
    ));
}

#[test]
fn path_only_dzil_is_not_a_candidate() {
    let fixture = Fixture::new("no-path-dzil");
    fixture.write(DIST_INI, "name = App-Dist\n");

    let detection = detect_dist_zilla(&fixture.root, DistZillaDetectOptions::default());
    assert_eq!(detection.dzil_tool, DistZillaToolDisposition::NotConfigured);
}

#[test]
fn configured_dzil_is_selected_when_present() {
    let fixture = Fixture::new("configured-dzil");
    fixture.write(DIST_INI, "name = App-Dist\n");
    fixture.write("bin/dzil", "#!/bin/sh\n");

    let detection = detect_dist_zilla(
        &fixture.root,
        DistZillaDetectOptions { configured_dzil: Some(std::path::Path::new("bin/dzil")) },
    );
    assert!(matches!(
        detection.dzil_tool,
        DistZillaToolDisposition::Selected { path } if path == "bin/dzil"
    ));
}

#[test]
fn configured_dzil_missing_is_rejected() {
    let fixture = Fixture::new("missing-dzil");
    fixture.write(DIST_INI, "name = App-Dist\n");

    let detection = detect_dist_zilla(
        &fixture.root,
        DistZillaDetectOptions { configured_dzil: Some(std::path::Path::new("bin/dzil")) },
    );
    assert!(matches!(
        detection.dzil_tool,
        DistZillaToolDisposition::Rejected { reason_code: REASON_CONFIGURED_DZIL_MISSING, .. }
    ));
}

#[test]
fn dynamic_plugin_is_explicit_in_registry() {
    let note = classify_plugin_heading("AutoPrereqs");
    assert_eq!(note.effect, "plugin_generated_metadata");
}

#[test]
fn plugin_notes_consume_authoring_facts_without_reparse() {
    let content = "[AutoPrereqs]\n[MakeMaker]\n";
    let facts = parse_dist_ini(FileId::new("dist.ini", &Digest::of(content)), content);
    let notes = plugin_notes_from_authoring(&facts);
    assert_eq!(notes.len(), facts.plugins.len());
    assert!(notes.iter().any(|note| note.effect == "plugin_generated_metadata"));
    assert!(notes.iter().any(|note| note.effect == "declares_generated_eumm"));
}

#[test]
fn detect_with_plugin_headings_is_order_independent() {
    let fixture = Fixture::new("plugins");
    fixture.write(DIST_INI, "name = App-Dist\n");
    let headings = vec!["MakeMaker".to_string(), "AutoPrereqs".to_string()];
    let forward =
        detect_dist_zilla_with_plugins(&fixture.root, DistZillaDetectOptions::default(), &headings);
    let reverse = detect_dist_zilla_with_plugins(
        &fixture.root,
        DistZillaDetectOptions::default(),
        &["AutoPrereqs".to_string(), "MakeMaker".to_string()],
    );
    assert_eq!(forward.plugin_notes.len(), reverse.plugin_notes.len());
}
