# Security Scan Report

**Generated:** 2026-06-08
**Scan Type:** Weekly Scheduled
**Repository:** EffortlessMetrics/perl-lsp-swarm
**Severity Threshold:** medium

## Executive Summary

| Severity | Count | Auto-fixed | Manual Required |
|----------|-------|------------|-----------------|
| CRITICAL | 0 | 0 | 0 |
| HIGH | 0 | 0 | 0 |
| MEDIUM | 1 | 0 | 1 |
| LOW | 3 | 0 | 0 |

**Total Findings:** 4
**Auto-fixed:** 0
**Manual Review Required:** 1

## Critical Findings

_No critical severity findings._

---

## High Findings

_No high severity findings._

---

## Medium Findings

### VULN-001: Path Traversal Risk in stream_parse_file

| Attribute | Value |
|-----------|-------|
| **Severity** | MEDIUM |
| **STRIDE Category** | Tampering |
| **CWE** | CWE-22 (Path Traversal) |
| **File** | `archive/crates/perl-ts-advanced-parsers/src/streaming_parser.rs:324` |
| **Status** | Manual fix required |

**Description:**
The `stream_parse_file` function accepts a user-controlled file path without sanitization. An attacker could potentially provide paths like `../../etc/passwd` to access files outside the intended directory.

**Evidence:**
```rust
pub fn stream_parse_file(path: &str) -> Result<Vec<ParseEvent>, std::io::Error> {
    let file = std::fs::File::open(path)?;
    // ...
}
```

**Recommended Fix:**
1. Validate the path is within an allowed directory using canonicalization
2. Reject paths containing `..` components
3. Use std::fs::canonicalize to resolve symlinks and verify containment
4. Consider using a sandboxed directory for file operations

**Validation Notes:**
- **Reachability:** LOW - This is legacy archive code with no active callers in the current codebase
- **Exploitability:** LOW - Would require explicit calling of this function with untrusted input
- **Impact:** Could allow reading arbitrary files the process has access to

---

## Low Findings

### VULN-002: Environment Variable Heuristic Values

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW |
| **STRIDE Category** | Information Disclosure |
| **CWE** | CWE-200 (Exposure of Sensitive Information) |
| **File** | `archive/crates/perl-ts-heredoc-analysis/src/dynamic_delimiter_recovery.rs:683` |
| **Status** | Informational |

**Description:**
When resolving environment variables, non-whitelisted variables return heuristic values (e.g., `http://localhost` for variables containing "url") instead of actual values or explicit errors. This could mask the fact that a variable wasn't properly resolved.

**Validation Notes:**
- **Exploitability:** FALSE - Returns safe heuristic values, not actual sensitive data
- **Reachability:** MEDIUM - Could affect parsing accuracy for scripts relying on custom env vars

---

### VULN-003: Mutex Lock Unwrap Potential Panic

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW |
| **STRIDE Category** | Denial of Service |
| **CWE** | CWE-835 (Loop with Unreachable Exit Condition) |
| **File** | `archive/crates/perl-ts-advanced-parsers/src/lsp_server.rs:176` |
| **Status** | Informational |

**Description:**
Using `lock().unwrap_or_else(|e| e.into_inner())` could cause a panic if a mutex is poisoned. However, this pattern is acceptable for internal server state where thread safety is managed.

**Validation Notes:**
- **Exploitability:** FALSE - Only panics on thread panic (poisoned mutex), not normal operation
- **Reachability:** LOW - Internal state management, not exposed to external input

---

### VULN-004: Unsafe FFI Block with Proper Documentation

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW |
| **STRIDE Category** | Elevation of Privilege |
| **CWE** | CWE-242 (Use of Innately Unsafe Function) |
| **File** | `archive/crates/tree-sitter-perl-rs/src/language_binding.rs:85` |
| **Status** | Informational |

**Description:**
An `unsafe{}` block is used for FFI calls to tree-sitter. The code properly documents the SAFETY requirements.

**Validation Notes:**
- **Exploitability:** FALSE - Properly documented SAFETY comments present
- **Reachability:** LOW - Standard tree-sitter FFI pattern, well-understood

---

## Appendix

### Threat Model
- **Version:** 2026-05-25
- **Location:** `.factory/threat-model.md`
- **Status:** Current (within 90-day refresh window)

### Scan Metadata
- **Commits Scanned:** 1
- **Files Scanned:** 16 Rust source files in `archive/` directory
- **Commit Analyzed:** `623c0ceb7` - "fix(ci): run coverage-lane integration tests single-threaded for determinism"
- **Scan Scope:** Last 7 days of commits

### Findings by Category
| Category | Count | Severity |
|----------|-------|----------|
| Path Traversal | 1 | MEDIUM |
| Information Disclosure | 1 | LOW |
| Denial of Service | 1 | LOW |
| Elevation of Privilege | 1 | LOW |

### Notes
- All findings are in `archive/` directory, which contains legacy code
- No production `crates/` source code was modified in this period
- The single commit in the scan period was a CI configuration change
- MEDIUM finding requires manual review due to low (but present) reachability in legacy code

---

## References
- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://docs.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
- [Rust Security Advisories](https://rustsec.org/)
