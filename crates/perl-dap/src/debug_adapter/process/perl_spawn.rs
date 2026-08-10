//! Perl executable validation and spawn-error formatting for process launch.

use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

/// Strict regex for valid Perl interpreter base names.
///
/// Admits `perl`, `perl5`, `perl5.38`, `perl5.38.2` while rejecting
/// `perlevil`, `perlscript`, `perl_backdoor`, etc.
///
/// This is a `LazyLock` regex initializer — an allowed exception per
/// the project coding standards.
static PERL_NAME_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^perl(\d+(\.\d+)*)?$"));

pub(super) fn is_valid_perl_interpreter(perl_interpreter: &str) -> bool {
    let trimmed = perl_interpreter.trim();
    if trimmed.is_empty() {
        return false;
    }

    let candidate = Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(trimmed)
        .to_ascii_lowercase();

    let candidate = candidate.strip_suffix(".exe").unwrap_or(&candidate);

    match &*PERL_NAME_RE {
        Ok(re) => re.is_match(candidate),
        Err(_) => {
            // Regex compilation failure — fall back to exact "perl" match only
            // (deny-by-default for versioned names when the validator is broken).
            candidate == "perl"
        }
    }
}

pub(super) fn format_perl_spawn_error(perl_interpreter: &str, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        #[cfg(windows)]
        {
            return format!(
                "Perl executable ('{perl_interpreter}') is not available on PATH. Install Perl from \
                    https://strawberryperl.com (or ActivePerl), then reload VS Code. \
                    You can also set launch.json `perlPath` to a full Perl path."
            );
        }
        #[cfg(not(windows))]
        {
            return format!(
                "Perl executable ('{perl_interpreter}') is not available on PATH. Install Perl with your package manager \
                    (for example `brew install perl`, `apt install perl`, or your distro equivalent), \
                    then reload VS Code. You can also set launch.json `perlPath` to a full Perl path."
            );
        }
    }

    format!(
        "Perl executable ('{perl_interpreter}') could not be started: {}. \
         Check file permissions, antivirus/AppLocker policy, and sandbox restrictions.",
        error
    )
}
