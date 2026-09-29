//! Red-first falsifiers for `ClientSession` (#8386, "C01").
//!
//! These prove the connection/session reset points directly: shutdown drain
//! is exactly-once, replacement cannot inherit prior identity, and the
//! session owner does not embed a second reverse-request registry.

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use perl_lsp_rs_core::governance::FeatureProfile;

    use super::super::client_session::{ClientSession, ProgressTokenInstall, ShutdownAdmission};
    use super::super::types::{PendingWorkspaceConfigurationRequest, ServerRequestId};
    use crate::protocol::JsonRpcId;
    use std::sync::atomic::Ordering;

    type TestResult = Result<(), String>;

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
    fn shutdown_drains_progress_and_pending_reverse_requests_exactly_once() -> TestResult {
        let session = session();
        let generation = plant_connection_state(&session);

        if session.begin_shutdown() != ShutdownAdmission::First {
            return Err("first shutdown must admit drain".to_string());
        }
        if !session_handles_are_empty(&session) {
            return Err("first shutdown must drain every session-owned handle".to_string());
        }
        if !session.client_supports_pull_diags.load(Ordering::Acquire) {
            return Err("shutdown must not invert a pull client into a push-diagnostics transport"
                .to_string());
        }
        if !session.shutdown_received.load(Ordering::Acquire) {
            return Err("shutdown_received must stay set on the terminal connection".to_string());
        }
        if session.authorize_generation(generation) {
            return Err("pre-shutdown generation must not authorize after drain".to_string());
        }

        if session.install_progress_token("late-progress".to_string(), None)
            != ProgressTokenInstall::Shutdown
        {
            return Err("late install after drain must return Shutdown".to_string());
        }
        if !session.progress_tokens.lock().is_empty() {
            return Err(
                "a late producer must not retain session-owned progress after shutdown".to_string()
            );
        }
        if session.begin_shutdown() != ShutdownAdmission::AlreadyShutdown {
            return Err("second shutdown must observe AlreadyShutdown".to_string());
        }
        if !session.progress_tokens.lock().is_empty() {
            return Err(
                "a second shutdown must not re-run drain (exactly-once admission)".to_string()
            );
        }
        Ok(())
    }

    #[test]
    fn replacement_connection_cannot_inherit_prior_capability_or_progress_identity() -> TestResult {
        let session = session();
        let old_generation = plant_connection_state(&session);

        session.replace_connection();

        if !session_handles_are_empty(&session) {
            return Err("replacement must drain every session-owned handle".to_string());
        }
        if session.client_supports_pull_diags.load(Ordering::Acquire) {
            return Err("replacement must reset pull-diagnostics transport".to_string());
        }
        if session.initialize_requested.load(Ordering::Acquire) {
            return Err("replacement must clear initialize_requested".to_string());
        }
        if session.initialized.load(Ordering::Acquire) {
            return Err("replacement must clear initialized".to_string());
        }
        if session.shutdown_received.load(Ordering::Acquire) {
            return Err("replacement must clear shutdown_received".to_string());
        }
        if &*session.trace_level.lock() != "off" {
            return Err(format!(
                "replacement must restore default trace level, got {:?}",
                session.trace_level.lock()
            ));
        }
        if session.next_request_id.load(Ordering::Acquire) != 1 {
            return Err(format!(
                "replacement must reset next_request_id to 1, got {}",
                session.next_request_id.load(Ordering::Acquire)
            ));
        }
        if session.authorize_generation(old_generation) {
            return Err(
                "old session identity must not authorize the replacement connection".to_string()
            );
        }
        let new_generation = session.generation();
        if new_generation == old_generation {
            return Err("replacement must mint a new generation".to_string());
        }
        if !session.authorize_generation(new_generation) {
            return Err("replacement generation must authorize the new connection".to_string());
        }

        plant_connection_state(&session);
        if !session.progress_tokens.lock().contains("progress-old") {
            return Err("control: a replacement session can own its own progress token".to_string());
        }
        if session.authorize_generation(old_generation) {
            return Err("planting new state must not revive the retired generation".to_string());
        }
        Ok(())
    }

    #[test]
    fn unobserved_and_retired_generations_cannot_authorize() -> TestResult {
        let session = session();
        if session.authorize_generation(0) {
            return Err("generation 0 is never a live identity".to_string());
        }
        let live = session.generation();
        if live != 1 {
            return Err(format!("fresh session generation must start at 1, got {live}"));
        }
        if !session.authorize_generation(live) {
            return Err("live generation must authorize before shutdown".to_string());
        }

        session.begin_shutdown();
        if session.authorize_generation(live) {
            return Err(
                "shutdown must retire the generation that authorized before drain".to_string()
            );
        }
        if !session.authorize_generation(session.generation()) {
            return Err(
                "post-shutdown generation must authorize the terminal connection".to_string()
            );
        }
        Ok(())
    }

    #[test]
    fn shutdown_keeps_the_same_connection_terminal() -> TestResult {
        let session = session();
        plant_connection_state(&session);
        if session.begin_shutdown() != ShutdownAdmission::First {
            return Err("first shutdown must admit drain".to_string());
        }
        if !session.initialize_requested.load(Ordering::Acquire) {
            return Err("terminal connection must keep initialize_requested".to_string());
        }
        if !session.initialized.load(Ordering::Acquire) {
            return Err("terminal connection must keep initialized".to_string());
        }
        if !session.shutdown_received.load(Ordering::Acquire) {
            return Err("terminal connection must keep shutdown_received".to_string());
        }
        if !session_handles_are_empty(&session) {
            return Err("terminal connection must drain session-owned handles".to_string());
        }
        if !session.client_supports_pull_diags.load(Ordering::Acquire) {
            return Err(
                "a terminal connection keeps its negotiated pull-diagnostics transport".to_string()
            );
        }
        Ok(())
    }

    #[test]
    fn shutdown_does_not_invert_pull_diagnostics_into_push() -> TestResult {
        let session = session();
        plant_connection_state(&session);
        if session.begin_shutdown() != ShutdownAdmission::First {
            return Err("first shutdown must admit drain".to_string());
        }
        if !session.client_supports_pull_diags.load(Ordering::Acquire) {
            return Err(
                "publish_diagnostics treats a false pull flag as permission to push".to_string()
            );
        }
        Ok(())
    }

    #[test]
    fn concurrent_shutdown_admits_exactly_one_drain() -> TestResult {
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
            join.join().map_err(|_| "shutdown racer thread panicked".to_string())?;
        }

        if first != 1 {
            return Err(format!("exactly one racer may drain, got {first}"));
        }
        if already != racers - 1 {
            return Err(format!("every other racer must observe already-shutdown, got {already}"));
        }
        if !session_handles_are_empty(&session) {
            return Err("concurrent shutdown must leave session-owned handles empty".to_string());
        }
        if session.install_progress_token("late-progress".to_string(), None)
            != ProgressTokenInstall::Shutdown
        {
            return Err("post-race late install must return Shutdown".to_string());
        }
        if session.begin_shutdown() != ShutdownAdmission::AlreadyShutdown {
            return Err("post-race shutdown must still observe AlreadyShutdown".to_string());
        }
        if !session.progress_tokens.lock().is_empty() {
            return Err(
                "post-race shutdown must still refuse a second drain and late install".to_string()
            );
        }
        Ok(())
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
    fn client_session_does_not_embed_a_second_reverse_request_registry() -> TestResult {
        let code = code_without_doc_comments(include_str!("client_session.rs"));
        if code.contains("ServerRequestRegistry") {
            return Err(
                "ClientSession must consume #7007 rather than embed a second registry type"
                    .to_string(),
            );
        }
        if !code.contains("pending_workspace_configuration_requests") {
            return Err(
                "the existing feature-specific pending table remains the live reverse-request map"
                    .to_string(),
            );
        }
        Ok(())
    }
}
