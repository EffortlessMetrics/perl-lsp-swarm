//! Server-to-client refresh requests (LSP 3.16+).
//!
//! Each method checks the relevant client capability before sending.

use super::*;

#[allow(dead_code)]
impl LspServer {
    /// Allocate the next server-to-client request ID.
    ///
    /// Guarantees a positive `i32` result. Normalises any non-positive stored
    /// value (e.g. due to a concurrent bug) back to `1`, and wraps from
    /// `i32::MAX` back to `1` so the counter never overflows or returns zero.
    pub(crate) fn next_server_request_id(&self) -> ServerRequestId {
        loop {
            let current = self.next_request_id.load(Ordering::Relaxed);
            let emit = if current < 1 { 1 } else { current };
            let next = if emit == i32::MAX { 1 } else { emit + 1 };
            if self
                .next_request_id
                .compare_exchange(current, next, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                // `emit` is always >= 1 by the normalization above, so
                // ServerRequestId::new(emit) is always Some.  Fall back to
                // ServerRequestId::new(1) (always Some) on the impossible None branch.
                if let Some(id) = ServerRequestId::new(emit) {
                    return id;
                }
                if let Some(id) = ServerRequestId::new(1) {
                    return id;
                }
                // Defensive: loop and retry if somehow both are None (unreachable in practice).
            }
        }
    }

    /// Send a server-to-client request, allocate a typed ID, and return it.
    ///
    /// This is the canonical outbound request entry point. All server→client
    /// requests should go through this method so the allocator guarantee holds.
    pub(crate) fn send_request(&self, method: &str, params: Value) -> io::Result<ServerRequestId> {
        let request_id = self.next_server_request_id();
        self.outbound.send_request(request_id, method, params)?;
        Ok(request_id)
    }

    /// Request client to refresh code lenses (workspace/codeLens/refresh)
    pub fn request_code_lens_refresh(&self) -> io::Result<()> {
        if !self.client_capabilities.lock().code_lens_refresh_support {
            return Ok(());
        }
        let _ = self.send_request("workspace/codeLens/refresh", json!(null))?;
        tracing::debug!("Requested code lens refresh");
        Ok(())
    }

    /// Request client to refresh semantic tokens (workspace/semanticTokens/refresh)
    pub fn request_semantic_tokens_refresh(&self) -> io::Result<()> {
        if !self.client_capabilities.lock().semantic_tokens_refresh_support {
            return Ok(());
        }
        let _ = self.send_request("workspace/semanticTokens/refresh", json!(null))?;
        tracing::debug!("Requested semantic tokens refresh");
        Ok(())
    }

    /// Request client to refresh inlay hints (workspace/inlayHint/refresh)
    pub fn request_inlay_hint_refresh(&self) -> io::Result<()> {
        if !self.client_capabilities.lock().inlay_hint_refresh_support {
            return Ok(());
        }
        let _ = self.send_request("workspace/inlayHint/refresh", json!(null))?;
        tracing::debug!("Requested inlay hint refresh");
        Ok(())
    }

    /// Request client to refresh inline values (workspace/inlineValue/refresh)
    pub fn request_inline_value_refresh(&self) -> io::Result<()> {
        if !self.client_capabilities.lock().inline_value_refresh_support {
            return Ok(());
        }
        let _ = self.send_request("workspace/inlineValue/refresh", json!(null))?;
        tracing::debug!("Requested inline value refresh");
        Ok(())
    }

    /// Request client to refresh diagnostics (workspace/diagnostic/refresh)
    pub fn request_diagnostic_refresh(&self) -> io::Result<()> {
        if !self.client_capabilities.lock().diagnostic_refresh_support {
            return Ok(());
        }
        let _ = self.send_request("workspace/diagnostic/refresh", json!(null))?;
        tracing::debug!("Requested diagnostic refresh");
        Ok(())
    }

    /// Request client to refresh folding ranges (workspace/foldingRange/refresh)
    pub fn request_folding_range_refresh(&self) -> io::Result<()> {
        if !self.client_capabilities.lock().folding_range_refresh_support {
            return Ok(());
        }
        let _ = self.send_request("workspace/foldingRange/refresh", json!(null))?;
        tracing::debug!("Requested folding range refresh");
        Ok(())
    }

    /// Expose the raw `next_request_id` field for testing wrapping behavior.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub fn set_next_request_id_for_test(&self, value: i32) {
        self.next_request_id.store(value, Ordering::SeqCst);
    }
}
