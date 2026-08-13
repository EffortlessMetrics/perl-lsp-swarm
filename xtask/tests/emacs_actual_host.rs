use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use xtask::emacs_actual_host::{
    ActualHostRunPlan, BarrierLedger, CandidateIdentity, CleanupObservation, ClientIdentity,
    EmacsClientSubject, EmacsIdentity, HermeticEmacsRunner, HostBarrier, sha256_file,
    validate_cleanup,
};

fn failure<T>(result: anyhow::Result<T>) -> Result<String, Box<dyn Error>> {
    match result {
        Ok(_) => Err("operation unexpectedly succeeded".into()),
        Err(error) => Ok(error.to_string()),
    }
}

fn materialized_plan(subject: EmacsClientSubject) -> Result<(TempDir, ActualHostRunPlan), Box<dyn Error>> {
    let temp = tempfile::Builder::new().prefix("host-runner-plan-").tempdir()?;
    let emacs = temp.path().join(if cfg!(windows) { "emacs.exe" } else { "emacs" });
    let candidate = temp.path().join(if cfg!(windows) { "perllsp.exe" } else { "perllsp" });
    let driver = temp.path().join("driver.el");
    let loaded_file = temp.path().join("client.el");
    fs::write(&emacs, b"fake emacs subject")?;
    fs::write(&candidate, b"fake exact perllsp candidate")?;
    fs::write(&driver, b"(message \"driver\")\n")?;
    fs::write(&loaded_file, b";; exact client source\n")?;

    let source_ref = subject
        .required_emacs_ref()
        .map(str::to_owned)
        .unwrap_or_else(|| match subject.source_state() {
            "released" => format!("release-{}", subject.client_version()),
            _ => "0123456789abcdef0123456789abcdef01234567".to_owned(),
        });
    let package_sha256 = (subject.source_state() == "released").then(|| "package-digest".to_owned());

    let plan = ActualHostRunPlan {
        emacs: EmacsIdentity {
            executable: emacs,
            version: "30.1".into(),
            build_ref: "emacs-test-ref".into(),
            sha256: "emacs-digest".into(),
        },
        client: ClientIdentity {
            subject,
            source_ref,
            loaded_file: loaded_file.clone(),
            loaded_file_sha256: sha256_file(&loaded_file)?,
            package_sha256,
        },
        candidate: CandidateIdentity {
            path: candidate.clone(),
            version: "perllsp 0.18.0".into(),
            sha256: sha256_file(&candidate)?,
        },
        driver: driver.clone(),
        driver_sha256: sha256_file(&driver)?,
        fixture_identity: "fixture:project-root-matrix:v1".into(),
        journey: "manual-eglot".into(),
        platform: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        timeout_seconds: 45,
    };
    Ok((temp, plan))
}

#[test]
fn runner_declares_all_six_initial_client_subjects() {
    assert_eq!(EmacsClientSubject::ALL.len(), 6);
    assert_eq!(EmacsClientSubject::BundledEglotEmacs294.client_version(), "1.12.29");
    assert_eq!(EmacsClientSubject::BundledEglotEmacs301.client_version(), "1.17.30");
    assert_eq!(EmacsClientSubject::StandaloneEglot123.client_version(), "1.23");
    assert_eq!(EmacsClientSubject::StandaloneEglot124Source.client_version(), "1.24");
    assert_eq!(EmacsClientSubject::LspMode1000.client_version(), "10.0.0");
    assert_eq!(EmacsClientSubject::LspMode1001Source.client_version(), "10.0.1-dev");
}

#[test]
fn every_initial_subject_materializes_a_valid_isolated_plan() -> Result<(), Box<dyn Error>> {
    for subject in EmacsClientSubject::ALL {
        let (_temp, plan) = materialized_plan(subject)?;
        plan.validate()?;
    }
    Ok(())
}

#[test]
fn released_subject_cannot_fall_back_to_ambient_package_state() -> Result<(), Box<dyn Error>> {
    let (_temp, mut plan) = materialized_plan(EmacsClientSubject::LspMode1000)?;
    plan.client.package_sha256 = None;
    assert_eq!(
        failure(plan.validate())?,
        "released client subject requires an exact package digest"
    );
    Ok(())
}

#[test]
fn source_subject_cannot_float_at_head() -> Result<(), Box<dyn Error>> {
    let (_temp, mut plan) = materialized_plan(EmacsClientSubject::StandaloneEglot124Source)?;
    plan.client.source_ref = "HEAD".into();
    assert_eq!(
        failure(plan.validate())?,
        "upstream-source client subject requires an immutable commit/ref"
    );
    Ok(())
}

#[test]
fn candidate_hash_mismatch_fails_before_host_launch() -> Result<(), Box<dyn Error>> {
    let (_temp, mut plan) = materialized_plan(EmacsClientSubject::StandaloneEglot123)?;
    plan.candidate.sha256 = "wrong-digest".into();
    let error = failure(HermeticEmacsRunner::new(plan))?;
    assert!(error.starts_with("candidate digest mismatch:"));
    Ok(())
}

#[test]
fn command_uses_only_checked_driver_exact_subject_and_isolated_state() -> Result<(), Box<dyn Error>> {
    let (_temp, plan) = materialized_plan(EmacsClientSubject::BundledEglotEmacs301)?;
    let expected_candidate = plan.candidate.path.clone();
    let expected_driver = plan.driver.clone();
    let runner = HermeticEmacsRunner::new(plan)?;
    let command = runner.command()?;
    let args = command.get_args().collect::<Vec<_>>();

    assert!(args.contains(&OsStr::new("--batch")));
    assert!(args.contains(&OsStr::new("--quick")));
    assert!(args.contains(&OsStr::new("--no-site-file")));
    assert!(args.contains(&expected_driver.as_os_str()));

    let env_value = |name: &str| {
        command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new(name))
            .and_then(|(_, value)| value)
    };
    assert_eq!(env_value("PERLLSP_ACTUAL_HOST_SUBJECT"), Some(expected_candidate.as_os_str()));
    assert_eq!(env_value("HOME"), Some(runner.root().join("home").as_os_str()));
    assert_eq!(runner.isolated_paths().len(), 8);
    assert!(env_value("PATH").is_some());
    Ok(())
}

#[test]
fn readiness_barriers_are_monotonic_not_sleep_based() -> Result<(), Box<dyn Error>> {
    let mut ledger = BarrierLedger::default();
    ledger.record(HostBarrier::HostStarted)?;
    ledger.record(HostBarrier::ClientLoaded)?;
    ledger.record(HostBarrier::InitializeObserved)?;
    assert_eq!(ledger.last_completed(), Some(HostBarrier::InitializeObserved));
    assert!(failure(ledger.record(HostBarrier::ClientLoaded))?.contains("host barrier regression"));
    Ok(())
}

#[test]
fn cleanup_fails_closed_when_exact_candidate_survives() -> Result<(), Box<dyn Error>> {
    let observation = CleanupObservation {
        graceful_shutdown_completed: true,
        emacs_exited: true,
        candidate_exited: false,
        descendant_pids_remaining: Vec::new(),
        pending_host_actions: 0,
        pending_server_requests: 0,
        locked_test_artifacts: 0,
    };
    assert_eq!(failure(validate_cleanup(&observation))?, "exact candidate process remained alive");
    Ok(())
}

#[test]
fn runner_artifacts_stay_inside_run_root() -> Result<(), Box<dyn Error>> {
    let (_temp, plan) = materialized_plan(EmacsClientSubject::LspMode1001Source)?;
    let runner = HermeticEmacsRunner::new(plan)?;
    assert!(runner.artifacts().starts_with(runner.root()));
    assert_eq!(runner.artifacts().file_name(), Some(OsStr::new("artifacts")));
    assert!(Path::new(runner.artifacts()).is_dir());
    Ok(())
}
