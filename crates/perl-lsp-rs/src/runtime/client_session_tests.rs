//! Red-first falsifiers for `ClientSession` (#8386, "C01").
//!
//! These prove the connection/session reset points directly: shutdown drain
//! is exactly-once, replacement cannot inherit prior identity, and the
//! session owner does not embed a second reverse-request registry.

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use perl_lsp_rs_core::governance::FeatureProfile;

    use super::super::client_session::{ClientSession, ShutdownAdmission};
    use super::super::types::{PendingWorkspaceConfigurationRequest, ServerRequestId};
    use crate::protocol::JsonRpcId;
    use std::sync::atomic::Ordering;

    fn session() -> ClientSession {
        ClientSession::new(FeatureProfile::current())
    }

    fn plant_connection_state(session: &ClientSession) -> u64 {
        session.initialize_requested.store(true, Ordering::Release);
        session.initialized.store(true, Ordering::Release);
        session.client_capabilities.lock().work_done_progress_support = true;
        session.client_supports_pull_diags.store(true, Ordering::Release);
        session.progress_tokens.lock().insert("progress-old".to_string());
        session
            .progress_token_to_request
            .lock()
            .insert("progress-old".to_string(), JsonRpcId::Integer(7));
        session.cancelled.lock().insert(JsonRpcId::Integer(7));
        session.pending_request_ids.lock().insert(JsonRpcId::Integer(7));
        session.pending_workspace_configuration_requests.lock().insert(
            ServerRequestId::for_test(3),
            PendingWorkspaceConfigurationRequest {
                folder_uris: vec!["file:///old".to_string()],
                includes_global_item: true,
                created_at: Instant::now(),
            },
        );
        *session.pending_startup_log.lock() = Some("startup".to_string());
        *session.trace_level.lock() = "verbose".to_string();
        session.generation()
    }

    fn session_handles_are_empty(session: &ClientSession) -> bool {
        session.progress_tokens.lock().is_empty()
            && session.progress_token_to_request.lock().is_empty()
            && session.cancelled.lock().is_empty()
            && session.pending_request_ids.lock().is_empty()
            && session.pending_workspace_configuration_requests.lock().is_empty()
            && session.pending_startup_log.lock().is_none()
            && !session.client_capabilities.lock().work_done_progress_support
            && session.text_sync_session.lock().is_none()
            && session.position_encoding_session_context.lock().is_none()
    }

    #[test]
    fn shutdown_drains_progress_and_pending_reverse_requests_exactly_once() {
        let session = session();
        let generation = plant_connection_state(&session);

        assert_eq!(session.begin_shutdown(), ShutdownAdmission::First);
        assert!(session_handles_are_empty(&session));
        assert!(
            session.client_supports_pull_diags.load(Ordering::Acquire),
            "shutdown must not invert a pull client into a push-diagnostics transport"
        );
        assert!(session.shutdown_received.load(Ordering::Acquire));
        assert!(
            !session.authorize_generation(generation),
            "pre-shutdown generation must not authorize after drain"
        );

        session.progress_tokens.lock().insert("late-progress".to_string());
        assert_eq!(session.begin_shutdown(), ShutdownAdmission::AlreadyShutdown);
        assert!(
            session.progress_tokens.lock().contains("late-progress"),
            "a second shutdown must not re-run drain (exactly-once admission)"
        );
    }

    #[test]
    fn replacement_connection_cannot_inherit_prior_capability_or_progress_identity() {
        let session = session();
        let old_generation = plant_connection_state(&session);

        session.replace_connection();

        assert!(session_handles_are_empty(&session));
        assert!(!session.client_supports_pull_diags.load(Ordering::Acquire));
        assert!(!session.initialize_requested.load(Ordering::Acquire));
        assert!(!session.initialized.load(Ordering::Acquire));
        assert!(!session.shutdown_received.load(Ordering::Acquire));
        assert_eq!(&*session.trace_level.lock(), "off");
        assert_eq!(session.next_request_id.load(Ordering::Acquire), 1);
        assert!(
            !session.authorize_generation(old_generation),
            "old session identity must not authorize the replacement connection"
        );
        let new_generation = session.generation();
        assert_ne!(new_generation, old_generation);
        assert!(session.authorize_generation(new_generation));

        plant_connection_state(&session);
        assert!(
            session.progress_tokens.lock().contains("progress-old"),
            "control: a replacement session can own its own progress token"
        );
        assert!(
            !session.authorize_generation(old_generation),
            "planting new state must not revive the retired generation"
        );
    }

    #[test]
    fn unobserved_and_retired_generations_cannot_authorize() {
        let session = session();
        assert!(!session.authorize_generation(0), "generation 0 is never a live identity");
        let live = session.generation();
        assert_eq!(live, 1);
        assert!(session.authorize_generation(live));

        session.begin_shutdown();
        assert!(
            !session.authorize_generation(live),
            "shutdown must retire the generation that authorized before drain"
        );
        assert!(session.authorize_generation(session.generation()));
    }

    #[test]
    fn shutdown_keeps_the_same_connection_terminal() {
        let session = session();
        plant_connection_state(&session);
        assert_eq!(session.begin_shutdown(), ShutdownAdmission::First);
        assert!(session.initialize_requested.load(Ordering::Acquire));
        assert!(session.initialized.load(Ordering::Acquire));
        assert!(session.shutdown_received.load(Ordering::Acquire));
        assert!(session_handles_are_empty(&session));
        assert!(
            session.client_supports_pull_diags.load(Ordering::Acquire),
            "a terminal connection keeps its negotiated pull-diagnostics transport"
        );
    }

    #[test]
    fn shutdown_does_not_invert_pull_diagnostics_into_push() {
        let session = session();
        plant_connection_state(&session);
        assert_eq!(session.begin_shutdown(), ShutdownAdmission::First);
        assert!(
            session.client_supports_pull_diags.load(Ordering::Acquire),
            "publish_diagnostics treats a false pull flag as permission to push"
        );
    }

    #[test]
    fn concurrent_shutdown_admits_exactly_one_drain() {
        use std::sync::Arc;
        use std::sync::mpsc;

        let session = Arc::new(session());
        plant_connection_state(&session);

        let racers = 8;
        let (tx, rx) = mpsc::channel();
        let mut joins = Vec::new();
        for _ in 0..racers {
            let session = Arc::clone(&session);
            let tx = tx.clone();
            joins.push(std::thread::spawn(move || {
                let _ = tx.send(session.begin_shutdown());
            }));
        }
        drop(tx);

        let mut first = 0;
        let mut already = 0;
        for admission in rx {
            match admission {
                ShutdownAdmission::First => first += 1,
                ShutdownAdmission::AlreadyShutdown => already += 1,
            }
        }
        for join in joins {
            let _ = join.join();
        }

        assert_eq!(first, 1, "exactly one racer may drain");
        assert_eq!(already, racers - 1, "every other racer must observe already-shutdown");
        assert!(session_handles_are_empty(&session));
        session.progress_tokens.lock().insert("late-progress".to_string());
        assert_eq!(session.begin_shutdown(), ShutdownAdmission::AlreadyShutdown);
        assert!(
            session.progress_tokens.lock().contains("late-progress"),
            "post-race shutdown must still refuse a second drain"
        );
    }

    fn code_without_doc_comments(source: &str) -> String {
        source
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !trimmed.starts_with("//")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn client_session_does_not_embed_a_second_reverse_request_registry() {
        let code = code_without_doc_comments(include_str!("client_session.rs"));
        assert!(
            !code.contains("ServerRequestRegistry"),
            "ClientSession must consume #7007 rather than embed a second registry type"
        );
        assert!(
            code.contains("pending_workspace_configuration_requests"),
            "the existing feature-specific pending table remains the live reverse-request map"
        );
    }
}
