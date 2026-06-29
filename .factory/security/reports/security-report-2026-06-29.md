# Security Scan Report

**Generated:** 2026-06-29
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

No security vulnerabilities were identified at or above the medium severity threshold in the last 7 days of commits.

## Security Controls Verified

The following security controls were verified as part of this scan:

### Command Injection Prevention (STRIDE-Tampering, CWE-78)
- **Files:** `perl-dap/src/debug_adapter/process/perl_spawn.rs`, `perl-dap/src/debug_adapter/process.rs`
- **Status:** Verified
- **Detail:** The codebase uses `perl_subprocess_runtime::resolve_program()` to resolve bare program names to absolute paths before passing to `Command::new`, preventing Windows CWD-first search attacks (binary-planting RCE).

### Path Traversal Prevention (STRIDE-Tampering, CWE-22)
- **Files:** `perl-lsp-rs/src/util/`, `perl-dap/src/security/`
- **Status:** Verified
- **Detail:** `validate_workspace_path()` uses canonicalization with symlink detection. `resolve_path_from_args()` checks for `..` components and validates against workspace roots.

### DAP Expression Injection Prevention (STRIDE-Tampering)
- **Files:** `perl-dap/src/debug_adapter/safe_eval.rs`, `perl-dap/src/eval/validator.rs`, `perl-dap/src/eval/patterns.rs`
- **Status:** Verified
- **Detail:** Comprehensive blocking of dangerous Perl operations including newlines/carriage returns (command injection vector), backticks (shell execution), dangerous ops blocklist with regex patterns, and regex mutation operators (s///, tr///, y///).

### Sandbox Fail-Closed (STRIDE-Elevation of Privilege)
- **Files:** `perl-dap/src/security/sandbox.rs`
- **Status:** Verified
- **Detail:** Implements explicit fail-closed behavior when firejail is unavailable on Linux, and when Windows job objects are not yet implemented. Perl taint mode (-T) is enforced for script execution.

### Environment Isolation (STRIDE-Information Disclosure)
- **Files:** `perl-dap/src/debug_adapter/process/perl_info.rs`
- **Status:** Verified
- **Detail:** `PerlOracleEnv` denies ambient PERL5LIB/PERL5OPT by default. Bridge adapter has explicit env config with deny-by-default for PERL5LIB/PERL5OPT passthrough.

### Timeout Enforcement (STRIDE-Denial of Service)
- **Files:** `perl-dap/src/debug_adapter/process.rs`
- **Status:** Verified
- **Detail:** `MAX_TIMEOUT_MS = 300_000` (5 minutes) prevents resource exhaustion.

## Appendix

### Threat Model
- **Version:** 2026-06-01
- **Location:** .factory/threat-model.md
- **Status:** Current (within 90-day threshold)

### Scan Metadata
- **Commits Scanned:** 1 (a45478ad6 - docs(concepts): external-truth gate)
- **Files Analyzed:** Key security-sensitive files in perl-dap, perl-lsp-rs, perl-ci-hygiene, and xtask
- **Scan Duration:** ~5 minutes
- **Skills Used:** security-reviewer (STRIDE analysis, OWASP Top 10)

### References
- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://docs.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
