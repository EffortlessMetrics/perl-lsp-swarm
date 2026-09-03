use super::config::TcpAttachConfig;
use super::event::DapEvent;
use super::reader::{TcpOutputDropAccounting, spawn_reader};
use anyhow::{Context, Result};
use perl_lsp_rs_core::transport::framing::frame;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// Monotonic reader epoch: bumped on every disconnect so a reader parked
    /// in cancellation-aware admission for a stale connection retires instead
    /// of delivering stale events or clobbering a replacement connection's
    /// state (#9521).
    reader_epoch: Arc<AtomicU64>,
}

impl TcpAttachSession {
    /// Create a new TCP attach session
    pub fn new() -> Self {
        Self {
            stream: None,
            connected: Arc::new(Mutex::new(false)),
            event_sender: None,
            drop_accounting: Arc::new(TcpOutputDropAccounting::new()),
            reader_epoch: Arc::new(AtomicU64::new(0)),
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
    /// The reader epoch is bumped before the socket shuts down, so a reader
    /// parked in cancellation-aware admission for this connection retires
    /// instead of later delivering stale events or overwriting the shared
    /// connection state of a replacement connection (#9521).
    pub fn disconnect(&mut self) -> Result<()> {
        self.reader_epoch.fetch_add(1, Ordering::SeqCst);
        if let Some(stream) = self.stream.take() {
            stream.shutdown(std::net::Shutdown::Both)?;
            tracing::info!("Disconnected from Perl debugger");
        }
        self.set_connected(false);
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

        let reader_id = self.reader_epoch.load(Ordering::SeqCst);
        spawn_reader(
            stream,
            Arc::clone(&self.connected),
            self.event_sender.clone(),
            Arc::clone(&self.drop_accounting),
            Arc::clone(&self.reader_epoch),
            reader_id,
        );
        Ok(())
    }

    fn set_connected(&self, connected: bool) {
        *self.connected.lock().unwrap_or_else(|error| error.into_inner()) = connected;
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

    #[test]
    fn new_session_starts_disconnected() {
        let session = TcpAttachSession::new();
        assert!(!session.is_connected());
    }
}
