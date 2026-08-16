/// Debug adapter operating mode.
///
/// `perl-dap` currently exposes one product runtime: the native adapter over the
/// local Perl debugger. The enum remains as a typed configuration boundary so a
/// future independently reviewed backend mode does not require an unstructured
/// flag, but no alternate DAP server is selectable here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DapMode {
    /// Native adapter using the local Perl debugger runtime.
    #[default]
    Native,
}
