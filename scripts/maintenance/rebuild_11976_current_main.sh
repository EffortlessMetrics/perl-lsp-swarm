#!/usr/bin/env bash
set -euo pipefail

branch="repair/11976-current-main"
source_commit="4e3a89b190e259f1265672d89c77b34690654cb3"

# The workflow checks out the named branch with credentials retained.
git config user.name EffortlessSteven
git config user.email git@effortlesssteven.com
git fetch origin main fix/4997-generic-channel-ai-arm

# Consume any main movement before reconstructing the candidate. A conflict here
# is a new ownership collision and must be reviewed rather than guessed.
git merge --no-edit origin/main

git cat-file -e "${source_commit}^{commit}"
parent_commit=$(git rev-parse "${source_commit}^")

if ! git cherry-pick "$source_commit"; then
  mapfile -t conflicts < <(git diff --name-only --diff-filter=U | sort)
  printf 'Conflicted files:\n'
  printf '  %s\n' "${conflicts[@]}"

  expected=(
    crates/perl-lsp-rs-core/src/config/mod.rs
    crates/perl-lsp-rs-core/tests/perllsp_settings_schema_tests.rs
  )
  mapfile -t expected_sorted < <(printf '%s\n' "${expected[@]}" | sort)
  diff -u <(printf '%s\n' "${expected_sorted[@]}") <(printf '%s\n' "${conflicts[@]}")

  for path in "${conflicts[@]}"; do
    git checkout --ours -- "$path"
    patch="/tmp/$(printf '%s' "$path" | tr '/' '_').patch"
    git diff --binary "$parent_commit" "$source_commit" -- "$path" > "$patch"
    git apply --reject --recount --whitespace=nowarn "$patch" || true
  done

  python3 - <<'PY'
from pathlib import Path


def replace_exact(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one current-main anchor, found {count}")
    file.write_text(text.replace(old, new), encoding="utf-8")


config = "crates/perl-lsp-rs-core/src/config/mod.rs"
old_config = '''/// Configuration for AI-powered inline completions.
///
/// Disabled by default. When enabled, the server calls an external AI provider
/// for inline completion suggestions, falling back to deterministic rules on
/// timeout, error, or when AI is disabled.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiCompletionConfig {
    /// Whether the user explicitly enabled AI completions via the LSP client
    /// configuration channel. Default: false.
    pub user_enabled: bool,
'''
new_config = '''/// A server-owned activation disposition for credentialed remote AI egress.
///
/// This type is deliberately not constructible from any client payload: no LSP
/// channel (`initializationOptions`, `workspace/didChangeConfiguration`,
/// `workspace/configuration` results, project files) can prove user/machine
/// provenance, so generic arrivals never strengthen this disposition (#4997).
/// Transport position, key spelling, client names, and client-supplied scope
/// labels confer no authority.
///
/// [`AiActivationAuthority::TrustedUserOperator`] is reserved for a future
/// server-owned operator adapter (#10817) and for tests proving the rule is
/// not "activation is impossible"; constructing it from client-derived data is
/// a security defect. Mirrors the [`ExternalIncludePathAuthority`] pattern
/// landed for external include roots (#4998) so the canonical observation
/// train (#10807/#10813/#10817) consumes one vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AiActivationAuthority {
    /// No accepted trusted-user/operator activation evidence exists. Remote
    /// backend construction fails closed (#4997).
    #[default]
    Unavailable,
    /// An explicitly trusted user/operator adapter admitted activation.
    TrustedUserOperator,
}

impl AiCompletionConfig {
    /// Admit a trusted user/operator activation (#4997).
    ///
    /// The only sanctioned way to make remote AI backend construction eligible.
    /// Intentionally takes no client-derived arguments: a future server-owned
    /// adapter (#10817) supplies independently verified user/machine evidence;
    /// until then only tests and internal fixtures may call this. Passing
    /// client payload content here is a security defect.
    pub fn admit_trusted_user_operator_activation(&mut self) {
        self.activation_authority = AiActivationAuthority::TrustedUserOperator;
    }
}

/// Configuration for AI-powered inline completions.
///
/// Disabled by default. When enabled, the server calls an external AI provider
/// for inline completion suggestions, falling back to deterministic rules on
/// timeout, error, or when AI is disabled.
///
/// Arming the remote backend additionally requires
/// [`AiActivationAuthority::TrustedUserOperator`]; no current client channel
/// can supply it (#4997), so remote activation fails closed in production
/// until the trusted-operator adapter lands (#10817).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiCompletionConfig {
    /// Whether a trusted user/operator activation was admitted (#4997).
    ///
    /// Only [`Self::admit_trusted_user_operator_activation`] may set the
    /// underlying authority to trusted. Generic LSP traffic cannot write this
    /// field; malformed or absent client values never clear an accepted
    /// activation either.
    #[serde(default)]
    pub activation_authority: AiActivationAuthority,
    /// Whether trusted user/operator activation requested AI completions.
    ///
    /// Generic LSP payloads cannot write this field (#4997); before the
    /// trusted-operator adapter lands it can only be set by internal fixtures
    /// and tests. Default: false.
    pub user_enabled: bool,
'''
replace_exact(config, old_config, new_config)

tests = "crates/perl-lsp-rs-core/tests/perllsp_settings_schema_tests.rs"
old_assertions = '''    assert!(server.ai_completion.user_enabled);
    assert_eq!(server.ai_completion.model, "fixture-model");
    assert_eq!(server.ai_completion.timeout_ms, 2200);
    assert_eq!(server.ai_completion.max_output_tokens, 96);
    assert_eq!(server.ai_completion.max_inflight, 2);
    assert!(!server.ai_completion.fallback);
    assert!(server.ai_completion.local_model_mode);
    assert!(!server.ai_completion.streaming.user_enabled);
    assert_eq!(server.ai_completion.streaming.update_debounce_ms, 80);
'''
new_assertions = '''    // #4997: activation/selection fields from the generic schema are rejected;
    // compiled defaults survive. Envelope fields remain behavior-backed.
    assert!(!server.ai_completion.user_enabled);
    assert_eq!(
        server.ai_completion.activation_authority,
        perl_lsp_rs_core::config::AiActivationAuthority::Unavailable
    );
    assert_eq!(server.ai_completion.provider, "openai_compat");
    assert_eq!(server.ai_completion.model, "gpt-4o-mini");
    assert_eq!(server.ai_completion.timeout_ms, 2200);
    assert_eq!(server.ai_completion.max_output_tokens, 96);
    assert_eq!(server.ai_completion.max_inflight, 2);
    assert!(!server.ai_completion.fallback);
    assert!(server.ai_completion.local_model_mode);
    assert!(server.ai_completion.streaming.user_enabled);
    assert_eq!(server.ai_completion.streaming.update_debounce_ms, 80);
'''
replace_exact(tests, old_assertions, new_assertions)

old_security_tail = '''    assert!(ai.get("apiKeyHeader").is_none());
    assert!(ai.get("apiKeyPrefix").is_none());

    Ok(())
}
'''
new_security_tail = '''    assert!(ai.get("apiKeyHeader").is_none());
    assert!(ai.get("apiKeyPrefix").is_none());

    // #4997: activation and selection fields remain documented for the future
    // trusted adapter but advertise no generic client transport.
    for activation_field in ["enabled", "provider", "model"] {
        let field = ai
            .get(activation_field)
            .unwrap_or_else(|| panic!("aiCompletion.{activation_field} must stay documented"));
        assert_eq!(
            field["x-perllsp-transports"],
            json!([]),
            "aiCompletion.{activation_field} must not advertise client transports (#4997)",
        );
        assert_eq!(field["x-perllsp-scope"], json!("machine"));
    }
    let streaming_enabled =
        &perl["aiCompletion"]["properties"]["streaming"]["properties"]["enabled"];
    assert_eq!(
        streaming_enabled["x-perllsp-transports"],
        json!([]),
        "streaming.enabled must not advertise client transports (#4997)",
    );

    Ok(())
}
'''
replace_exact(tests, old_security_tail, new_security_tail)

for reject in [Path(config + ".rej"), Path(tests + ".rej")]:
    if not reject.exists():
        raise SystemExit(f"expected rejected hunk missing: {reject}")
    reject.unlink()
PY

  if find . -name '*.rej' -print -quit | grep -q .; then
    echo "Unresolved rejected hunks remain:" >&2
    find . -name '*.rej' -print -exec sh -c 'echo "--- $1"; cat "$1"' _ {} \;
    exit 1
  fi

  git add -- "${conflicts[@]}"
  git cherry-pick --continue
fi

python3 scripts/maintenance/address_11976_review.py

cargo fmt --all
cargo fmt --all -- --check
python3 -m json.tool schemas/perllsp-settings.schema.json >/dev/null
python3 -m json.tool .ci/fixtures/zed-perl-upstream/settings-behavior.v1.json >/dev/null
cargo test -p perl-lsp-rs-core hostile_and_malformed_traffic_preserves_accepted_trusted_ai_state --locked
cargo test -p perl-lsp-rs-core generic_channel_ai_activation_shapes_fail_closed_across_clients --locked
cargo test -p perl-lsp-rs-core --test perllsp_settings_schema_tests --locked
cargo test -p xtask --test zed_settings_behavior --locked
cargo test -p perl-lsp-rs --test lsp_streaming_completion_tests --locked hostile_generic
cargo clippy -p perl-lsp-rs-core -p perl-lsp-rs --lib --locked -- -D warnings
git diff --check

# Publish only the reviewed candidate; one-shot branch machinery never reaches the
# product diff.
git rm -- \
  .github/workflows/rebuild-11976-current-main.yml \
  .github/workflows/rebuild-11976-current-main-v2.yml \
  scripts/maintenance/address_11976_review.py \
  scripts/maintenance/rebuild_11976_current_main.sh

git add -A
git diff --cached --check
if ! git diff --cached --quiet; then
  git commit -m 'fix(ai-completion): address security review on current main (#4997)'
fi
git push origin HEAD:"$branch"
