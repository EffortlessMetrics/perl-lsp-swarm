#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Cold,
    Warm,
}

impl Mode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum InputKind {
    Str,
    Bytes,
}

impl InputKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Str => "str",
            Self::Bytes => "bytes",
        }
    }
}

pub(crate) struct Config {
    pub(crate) file_path: String,
    pub(crate) mode: Mode,
    pub(crate) input: InputKind,
    pub(crate) iterations: u64,
}

pub(crate) struct BenchSummary {
    pub(crate) mode: Mode,
    pub(crate) input: InputKind,
    pub(crate) iterations: u64,
    pub(crate) total_us: u128,
    pub(crate) avg_us: u128,
    pub(crate) has_error: bool,
}
