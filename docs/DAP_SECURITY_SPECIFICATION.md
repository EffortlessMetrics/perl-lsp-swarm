# DAP Security Specification
<!-- Labels: security:enterprise, validation:comprehensive, compliance:maintained -->

**Issue**: #207 - Debug Adapter Protocol Support
**Status**: Reconciled with implementation (2026-07-22)
**Version**: 0.9.x
**Date**: 2026-07-22

---

## Executive Summary

This specification documents the security measures implemented by the DAP
server, aligned with the existing enterprise security framework
(`docs/how-to/SECURITY_DEVELOPMENT_GUIDE.md`). Security measures are exercised
by the test suites listed under each section.

> **Scope note.** This document describes what the code **actually does today**,
> not aspirational guarantees. Where the implementation has a known gap, that
> gap is named explicitly rather than papered over. The direction of truth is
> always implementation → spec, never the reverse (see #4641).

**Key Security Domains**:
1. **Path Traversal Prevention**: Canonical path validation within workspace boundaries
2. **Safe Evaluation (admission control)**: Non-mutating eval default with explicit opt-in for side effects
3. **Timeout Enforcement**: Per-query timeouts for debugger I/O (the long-running debuggee is exempt — see §3)
4. **Unicode Boundary Safety**: Symmetric UTF-16 ↔ UTF-8 conversion (PR #153 infrastructure)
5. **Input Validation**: Expression policy validation, newline rejection, interpreter name validation, and shell-argument quoting (command injection prevention)

---

## 1. Path Traversal Prevention

### 1.1 Threat Model

**Attack Vector**: Malicious breakpoint or completion paths attempting directory traversal

**Examples**:
- `file:///workspace/../../../etc/passwd`
- `file:///workspace/lib/../../sensitive_data.pl`
- `\\server\share\..\..\..\etc\passwd` (Windows UNC)

**Impact**: Unauthorized file access, information disclosure

### 1.2 Defense Implementation

The canonical implementation lives in `perl-parser-core` and is re-exported
through a thin DAP wrapper so both LSP and DAP share one validation routine.

#### 1.2.1 Canonical Path Validation

**Core implementation**: `crates/perl-parser-core/src/syntax/path_security.rs`
— function `validate_workspace_path(path, workspace_root)`.

**DAP wrapper**: `crates/perl-dap/src/security/mod.rs` — function
`validate_path(path, workspace_root)`, which delegates to the core routine and
maps `WorkspacePathError` into the DAP `SecurityError` enum.

```rust
// crates/perl-dap/src/security/mod.rs
use perl_parser_core::path_security::{WorkspacePathError, validate_workspace_path};

/// Validate that a path is within the workspace boundary.
pub fn validate_path(path: &Path, workspace_root: &Path) -> Result<PathBuf, SecurityError> {
    validate_workspace_path(path, workspace_root).map_err(SecurityError::from)
}
```

The core routine (`validate_workspace_path`) does the following:

1. **Rejects null bytes and control characters** (tab is explicitly allowed) —
   `WorkspacePathError::InvalidPathCharacters`. This blocks the classic
   `"file\x00.pm"` truncation attack.
2. **Canonicalizes the workspace root** (`workspace_root.canonicalize()`).
3. **Resolves the candidate**: absolute paths are kept as-is; relative paths are
   joined to the workspace root.
4. **Existing paths are canonicalized directly.** If the canonical result does
   not start with the workspace root, a symlink-component scan distinguishes a
   symlink escape (`SymlinkOutsideWorkspace`) from a direct outside-workspace
   access (`PathOutsideWorkspace`).
5. **Non-existing paths are normalized** via
   `normalize_path_within_workspace`, which processes path components while
   preventing escape beyond workspace depth (`PathTraversalAttempt`).
6. **Final boundary check**: the resolved path must `starts_with` the canonical
   workspace root, otherwise `PathOutsideWorkspace` is returned.

```rust
// crates/perl-parser-core/src/syntax/path_security.rs
pub fn validate_workspace_path(
    path: &Path,
    workspace_root: &Path,
) -> Result<PathBuf, WorkspacePathError> {
    // Reject null bytes / control chars ...
    let workspace_canonical = normalize_filesystem_path(workspace_root.canonicalize()?);
    let resolved = if path.is_absolute() { path.to_path_buf() } else { workspace_root.join(path) };

    let final_path = if let Ok(canonical) = resolved.canonicalize() {
        let canonical = normalize_filesystem_path(canonical);
        if !canonical.starts_with(&workspace_canonical) {
            if path_has_symlink_component(&resolved) {
                return Err(WorkspacePathError::SymlinkOutsideWorkspace(/* … */));
            }
            return Err(WorkspacePathError::PathOutsideWorkspace(/* … */));
        }
        canonical
    } else {
        normalize_path_within_workspace(path, &workspace_canonical)?
    };

    if !final_path.starts_with(&workspace_canonical) {
        return Err(WorkspacePathError::PathOutsideWorkspace(/* … */));
    }
    Ok(final_path)
}
```

#### 1.2.2 Platform-Specific Behavior

Platform differences are handled **inside** the shared core routine rather than
via separate `#[cfg]` entry points:

**Windows** — `normalize_filesystem_path` strips the `\\?\` and `\\?\UNC\`
verbatim prefixes that `canonicalize()` produces on Windows, so the boundary
`starts_with` check compares against a normalized form. Mixed-separator
traversal (`..\..\windows\system32`) is caught at the completion-sanitization
layer (`sanitize_completion_path_input`), which rejects `..` components and
backslash traversal regardless of platform.

**Unix** — symlink escapes are detected by walking the path prefix-by-prefix
(`path_has_symlink_component`) and reporting `SymlinkOutsideWorkspace` when a
symlink resolves to a target outside the workspace root. Symlinks that stay
within the workspace are allowed.

There is **no** separate `validate_windows_path` / `validate_unix_path` entry
point; the single `validate_workspace_path` covers both platforms.

### 1.3 Test Coverage

Path-traversal tests live in `crates/perl-dap/tests/`:

| Test file | Coverage |
|-----------|----------|
| `security_path_traversal_tests.rs` | Parent-dir escape, mixed separators, absolute paths |
| `security_dap_path_traversal_hardened_tests.rs` | Null-byte injection, control chars, deep nesting |
| `security_dap_security_ac16_tests.rs` | AC16 path-validation scenarios |
| `security_regression_tests.rs` | Regression guards for prior traversal findings |
| `dap_security_tests.rs` / `dap_security_validation_tests.rs` | Broad security-validation coverage |

The core routine also has its own unit tests in
`crates/perl-parser-core/src/syntax/path_security.rs` (symlink escapes,
Windows reserved filenames, Unicode paths, null-byte/control-char injection,
very-long paths).

---

## 2. Safe Evaluation (Admission Control)

> **Important framing.** The safe-eval path is **admission control, not a sandboxed interpreter boundary**. This is the code's own characterization (see `crates/perl-dap/src/debug_adapter/safe_eval.rs` module doc and `crates/perl-dap/src/eval/validator.rs`). It validates an expression's *policy* before forwarding it to the Perl debugger for evaluation; it does **not** provide interpreter isolation, OS sandboxing, or a `Safe::Gem`-style compartment. A deployer who needs true isolation must run the debuggee in an external sandbox.

### 2.1 Threat Model

**Attack Vector**: Malicious evaluate requests with side effects

**Examples**:
- `$var = 42` (assignment without opt-in)
- `system("rm -rf /")` (command injection)
- `eval { require 'dangerous.pm' }` (code loading)

**Impact**: Unintended state modification, code injection, privilege escalation

### 2.2 Defense Implementation

Policy validation runs in two places, both consulted before an expression is
sent to the debugger:

1. **`crates/perl-dap/src/debug_adapter/safe_eval.rs`** —
   `validate_safe_expression(expression) -> Option<String>`, the validator used
   at the dispatch seam in `crates/perl-dap/src/debug_adapter/evaluation.rs`.
2. **`crates/perl-dap/src/eval/validator.rs`** — the `SafeEvaluator` microcrate
   type, re-invoked from the same handler to keep evaluation policy aligned with
   the shared DAP security logic.

When `allowSideEffects` is `false` (the default), the handler rejects the
request if **either** validator returns an error.

`allowSideEffects: true` is honored **only** for the explicit `repl` evaluation
context. In every other context — `watch`, `hover`, `variables`, an unrecognized
label, or an absent `context` field — the request is refused outright rather
than silently downgraded to a screened evaluation, so a client cannot believe it
received side-effect authority it never had.

That boundary exists because the read-oriented contexts are driven by the editor
rather than by a deliberate user action: a hover fires on mouse movement and
watch expressions re-evaluate on every stop. Only the `repl` context represents
a user typing an expression they intend to run.

The decision is taken before any debugger command is constructed, so a refusal
is never preceded by a debugger write. It is owned by
`crates/perl-dap/src/eval/trust.rs`, which keys off the typed
`EvaluateContext` produced by the single label-mapping authority
`EvaluateContext::from_dap_label` — the native adapter and the external-peer
bridges cannot disagree about what `"repl"` means. Label matching is exact, so a
near-miss such as `"REPL"` or `"repl-console"` carries no side-effect authority.

Trusted REPL execution is additionally gated on a process-owned
`ReplTrustPolicy`. It is deliberately not derived from project or workspace
configuration, so a checked-in project file cannot grant broader execution
authority to whoever opens the folder.

Admitting an expression through the REPL boundary is **not** sandboxing. The
expression runs with the debuggee's full authority. The guarantee is only that
side-effectful evaluation is confined to the one context where the user
explicitly asked for it.

#### 2.2.1 What the validators block

The admission-control checks (in `validate_safe_expression` /
`SafeEvaluator::validate`) reject the following when `allowSideEffects` is
false:

- **Assignment operators** (`=`, `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `.=`,
  `&=`, `|=`, `^=`, `<<=`, `>>=`, `&&=`, `||=`, `//=`, `x=`), while
  **allowing** comparison operators (`==`, `!=`, `<=`, `>=`, `<=`, `=~`, `!~`).
- **Increment/decrement** (`++`, `--`).
- **Dangerous builtins** via a deny-list regex covering: process control
  (`system`, `exec`, `fork`, `exit`, `kill`, …), I/O (`print`, `say`, `open`,
  `close`, `readline`, …), filesystem (`mkdir`, `unlink`, `chroot`, …), code
  loading (`eval`, `require`, `do`), the tie mechanism (`tie`, `untie`),
  network (`socket`, `connect`, `bind`, …), and IPC (`msg*`, `sem*`, `shm*`).
  The full list is in `crates/perl-dap/src/eval/patterns.rs`
  (`DANGEROUS_OPERATIONS`).
- **`CORE::` / `CORE::GLOBAL::` qualified** forms of the above (explicitly
  blocked so a caller cannot bypass the deny-list via `CORE::system`).
- **Dynamic subroutine calls** `&{...}` (blocks `&{"sys"."tem"}("ls")`).
- **Glob / filehandle reads** (`<*...>` and leading `<`).
- **Backticks** (shell execution) — blocked unconditionally.
- **Regex mutation operators** (`s///`, `tr///`, `y///`).

Context-aware filters reduce false positives, allowing: sigil-prefixed
identifiers (`$print`, `@say`, `%exit`), simple braced scalars (`${print}`),
package-qualified names (`Foo::print`) unless `CORE::`, single-quoted string
literals, and escape sequences (`\s` in regex literals).

#### 2.2.2 Newline / carriage-return rejection (command injection)

Before the policy validators run, the evaluate handler in
`crates/perl-dap/src/debug_adapter/evaluation.rs` rejects any expression
containing `\n` or `\r` outright:

```rust
// crates/perl-dap/src/debug_adapter/evaluation.rs
// Security: Reject expressions with newlines to prevent command injection
if expression.contains('\n') || expression.contains('\r') {
    return /* … */ DapMessage::Response { message: "Expression cannot contain newlines" };
}
```

The same newline rejection is encoded in
`crates/perl-dap/src/security/mod.rs::validate_expression` and applied to
breakpoint **conditions** via `validate_condition`.

#### 2.2.3 Perl-side evaluation

The Perl shim evaluates the (already policy-validated) expression inside the
debugger context via `eval $expr`. There is **no** `Safe.pm` compartment; the
shim relies on the Rust-side admission control to screen the expression before
it reaches `eval`. This is explicitly documented in
`crates/perl-dap/tests/safe_eval_documentation_clarification_test.rs`.

### 2.3 Test Coverage

> Current behavior is policy validation plus timeout framing, **not** a
> sandboxed interpreter boundary. `allowSideEffects: true` skips the safe-mode
> validators and evaluates in the debugger context — and is honored only for the
> explicit `repl` context, having no effect on `watch`, `hover`, `variables`, an
> unrecognized label, or an absent context, which are refused instead.

Relevant test files in `crates/perl-dap/tests/`:

| Test file | Coverage |
|-----------|----------|
| `safe_evaluation_tests.rs` | Safe-eval policy: blocked vs. allowed expressions |
| `safe_eval_documentation_clarification_test.rs` | Asserts the "admission control" framing is accurate |
| `eval_safe_evaluator.rs` | `SafeEvaluator` microcrate unit tests |
| `security_evaluate_tests.rs` | Evaluate security scenarios |
| `eval_timeout_and_exception_tests.rs` | Evaluation timeout + exception behavior |
| `security_regression_tests.rs` | Regression guards |

The microcrate also has its own unit tests in
`crates/perl-dap/src/eval/validator.rs`.

---

## 3. Timeout Enforcement

### 3.1 What is enforced

There are two distinct timeout surfaces, and they must not be conflated:

**A. Per-query evaluation timeout (enforced).** Each `evaluate` request sent to
the Perl debugger is bounded by a per-query deadline. The handler computes the
budget via `DebugAdapter::debugger_timeout_budget_ms(5000)` — a 5-second default
that is inflated only when `cargo llvm-cov` profiling is active. If the framed
debugger output does not arrive within the budget, the request fails with an
`evaluate timed out after {timeout_ms}ms` message.

```rust
// crates/perl-dap/src/debug_adapter/evaluation.rs
// AC10.3: Get timeout configuration (5s default, 30s hard limit)
let timeout_ms = Self::debugger_timeout_budget_ms(5000) as u32;
```

**B. Timeout-value validation (enforced).** `crates/perl-dap/src/security/mod.rs`
caps any configured timeout at `MAX_TIMEOUT_MS = 300_000` (5 minutes) and clamps
zero to 1 ms:

```rust
// crates/perl-dap/src/security/mod.rs
pub const MAX_TIMEOUT_MS: u32 = 300_000;   // 5 minutes
pub const DEFAULT_TIMEOUT_MS: u32 = 5_000; // 5 seconds

pub fn validate_timeout(timeout_ms: u32) -> Result<u32, SecurityError> {
    if timeout_ms > MAX_TIMEOUT_MS {
        return Err(SecurityError::ExcessiveTimeout(timeout_ms));
    }
    Ok(timeout_ms.max(1))
}
```

This applies to timeouts supplied by the client (e.g. the TCP attach
`timeout`/`timeoutMs` field, validated in `AttachConfiguration::validate` and
capped at 5 minutes).

### 3.3 Debuggee Wall-Clock Timeout (#4640)

The `perl -d` debuggee process is a long-running subprocess that, unlike the
short-lived probes (version check, syntax check), has no built-in timeout. A
debuggee that hits an infinite loop, blocks on `<STDIN>`, deadlocks, or waits
on a network call stays alive forever, holding the adapter session open
indefinitely.

**Configuration**: The `debuggeeTimeoutSeconds` field in the launch
configuration controls the wall-clock timeout:

```json
// launch.json
{
  "type": "perl",
  "request": "launch",
  "program": "${workspaceFolder}/script.pl",
  "debuggeeTimeoutSeconds": 60
}
```

- **Default**: `0` (disabled). This preserves compatibility with legitimate
  long-running debug sessions (e.g. a server process paused at a breakpoint
  for minutes). Users who want timeout enforcement set a positive value.
- **When the timeout fires**: The adapter sends a `terminated` event with
  `reason: "debuggee_timeout"` to the client, then kills the debuggee process.
  The `TerminationState.emitted` flag ensures only one `terminated` event
  reaches the client even if the output reader concurrently observes EOF.
- **Generation-aware**: The watchdog checks the session generation before
  acting, so a replaced session (restart / relaunch) is not killed by a
  stale watchdog from the prior session.

**Implementation**: `DebugAdapter::start_debuggee_watchdog` spawns a watchdog
thread that sleeps for the configured duration, then checks whether the
debuggee process is still alive. If alive, it emits the `terminated` event and
calls `terminate_child_process` to kill the debuggee. The output reader
thread's subsequent EOF handling performs session-state cleanup.

### 3.4 Configuration that does exist

Launch/attach configuration types live in `crates/perl-dap/src/config/mod.rs`:

- `LaunchConfiguration` — `program`, `args`, `cwd`, `env`, `perl_path`,
  `include_paths`, `debuggeeTimeoutSeconds` (wall-clock timeout, default 0 =
  disabled — see §3.3).
- `AttachConfiguration` — `host`, `port`, `timeout_ms` (connection timeout,
  capped at 5 minutes), `stop_on_entry`.

There is no `DapConfig`/`DapSession` struct carrying per-operation timeouts;
that shape was aspirational and has been removed from this spec.

---

## 4. Unicode Boundary Safety

### 4.1 Threat Model

**Attack Vector**: UTF-16 boundary arithmetic overflow in variable rendering

**Example**: Truncating multi-byte emoji at surrogate pair boundary

**Impact**: Invalid UTF-8 output, potential crashes, information disclosure

### 4.2 Defense Implementation

#### 4.2.1 Symmetric Position Conversion (PR #153 Reuse)

```rust
// crates/perl-dap/src/variables/renderer.rs
use ropey::Rope;
use lsp_types::Position;

/// Render variable value with UTF-16 safe truncation (AC8, AC16)
pub fn render_variable_value(value: &str, rope: &Rope) -> String {
    // Truncate large values (1KB preview max)
    if value.len() > 1024 {
        let safe_truncate = ensure_utf16_safe_truncation(value, 1024);
        format!("{}…", safe_truncate)
    } else {
        value.to_string()
    }
}
```

The truncation helper backs up to a Rust `is_char_boundary` and additionally
checks for a trailing 4-byte (surrogate-pair) sequence so no surrogate pair is
split. Breakpoint/source positions reuse the shared LSP position mapper:

```rust
use perl_lsp::textdoc::{lsp_pos_to_byte, byte_to_lsp_pos, PosEnc};

pub fn dap_position_to_byte(rope: &Rope, line: u32, column: u32) -> Result<usize> {
    let pos = Position { line, character: column };
    lsp_pos_to_byte(rope, pos, PosEnc::Utf16)
}
```

**Implementation Notes**:
- UTF-16 safe truncation implemented directly in `perl-dap`
  (`crates/perl-dap/src/variables/renderer.rs`).
- Follows PR #153 symmetric conversion patterns for boundary validation.
- Prevents UTF-16 surrogate pair splitting during variable value truncation.
- Uses Rust's `is_char_boundary()` for UTF-8 correctness.

### 4.3 Test Coverage

Relevant test files in `crates/perl-dap/tests/`:

| Test file | Coverage |
|-----------|----------|
| `variable_rendering_tests.rs` | Value rendering + truncation |
| `variables_deep_truncation.rs` | Deep-structure truncation safety |
| `variables_dap_deep_structure_truncation.rs` | DAP-specific deep truncation |
| `dap_variable_reference_hardening_tests.rs` | Variable-reference hardening |

---

## 5. Input Validation

### 5.1 Expression validation

Expression input validation is split across two modules (there is **no**
`crates/perl-dap/src/eval/sanitizer.rs` — that path referenced by earlier drafts
does not exist):

- **`crates/perl-dap/src/security/mod.rs::validate_expression`** — rejects
  expressions containing `\n` or `\r` (protocol/command injection prevention).
  Applied to evaluate expressions and breakpoint conditions
  (`validate_condition`).
- **`crates/perl-dap/src/eval/validator.rs::SafeEvaluator::validate`** — the
  policy validator described in §2.2 (assignment ops, dangerous builtins,
  backticks, increment/decrement, regex mutation, newline rejection).

```rust
// crates/perl-dap/src/security/mod.rs
pub fn validate_expression(expression: &str) -> Result<(), SecurityError> {
    if expression.contains('\n') || expression.contains('\r') {
        return Err(SecurityError::InvalidExpression);
    }
    Ok(())
}
```

### 5.2 Interpreter name validation

Before spawning `perl -d`, the launch path validates the interpreter name in
`crates/perl-dap/src/debug_adapter/process/perl_spawn.rs`:

```rust
// crates/perl-dap/src/debug_adapter/process/perl_spawn.rs
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
    candidate == "perl" || candidate.starts_with("perl")
}
```

This guards against a launch config that points the interpreter at an
arbitrary executable.

### 5.3 Command-injection prevention (flag arguments and shell quoting)

Two layers prevent command injection into the spawned `perl` command line:

- **Flag-argument handling** in `crates/perl-dap/src/debug_adapter/process.rs`:
  script arguments are passed as discrete argv elements, not concatenated into
  a shell string. The code comments note this "prevents command injection via
  flag arguments (e.g., `-e malicious_code`)".
- **Platform-aware shell-argument quoting** in
  `crates/perl-dap/src/command_args/mod.rs::format_command_args`: arguments
  containing whitespace or quotes are wrapped/escaped per platform rules
  (double-quote escaping on Windows; single- or double-quote wrapping on Unix).

```rust
// crates/perl-dap/src/command_args/mod.rs
pub fn format_command_args(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|arg| {
            if arg.is_empty() || arg.chars().any(char::is_whitespace) {
                // platform-specific quoting/escaping …
            } else {
                arg.clone()
            }
        })
        .collect()
}
```

> **Known limitation (aspirational, not implemented).** Earlier drafts of this
> spec described a `sanitize_expression` routine in a non-existent
> `eval/sanitizer.rs` that enforced a 10 000-character length limit and
> balanced-delimiter validation. **Neither the file nor those checks exist in
> the implementation.** They are recorded here as a known gap, not a guarantee.

---

## 6. Security Audit Checklist

### 6.1 Pre-Release Validation

**Path Security**:
- [x] All file paths canonicalized before use
- [x] Path traversal attempts rejected with error
- [x] Symlink resolution within workspace boundaries (Unix)
- [x] Null-byte / control-character rejection
- [ ] UNC path validation (Windows) — partial: verbatim-prefix normalization
      exists; dedicated UNC traversal checks are not a separate entry point

**Evaluation Security (admission control)**:
- [x] Default safe evaluation mode (no side effects)
- [x] Explicit `allowSideEffects` opt-in required to skip validators
- [x] Per-query evaluation timeout (5 s default)
- [x] Dangerous function detection (system, exec, eval, …)
- [x] Newline / carriage-return rejection (command injection)
- [ ] Expression length limit — **not implemented** (see §5.1)

**Unicode Security**:
- [x] UTF-16 ↔ UTF-8 conversion symmetric (PR #153)
- [x] Emoji and multi-byte character truncation safe
- [x] No surrogate pair splitting

**DoS Prevention**:
- [x] Per-query evaluation timeout configurable
- [ ] **Debuggee wall-clock timeout — not enforced** (#4640, see §3.2)
- [ ] Recursion depth limits — not enforced as a configurable bound

### 6.2 Continuous Validation

Security tests are ordinary `cargo test` targets; there is **no** separate
"zero findings" CI gate. Run the security test set with:

```bash
cargo test -p perl-dap --test security_path_traversal_tests
cargo test -p perl-dap --test security_dap_path_traversal_hardened_tests
cargo test -p perl-dap --test security_evaluate_tests
cargo test -p perl-dap --test safe_evaluation_tests
cargo test -p perl-dap --test safe_eval_documentation_clarification_test
cargo test -p perl-dap --test eval_timeout_and_exception_tests
cargo test -p perl-dap --test security_regression_tests
```

**Dependency auditing**:

```bash
# Check for known vulnerabilities
cargo audit
```

---

## 7. Security Incident Response

### 7.1 Vulnerability Reporting

**Contact**: See `SECURITY.md`
**Response Time**: 72 hours
**Disclosure Timeline**: 90 days coordinated disclosure

### 7.2 Security Patch Process

1. **Triage**: Assess severity (CVSS score)
2. **Fix Development**: Implement patch with regression tests
3. **Validation**: Security team review + penetration testing
4. **Release**: Coordinated disclosure with CVE assignment
5. **Notification**: Security advisory via GitHub Security Advisories

---

## 8. Compliance Summary

### 8.1 Security Standards Alignment

**Enterprise Security Framework** (`docs/how-to/SECURITY_DEVELOPMENT_GUIDE.md`):
- Path traversal prevention (canonical path validation)
- UTF-16 position security (PR #153 symmetric conversion)
- LSP error recovery patterns (safe logging)
- Secure defaults (safe evaluation mode — admission control)

**OWASP Top 10 Coverage**:
- A01:2021 - Broken Access Control (path traversal prevention)
- A03:2021 - Injection (expression policy validation, newline rejection,
  shell-argument quoting, interpreter name validation)
- A04:2021 - Insecure Design (secure defaults, per-query timeout enforcement)

### 8.2 Test Coverage

| Security Domain | Test files |
|-----------------|------------|
| Path Traversal | `security_path_traversal_tests.rs`, `security_dap_path_traversal_hardened_tests.rs` |
| Safe Evaluation | `safe_evaluation_tests.rs`, `eval_safe_evaluator.rs`, `security_evaluate_tests.rs` |
| Timeout Enforcement | `eval_timeout_and_exception_tests.rs` |
| Unicode Safety | `variable_rendering_tests.rs`, `variables_deep_truncation.rs` |
| Command Injection | `command_args_integration_tests.rs`, `shell_integration_tests.rs` |

> **Coverage claims.** Earlier drafts of this spec asserted "100% test
> coverage" per domain and "zero security findings." Those claims were
> aspirational and are contradicted by the audit pass that filed #4637
> (command-injection vectors), #4638 (defense-in-depth gaps), #4639
> (Windows-broken cluster), and #4640 (missing debuggee timeout). They have been
> removed in favor of the factual test-file table above. Refer to the linked
> issues for the current known-findings inventory.

---

## 9. References

- [Security Development Guide](how-to/SECURITY_DEVELOPMENT_GUIDE.md): Enterprise security framework
- [Position Tracking Guide](reference/POSITION_TRACKING_GUIDE.md): UTF-16 ↔ UTF-8 conversion (PR #153)
- [DAP Implementation Specification](reference/DAP_IMPLEMENTATION_SPECIFICATION.md): Primary technical specification
- [DAP Protocol Schema](reference/DAP_PROTOCOL_SCHEMA.md): JSON-RPC message schemas
- [OWASP Top 10 2021](https://owasp.org/www-project-top-ten/)

### 9.1 Implementation file index

| Spec section | Implementation file |
|--------------|---------------------|
| §1 Path validation (core) | `crates/perl-parser-core/src/syntax/path_security.rs` |
| §1 Path validation (DAP wrapper) | `crates/perl-dap/src/security/mod.rs` |
| §2 Safe eval (dispatch seam) | `crates/perl-dap/src/debug_adapter/safe_eval.rs` |
| §2 Safe eval (microcrate) | `crates/perl-dap/src/eval/validator.rs` |
| §2 Dangerous-op patterns | `crates/perl-dap/src/eval/patterns.rs` |
| §2 Evaluate handler | `crates/perl-dap/src/debug_adapter/evaluation.rs` |
| §3 Timeout validation | `crates/perl-dap/src/security/mod.rs` |
| §3 Debuggee launch (no wall-clock timeout) | `crates/perl-dap/src/debug_adapter/process.rs` |
| §3 Config types | `crates/perl-dap/src/config/mod.rs` |
| §4 Unicode / rendering | `crates/perl-dap/src/variables/renderer.rs` |
| §5 Interpreter validation | `crates/perl-dap/src/debug_adapter/process/perl_spawn.rs` |
| §5 Shell-argument quoting | `crates/perl-dap/src/command_args/mod.rs` |
| §2/§5 Debug session struct | `crates/perl-dap/src/debug_adapter/session.rs` |

---

**End of DAP Security Specification**
