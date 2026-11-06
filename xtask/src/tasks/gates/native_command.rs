//! Bounded argv launch for native gates. This is not a shell interpreter:
//! only argv, `env NAME=value` / `env -u NAME`, and `&&` are supported.
//! Command text uses POSIX quotes and double-quote escapes, not cmd quoting.
//! Unquoted backslashes are rejected: use forward slashes or single quotes for
//! native paths (including UNC paths and trailing separators).
//! Shell-dependent policy commands must use an explicit supported shell.
use std::path::Path;
use std::process::Command;

pub(super) const ROUTED_BINARY_ASSIGNMENT: &str = "PERL_LSP_BIN=\"$PWD/target/debug/perllsp\"";

pub(super) fn processes(command: &str, cwd: &Path) -> Result<Vec<Command>, String> {
    let segments = segments(command)?;
    let mut result = Vec::new();
    for segment in segments {
        let tokens = segment
            .iter()
            .map(|word| {
                let mut tokens = shlex::split(word).ok_or("invalid command quoting")?;
                if tokens.len() != 1 {
                    return Err("invalid argv word");
                }
                tokens.pop().ok_or("missing argv word")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut index = 0;
        let mut env = Vec::new();
        let mut assignments_seen = false;
        let mut binary_binding = None;
        if tokens.first().is_some_and(|token| token == "env") {
            index += 1;
            while let Some(token) = tokens.get(index) {
                if token == "-u" {
                    if assignments_seen {
                        return Err("env options must precede assignments".into());
                    }
                    index += 1;
                    let name = tokens.get(index).ok_or("env -u requires a name")?;
                    if !env_name(name) {
                        return Err("invalid env name".into());
                    }
                    env.push((name.clone(), None));
                } else if let Some((name, value)) = token.split_once('=') {
                    assignments_seen = true;
                    if !env_name(name) {
                        return Err("invalid env assignment".into());
                    }
                    if name == "PERL_LSP_BIN" && segment[index] == ROUTED_BINARY_ASSIGNMENT {
                        binary_binding = Some(env.len());
                    }
                    let value: std::ffi::OsString = value.into();
                    env.push((name.to_string(), Some(value)));
                } else {
                    if token.starts_with('-') {
                        return Err("unsupported env option".into());
                    }
                    break;
                }
                index += 1;
            }
        }
        if let Some(binding) = binary_binding {
            // Resolve against the final child-local target setting, including
            // an unset or relative override. No parent environment is changed.
            let target = env
                .iter()
                .rev()
                .find(|(name, _)| {
                    if cfg!(windows) {
                        name.eq_ignore_ascii_case("CARGO_TARGET_DIR")
                    } else {
                        name == "CARGO_TARGET_DIR"
                    }
                })
                .map_or_else(|| std::env::var_os("CARGO_TARGET_DIR"), |(_, value)| value.clone());
            if target.as_ref().is_some_and(|value| value.is_empty()) {
                return Err("empty Cargo target cannot bind the routed binary".into());
            }
            let target = target.map(std::path::PathBuf::from).unwrap_or_else(|| cwd.join("target"));
            let target = if target.is_absolute() { target } else { cwd.join(target) };
            let binary = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
            env[binding].1 = Some(target.join("debug").join(binary).into_os_string());
        }
        let program = tokens.get(index).filter(|s| !s.is_empty()).ok_or("missing executable")?;
        let extension = Path::new(program).extension().and_then(|extension| extension.to_str());
        if segment[index..].contains(&ROUTED_BINARY_ASSIGNMENT) {
            return Err("routed binary binding requires the env prefix".into());
        }
        if program.split_once('=').is_some_and(|(name, _)| env_name(name))
            || extension.is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            })
        {
            return Err(
                "shell assignment/batch launch is unsupported; use an explicit shell".into()
            );
        }
        // Preserve the existing exit-code test seam without arbitrary cmd text.
        let mut process = if cfg!(windows) && program == "exit" {
            let args = &tokens[index + 1..];
            if args.len() != 1 || args[0].parse::<i32>().is_err() {
                return Err("exit requires one integer status".into());
            }
            let mut process = Command::new("cmd");
            process.args(["/D", "/C", "exit", &args[0]]);
            process
        } else {
            let mut process = Command::new(program);
            process.args(&tokens[index + 1..]);
            process
        };
        for (name, value) in env {
            if let Some(value) = value {
                process.env(name, value);
            } else {
                process.env_remove(name);
            }
        }
        result.push(process);
    }
    Ok(result)
}

fn env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

// Locate the only supported operator while retaining quote provenance for
// shlex. Quoted metacharacters are argv bytes, never executable shell syntax.
fn segments(command: &str) -> Result<Vec<Vec<&str>>, String> {
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    let mut words = Vec::new();
    let mut result = Vec::new();
    let mut chars = command.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            if quote.is_none() {
                return Err(
                    "unquoted backslash is unsupported; use forward slashes or single-quote native paths"
                        .into(),
                );
            }
            escaped = true;
            continue;
        }
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            }
            continue;
        }
        if c == '$'
            && quote == Some('"')
            && command[..index].ends_with("PERL_LSP_BIN=\"")
            && command[index..].starts_with("$PWD/target/debug/perllsp\"")
        {
            continue;
        }
        if c == '$' || c == '`' {
            return Err(
                "unsupported shell expansion; pass child environment/argv explicitly".into()
            );
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            continue;
        }
        if c == '&' && chars.peek().is_some_and(|(_, next)| *next == '&') {
            chars.next();
            if start < index {
                words.push(&command[start..index]);
            }
            if words.is_empty() {
                return Err("missing command before &&".into());
            }
            result.push(std::mem::take(&mut words));
            start = index + 2;
        } else if ";&|<>()*?[]{}#\r\n".contains(c) {
            return Err("unsupported native shell syntax; use an explicit supported shell".into());
        } else if c.is_whitespace() {
            if start < index {
                words.push(&command[start..index]);
            }
            start = index + c.len_utf8();
        }
    }
    if quote.is_some() || escaped {
        return Err("invalid command quoting".into());
    }
    if start < command.len() {
        words.push(&command[start..]);
    }
    if words.is_empty() {
        return Err("missing executable".into());
    }
    result.push(words);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_unquoted_backslashes_refuse_without_corrupting_argv()
    -> Result<(), Box<dyn std::error::Error>> {
        let cwd = std::env::current_dir()?;
        for command in [
            r"cargo --manifest-path C:\repo\Cargo.toml",
            r"target\debug\tool.exe",
            r"env ROOT=C:\repo cargo test",
            r"cargo build && cargo test --manifest-path C:\repo\Cargo.toml",
            r"cargo path\ with\ spaces",
            r"cargo escaped\&value",
        ] {
            let Err(error) = processes(command, &cwd) else {
                return Err(format!("raw backslash accepted: {command}").into());
            };
            assert!(error.contains("unquoted backslash"), "{command}: {error}");
        }
        Ok(())
    }

    #[test]
    fn native_quoted_windows_paths_and_encoded_argv_remain_literal()
    -> Result<(), Box<dyn std::error::Error>> {
        let cwd = std::env::current_dir()?;
        let commands = processes(
            r#"'C:\repo with spaces\tool.exe' --manifest-path 'C:\repo\Cargo.toml' '\\server\share\' "C:\repo\Cargo.toml" C:/repo/Cargo.toml"#,
            &cwd,
        )?;
        assert_eq!(commands[0].get_program(), r"C:\repo with spaces\tool.exe");
        assert_eq!(
            commands[0].get_args().collect::<Vec<_>>(),
            [
                "--manifest-path",
                r"C:\repo\Cargo.toml",
                r"\\server\share\",
                r"C:\repo\Cargo.toml",
                "C:/repo/Cargo.toml"
            ]
        );
        // The maintained fixture encoder is the lossless producer contract;
        // do not invent Windows-shell escaping for arbitrary command strings.
        for value in [r"C:\repo\Cargo.toml", r"\\server\share\", r"C:\O'Brien & space\"] {
            let encoded = shlex::try_quote(value)?;
            let commands = processes(&format!("cargo {encoded}"), &cwd)?;
            assert_eq!(commands[0].get_args().collect::<Vec<_>>(), [value]);
        }
        Ok(())
    }

    #[test]
    fn native_argv_and_environment_are_literal() -> Result<(), Box<dyn std::error::Error>> {
        let cwd = std::env::current_dir()?;
        let commands = processes(
            r#"env -u REMOVE_ME FIXTURE='a & b % ! $HOME' "C:\path with spaces\test.exe" --skip 'literal && value'"#,
            &cwd,
        )?;
        assert_eq!(commands.len(), 1);
        let command = &commands[0];
        assert_eq!(command.get_program(), r"C:\path with spaces\test.exe");
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["--skip", "literal && value"]);
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == "FIXTURE" && value == Some("a & b % ! $HOME".as_ref()))
        );
        assert!(command.get_envs().any(|(key, value)| key == "REMOVE_ME" && value.is_none()));
        Ok(())
    }

    #[test]
    fn native_policy_binding_uses_child_directory() -> Result<(), Box<dyn std::error::Error>> {
        let cwd = std::env::current_dir()?.join("space & percent% directory");
        let commands = processes(
            "cargo build -p perllsp --locked && env PERL_LSP_BIN=\"$PWD/target/debug/perllsp\" cargo test --locked --tests -p tiny",
            &cwd,
        )?;
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[1].get_program(), "cargo");
        assert_eq!(
            commands[1].get_args().collect::<Vec<_>>(),
            ["test", "--locked", "--tests", "-p", "tiny"]
        );
        let binary = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| cwd.join("target"));
        assert!(commands[1].get_envs().any(|(key, value)| key == "PERL_LSP_BIN"
            && value == Some(target.join("debug").join(binary).as_os_str())));
        assert!(!commands[0].get_envs().any(|(key, _)| key == "PERL_LSP_BIN"));
        Ok(())
    }

    #[test]
    fn native_binding_preserves_literal_provenance_and_effective_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let cwd = std::env::current_dir()?.join("child directory");
        let literal = processes(
            r#"env DUMMY='PERL_LSP_BIN="$PWD/target/debug/perllsp"' PERL_LSP_BIN='$PWD/target/debug/perllsp' cargo test"#,
            &cwd,
        )?;
        assert!(literal[0].get_envs().any(|(key, value)| key == "PERL_LSP_BIN"
            && value == Some("$PWD/target/debug/perllsp".as_ref())));
        for (command, target) in [
            (
                r#"env -u CARGO_TARGET_DIR PERL_LSP_BIN="$PWD/target/debug/perllsp" cargo test"#,
                cwd.join("target"),
            ),
            (
                r#"env PERL_LSP_BIN="$PWD/target/debug/perllsp" CARGO_TARGET_DIR='custom dir & percent%' cargo test"#,
                cwd.join("custom dir & percent%"),
            ),
        ] {
            let commands = processes(command, &cwd)?;
            let binary = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
            assert!(commands[0].get_envs().any(|(key, value)| key == "PERL_LSP_BIN"
                && value == Some(target.join("debug").join(binary).as_os_str())));
        }
        Ok(())
    }

    #[test]
    fn native_unsupported_syntax_refuses_before_any_launch()
    -> Result<(), Box<dyn std::error::Error>> {
        let cwd = std::env::current_dir()?;
        for command in [
            "env -S 'cargo test'",
            "env X=y -u X cargo test",
            "env BAD-NAME=x cargo test",
            "X=y cargo test",
            "cargo build; cargo test",
            "cargo test | tee log",
            "cargo build || cargo test",
            "cargo test > log",
            "cargo $UNKNOWN",
            "cargo $(echo test)",
            "cargo &&",
            "cargo && && cargo",
            "'unterminated",
            "tool.cmd arg",
            "env -u",
            "cargo PERL_LSP_BIN=\"$PWD/target/debug/perllsp\"",
        ] {
            assert!(processes(command, &cwd).is_err(), "accepted {command}");
        }
        Ok(())
    }
}
