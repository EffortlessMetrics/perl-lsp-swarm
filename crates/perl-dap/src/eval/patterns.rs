//! Dangerous operation patterns
//!
//! This module defines the patterns used to detect dangerous Perl operations
//! that should be blocked during safe expression evaluation.

use regex::Regex;
use std::sync::LazyLock;

/// List of dangerous Perl operations that can mutate state, perform I/O, or execute code
///
/// Categories:
/// - State mutation: push, pop, shift, unshift, splice, delete, undef, srand
/// - Process control: system, exec, fork, exit, dump, kill, alarm, sleep, wait, waitpid
/// - I/O: qx, readpipe, syscall, open, close, print, say, printf, etc.
/// - Filesystem: mkdir, rmdir, unlink, rename, chdir, chmod, chown, chroot, truncate
/// - Code loading: eval, require, do (file)
/// - Tie mechanism: can execute arbitrary code via FETCH/STORE
/// - Network: socket, connect, bind, listen, accept, send, recv, shutdown
/// - IPC: msg*, sem*, shm*
///
/// `each` is denied because it advances the hash iterator. `keys`/`values` also
/// reset that iterator (perlfunc). That reset is accepted: they return a complete
/// snapshot rather than walking the iterator, and Watch/repl cannot send
/// `allowSideEffects`. Do not add `keys` or `values` to this list (#18436 item 2).
pub const DANGEROUS_OPERATIONS: &[&str] = &[
    // State mutation
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "delete",
    "undef",
    "srand",
    "bless",
    "reset",
    "each",
    // Process control
    "system",
    "exec",
    "fork",
    "exit",
    "dump",
    "kill",
    "alarm",
    "sleep",
    "wait",
    "waitpid",
    "setpgrp",
    "setpriority",
    "umask",
    "lock",
    // I/O
    "qx",
    "readpipe",
    "syscall",
    "open",
    "close",
    "print",
    "say",
    "printf",
    "sysread",
    "syswrite",
    "glob",
    "readline",
    "ioctl",
    "fcntl",
    "flock",
    "select",
    "dbmopen",
    "dbmclose",
    "binmode",
    "opendir",
    "closedir",
    "readdir",
    "rewinddir",
    "seekdir",
    "telldir",
    "seek",
    "sysseek",
    "formline",
    "write",
    "pipe",
    "socketpair",
    // Filesystem
    "mkdir",
    "rmdir",
    "unlink",
    "rename",
    "chdir",
    "chmod",
    "chown",
    "chroot",
    "truncate",
    "utime",
    "symlink",
    "link",
    // Code loading/execution
    "eval",
    "require",
    "do",
    // Tie mechanism (can execute arbitrary code)
    "tie",
    "untie",
    // Network
    "socket",
    "connect",
    "bind",
    "listen",
    "accept",
    "send",
    "recv",
    "shutdown",
    "setsockopt",
    // IPC
    "msgget",
    "msgsnd",
    "msgrcv",
    "msgctl",
    "semget",
    "semop",
    "semctl",
    "shmget",
    "shmat",
    "shmdt",
    "shmctl",
];

/// Assignment operators that indicate mutation
pub const ASSIGNMENT_OPERATORS: &[&str] = &[
    "=", "+=", "-=", "*=", "/=", "%=", "**=", ".=", "&=", "|=", "^=", "<<=", ">>=", "&&=", "||=",
    "//=", "x=",
];

/// Compiled regex for dangerous operations
///
/// Pattern matches word boundaries around operation names.
pub static DANGEROUS_OPS_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    let pattern = format!(r"\b(?:{})\b", DANGEROUS_OPERATIONS.join("|"));
    Regex::new(&pattern)
});

/// Compiled regex for regex mutation operators (s///, tr///, y///)
///
/// Matches s, tr, y followed by a delimiter character (not alphanumeric/underscore/whitespace).
pub static REGEX_MUTATION_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    // Match s, tr, y followed by a delimiter character (not alphanumeric/underscore/whitespace)
    Regex::new(r"\b(?:s|tr|y)[^\w\s]")
});

/// Compiled regex to tokenize operator runs so assignment operators can be
/// identified without misclassifying comparisons (e.g. `==`, `!=`, `<=`, `>=`).
pub static ASSIGNMENT_OP_TOKENS_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"([!~^&|+\-*/%=<>]+)"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dangerous_ops_regex() {
        use perl_tdd_support::must;
        let re = must(DANGEROUS_OPS_RE.as_ref());

        // Should match dangerous ops
        assert!(re.is_match("system('ls')"));
        assert!(re.is_match("eval($code)"));
        assert!(re.is_match("print 'hello'"));

        // Should NOT match as standalone (would need full validator for context)
        // The regex just does raw matching - context is handled by validator
    }

    #[test]
    fn test_regex_mutation_regex() {
        use perl_tdd_support::must;
        let re = must(REGEX_MUTATION_RE.as_ref());

        // Should match s///, tr///, y///
        assert!(re.is_match("s/foo/bar/"));
        assert!(re.is_match("tr/a-z/A-Z/"));
        assert!(re.is_match("y/abc/xyz/"));
    }

    #[test]
    fn keys_and_values_are_admitted_each_remains_denied() {
        use perl_tdd_support::must;
        let re = must(DANGEROUS_OPS_RE.as_ref());
        let evaluator = crate::eval::SafeEvaluator::new();

        assert!(!DANGEROUS_OPERATIONS.contains(&"keys"));
        assert!(!DANGEROUS_OPERATIONS.contains(&"values"));
        assert!(DANGEROUS_OPERATIONS.contains(&"each"));

        for expr in ["keys %h", "values %$ref", "scalar keys %h"] {
            assert!(
                !re.is_match(expr),
                "read-only hash snapshot {expr:?} must not match the deny-list"
            );
            let result = evaluator.validate(expr);
            assert!(result.is_ok(), "SafeEvaluator must admit {expr:?}: {result:?}");
        }

        assert!(re.is_match("each %h"));
        assert!(
            evaluator.validate("each %h").is_err(),
            "each advances the iterator and must remain denied"
        );

        let compound = "keys %h; system('true')";
        assert!(
            re.is_match(compound),
            "compound expressions must still match remaining dangerous ops"
        );
        assert!(
            evaluator.validate(compound).is_err(),
            "SafeEvaluator must still refuse remaining dangerous ops in {compound:?}"
        );
    }
}
