//! Falsifiers for the typed, bounded session-warning dedup store (#9769).
//!
//! Store-level tests prove the reviewed bound semantics (hard per-family cap,
//! saturation that still emits, lifecycle clears that stay family-local, and
//! fingerprint-only retention). Server-level tests prove the migrated emit
//! paths keep their decision and wording policy: repeated subjects warn once,
//! genuinely different subjects still warn, and adversarial distinct values
//! cannot grow retained state.

use std::io::Write;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{Value, json};

use super::LspServer;
use super::session_warning_dedup::{
    PER_FAMILY_ENTRY_CAP, SessionWarningCode, SessionWarningDecision, SessionWarningFamily,
    SessionWarningIdentity, SessionWarningSubjectTag,
};

fn profile_identity(profile: &str) -> SessionWarningIdentity {
    SessionWarningIdentity::fingerprinted(
        SessionWarningCode::ClientSettingInvalidValue,
        SessionWarningSubjectTag::None,
        profile,
    )
}

fn note_critic(server: &LspServer, identity: SessionWarningIdentity) -> SessionWarningDecision {
    server.session_warning_dedup.note(SessionWarningFamily::Critic, identity)
}

// -------------------------------------------------------------------------
// Store-level bound and identity semantics
// -------------------------------------------------------------------------

#[test]
fn repeated_subject_within_bound_suppresses_after_first_emission() {
    let server = LspServer::new();
    let identity = profile_identity("/repo/.perlcriticrc");

    assert_eq!(note_critic(&server, identity), SessionWarningDecision::EmitFirst);
    assert_eq!(note_critic(&server, identity), SessionWarningDecision::Suppress);
    assert_eq!(note_critic(&server, identity), SessionWarningDecision::Suppress);

    let snapshot = server.session_warning_dedup_snapshot();
    assert_eq!(snapshot.critic.entries, 1);
    assert_eq!(snapshot.critic.inserted, 1);
    assert_eq!(snapshot.critic.suppressed, 2);
}

#[test]
fn genuinely_different_subjects_never_cross_suppress() {
    let server = LspServer::new();
    let first = profile_identity("/alpha/.perlcriticrc");
    let second = profile_identity("/beta/.perlcriticrc");

    assert_eq!(note_critic(&server, first), SessionWarningDecision::EmitFirst);
    assert_eq!(note_critic(&server, second), SessionWarningDecision::EmitFirst);
    assert_eq!(note_critic(&server, first), SessionWarningDecision::Suppress);
}

#[test]
fn many_distinct_subjects_stop_retaining_at_the_hard_bound() {
    let server = LspServer::new();
    let distinct = PER_FAMILY_ENTRY_CAP + 1_000;

    for index in 0..distinct {
        let identity = profile_identity(&format!("/adversarial/profile-{index}"));
        let expected = if index < PER_FAMILY_ENTRY_CAP {
            SessionWarningDecision::EmitFirst
        } else {
            SessionWarningDecision::EmitWithoutRetaining
        };
        assert_eq!(note_critic(&server, identity), expected, "subject {index}");
    }

    let snapshot = server.session_warning_dedup_snapshot();
    assert_eq!(snapshot.critic.entries, PER_FAMILY_ENTRY_CAP);
    assert_eq!(snapshot.critic.high_water_entries, PER_FAMILY_ENTRY_CAP);
    assert_eq!(
        snapshot.critic.emitted_without_retaining,
        u64::try_from(distinct - PER_FAMILY_ENTRY_CAP).unwrap_or(u64::MAX)
    );
    // The bound is enforced, not documented: retained growth stops exactly at
    // the cap no matter how many distinct subjects arrive.
    for index in distinct..(distinct + 500) {
        note_critic(&server, profile_identity(&format!("/adversarial/profile-{index}")));
    }
    assert_eq!(server.session_warning_dedup_snapshot().critic.entries, PER_FAMILY_ENTRY_CAP);
}

#[test]
fn saturation_still_emits_actionable_warnings() {
    let server = LspServer::new();
    for index in 0..PER_FAMILY_ENTRY_CAP {
        note_critic(&server, profile_identity(&format!("/pressure/p-{index}")));
    }
    // A saturated table must never silently swallow a new actionable warning.
    let fresh = profile_identity("/pressure/unseen");
    assert_eq!(note_critic(&server, fresh), SessionWarningDecision::EmitWithoutRetaining);
    assert_eq!(note_critic(&server, fresh), SessionWarningDecision::EmitWithoutRetaining);
}

#[test]
fn identity_footprint_is_fixed_size_regardless_of_subject() {
    // The byte bound rides on every identity being fixed-size: code + tag +
    // 64-bit fingerprint (16 bytes with alignment). A variable-length
    // identity would break the per-family byte budget.
    assert!(std::mem::size_of::<SessionWarningIdentity>() <= 16);
    let small = profile_identity("p");
    let large = profile_identity(&"\u{1F600}".repeat(100_000));
    assert_eq!(
        std::mem::size_of_val(&small),
        std::mem::size_of_val(&large),
        "identity footprint must not depend on the subject it summarizes"
    );
}

#[test]
fn large_unicode_value_is_bounded_and_never_retained_raw() {
    let server = LspServer::new();
    let huge = "\u{1F600}".repeat(100_000);
    let identity = SessionWarningIdentity::fingerprinted(
        SessionWarningCode::ClientSettingInvalidValue,
        SessionWarningSubjectTag::ClientCriticEngine,
        &huge,
    );
    assert_eq!(
        server.session_warning_dedup.note(SessionWarningFamily::ClientSetting, identity),
        SessionWarningDecision::EmitFirst
    );
    let snapshot = server.session_warning_dedup_snapshot();
    assert_eq!(snapshot.client_setting.entries, 1);
    // Only the fingerprint is retained: neither the identity nor any counter
    // surface can echo the raw payload back.
    let debugged = format!("{identity:?}");
    assert!(!debugged.contains('\u{1F600}'));
}

#[test]
fn no_secret_or_absolute_path_sentinel_appears_in_retained_identity() {
    const SECRET_SENTINEL: &str = "sk-SECRET-api-key-0123456789abcdef";
    const PATH_SENTINEL: &str = "C:\\Users\\private\\absolute\\perlcriticrc";

    let secret_identity = SessionWarningIdentity::fingerprinted(
        SessionWarningCode::AiBackendAuthFailure,
        SessionWarningSubjectTag::None,
        &format!("execution failed: authorization header {SECRET_SENTINEL}"),
    );
    let path_identity = profile_identity(PATH_SENTINEL);

    for rendered in [format!("{secret_identity:?}"), format!("{path_identity:?}")] {
        assert!(!rendered.contains(SECRET_SENTINEL), "raw secret leaked: {rendered}");
        assert!(!rendered.contains(PATH_SENTINEL), "absolute path leaked: {rendered}");
    }
}

#[test]
fn lifecycle_clear_releases_only_its_own_family() {
    let server = LspServer::new();
    note_critic(&server, profile_identity("/cfg/old-profile"));
    assert_eq!(
        server.session_warning_dedup.note_client_setting(
            "critic.engine",
            "string",
            "not-an-engine"
        ),
        SessionWarningDecision::EmitFirst
    );
    let auth = SessionWarningIdentity::subjectless(SessionWarningCode::AiBackendAuthFailure);
    assert_eq!(
        server.session_warning_dedup.note(SessionWarningFamily::AiBackend, auth),
        SessionWarningDecision::EmitFirst
    );

    // Critic configuration movement clears only the critic family (#9769).
    server.session_warning_dedup.clear_family(SessionWarningFamily::Critic);

    let snapshot = server.session_warning_dedup_snapshot();
    assert_eq!(snapshot.critic.entries, 0);
    assert_eq!(snapshot.critic.cleared_by_lifecycle, 1);
    assert_eq!(snapshot.client_setting.entries, 1, "unrelated family must survive");
    assert_eq!(snapshot.client_setting.cleared_by_lifecycle, 0);
    assert_eq!(snapshot.ai_backend.entries, 1, "unrelated family must survive");
    assert_eq!(snapshot.ai_backend.cleared_by_lifecycle, 0);

    // The cleared family accepts the newly relevant warning again.
    assert_eq!(
        note_critic(&server, profile_identity("/cfg/old-profile")),
        SessionWarningDecision::EmitFirst
    );
}

#[test]
fn forget_releases_an_identity_so_a_failed_send_can_retry() {
    let server = LspServer::new();
    let auth = SessionWarningIdentity::subjectless(SessionWarningCode::AiBackendAuthFailure);
    assert_eq!(
        server.session_warning_dedup.note(SessionWarningFamily::AiBackend, auth),
        SessionWarningDecision::EmitFirst
    );
    server.session_warning_dedup.forget(SessionWarningFamily::AiBackend, &auth);
    assert_eq!(
        server.session_warning_dedup.note(SessionWarningFamily::AiBackend, auth),
        SessionWarningDecision::EmitFirst
    );
}

#[test]
fn guarded_emission_holds_the_reservation_through_the_send() {
    let server = LspServer::new();
    let auth = SessionWarningIdentity::subjectless(SessionWarningCode::AiBackendAuthFailure);

    // A failed send rolls the retention back inside the same lock hold, so a
    // concurrent caller that suppressed against the reservation is followed
    // by a retryable identity -- exactly the pre-#9769 atomicity.
    let failed =
        server
            .session_warning_dedup
            .emit_once_with(SessionWarningFamily::AiBackend, auth, || false);
    assert_eq!(failed, SessionWarningDecision::EmitFirst);
    assert_eq!(server.session_warning_dedup_snapshot().ai_backend.entries, 0);

    // A successful send keeps the reservation: later occurrences suppress.
    let delivered =
        server.session_warning_dedup.emit_once_with(SessionWarningFamily::AiBackend, auth, || true);
    assert_eq!(delivered, SessionWarningDecision::EmitFirst);
    assert_eq!(
        server.session_warning_dedup.note(SessionWarningFamily::AiBackend, auth),
        SessionWarningDecision::Suppress
    );
    assert_eq!(server.session_warning_dedup_snapshot().ai_backend.entries, 1);
}

/// Realistic wrong implementation: `note()` releases the family lock before
/// the send, so a concurrent caller can answer `Suppress` against a
/// reservation whose send then fails. Caller A blocks inside its send; while
/// that send is unresolved, a concurrent dedup query must not be answered at
/// all (the family lock is held through decide/send/rollback), and once A's
/// send has failed, the next query must emit again.
#[test]
fn concurrent_failure_window_never_suppresses_an_undelivered_warning()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    let server = Arc::new(LspServer::new());
    let auth = SessionWarningIdentity::subjectless(SessionWarningCode::AiBackendAuthFailure);

    let (entered_send, a_is_sending) = mpsc::channel::<()>();
    let (release_a, a_may_finish) = mpsc::channel::<()>();
    let server_a = Arc::clone(&server);
    let caller_a = std::thread::spawn(move || {
        server_a.session_warning_dedup.emit_once_with(SessionWarningFamily::AiBackend, auth, || {
            // A is now inside the critical section; hold the send open
            // until the test releases it, then report the send as failed.
            entered_send.send(()).ok();
            let _released = a_may_finish.recv();
            false
        })
    });

    a_is_sending.recv().map_err(|_| "caller A never entered its send")?;

    // B asks the store while A's send is unresolved.
    let (b_answer, b_answered) = mpsc::channel::<SessionWarningDecision>();
    let server_b = Arc::clone(&server);
    let caller_b = std::thread::spawn(move || {
        let decision = server_b.session_warning_dedup.note(SessionWarningFamily::AiBackend, auth);
        b_answer.send(decision).ok();
    });

    // With decide/send/rollback in one critical section, B is parked on the
    // family lock and cannot answer before A's send resolves. An immediate
    // `Suppress` here is exactly the lost-warning race.
    let premature = b_answered.recv_timeout(Duration::from_secs(2));
    if let Ok(answer) = premature {
        return Err(format!(
            "the store answered {answer:?} while the reservation's send was still unresolved"
        )
        .into());
    }

    // A's send fails; the rollback must make B's parked query emit again.
    release_a.send(()).map_err(|_| "caller A exited before release")?;
    let decision_a = caller_a.join().map_err(|_| "caller A panicked")?;
    caller_b.join().map_err(|_| "caller B panicked")?;
    let decision_b = b_answered
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "caller B never answered after the send resolved")?;

    assert_eq!(decision_a, SessionWarningDecision::EmitFirst);
    assert_eq!(
        decision_b,
        SessionWarningDecision::EmitFirst,
        "a concurrent caller must not suppress against a reservation whose send failed"
    );
    Ok(())
}

#[test]
fn client_setting_identity_is_setting_and_value_type_aware() {
    let server = LspServer::new();

    // Same setting + normalized value suppresses.
    assert_eq!(
        server.session_warning_dedup.note_client_setting(
            "critic.engine",
            "string",
            "not-an-engine"
        ),
        SessionWarningDecision::EmitFirst
    );
    assert_eq!(
        server.session_warning_dedup.note_client_setting(
            "critic.engine",
            "string",
            "not-an-engine"
        ),
        SessionWarningDecision::Suppress
    );

    // A different setting with an equivalent value stays distinct.
    assert_eq!(
        server.session_warning_dedup.note_client_setting(
            "formatting.engine",
            "string",
            "not-an-engine"
        ),
        SessionWarningDecision::EmitFirst
    );

    // The same setting with a different value type stays distinct: the JSON
    // type was part of the reviewed pre-#9769 identity too.
    assert_eq!(
        server.session_warning_dedup.note_client_setting(
            "critic.engine",
            "number",
            "not-an-engine"
        ),
        SessionWarningDecision::EmitFirst
    );

    // An unknown setting name cannot be represented by a bounded tag: the
    // warning still emits, but nothing is retained for it.
    assert_eq!(
        server.session_warning_dedup.note_client_setting(
            "critic.unknownKnob",
            "string",
            "some-value"
        ),
        SessionWarningDecision::EmitWithoutRetaining
    );

    let snapshot = server.session_warning_dedup_snapshot();
    assert_eq!(snapshot.client_setting.entries, 3);
    assert_eq!(snapshot.client_setting.emitted_without_retaining, 1);
}

#[test]
fn fresh_servers_start_with_zero_retained_warning_state() {
    // No global/static registry exists: every server session begins empty and
    // shutdown releases its store by drop.
    let first = LspServer::new();
    let second = LspServer::new();
    note_critic(&first, profile_identity("/only/first/server"));
    let first_snapshot = first.session_warning_dedup_snapshot();
    let second_snapshot = second.session_warning_dedup_snapshot();
    assert_eq!(first_snapshot.critic.entries, 1);
    assert_eq!(second_snapshot.critic.entries, 0);
    drop(first);
    // `second` is unaffected by the dropped server's retained identities.
    assert_eq!(second.session_warning_dedup_snapshot().critic.entries, 0);
}

// -------------------------------------------------------------------------
// Migrated emit paths (decision and wording policy unchanged)
// -------------------------------------------------------------------------

#[derive(Clone, Default)]
struct OutputCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl OutputCapture {
    fn messages(&self) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let bytes = self.buffer.lock().clone();
        let mut framer = perl_lsp_rs_core::transport::framing::ContentLengthFramer::new();
        framer.push(&bytes);
        let mut messages = Vec::new();
        while let Some(body) = framer.try_next()? {
            messages.push(serde_json::from_slice::<Value>(&body)?);
        }
        Ok(messages)
    }
}

impl Write for OutputCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn server_with_output_capture() -> (LspServer, OutputCapture) {
    let output = OutputCapture::default();
    let server = LspServer::with_output(Arc::new(Mutex::new(
        Box::new(output.clone()) as Box<dyn Write + Send>
    )));
    (server, output)
}

fn warning_texts(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| {
            message.get("method").and_then(Value::as_str) == Some("window/showMessage")
        })
        .filter_map(|message| {
            message.pointer("/params/message").and_then(Value::as_str).map(str::to_string)
        })
        .collect()
}

#[test]
fn repeated_ai_auth_failure_warns_once_until_configuration_moves()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output) = server_with_output_capture();

    server.notify_ai_auth_failure();
    server.notify_ai_auth_failure();
    server.notify_ai_auth_failure();

    // A configuration notification starts a new user-visible configuration
    // session: the old auth failure may surface feedback again.
    server.test_handle_did_change_configuration(Some(json!({
        "settings": { "perl": {} }
    })));
    server.notify_ai_auth_failure();

    drop(server);
    let texts = warning_texts(&output.messages()?);
    assert_eq!(texts.len(), 2, "auth failures must warn once per configuration session: {texts:?}");
    assert!(texts.iter().all(|text| text.contains("AI inline completion authentication failed")));
    Ok(())
}

#[test]
fn adversarial_distinct_client_values_still_warn_but_never_grow_retention()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output) = server_with_output_capture();

    let rounds = PER_FAMILY_ENTRY_CAP + 20;
    for index in 0..rounds {
        server.test_handle_did_change_configuration(Some(json!({
            "settings": {
                "perl": {
                    "critic": { "engine": format!("bogus-engine-{index}") }
                }
            }
        })));
    }
    // A repeated value inside the retained window is still suppressed.
    server.test_handle_did_change_configuration(Some(json!({
        "settings": {
            "perl": { "critic": { "engine": "bogus-engine-0" } }
        }
    })));

    let snapshot = server.session_warning_dedup_snapshot();
    drop(server);
    assert_eq!(snapshot.client_setting.entries, PER_FAMILY_ENTRY_CAP);
    assert_eq!(snapshot.client_setting.high_water_entries, PER_FAMILY_ENTRY_CAP);
    assert_eq!(snapshot.client_setting.suppressed, 1);
    assert_eq!(snapshot.client_setting.emitted_without_retaining, 20);

    // Every distinct invalid value still produced its actionable warning:
    // saturation never silently drops warnings.
    let texts = warning_texts(&output.messages()?);
    assert_eq!(texts.len(), rounds, "each distinct value must warn exactly once");
    Ok(())
}
