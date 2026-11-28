use crate::common::{LspServer, PENDING_CAP, read_notification_for_uri};

#[cfg(test)]
mod read_notification_for_uri_tests {
    use super::{LspServer, PENDING_CAP, read_notification_for_uri};
    use serde_json::{Value, json};
    use std::collections::VecDeque;
    use std::io::BufWriter;
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

    /// Build an LspServer whose transport channels are synthetic: the child
    /// process is this test binary invoked with a filter that matches no
    /// test (the libtest harness exits immediately), so stdin/stdout exist
    /// but nothing is ever expected from them. Notifications to observe are
    /// preloaded into `rx` / `pending` directly.
    fn synthetic_server(
        rx_messages: Vec<Value>,
        pending_messages: Vec<Value>,
    ) -> TestResult<(LspServer, mpsc::Sender<Value>)> {
        let mut child: Child = Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("__no_such_synthetic_test__")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin: ChildStdin = child.stdin.take().ok_or("synthetic child must expose stdin")?;
        let (tx, rx) = mpsc::channel::<Value>();
        for message in rx_messages {
            tx.send(message)?;
        }
        let (_err_tx, err_rx) = mpsc::channel::<String>();
        let idle = || std::thread::spawn(|| ());
        let server = LspServer {
            process: Mutex::new(child),
            writer: Mutex::new(BufWriter::new(stdin)),
            rx: Mutex::new(rx),
            err_rx: Mutex::new(err_rx),
            _stdout_thread: idle(),
            _stderr_thread: idle(),
            pending: Mutex::new(pending_messages.into_iter().collect::<VecDeque<Value>>()),
            stderr_tail: Arc::new(Mutex::new((VecDeque::new(), Vec::new()))),
            shutdown_initiated: std::sync::atomic::AtomicBool::new(false),
        };
        Ok((server, tx))
    }

    fn ready(uri: &str) -> Value {
        json!({"jsonrpc": "2.0", "method": "perl-lsp/active-document-ready", "params": {"uri": uri, "generation": 1}})
    }

    /// The poll loop returns the first notification matching method AND
    /// params.uri, while every non-matching message it had to consume stays
    /// observable in `pending` for later readers.
    #[test]
    fn returns_matching_notification_and_requeues_the_rest() -> TestResult<()> {
        let response = json!({"jsonrpc": "2.0", "id": 7, "result": {}});
        let other_uri = ready("file:///workspace/Other.pl");
        let wanted = ready("file:///workspace/Wanted.pm");
        let (server, _tx) = synthetic_server(vec![response, other_uri, wanted], vec![])?;

        let found = read_notification_for_uri(
            &server,
            "perl-lsp/active-document-ready",
            "file:///workspace/Wanted.pm",
            Duration::from_secs(5),
        );

        let found = found.ok_or("matching notification should be found")?;
        assert_eq!(
            found.pointer("/params/uri").and_then(Value::as_str),
            Some("file:///workspace/Wanted.pm")
        );
        let pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            pending.iter().any(|message| message.get("id").is_some_and(|id| *id == json!(7))),
            "consumed non-matching response must be requeued for later readers"
        );
        assert!(
            pending.iter().any(|message| {
                message.pointer("/params/uri").and_then(Value::as_str)
                    == Some("file:///workspace/Other.pl")
            }),
            "consumed non-matching notification must be requeued for later readers"
        );

        Ok(())
    }

    /// Buffered matches are answered from `pending` without consuming `rx`.
    #[test]
    fn scans_buffered_pending_before_polling() -> TestResult<()> {
        let buffered = ready("file:///workspace/Buffered.pm");
        let (server, tx) = synthetic_server(vec![], vec![buffered])?;

        let found = read_notification_for_uri(
            &server,
            "perl-lsp/active-document-ready",
            "file:///workspace/Buffered.pm",
            Duration::from_secs(5),
        );

        assert!(found.is_some(), "buffered match should be returned from pending");
        // Nothing was pending in rx; the untouched sender must still deliver.
        tx.send(json!({"probe": true}))?;
        drop(tx);
        assert!(
            server.rx.lock().unwrap_or_else(|e| e.into_inner()).recv().is_ok(),
            "pending-first scan must not consume rx"
        );

        Ok(())
    }

    /// Requests carrying an id, wrong methods, or wrong URIs never match,
    /// and an empty transport yields None once the budget elapses.
    #[test]
    fn mismatched_shapes_never_match_and_empty_transport_times_out() -> TestResult<()> {
        let request_with_id = json!({"jsonrpc": "2.0", "id": 3, "method": "perl-lsp/active-document-ready", "params": {"uri": "file:///workspace/Wanted.pm"}});
        let wrong_method = json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": "file:///workspace/Wanted.pm"}});
        let (server, _tx) = synthetic_server(vec![request_with_id, wrong_method], vec![])?;

        let started = std::time::Instant::now();
        let found = read_notification_for_uri(
            &server,
            "perl-lsp/active-document-ready",
            "file:///workspace/Wanted.pm",
            Duration::from_millis(200),
        );

        assert!(found.is_none(), "no message may match by id-carrier, method, or uri alone");
        assert!(
            started.elapsed() >= Duration::from_millis(150),
            "the budget must be honored before returning None"
        );

        let _ = PENDING_CAP; // keep the cap constant linked to this module's contract
        Ok(())
    }
}
