# Security Scan Report

**Generated:** 2026-07-13
**Scan Type:** Weekly Scheduled
**Repository:** EffortlessMetrics/perl-lsp-swarm
**Severity Threshold:** medium

## Executive Summary

| Severity | Count | Auto-fixed | Manual Required |
|----------|-------|------------|-----------------|
| CRITICAL | 0 | 0 | 0 |
| HIGH | 0 | 0 | 0 |
| MEDIUM | 0 | 0 | 0 |
| LOW | 0 | 0 | 0 |

**Total Findings:** 0
**Auto-fixed:** 0
**Manual Review Required:** 0

## Scan Results

No security vulnerabilities were identified at or above the MEDIUM severity threshold. The codebase demonstrates comprehensive security controls across all STRIDE threat categories.

## Security Controls Verified

The following security controls were verified during the scan:

### Path Traversal (T019, T021)
- **Location:** `crates/perl-parser-core/src/syntax/path_security.rs`
- **Status:** Implemented
- **Controls:** Canonicalization with symlink detection, null byte injection protection, control character filtering, Windows reserved filename checks, and 100-heredoc depth limits

### Command Injection (T022)
- **Location:** `crates/perl-lsp-rs/src/execute_command/provider.rs`
- **Status:** Implemented
- **Controls:** Resolves bare program names to absolute paths before `Command::new()`, preventing Windows CWD-first binary-planting attacks. Sandbox enforces firejail/sandbox-exec with Perl taint mode (-T)

### Parser Buffer/Memory (T004, T014)
- **Location:** `crates/perl-lsp-rs-core/src/transport/framing.rs`
- **Status:** Implemented
- **Controls:** MAX_FRAME_SIZE=16MB, MAX_HEADER_BYTES=4KB, MAX_DESYNC_BUFFER_BYTES=64KB limits prevent memory exhaustion. Heredoc parsing has depth limits (100) and timeout enforcement

### DAP Expression Injection (T007)
- **Location:** `crates/perl-dap/src/eval/validator.rs`
- **Status:** Implemented
- **Controls:** Blocks newlines/carriage returns in expressions, preventing protocol injection. Comprehensive dangerous operation blocklist (60+ ops) with sigil-prefixed identifier exceptions properly implemented

### ReDoS Prevention (T017)
- **Location:** Multiple regex usage sites
- **Status:** Implemented
- **Controls:** All regex patterns use LazyLock/OnceLock with graceful degradation (Option<&Regex>) when compilation fails

### Information Disclosure (T010, T012)
- **Location:** Multiple locations
- **Status:** Implemented
- **Controls:** URI scheme allowlisting, path redaction in diagnostics, and workspace boundary validation present

## Files Scanned

| File | Category |
|------|----------|
| crates/perl-parser-core/src/syntax/path_security.rs | Path Security |
| crates/perl-lsp-rs-core/src/transport/framing.rs | Transport |
| crates/perl-dap/src/security/mod.rs | DAP Security |
| crates/perl-lsp-rs/src/security/sandbox.rs | Subprocess Sandboxing |
| crates/perl-dap/src/eval/validator.rs | DAP Validation |
| crates/perl-dap/src/eval/patterns.rs | Pattern Matching |
| crates/perl-parser-core/src/syntax/path_normalize.rs | Path Normalization |
| crates/perl-parser-core/src/engine/parser/heredoc_security_tests.rs | Heredoc Security |
| crates/perl-lsp-rs-core/src/runtime/input_validation/mod.rs | Input Validation |
| crates/perl-lsp-rs/src/execute_command/provider.rs | Command Execution |
| crates/perl-dap/src/stack/parser.rs | DAP Stack |
| crates/perl-dap/src/variables/parser.rs | DAP Variables |
| crates/perl-dap/src/debug_adapter/patterns.rs | Debug Patterns |

## Appendix

### Threat Model
- **Version:** 2026-05-25
- **Location:** .factory/threat-model.md
- **Status:** Current (within 90-day refresh window)

### Scan Metadata
- **Commits Scanned:** 1
- **Scan Duration:** ~5 minutes
- **Skills Used:** threat-model-generation (for context), commit-security-scan, vulnerability-validation
- **Tool:** security-reviewer subagent

### Threat Model Coverage

| STRIDE Category | Threats Covered | Status |
|-----------------|-----------------|--------|
| Spoofing (S) | T001, T002, T003 | Verified |
| Tampering (T) | T004, T005, T006, T007 | Verified |
| Repudiation (R) | T008, T009 | Verified |
| Information Disclosure (I) | T010, T011, T012, T013 | Verified |
| Denial of Service (D) | T014, T015, T016, T017, T018 | Verified |
| Elevation of Privilege (E) | T019, T020, T021, T022, T023, T024 | Verified |

### Acknowledged Limitations

| ID | Limitation | Impact | Mitigation Status |
|----|------------|--------|-------------------|
| T021 | sandbox_escape_path does not escape paths with literal newlines/null bytes | Low | Documented as out-of-scope for macOS HFS+ (pathological edge case) |

### References
- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://docs.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
- [Rust Security Guidelines](https://rustsec.org/)
