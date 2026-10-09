//! What an LSP `exit` notification ends.
//!
//! LSP defines `exit` as the final teardown step of one client session, but
//! it does not define what owns the host process. That is a transport
//! decision: stdio has exactly one client, so that client's `exit` owns the
//! process; socket mode accepts independent sessions, so one connection's
//! `exit` must end only that connection (#17331).

/// What the LSP `exit` notification terminates for this server instance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ExitPolicy {
    /// `exit` terminates the process (stdio: the single client owns it).
    #[default]
    TerminateProcess,
    /// `exit` ends only the sending connection; the process keeps accepting
    /// and serving other sessions (socket: one of many concurrent clients).
    EndConnection,
}
