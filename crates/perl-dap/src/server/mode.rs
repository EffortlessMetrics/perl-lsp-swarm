/// Debug adapter operating mode.
///
/// The shipped `perl-dap` CLI always selects [`DapMode::Native`]. The bridge
/// variant remains only for source compatibility with legacy library consumers
/// and conformance comparisons. Its implementation is default-off behind the
/// `legacy-pls-bridge` feature.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DapMode {
    /// Native adapter using `perl -d` directly.
    #[default]
    Native,
    /// Legacy compatibility bridge proxying to `Perl::LanguageServer`.
    #[doc(hidden)]
    #[deprecated(
        note = "legacy Perl::LanguageServer compatibility; use DapMode::Native instead"
    )]
    Bridge,
}
