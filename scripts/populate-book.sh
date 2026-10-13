#!/usr/bin/env bash
# Populate mdBook with existing documentation
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_BLOB_URL="https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main"
# CI's committed-copy freshness check (scripts/ci/validate_book_copy_freshness.sh)
# redirects generation into a scratch tree so it can diff committed copies
# against fresh output without mutating book/src.
BOOK_SRC="${PERL_LSP_BOOK_SRC:-$REPO_ROOT/book/src}"
# The freshness self-test can supply a disposable source tree to prove that
# absent canonical inputs fail the complete population/check path.
DOCS_DIR="${PERL_LSP_DOCS_DIR:-$REPO_ROOT/docs}"

echo "Populating mdBook with existing documentation..."

# Create directory structure
echo "Creating directory structure..."
mkdir -p "$BOOK_SRC/getting-started"
mkdir -p "$BOOK_SRC/user-guides"
mkdir -p "$BOOK_SRC/architecture"
mkdir -p "$BOOK_SRC/developer"
mkdir -p "$BOOK_SRC/lsp"
mkdir -p "$BOOK_SRC/advanced"
mkdir -p "$BOOK_SRC/reference"
mkdir -p "$BOOK_SRC/dap"
mkdir -p "$BOOK_SRC/ci"
mkdir -p "$BOOK_SRC/process"
mkdir -p "$BOOK_SRC/resources"

# Function to copy and adapt a doc file

# Apply sed edits through a temporary file so the helper works with both GNU and BSD sed.
sed_in_place() {
    local file="$1"
    shift
    local tmp="${file}.tmp"
    sed "$@" "$file" > "$tmp"
    mv "$tmp" "$file"
}

copy_doc() {
    local source="$1"
    local dest="$2"

    if [ -f "$source" ]; then
        echo "  Copying $(basename "$source") to $dest"
        cp "$source" "$dest"
    else
        echo "  Error: Source file not found: $source" >&2
        return 1
    fi
}

# Canonical docs are authored against the docs tree; once copied under
# book/src their docs-tree-relative links no longer resolve (the book has no
# EDITORS/, how-to/, crates/, ... directories). rewrite_book_links maps each
# such link family to the repository URL it canonically names. It also repairs
# the two families whose docs-tree targets are themselves wrong: `docs/.ci`
# and `docs/crates` do not exist — the real paths live at the repository root.
rewrite_book_links() {
    local file="$1"
    [ -f "$file" ] || return 0
    local repo="$REPO_BLOB_URL"
    sed_in_place "$file" \
        -e "s#](\.\./EDITORS/#](${repo}/docs/EDITORS/#g" \
        -e "s#](\.\./examples/#](${repo}/docs/examples/#g" \
        -e "s#](\.\./integrations/#](${repo}/docs/integrations/#g" \
        -e "s#](\.\./adr/#](${repo}/docs/adr/#g" \
        -e "s#](\.\./benchmarks/#](${repo}/docs/benchmarks/#g" \
        -e "s#](\.\./archive/#](${repo}/docs/archive/#g" \
        -e "s#](\.\./ci/#](${repo}/docs/ci/#g" \
        -e "s#](\.\./forensics/#](${repo}/docs/forensics/#g" \
        -e "s#](\.\./specs/#](${repo}/docs/specs/#g" \
        -e "s#](\.\./releases/#](${repo}/docs/releases/#g" \
        -e "s#](\.\./how-to/#](${repo}/docs/how-to/#g" \
        -e "s#](\.\./reference/#](${repo}/docs/reference/#g" \
        -e "s#](\.\./tutorials/#](${repo}/docs/tutorials/#g" \
        -e "s#](\.\./project/#](${repo}/docs/project/#g" \
        -e "s#](\.\./explanation/#](${repo}/docs/explanation/#g" \
        -e "s#](\.\./INDEX\.md#](${repo}/docs/INDEX.md#g" \
        -e "s#](\.\./\.ci/#](${repo}/.ci/#g" \
        -e "s#](\.\./crates/#](${repo}/crates/#g" \
        -e "s#](\.\./CLAUDE\.md#](${repo}/CLAUDE.md#g" \
        -e "s#](\.\./CONTRIBUTING\.md#](${repo}/CONTRIBUTING.md#g" \
        -e "s#](\.\./\.\./install\.sh#](${repo}/install.sh#g" \
        -e "s#](\.\./\.\./scripts/#](${repo}/scripts/#g" \
        -e "s#](\.\./\.\./CHANGELOG\.md#](${repo}/CHANGELOG.md#g" \
        -e "s#](\.\./\.\./Cargo\.toml#](${repo}/Cargo.toml#g" \
        -e "s#](\.\./\.\./\.perl-lsp\.toml\.example#](${repo}/.perl-lsp.toml.example#g" \
        -e "s#](\.\./\.\./CLAUDE\.md#](${repo}/CLAUDE.md#g" \
        -e "s#](\.\./\.\./CONTRIBUTING\.md#](${repo}/CONTRIBUTING.md#g" \
        -e "s#](\.\./\.\./AGENTS\.md#](${repo}/AGENTS.md#g" \
        -e "s#](\.\./\.\./templates/#](${repo}/templates/#g" \
        -e "s#](\.\./\.\./\.github/#](${repo}/.github/#g" \
        -e "s#](\.\./\.\./features\.toml#](${repo}/features.toml#g" \
        -e "s#](\.\./\.\./RELEASE_HISTORY\.md#](${repo}/RELEASE_HISTORY.md#g" \
        -e "s#](\.\./\.\./NOW_NEXT_LATER\.md#](${repo}/NOW_NEXT_LATER.md#g" \
        -e "s#](\.\./\.\./\.ci/#](${repo}/.ci/#g" \
        -e "s#](\.\./\.\./crates/#](${repo}/crates/#g" \
        -e "s#](\.\./\.\./docs/#](${repo}/docs/#g" \
        -e "s#](\.\./\.\./\.\./docs/#](${repo}/docs/#g"
}

# Bare docs-tree links (](CI.md), (adr/0002-...md), (./X)) resolve against the
# copied source's own docs directory, which does not exist under book/src.
# rewrite_bare_book_links maps them to the repository URL for that directory.
# The bare in-book chapter names produced by the family rewriters are temp-
# protected so they keep resolving inside the book.
rewrite_bare_book_links() {
    local file="$1"
    local base="$2"
    [ -f "$file" ] || return 0
    sed_in_place "$file" \
        -e 's#](editor-setup-canonical\.md#](:PROTECT_editor-setup-canonical.md#g' \
        -e 's#](configuration-canonical\.md#](:PROTECT_configuration-canonical.md#g' \
        -e 's#](native-critic-rule-matrix\.md#](:PROTECT_native-critic-rule-matrix.md#g' \
        -e 's#](performance-slo\.md#](:PROTECT_performance-slo.md#g' \
        -e 's#](configuration-schema\.md#](:PROTECT_configuration-schema.md#g' \
        -e "s#](\./#](${base}/#g" \
        -e "s#](\([^.):/#][^):]*\.md\)#](${base}/\1#g" \
        -e 's#](:PROTECT_#](#g'
}

# copy_doc plus docs-tree link rewriting for the book context.
copy_book_doc() {
    local source="$1"
    local dest="$2"

    copy_doc "$source" "$dest"
    rewrite_book_links "$dest"
    local srcdir="$(dirname "$source")"
    local reldir="${srcdir#"$DOCS_DIR"}"
    rewrite_bare_book_links "$dest" "${REPO_BLOB_URL}/docs${reldir}"
    python3 "$REPO_ROOT/scripts/ci/rewrite_book_source_links.py" "$source" "$dest" "$REPO_ROOT"
}

# Adapt source-relative links when canonical docs are copied into the book.
copy_lsp_doc() {
    local source="$1"
    local dest="$2"

    copy_doc "$source" "$dest"
    if [ -f "$dest" ]; then
        sed_in_place "$dest" \
            -e 's#../../CONTRIBUTING.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CONTRIBUTING.md#g' \
            -e 's#../reference/ARCHITECTURE.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/ARCHITECTURE.md#g' \
            -e 's#../reference/COMMANDS_REFERENCE.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/COMMANDS_REFERENCE.md#g' \
            -e 's#../../features.toml#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/features.toml#g' \
            -e 's#../project/CURRENT_STATUS.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/CURRENT_STATUS.md#g' \
            -e 's#../reference/LSP_FEATURES.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/LSP_FEATURES.md#g' \
            -e 's#../project/protocols/verification.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/protocols/verification.md#g'
    fi
    rewrite_book_links "$dest"
    rewrite_bare_book_links "$dest" "$REPO_BLOB_URL/docs/tutorials"
}

copy_development_doc() {
    local source="$1"
    local dest="$2"

    copy_doc "$source" "$dest"
    if [ -f "$dest" ]; then
        sed_in_place "$dest" \
            -e 's#../../CONTRIBUTING.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CONTRIBUTING.md#g' \
            -e 's#ORIENTATION.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/ORIENTATION.md#g' \
            -e 's#../reference/ARCHITECTURE.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/ARCHITECTURE.md#g' \
            -e 's#../reference/COMMANDS_REFERENCE.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/COMMANDS_REFERENCE.md#g' \
            -e 's#../tutorials/LSP_DEVELOPMENT_GUIDE.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/tutorials/LSP_DEVELOPMENT_GUIDE.md#g' \
            -e 's#CURRENT_STATUS.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/CURRENT_STATUS.md#g' \
            -e 's#](\.\.\/project\/ROADMAP\.md)#](https:\/\/github.com\/EffortlessMetrics\/perl-lsp-swarm\/blob\/main\/docs\/project\/ROADMAP.md)#g'
    fi
    rewrite_book_links "$dest"
    rewrite_bare_book_links "$dest" "$REPO_BLOB_URL/docs/project"
}

copy_testing_doc() {
    local source="$1"
    local dest="$2"

    copy_doc "$source" "$dest"
    if [ -f "$dest" ]; then
        sed_in_place "$dest" \
            -e 's#../../crates/perl-corpus/#https://github.com/EffortlessMetrics/perl-lsp-swarm/tree/main/crates/perl-corpus/#g' \
            -e 's#../reference/COMMANDS_REFERENCE.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/COMMANDS_REFERENCE.md#g' \
            -e 's#../../CONTRIBUTING.md#https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CONTRIBUTING.md#g'
    fi
    rewrite_book_links "$dest"
    rewrite_bare_book_links "$dest" "$REPO_BLOB_URL/docs/tutorials"
}

# The canonical configuration reference is authored in the repository docs tree,
# but its links must resolve inside the published book tree after copying.
copy_config_doc() {
    local source="$1"
    local dest="$2"

    copy_doc "$source" "$dest"
    if [ -f "$dest" ]; then
        sed_in_place "$dest" \
            -e 's#NATIVE_CRITIC_RULE_MATRIX.md#native-critic-rule-matrix.md#g' \
            -e 's#../tutorials/DAP_USER_GUIDE.md#../dap/user-guide.md#g' \
            -e 's#../how-to/EDITOR_SETUP.md#editor-setup-canonical.md#g' \
            -e 's#../how-to/PERFORMANCE_TUNING.md#../advanced/performance-guide.md#g' \
            -e 's#PERFORMANCE_SLO.md#performance-slo.md#g' \
            -e 's#LSP_FEATURES.md#../user-guides/lsp-features.md#g' \
            -e 's#../how-to/THREADING_CONFIGURATION_GUIDE.md#../advanced/threading-configuration.md#g' \
            -e 's#CONFIGURATION_SCHEMA.md#configuration-schema.md#g'
    fi
    rewrite_book_links "$dest"
    rewrite_bare_book_links "$dest" "$REPO_BLOB_URL/docs/reference"
}

# Getting Started section
echo "Setting up Getting Started..."
# editor-setup.md and configuration.md are committed canonical-pointer stubs
# (see #3642 and #5034) — do not overwrite them with full canonical docs, or
# the published book drifts back to copies that go stale independently of the
# docs sources. Copy each canonical source to reference/ for in-book linking.
copy_book_doc "$DOCS_DIR/how-to/EDITOR_SETUP.md" "$BOOK_SRC/reference/editor-setup-canonical.md"
copy_config_doc "$DOCS_DIR/reference/CONFIG.md" "$BOOK_SRC/reference/configuration-canonical.md"
copy_book_doc "$DOCS_DIR/project/ORIENTATION.md" "$BOOK_SRC/getting-started/first-steps.md"

copy_book_doc "$DOCS_DIR/tutorials/GETTING_STARTED.md" "$BOOK_SRC/getting-started/installation.md"

# User Guides section
echo "Setting up User Guides..."
copy_book_doc "$DOCS_DIR/reference/LSP_FEATURES.md" "$BOOK_SRC/user-guides/lsp-features.md"
copy_book_doc "$DOCS_DIR/reference/WORKSPACE_NAVIGATION_GUIDE.md" "$BOOK_SRC/user-guides/workspace-navigation.md"
copy_book_doc "$DOCS_DIR/how-to/DEBUGGING.md" "$BOOK_SRC/user-guides/debugging.md"
copy_book_doc "$DOCS_DIR/how-to/TROUBLESHOOTING.md" "$BOOK_SRC/user-guides/troubleshooting.md"
copy_book_doc "$DOCS_DIR/reference/KNOWN_LIMITATIONS.md" "$BOOK_SRC/user-guides/known-limitations.md"

# Architecture section
echo "Setting up Architecture..."
copy_book_doc "$DOCS_DIR/reference/ARCHITECTURE_OVERVIEW.md" "$BOOK_SRC/architecture/overview.md"
copy_book_doc "$DOCS_DIR/reference/CRATE_ARCHITECTURE_GUIDE.md" "$BOOK_SRC/architecture/crate-structure.md"
copy_book_doc "$DOCS_DIR/reference/MODERN_ARCHITECTURE.md" "$BOOK_SRC/architecture/modern-architecture.md"

copy_book_doc "$DOCS_DIR/reference/ARCHITECTURE_OVERVIEW.md" "$BOOK_SRC/architecture/parser-design.md"

copy_book_doc "$DOCS_DIR/reference/LSP_IMPLEMENTATION_GUIDE.md" "$BOOK_SRC/architecture/lsp-implementation.md"
copy_book_doc "$DOCS_DIR/reference/CRATE_ARCHITECTURE_DAP.md" "$BOOK_SRC/architecture/dap-implementation.md"

# Developer Guides section
echo "Setting up Developer Guides..."
# contributing.md is a committed canonical-pointer stub. Do not replace it with
# root CONTRIBUTING.md: repository-relative links would resolve under the book
# tree, and the copied workflow would drift independently between population runs.
copy_book_doc "$DOCS_DIR/reference/COMMANDS_REFERENCE.md" "$BOOK_SRC/developer/commands-reference.md"
copy_testing_doc "$DOCS_DIR/tutorials/COMPREHENSIVE_TESTING_GUIDE.md" "$BOOK_SRC/developer/testing-guide.md"
copy_book_doc "$DOCS_DIR/reference/TEST_INFRASTRUCTURE_GUIDE.md" "$BOOK_SRC/developer/test-infrastructure.md"
copy_book_doc "$DOCS_DIR/reference/API_DOCUMENTATION_STANDARDS.md" "$BOOK_SRC/developer/api-documentation-standards.md"
copy_development_doc "$DOCS_DIR/project/DEVELOPMENT.md" "$BOOK_SRC/developer/development-workflow.md"

# LSP Development section
echo "Setting up LSP Development..."
copy_lsp_doc "$DOCS_DIR/tutorials/LSP_DEVELOPMENT_GUIDE.md" "$BOOK_SRC/lsp/implementation-guide.md"
copy_book_doc "$DOCS_DIR/reference/LSP_PROVIDERS_REFERENCE.md" "$BOOK_SRC/lsp/providers-reference.md"
copy_book_doc "$DOCS_DIR/reference/LSP_FEATURE_IMPLEMENTATION_BEST_PRACTICES.md" "$BOOK_SRC/lsp/feature-implementation.md"
copy_book_doc "$DOCS_DIR/reference/LSP_CANCELLATION_PROTOCOL.md" "$BOOK_SRC/lsp/cancellation-system.md"
copy_book_doc "$DOCS_DIR/explanation/ERROR_HANDLING_STRATEGY.md" "$BOOK_SRC/lsp/error-handling.md"

# Advanced Topics section
echo "Setting up Advanced Topics..."
copy_book_doc "$DOCS_DIR/how-to/PERFORMANCE_PRESERVATION_GUIDE.md" "$BOOK_SRC/advanced/performance-guide.md"
copy_book_doc "$DOCS_DIR/how-to/INCREMENTAL_PARSING_GUIDE.md" "$BOOK_SRC/advanced/incremental-parsing.md"
copy_book_doc "$DOCS_DIR/how-to/THREADING_CONFIGURATION_GUIDE.md" "$BOOK_SRC/advanced/threading-configuration.md"
copy_book_doc "$DOCS_DIR/how-to/SECURITY_DEVELOPMENT_GUIDE.md" "$BOOK_SRC/advanced/security-development.md"
copy_book_doc "$DOCS_DIR/reference/MUTATION_TESTING_METHODOLOGY.md" "$BOOK_SRC/advanced/mutation-testing.md"

# Reference section
echo "Setting up Reference..."
# current-status.md is a committed canonical-pointer stub (see #3642) — do not
# overwrite it with CURRENT_STATUS.md, or the published book drifts back to a
# copy that goes stale independently of the canonical status overview.
# Also copy modular status files (linked from the stub)
mkdir -p "$BOOK_SRC/reference/status"
copy_book_doc "$DOCS_DIR/project/status/index.md" "$BOOK_SRC/reference/status/index.md"
copy_book_doc "$DOCS_DIR/project/status/lsp.md" "$BOOK_SRC/reference/status/lsp.md"
copy_book_doc "$DOCS_DIR/project/status/tests.md" "$BOOK_SRC/reference/status/tests.md"
copy_book_doc "$DOCS_DIR/project/status/parser.md" "$BOOK_SRC/reference/status/parser.md"
copy_book_doc "$DOCS_DIR/project/status/quality.md" "$BOOK_SRC/reference/status/quality.md"
copy_book_doc "$DOCS_DIR/project/status/release.md" "$BOOK_SRC/reference/status/release.md"
copy_book_doc "$DOCS_DIR/project/ROADMAP.md" "$BOOK_SRC/reference/roadmap.md"
copy_book_doc "$DOCS_DIR/project/MILESTONES.md" "$BOOK_SRC/reference/milestones.md"
copy_book_doc "$DOCS_DIR/reference/NATIVE_CRITIC_RULE_MATRIX.md" "$BOOK_SRC/reference/native-critic-rule-matrix.md"
copy_book_doc "$DOCS_DIR/reference/PERFORMANCE_SLO.md" "$BOOK_SRC/reference/performance-slo.md"
copy_book_doc "$DOCS_DIR/reference/CONFIGURATION_SCHEMA.md" "$BOOK_SRC/reference/configuration-schema.md"
# stability.md is a committed mdBook include of the canonical contract. Do not
# overwrite it with a copied snapshot, which can drift between population runs.
copy_book_doc "$DOCS_DIR/how-to/UPGRADING.md" "$BOOK_SRC/reference/upgrading.md"
copy_book_doc "$DOCS_DIR/reference/ERROR_HANDLING_API_CONTRACTS.md" "$BOOK_SRC/reference/error-handling-contracts.md"
copy_book_doc "$DOCS_DIR/reference/LSP_MISSING_FEATURES_REPORT.md" "$BOOK_SRC/reference/lsp-missing-features.md"

# DAP section
echo "Setting up DAP..."
copy_book_doc "$DOCS_DIR/tutorials/DAP_USER_GUIDE.md" "$BOOK_SRC/dap/user-guide.md"
copy_book_doc "$DOCS_DIR/reference/DAP_IMPLEMENTATION_SPECIFICATION.md" "$BOOK_SRC/dap/implementation.md"
copy_book_doc "$DOCS_DIR/DAP_SECURITY_SPECIFICATION.md" "$BOOK_SRC/dap/security.md"
copy_book_doc "$DOCS_DIR/tutorials/DAP_BRIDGE_SETUP_GUIDE.md" "$BOOK_SRC/dap/bridge-setup.md"
copy_book_doc "$DOCS_DIR/reference/DAP_PROTOCOL_SCHEMA.md" "$BOOK_SRC/dap/protocol-schema.md"

# CI & Quality section
echo "Setting up CI & Quality..."
copy_book_doc "$DOCS_DIR/project/CI.md" "$BOOK_SRC/ci/overview.md"
copy_book_doc "$DOCS_DIR/project/CI_LOCAL_VALIDATION.md" "$BOOK_SRC/ci/local-validation.md"
copy_book_doc "$DOCS_DIR/project/CI_TEST_LANES.md" "$BOOK_SRC/ci/test-lanes.md"
copy_book_doc "$DOCS_DIR/project/CI_COST_TRACKING.md" "$BOOK_SRC/ci/cost-tracking.md"
copy_book_doc "$DOCS_DIR/explanation/DEBT_TRACKING.md" "$BOOK_SRC/ci/debt-tracking.md"

# Process & Governance section
echo "Setting up Process & Governance..."
copy_book_doc "$DOCS_DIR/project/AGENTIC_DEV.md" "$BOOK_SRC/process/agentic-dev.md"
copy_book_doc "$DOCS_DIR/project/LESSONS.md" "$BOOK_SRC/process/lessons.md"
copy_book_doc "$DOCS_DIR/project/CASEBOOK.md" "$BOOK_SRC/process/casebook.md"
copy_book_doc "$DOCS_DIR/project/DOCUMENTATION_TRUTH_SYSTEM.md" "$BOOK_SRC/process/documentation-truth.md"
copy_book_doc "$DOCS_DIR/project/QUALITY_SURFACES.md" "$BOOK_SRC/process/quality-surfaces.md"

# Additional Resources section
# Static resource files (adr.md, benchmarks.md, forensics.md, issue-tracking.md)
# are committed in book/src/resources/ and do not need to be generated.
echo "Setting up Additional Resources..."
copy_book_doc "$DOCS_DIR/project/GA_RUNBOOK.md" "$BOOK_SRC/resources/ga-runbook.md"

echo "Documentation population complete!"
echo "Next steps:"
echo "  1. Review the populated files"
echo "  2. Run: mdbook build book"
echo "  3. Run: mdbook serve book"
