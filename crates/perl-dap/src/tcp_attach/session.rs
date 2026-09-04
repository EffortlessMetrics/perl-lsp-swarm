use super::config::TcpAttachConfig;
use super::event::DapEvent;
use super::reader::{ReaderRetirement, TcpOutputDropAccounting, spawn_reader};
use anyhow::{Context, Result};
use perl_lsp_rs_core::transport::framing::frame;
use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

/// TCP attach session
///
/// Manages a TCP connection to a Perl debugger process.
pub struct TcpAttachSession {
    /// TCP stream to the debugger
    stream: Option<TcpStream>,
    /// Connection state
    connected: Arc<Mutex<bool>>,
    /// Event sender for DAP events
    ///
    /// Must come from a `sync_channel` (#9521): the fan-in queue into the
    /// forwarding thread is bounded, with output shed and state events applying
    /// backpressure under the reader's admission policy.
    event_sender: Option<SyncSender<DapEvent>>,
    /// Per-session accounting for output events shed under backpressure, so
    /// one session's losses never inflate another session's notices (#9521).
    drop_accounting: Arc<TcpOutputDropAccounting>,
    /// Reader retirement for this session: the epoch bump is serialized
    /// against admission attempts under a gate, so a reader parked in
    /// cancellation-aware admission for a stale connection retires instead of
    /// later delivering stale events or clobbering a replacement connection's
    /// state (#9521).
    reader_retirement: Arc<ReaderRetirement>,
}

impl TcpAttachSession {
    /// Create a new TCP attach session
    pub fn new() -> Self {
        Self {
            stream: None,
            connected: Arc::new(Mutex::new(false)),
            event_sender: None,
            drop_accounting: Arc::new(TcpOutputDropAccounting::new()),
            reader_retirement: Arc::new(ReaderRetirement::new()),
        }
    }

    /// Set the event sender
    pub fn set_event_sender(&mut self, sender: SyncSender<DapEvent>) {
        self.event_sender = Some(sender);
    }

    /// Connect to the debugger via TCP
    ///
    /// Uses the SSRF-approved addresses from `config.resolved_addrs` (populated
    /// by `validate()`) to prevent DNS-rebinding TOCTOU: the IP that was
    /// validated is the same IP that receives the connection (#5257).
    pub fn connect(&mut self, config: &mut TcpAttachConfig) -> Result<()> {
        config.validate()?;

        // Defense-in-depth: validate() should always populate resolved_addrs
        // on success, but guard against a future code path that might bypass it.
        if config.resolved_addrs.is_empty() {
            anyhow::bail!("No resolved addresses available after validation");
        }

        // DNS pinning: connect directly to the validated SocketAddrs instead of
        // re-resolving the host string. This closes the DNS-rebinding TOCTOU
        // window that would exist if we re-resolved at connect time.
        let timeout = config.timeout_duration();
        let mut last_err = None;
        for addr in &config.resolved_addrs {
            match TcpStream::connect_timeout(addr, timeout) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(timeout))?;
                    stream.set_write_timeout(Some(timeout))?;
                    self.stream = Some(stream);
                    self.set_connected(true);
                    // Each successful connection starts with fresh drop
                    // accounting: a replacement connection's notices must
                    // count only its own losses, never those inherited from
                    // the previous connection (#9521). Sharing is preserved
                    // within the connection (the retired reader keeps writing
                    // to the old handle, which decays unused).
                    self.drop_accounting = Arc::new(TcpOutputDropAccounting::new());
                    tracing::info!(address = %addr, "Successfully connected to Perl debugger");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(address = %addr, error = %e, "Failed to connect to resolved address");
                    last_err = Some(e);
                }
            }
        }
        let err = last_err.unwrap_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, "no addresses to connect")
        });
        anyhow::bail!("Failed to connect to any resolved address for '{}': {}", config.host, err);
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected.lock().map(|guard| *guard).unwrap_or(false)
    }

    /// Disconnect from the debugger
    ///
    /// The reader is retired before the socket shuts down, so a reader parked
    /// in cancellation-aware admission for this connection retires instead of
    /// later delivering stale events or overwriting the shared connection
    /// state of a replacement connection (#9521). The connected flag is
    /// cleared unconditionally after cleanup — a failed socket shutdown does
    /// not make the session live, and no retired reader remains authorized to
    /// clear the flag — while the shutdown error is still preserved and
    /// returned.
    pub fn disconnect(&mut self) -> Result<()> {
        self.reader_retirement.retire();
        let shutdown_result =
            self.stream.take().map(|stream| stream.shutdown(std::net::Shutdown::Both)).transpose();
        self.set_connected(false);
        shutdown_result?;
        tracing::info!("Disconnected from Perl debugger");
        Ok(())
    }

    /// Send a DAP message to the debugger
    pub fn send_message(&mut self, message: &str) -> Result<()> {
        let stream = self.stream.as_mut().context("Not connected to debugger")?;
        let framed = frame(message.as_bytes());
        stream.write_all(&framed).context("Failed to write to debugger")?;

        stream.flush().context("Failed to flush stream")?;
        Ok(())
    }

    /// Start reading messages from the debugger
    pub fn start_reader(&mut self) -> Result<()> {
        let stream = self
            .stream
            .as_ref()
            .context("No stream available")?
            .try_clone()
            .context("Failed to clone TCP stream for reader thread")?;

        spawn_reader(
            stream,
            Arc::clone(&self.connected),
            self.event_sender.clone(),
            Arc::clone(&self.drop_accounting),
            Arc::clone(&self.reader_retirement),
        );
        Ok(())
    }

    fn set_connected(&self, connected: bool) {
        *self.connected.lock().unwrap_or_else(|error| error.into_inner()) = connected;
    }

    /// Dropped-output total of the CURRENT connection's accounting
    /// (test instrumentation for the per-connection reset contract).
    #[cfg(test)]
    fn dropped_output_events_for_current_connection(&self) -> u64 {
        self.drop_accounting.dropped_output_events()
    }
}

impl Default for TcpAttachSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TcpAttachSession {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc::sync_channel;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn new_session_starts_disconnected() {
        let session = TcpAttachSession::new();
        assert!(!session.is_connected());
    }

    /// A reconnect is a new connection: its drop accounting starts at zero,
    /// and its first shed output counts only its own loss — never the
    /// previous connection's unreported drops (#9521 review).
    #[test]
    fn reconnect_starts_with_fresh_drop_accounting() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        // One accepted socket per connection; each writes two output frames,
        // so the capacity-1 fan-in queue sheds exactly one per connection.
        let frame_of = |seq: i64| -> Result<Vec<u8>, serde_json::Error> {
            Ok(perl_lsp_rs_core::transport::framing::frame(
                serde_json::to_vec(&serde_json::json!({
                    "type": "event",
                    "seq": seq,
                    "event": "output",
                    "body": { "category": "console", "output": format!("drop {seq}") }
                }))?
                .as_slice(),
            ))
        };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&frame_of(1)?);
        bytes.extend_from_slice(&frame_of(2)?);

        let server = thread::spawn(move || -> std::io::Result<()> {
            use std::io::Write;
            for _ in 0..2 {
                let (mut socket, _) = listener.accept()?;
                socket.write_all(&bytes)?;
                socket.flush()?;
            }
            Ok(())
        });

        let mut session = TcpAttachSession::new();
        let (event_tx, event_rx) = sync_channel::<DapEvent>(1);
        session.set_event_sender(event_tx);

        let mut config = TcpAttachConfig::new("127.0.0.1".to_string(), port).with_timeout(2000);
        session.connect(&mut config)?;
        session.start_reader()?;

        // Connection 1: capacity-1 queue takes the first output, sheds the
        // second once the reader admits it.
        let deadline = std::time::Instant::now() + Duration::from_millis(2000);
        while session.dropped_output_events_for_current_connection() == 0 {
            if std::time::Instant::now() > deadline {
                return Err("connection 1 never recorded its own drop".into());
            }
            thread::sleep(Duration::from_millis(10));
        }

        session.disconnect()?;
        session.connect(&mut config)?;
        assert_eq!(
            session.dropped_output_events_for_current_connection(),
            0,
            "a replacement connection must start with fresh drop accounting"
        );

        // Drain the previous connection's queued output so connection 2's
        // first frame is admitted and only its second is shed.
        loop {
            match event_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("the fan-in queue must stay alive for the replacement".into());
                }
            }
        }

        // Connection 2 sheds its own first output: the notice total counts
        // only this connection's loss.
        session.start_reader()?;
        let deadline = std::time::Instant::now() + Duration::from_millis(2000);
        while session.dropped_output_events_for_current_connection() == 0 {
            if std::time::Instant::now() > deadline {
                return Err("connection 2 never recorded its own drop".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            session.dropped_output_events_for_current_connection(),
            1,
            "connection 2's notice total must count only its own single loss"
        );

        let _ = session.disconnect();
        server.join().map_err(|e| format!("server thread failed: {e:?}"))??;
        Ok(())
    }
}
