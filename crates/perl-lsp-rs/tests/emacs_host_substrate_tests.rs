#[path = "support/emacs_host.rs"]
mod emacs_host;

use emacs_host::{
    EmacsClientKind, EmacsHostSandbox, EmacsHostSubject, ProjectRootEvidenceMode,
    RegistrationState,
};
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn fixture_subject(sandbox: &EmacsHostSandbox) -> Result<EmacsHostSubject, Box<dyn Error>> {
    let tools = sandbox.root().join("tools");
    fs::create_dir_all(&tools)?;
    let emacs = tools.join("emacs");
    let perllsp = tools.join("perllsp");
    fs::write(&emacs, b"fixture emacs")?;
    fs::write(&perllsp, b"fixture perllsp")?;

    Ok(EmacsHostSubject {
        emacs_executable: emacs,
        emacs_version: "29.4".to_string(),
        client_kind: EmacsClientKind::BundledEglot,
        client_version: "1.12.29".to_string(),
        client_source: "bundled_with_emacs".to_string(),
        perllsp_executable: perllsp,
        perllsp_version: "0.18.0-dev".to_string(),
        perllsp_sha256: "a".repeat(64),
        registration_state: RegistrationState::ManualClientRegistration,
        project_root_evidence_mode: ProjectRootEvidenceMode::ManuallyBoundFixture,
    })
}

fn command_env<'a>(command: &'a Command, key: &str) -> Option<Option<&'a OsStr>> {
    command
        .get_envs()
        .find(|(name, _)| *name == OsStr::new(key))
        .map(|(_, value)| value)
}

#[test]
fn sandbox_contains_shared_unicode_crlf_and_project_configuration_fixture()
-> Result<(), Box<dyn Error>> {
    let sandbox = EmacsHostSandbox::new()?;
    let fixture = fs::read(sandbox.project_root().join("t/unicode_crlf.t"))?;
    let fixture_text = String::from_utf8(fixture.clone())?;

    assert!(fixture_text.contains('😀'));
    assert!(fixture.windows(2).any(|pair| pair == b"\r\n"));
    assert!(sandbox.project_root().join("lib/My/Thing.pm").is_file());
    assert!(sandbox.project_root().join("lib/My/Sibling.pm").is_file());
    assert!(sandbox.project_root().join(".perl-lsp.toml").is_file());
    assert!(sandbox.logs_directory().starts_with(sandbox.root()));
    Ok(())
}

#[test]
fn prepared_command_binds_exact_subject_and_isolates_emacs_state()
-> Result<(), Box<dyn Error>> {
    let sandbox = EmacsHostSandbox::new()?;
    let subject = fixture_subject(&sandbox)?;
    let driver = sandbox.write_driver("bundled-eglot.el", "(message \"fixture\")")?;
    let prepared = sandbox.prepare_command(&subject, &driver)?;

    let args = prepared.command.get_args().collect::<Vec<_>>();
    assert_eq!(args.first().copied(), Some(OsStr::new("--quick")));
    assert!(args.contains(&OsStr::new("--batch")));
    assert!(args.contains(&OsStr::new("--load")));
    assert!(args.contains(&driver.as_os_str()));
    assert_eq!(prepared.command.get_current_dir(), Some(sandbox.project_root()));

    assert_eq!(
        command_env(&prepared.command, "PERL_LSP_BIN"),
        Some(Some(subject.perllsp_executable.as_os_str()))
    );
    assert_eq!(
        command_env(&prepared.command, "PERL_LSP_SHA256"),
        Some(Some(OsStr::new(&subject.perllsp_sha256)))
    );
    assert_eq!(
        command_env(&prepared.command, "PERL_LSP_CLIENT_KIND"),
        Some(Some(OsStr::new("bundled_eglot")))
    );
    assert_eq!(
        command_env(&prepared.command, "PERL_LSP_REGISTRATION_STATE"),
        Some(Some(OsStr::new("manual_client_registration")))
    );
    assert_eq!(
        command_env(&prepared.command, "PERL_LSP_PROJECT_ROOT_EVIDENCE_MODE"),
        Some(Some(OsStr::new("manually_bound_fixture")))
    );
    assert_eq!(command_env(&prepared.command, "EMACSLOADPATH"), Some(None));
    assert_eq!(command_env(&prepared.command, "PERL5LIB"), Some(None));

    assert!(prepared.emacs_stdout.starts_with(sandbox.logs_directory()));
    assert!(prepared.emacs_stderr.starts_with(sandbox.logs_directory()));
    assert!(prepared.server_stderr.starts_with(sandbox.logs_directory()));
    assert!(prepared.receipt_path.starts_with(sandbox.logs_directory()));
    Ok(())
}

#[test]
fn subject_validation_rejects_ambiguous_or_unbound_candidate_identity()
-> Result<(), Box<dyn Error>> {
    let sandbox = EmacsHostSandbox::new()?;
    let mut subject = fixture_subject(&sandbox)?;
    assert!(subject.validate().is_ok());

    subject.perllsp_executable = PathBuf::from("perllsp");
    assert!(subject.validate().is_err());

    subject = fixture_subject(&sandbox)?;
    subject.perllsp_sha256 = "ABC".to_string();
    assert!(subject.validate().is_err());

    subject = fixture_subject(&sandbox)?;
    subject.client_source.clear();
    assert!(subject.validate().is_err());
    Ok(())
}

#[test]
fn driver_paths_cannot_escape_the_isolated_profile() -> Result<(), Box<dyn Error>> {
    let sandbox = EmacsHostSandbox::new()?;

    assert!(sandbox.write_driver("../escape.el", "").is_err());
    assert!(sandbox.write_driver("./escape.el", "").is_err());
    assert!(sandbox.write_driver("escape.txt", "").is_err());
    assert!(sandbox.write_driver("clients/eglot.el", "(message \"ok\")").is_ok());
    Ok(())
}

#[test]
fn registration_and_project_evidence_states_are_not_interchangeable() {
    assert_ne!(
        RegistrationState::ManualClientRegistration.as_str(),
        RegistrationState::UpstreamBuiltinReleased.as_str()
    );
    assert_ne!(
        ProjectRootEvidenceMode::ManuallyBoundFixture.as_str(),
        ProjectRootEvidenceMode::StockProjectDiscovery.as_str()
    );
    assert_ne!(
        EmacsClientKind::BundledEglot.as_str(),
        EmacsClientKind::ExternalEglot.as_str()
    );
    assert_ne!(
        EmacsClientKind::ExternalEglot.as_str(),
        EmacsClientKind::LspMode.as_str()
    );
}
