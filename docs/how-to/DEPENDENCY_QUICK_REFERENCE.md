# Dependency Update Quick Reference

Quick commands and workflows for handling dependency updates.

## Quick Commands

### View Dependabot PRs

```bash
# All dependency PRs (labels are disabled in this repo; filter by author)
gh pr list --author "app/dependabot"

# CI-passing candidates (discovery only; inspect the version delta before merge)
gh pr list --author "app/dependabot" --search "status:success"

```

### Merge Dependabot PRs

```bash
# Auto-merge single PR (after CI passes)
gh pr merge <pr-number> --auto --squash

# Author + status is not a patch classifier. Do not pipe that query into
# auto-merge because it also selects minor and major updates.

# Manual merge with review
gh pr checkout <pr-number>
nix develop -c just ci-gate
gh pr merge <pr-number> --squash
```

### Check for Updates Manually

```bash
# Check outdated Cargo dependencies
cargo outdated --workspace

# Check for security vulnerabilities
cargo audit

# Check npm dependencies (VS Code extension)
cd vscode-extension && npm outdated

# Update Cargo.lock without changing versions
cargo update --workspace
```

### Dependency Tree Analysis

```bash
# Show full dependency tree
cargo tree

# Show only direct dependencies
cargo tree --depth 1

# Show duplicate dependencies
cargo tree -d

# Show dependencies of specific crate
cargo tree -p perl-parser
```

### Force Dependabot Actions

```bash
# Recreate PR (resolves conflicts)
gh pr comment <pr-number> -b "@dependabot recreate"

# Rebase PR
gh pr comment <pr-number> -b "@dependabot rebase"

# Merge PR (if allowed)
gh pr comment <pr-number> -b "@dependabot merge"

# Ignore dependency
gh pr comment <pr-number> -b "@dependabot ignore this dependency"

# Ignore major version
gh pr comment <pr-number> -b "@dependabot ignore this major version"
```

## Decision Matrix

| Update Type | Review Level | Auto-merge? | Testing Required |
|-------------|--------------|-------------|------------------|
| Security patch | High priority | After CI | Full suite |
| Patch (x.y.Z) | Quick review | Yes | CI gates |
| Minor (x.Y.0) | Standard review | No | Full suite + manual |
| Major (X.0.0) | Deep review | No | Comprehensive + benchmarks |

## Common Workflows

### Daily: Quick Check

```bash
# Check for new Dependabot PRs
gh pr list --author "app/dependabot"

# Fixes for Dependabot alerts arrive as ordinary Dependabot PRs; browse the
# alerts themselves under Security -> Dependabot alerts.
gh pr list --author "app/dependabot"
```

### Weekly: Batch Review (Monday)

```bash
# 1. List all new dependency PRs
gh pr list --author "app/dependabot" --json number,title

# 2. Inspect each candidate's version table and checks
gh pr view <pr-number>
gh pr checks <pr-number>

# 3. Enable auto-merge only for a reviewed patch or security PR
gh pr merge <pr-number> --auto --squash

# 4. Triage the rest by the highest semver impact in each PR body/version table:
#    x.y.Z -> patch, x.Y.0 -> minor (review changelog), X.0.0 -> major
#    (dedicated review)
```

### Monthly: Audit

```bash
# 1. Check for security advisories
cargo audit

# 2. Review ignored dependencies
cargo outdated --workspace

# 3. Check for unused dependencies
cargo machete

# 4. Verify license compliance
cargo deny check licenses
```

### Quarterly: Deep Dive

```bash
# 1. Evaluate major version updates
cargo outdated --workspace | grep -E "^[[:alnum:]].*->"

# 2. Benchmark impact
cd xtask && cargo run benchmark

# 3. Review dependency tree
cargo tree --depth 2 > deps.txt

# 4. Check for supply chain issues
cargo deny check sources
```

## Troubleshooting

### PR Has Conflicts

```bash
gh pr comment <pr-number> -b "@dependabot recreate"
```

### CI Failing

```bash
# Check locally
gh pr checkout <pr-number>
nix develop -c just ci-gate

# View logs
gh pr checks <pr-number>
```

### Multiple Conflicting Updates

```bash
# Check for conflicts
cargo tree -d

# Do not use Cargo's resolver to repair a Cargo.lock conflict.
# Preserve the accepted lock and route any required graph change
# through explicit dependency admission.

# Verify
cargo check --workspace
```

### Dependabot Stopped Working

```bash
# Check Dependabot status
gh api repos/:owner/:repo/dependabot/secrets

# Trigger manual check (via GitHub UI)
# Settings → Code Security → Dependabot → Check for updates
```

## Emergency Updates

### Critical Security Fix

```bash
# 1. Identify vulnerable dependency
cargo audit

# 2. Update immediately
cargo update -p <vulnerable-crate>

# 3. Run tests
cargo test --workspace

# 4. Commit and push
git add Cargo.lock
git commit -m "chore(deps): emergency update for CVE-XXXX-XXXXX"
git push
```

### Breaking Production

```bash
# 1. Identify problematic update
git log --oneline | grep "chore(deps)"

# 2. Revert merge commit
git revert <commit-hash>

# 3. Push revert
git push

# 4. Close Dependabot PR and ignore
gh pr close <pr-number>
gh pr comment <pr-number> -b "@dependabot ignore this dependency"
```

## Configuration Quick Reference

### Common `.github/dependabot.yml` Patterns

**Add dependency group**:
```yaml
groups:
  group-name:
    patterns:
      - "pattern*"
    update-types:
      - "minor"
      - "patch"
```

**Ignore dependency**:
```yaml
ignore:
  - dependency-name: "crate-name"
    update-types: ["version-update:semver-major"]
```

**Change schedule**:
```yaml
schedule:
  interval: "weekly"  # daily, weekly, monthly
  day: "monday"
  time: "09:00"
```

**Disable Dependabot labels**:
```yaml
# Disable all Dependabot labels, including GitHub's defaults.
labels: []
```

**Or apply custom labels**:
```yaml
# Each name must already exist in the repository; otherwise Dependabot
# ignores it and reports that it could not be found.
labels:
  - "custom-label"
```

## Resources

- Full Guide: [docs/how-to/DEPENDENCY_MANAGEMENT.md](DEPENDENCY_MANAGEMENT.md)
- Dependabot Config: [.github/dependabot.yml](../../.github/dependabot.yml)
- Dependabot Options: https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference
- Cargo Book: https://doc.rust-lang.org/cargo/
- Semantic Versioning: https://semver.org/

---

**Pro Tip**: Set up notifications for Dependabot PRs:
```bash
# Watch repository for security alerts
gh repo set-default
gh api --method PUT repos/:owner/:repo/subscription -f subscribed=true -f ignored=false
```
