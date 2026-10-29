# ============================================================================
# Security Scanning Commands (Issues #282, #15366)
# ============================================================================
# Typed, fail-closed local security surface. Every command records a per-tool
# outcome (clean | findings | instrument_failed | unavailable | not_run) into a
# receipt, never renders a missing instrument or skipped tool as success, and
# exits non-zero for any non-clean aggregate:
#
#   0 clean | 1 findings | 2 instrument_failed | 3 incomplete
#
# Quick start:
#   just security-scan      # Run all security checks (independent execution)
#   just security-quick     # Fast pre-commit check (cargo-audit denominator)
#   just security-report    # Full run + Markdown report in a fresh run directory
#   just security-verify target/security-reports/run-.../receipt.json
#
# Command contract: docs/SECURITY_SCANNING.md
# ============================================================================

repo_root := justfile_directory() / ".."
security_scan_script := repo_root / "scripts/security_scan.sh"

# Comprehensive security scan: every requested scanner runs independently even
# when an earlier one fails; missing tools are `unavailable`, not pass.
security-scan:
    @cd "{{repo_root}}" && bash "{{security_scan_script}}" scan

# Quick pre-commit check: cargo-audit only. Its narrower denominator is stated
# in the receipt; an unavailable scanner is not-proven, never success.
security-quick:
    @cd "{{repo_root}}" && bash "{{security_scan_script}}" quick

# Full scan plus Markdown report rendered from this run's typed receipt into a
# fresh private run directory (old artifacts can never satisfy a current run).
security-report:
    @cd "{{repo_root}}" && bash "{{security_scan_script}}" report

# Re-verify a prior receipt against its artifacts (existence, size, sha256).
security-verify receipt:
    @cd "{{repo_root}}" && bash "{{security_scan_script}}" verify "{{receipt}}"

# Cargo audit (strict mode; unavailable tool fails closed)
security-audit-strict:
    @echo "Running cargo-audit (strict mode)..."
    @if command -v cargo-audit >/dev/null 2>&1; then \
        cargo audit --deny warnings; \
    else \
        echo "UNAVAILABLE: cargo-audit not installed (fail-closed; install: cargo install cargo-audit --locked)"; \
        exit 3; \
    fi

# Cargo deny (policy enforcement; unavailable tool fails closed)
security-deny:
    @echo "Running cargo-deny policy checks..."
    @if command -v cargo-deny >/dev/null 2>&1; then \
        cargo deny --locked check advisories licenses bans sources; \
    else \
        echo "UNAVAILABLE: cargo-deny not installed (fail-closed; install: cargo install cargo-deny --locked)"; \
        exit 3; \
    fi

# Trivy vulnerability scanner (unavailable tool fails closed)
security-trivy:
    @echo "Running Trivy vulnerability scanner..."
    @if command -v trivy >/dev/null 2>&1; then \
        trivy fs --severity CRITICAL,HIGH,MEDIUM \
            --scanners vuln,config,secret \
            --skip-dirs target,archive,.runs,.git \
            --ignore-unfixed \
            --exit-code 1 \
            .; \
    else \
        echo "UNAVAILABLE: trivy not installed (fail-closed; install: https://aquasecurity.github.io/trivy/latest/getting-started/installation/)"; \
        exit 3; \
    fi

# Trivy with SARIF output (unavailable tool fails closed)
security-trivy-sarif:
    @echo "Running Trivy with SARIF output..."
    @if command -v trivy >/dev/null 2>&1; then \
        trivy fs --severity CRITICAL,HIGH,MEDIUM \
            --scanners vuln,config,secret \
            --skip-dirs target,archive,.runs,.git \
            --ignore-unfixed \
            --format sarif \
            --output trivy-results.sarif \
            .; \
        echo "SARIF report saved to: trivy-results.sarif"; \
    else \
        echo "UNAVAILABLE: trivy not installed (fail-closed; no SARIF produced)"; \
        exit 3; \
    fi

# Trivy Docker image scan (unavailable tool fails closed)
security-trivy-docker image='perl-lsp:dev':
    @echo "Running Trivy Docker image scan..."
    @if command -v trivy >/dev/null 2>&1; then \
        trivy image --severity CRITICAL,HIGH,MEDIUM \
            --ignore-unfixed \
            {{image}}; \
    else \
        echo "UNAVAILABLE: trivy not installed (fail-closed)"; \
        exit 3; \
    fi
