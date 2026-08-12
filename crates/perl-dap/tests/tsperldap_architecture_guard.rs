//! Guards the active DAP architecture against revival of the superseded
//! `Devel::TSPerlDAP` bundle/shim plan.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(repo_root().join(path))?)
}

#[test]
fn active_dap_surfaces_do_not_prescribe_the_old_shim_architecture()
-> Result<(), Box<dyn std::error::Error>> {
    const ACTIVE_SURFACES: &[&str] = &[
        "docs/reference/CRATE_ARCHITECTURE_DAP.md",
        "book/src/architecture/dap-implementation.md",
        "book/src/dap/implementation.md",
        "book/src/dap/security.md",
        "docs/tutorials/DAP_USER_GUIDE.md",
        "crates/perl-dap/README.md",
        "crates/perl-dap/CLAUDE.md",
    ];

    const OBSOLETE_DIRECTIVES: &[&str] = &[
        "Devel::TSPerlDAP** (CPAN module)",
        "Devel-TSPerlDAP/",
        "resources/perl-shim/",
        "--install-shim",
        "CPAN Perl shim",
        "bundled fallback shim",
        "Phase 2 (Week 3-6): Native Rust adapter (perl-dap) + CPAN Perl shim",
    ];

    for path in ACTIVE_SURFACES {
        let content = read(path)?;
        for directive in OBSOLETE_DIRECTIVES {
            assert!(
                !content.contains(directive),
                "active DAP surface {path} revives superseded shim directive {directive:?}"
            );
        }
    }

    Ok(())
}

#[test]
fn current_architecture_and_archive_state_the_transition_explicitly()
-> Result<(), Box<dyn std::error::Error>> {
    let current = read("docs/reference/CRATE_ARCHITECTURE_DAP.md")?;
    assert!(current.contains("replaces the 0.9-era greenfield design"));
    assert!(current.contains("does **not** ship or require"));
    assert!(current.contains("docs/archive/DAP_0_9_SHIM_DESIGN.md"));

    let archive = read("docs/archive/DAP_0_9_SHIM_DESIGN.md")?;
    assert!(archive.contains("Historical record; not product architecture"));
    assert!(archive.contains("must not be revived"));
    assert!(archive.contains("Issue #7295 owns that decision"));

    Ok(())
}
