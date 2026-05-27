/// Which status subsystems to regenerate.
#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum StatusSubsystem {
    Lsp,
    Tests,
    Parser,
    Quality,
    /// DAP debugger scorecard (launch success, latency, test counts).
    Dap,
    Workspace,
}

impl StatusSubsystem {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            StatusSubsystem::Lsp => "lsp",
            StatusSubsystem::Tests => "tests",
            StatusSubsystem::Parser => "parser",
            StatusSubsystem::Quality => "quality",
            StatusSubsystem::Dap => "dap",
            StatusSubsystem::Workspace => "workspace",
        }
    }
}
