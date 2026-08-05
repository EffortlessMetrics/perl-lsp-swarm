//! Constants used by runtime input validation.

/// Maximum allowed path length.
pub(crate) const MAX_PATH_LENGTH: usize = 4096;

/// Maximum allowed URI length.
pub(crate) const MAX_URI_LENGTH: usize = 4096;

/// Maximum allowed JSON payload size for LSP params.
pub(crate) const MAX_PARAMS_SIZE: usize = 1_000_000;

/// Maximum allowed LSP method name length.
pub(crate) const MAX_METHOD_LENGTH: usize = 100;

/// Maximum allowed per-line text length.
pub(crate) const MAX_LINE_LENGTH: usize = 100_000;

/// Allowed file extensions for Perl files.
pub(crate) const ALLOWED_EXTENSIONS: &[&str] = &["pl", "pm", "t", "pod"];

/// Allowed URI schemes for text document synchronization.
pub(crate) const ALLOWED_TEXT_DOCUMENT_URI_SCHEMES: &[&str] =
    &["file://", "untitled:", "opencode:"];

/// Allowed execute-command entries.
pub(crate) const ALLOWED_COMMANDS: &[&str] = &[
    // Keep the advertised LSP executeCommand identifiers valid at the
    // preflight boundary.  Command-specific argument and capability checks
    // remain in the runtime/provider dispatchers.
    "perl.runTests",
    "perl.runFile",
    "perl.runScript",
    "perl.runTestSub",
    "perl.runCritic",
    "perl.runTest",
    "perl.runTestFile",
    "perl.runSubtest",
    "perl.debugFile",
    "perl.debugTest",
    "perl.debugTests",
    "perl.debugTestFile",
    "perl.goToTest",
    "perl.goToImplementation",
    "perl.explainProviderDecision",
    "perl.workspaceTrustReport",
    "perl.agentContext",
    "perl.previewSafeDelete",
    "perl.safeDeleteSymbol",
    "perl.previewPackageRename",
    "perl.explainMissingModuleLookup",
    // Client-owned command identifiers remain accepted for compatibility;
    // they are not advertised by the server capability list.
    "perl.formatDocument",
    "perl.extractVariable",
    "perl.extractSubroutine",
    "perl.optimizeImports",
];
