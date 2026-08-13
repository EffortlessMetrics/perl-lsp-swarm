# Zed Perl server defaults and publication order

> **State:** compatibility contract implemented; no publication order is selected until the four actual-host rows pass.
>
> **Owner:** #7992. Parent: #7908. Host driver: #7984.

The candidate Zed default remains:

```jsonc
{
  "languages": {
    "Perl": {
      "language_servers": [
        "perlnavigator-server",
        "!perl-lsp",
        "!perllsp",
        "..."
      ]
    }
  }
}
```

Perl Navigator remains the enabled default. `perl-lsp` and `perllsp` remain independent dormant alternatives. `"..."` preserves unrelated user and extension registrations.

## Four required combinations

Run isolated real-Zed sessions for:

| Defaults | Extension | Question |
|---|---|---|
| current | public `0.4.0` | What is the current baseline? |
| candidate | public `0.4.0` | Is the unknown negated `perllsp` ID harmless before registration? |
| current | candidate three-server | Does the new extension create startup noise before the default ships? |
| candidate | candidate three-server | Is the intended final state quiet and selectable? |

Every row binds exact Zed, defaults, extension, profile, and process-inventory digests. All rows must use the same Zed host subject. The final candidate/candidate row must start only `perlnavigator-server`, produce no failed alternative, and retain explicit selection of either alternative.

## Selection cases

The receipt separately proves:

```text
default_only
  only perlnavigator-server starts

select_perllsp
  only perllsp starts as exact `perllsp --stdio`

select_perl_lsp
  only the independent perl-lsp provider starts

deliberate_multi_server
  perl-lsp and perllsp both start because the user selected both

missing_selected_server
  perllsp fails under its own ID and no provider starts as fallback

ellipsis_preserves_user_registration
  one independent user registration survives the reviewed trailing `...`
```

## Derived ruling

The validator computes the only acceptable ruling:

```text
candidate defaults + public extension is quiet
  -> zed_defaults_first_safe

candidate defaults + public extension is noisy
and current defaults + candidate extension is quiet
  -> extension_first_required

both intermediate combinations are noisy
  -> coordinated_release_required
```

A passing receipt cannot choose another ruling. It must record the unsafe interval avoided, the exact maintainer submission/release sequence, invalidation conditions, and evidence.

## Receipt

Start from:

```text
.ci/fixtures/zed-perl-upstream/receipts/default-order-template.json
```

The contract is:

```text
.ci/fixtures/zed-perl-upstream/default-order.v1.json
```

Validate with:

```bash
cargo run -p xtask --bin validate-zed-default-order -- \
  .ci/fixtures/zed-perl-upstream/default-order.v1.json \
  /path/to/default-order-receipt.json
```

The checked template remains `not_run` and `publication_order = unresolved`. Static source, a patch that applies, or a development-extension build cannot select the order.

## Limits

This receipt proves only the exact defaults/extension compatibility matrix and publication sequence. It does not prove settings behavior, managed download, the complete semantic journey, official-registry installation, or public Zed support.
