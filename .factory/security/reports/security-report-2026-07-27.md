# Security Scan Report

**Generated:** 2026-07-27
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

## Scan Details

### Commits Scanned (Last 7 Days)

| Commit | Author | Description |
|--------|--------|-------------|
| `4c7ace528` | Steven Zimmerman, CPA | fix(config): reject untrusted include_paths from .perl-lsp.toml (#4957) (#4991) |

### Analysis Summary

The most recent commit (`4c7ace528`) was a **security fix** that addressed path traversal vulnerabilities in the `.perl-lsp.toml` `include_paths` configuration. The fix:

1. **Problem Identified**: Workspace-supplied `.perl-lsp.toml` `include_paths` bypassed all path validation. Absolute entries were copied verbatim and relative entries were only checked deep in the resolver, silently.

2. **Vulnerability**: A hostile cloned repo could redirect module resolution outside the workspace (e.g., `include_paths = ["/etc"]` or `["../../../etc"]`), letting the LSP stat/read files outside the workspace boundary during go-to-definition/hover.

3. **Fix Applied**: `ProjectConfig::apply_to_workspace_config` now takes a `workspace_root` parameter and rejects:
   - Absolute entries
   - Relative entries that escape the workspace after normalization

4. **Return Value**: Returns `Vec<RejectedIncludePath>` so callers can surface actionable warnings (mirrors the `perlPath`/`perlArgs` precedent).

### Threat Model Status

- **Location:** `.factory/threat-model.md`
- **Version:** 2026-05-25
- **Last Modified:** 2026-06-01 (56 days ago)
- **Status:** Current (within 90-day threshold)

### Security Controls Verified

| Control | Location | Status |
|---------|----------|--------|
| Path Validation | `perl-parser-core/src/syntax/path_security.rs` | Active |
| DAP Expression Sanitization | `perl-dap/src/security/mod.rs` | Active |
| Transport Bounds | `perl-lsp-rs-core/src/transport/framing.rs` | Active (MAX_FRAME_SIZE=16MB) |
| File Content Limits | File reading operations | Active (1MB max content) |
| Command Allowlist | `get_supported_commands()` | Active |

## No New Vulnerabilities Found

The security scan of the last 7 days of commits found **no new security vulnerabilities** at or above the `medium` severity threshold.

The recent commit (`4c7ace528`) was a **proactive security fix** that:
- Rejected untrusted `include_paths` from `.perl-lsp.toml`
- Added workspace boundary validation for relative paths
- Implemented proper error reporting for rejected paths

This fix aligns with the threat model's P0 priorities (T019: Path traversal) and demonstrates active security posture.

## Appendix

### Threat Model Summary

**Key Attack Surfaces:**
| Surface | Components | Risk Level |
|---------|------------|------------|
| LSP Protocol | `perl-lsp-rs-core/src/transport/framing.rs` | High |
| File System | `perl-parser-core/src/syntax/path_security.rs` | Critical |
| DAP Protocol | `perl-dap/src/security/mod.rs` | High |
| Subprocesses | `perl-lsp-rs/src/security/sandbox.rs` | Critical |
| Workspace Index | Symbol resolution, file indexing | Medium |

**P0 Threats (Mitigated):**
- T004: Parser buffer overflow
- T014: Parser runaway loops
- T019: Path traversal (via `include_paths` fix)
- T022: Command injection

### Scan Metadata

- **Commits Scanned:** 1
- **Files Analyzed:** 10,044 (all workspace files)
- **Skills Used:** threat-model-generation (verification), commit-security-scan, vulnerability-validation
- **Scan Duration:** ~5 minutes

### References

- [STRIDE Threat Model](https://docs.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
- [CWE Database](https://cwe.mitre.org/)
- [Rust Security Guidelines](https://rust-sec.org/)
