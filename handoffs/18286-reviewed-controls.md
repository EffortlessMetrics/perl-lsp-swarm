# Reviewed test controls for #18286 — source packet, NOT_EXECUTED here

This additive evidence-only branch preserves a copyable version of the existing
[Draft PR #18287](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18287) patch and a precise qualification handoff.
It is not a competing implementation, a candidate branch update, a qualification
request, or a file to merge into main. Existing candidate/writer custody is retained.
Receipt-set #13424/#18283 ownership is unchanged. No production, schema, fixture,
workflow, permission or resource-policy change is proposed.

## Subject and current source

- Accepted issue: [#18286](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/18286).
- Candidate H: `3337c70f553c29ff50c0fc91af46467a3fb8a49f`; original base B: `3603efd4d5ba53b3115a4c0cc5a52b410bca65d5`.
- Existing branch: `test/13156-corpus-residual-controls`, still Open Draft at inspection.
- Published candidate tree (from existing execution report):
  `8551c21b0d20ac3ae7ef9e2f62c438d65b660052`.
- File: `crates/perl-corpus/tests/gold_repository_contract.rs`; base blob `07c60d5337e1d3327e8d911514e2880e87e80c65`;
  H blob `5196bb8e33ea663567f3876f58a1099af11fc8ab`.
- H file SHA256: `4ec683849b5028731c1d6682071a8785b2d603f0b3b886d8618d69cad219a19b`.
- Saved checkout inspected: `7e57f3ab9d214fc87d6630decc398b00f99521e0`. Its entire contract file
  equals the original base file. A separate live `main` file read returned the same
  base blob; this does not assert that the live main commit is the saved checkout.
- Reconstructed applyable patch below: 61 insertions, zero deletions, exactly this
  one test file. SHA256: `1b2a12be3deaa12626c4316a50d291fbd5e9972dc5d977cd525f670181926f81`.
  This hashes the exact two-header patch below, including final LF. It is a separate
  representation from the earlier report's patch hash; no equality is claimed.

## Independent source assessment

No material source finding in the three accepted controls. Existing review
[5607641173](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18287#pullrequestreview-5607641173) remains preserved.
This pass adds current-source compatibility, directly copyable controls, and a fourth
explicit fault recipe; it does not replace that review or upgrade runtime confidence.

- [Typed assertion test](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/3337c70f553c29ff50c0fc91af46467a3fb8a49f/crates/perl-corpus/tests/gold_repository_contract.rs#L1019): valid hover envelope
  and assertion are accepted first; replacing only the assertion with `{}` preserves
  version, fixture identity and a nonempty assertions array. The negative oracle
  requires both `typed assertions` and `expected_hover.json`, so an earlier metadata
  rejection cannot impersonate the intended failure. The actual HoverAssertion
  deserializer requires `kind`, `line`, `character` and the selected kind's payload;
  see [schema/deserializer](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/3603efd4d5ba53b3115a4c0cc5a52b410bca65d5/crates/perl-corpus/src/gold/mod.rs#L133).
- [Dangling member test](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/3337c70f553c29ff50c0fc91af46467a3fb8a49f/crates/perl-corpus/tests/gold_repository_contract.rs#L1162): accepts ordinary
  fixture membership, then creates a recognized sidecar name pointing to a missing
  target. The required error category and filename distinguish link rejection from
  treating the sidecar as absent. Setup errors propagate via `?`.
- [Nested payload test](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/3337c70f553c29ff50c0fc91af46467a3fb8a49f/crates/perl-corpus/tests/gold_repository_contract.rs#L1185): accepts a regular
  `lib/nested/Target.pm`, then adds `Linked.pm`. It exercises nested recursion and
  requires the payload-link category plus filename. Its positive counterpart is a
  valid member-layout input, not a complete fixture-directory acceptance claim.
- Reachability: `gold_repository_contract_holds` calls
  `validate_fixture_directory`, which calls `validate_fixture_members` and dispatches
  named sidecars through `validate_named_sidecar`. `lib` dispatch enters
  `validate_module_payload`, whose directory branch recurses. The earlier byte-fidelity
  walk independently rejects links. These are specific validator regression controls,
  not evidence of a current exploitable corpus-acceptance defect.
- Two controls are `cfg(unix)`. Windows exclusion does not prove them; existing Windows
  reparse policy/tests remain unchanged. No fixture population or schema rewrite.

## Checks actually performed in this session

Compiler-free PASS only:

- Retrieved exact B/H source and cumulative PR patch through the GitHub connector;
  recomputed both Git blob identities and H SHA256 from the returned bytes.
- `python3 /tmp/corpus18286-review/review.py`: asserted all bytes preceding
  `#[cfg(test)] mod tests` are identical across B, H and the saved checkout; replayed
  the published two-hunk patch into disposable B/current copies via `git apply --check`
  followed by `git apply`; resulting complete bytes match H exactly. Reported 61/0.
- Replayed each of the four isolated patches below into a disposable H copy using
  `git apply --check` and `git apply`; each changes only its named validator and
  preserves the entire test module byte-for-byte.
- Python filesystem probe: dangling link has `exists() == false` and lstat link type;
  nested valid-target link has lstat link type but followed stat regular-file type.
  This checks fixture assumptions, not the Rust validators.
- Git status remained clean; the candidate and saved checkout were never edited.
- `command -v rustfmt` found no direct installed executable. No tool was installed,
  and this session claims no new formatting result.

Rust compilation, Cargo, test listing/execution, and observed mutation failures:
**NOT_EXECUTED in this session.** No admission attempt, budget override, build,
installation, CI dispatch/rerun, Ready toggle, enqueue or merge was performed.
Apply checks and filesystem probes do not prove Rust behavior.

## Preserve earlier proof without inventing access

The existing [execution report](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18287#issuecomment-6360972432)
reports 28 passing Unix tests and three intended mutant failures at H, with unchanged
test bytes; executable SHA256
`1a455e279621efd740be1ac37737077152a8b5a273ecd392402dd348238af608`.
It also records a cleanup bookkeeping assertion failure and successful lease-release
readback. Preserve those reported outcomes and that failure. This session did not
access the old task, inspect its detailed logs/receipts, or independently validate
its execution/admission/settlement. Do not relabel that prior run as unexecuted,
and do not promote its summary into an independently verified current qualification.

The four fault patches below were reconstructed in this session. Their byte identities
are explicit and are not asserted to match the prior report's three unpublished
mutants. The nested-follow variant is additional to the three described there.

## Existing qualification handoff — no new request

Next owner remains the existing #18287 delivery/candidate owner for receipt recovery,
candidate judgment and continuation; #17903/#17900 retain the existing integration
route/cutover ownership. No receipt-set writer or new qualification owner is assigned.

1. Recover the existing H-bound green/mutant commands, complete outputs, source/control
   identities, genuine default admission and process-settlement/lease receipts through
   that owner. Reuse adequate proof with its original subject. This packet asks for no
   duplicate build, review, qualification run or request.
2. The PR body's older hosted-only/pre-queue focused-proof language is superseded by
   [#17903 correction 6340315499](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17903#issuecomment-6340315499)
   and [current #17903](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17903): development proof belongs in the admitted
   session; ordinary integration proof belongs on the actual admitted merge group G.
   Do not resurrect the older #17903 focused-hosted request. This issue's specific
   no-local-Cargo boundary still applies to this session; it was not relaxed.
3. If that owner identifies a genuinely missing proof obligation, the exact future
   commands below are recipes only, to be selected once on separately supported
   infrastructure with established applicable default admission. Do not invent or
   borrow smaller budgets, lower reserves, install tools, retry denied admission,
   or use a Mac/shared build. Preserve command failures and test identities/counts.
4. Before integration, the existing owner must make the current-source product/scope
   judgment with substantive review and all applicable protected qualification. The
   #17903 target is not evidence of installed cutover. No accepted qualification key,
   run/attempt or merge-group identity is asserted by this packet. Leave Draft until
   its real remaining conditions are met; no bypass, source refresh, direct merge,
   queue activation or readiness transition is authorized by this evidence branch.

### Future recipe, explicitly NOT_EXECUTED here

In the exact H checkout, after its owner establishes the applicable existing admission:

```bash
scripts/cargo-admitted test -p perl-corpus --locked --test gold_repository_contract -- tests::rejects_dangling_symlinked_fixture_members --exact --nocapture
scripts/cargo-admitted test -p perl-corpus --locked --test gold_repository_contract -- tests::rejects_nested_module_payload_symlinks --exact --nocapture
scripts/cargo-admitted test -p perl-corpus --locked --test gold_repository_contract -- tests::rejects_malformed_typed_named_assertion_members --exact --nocapture
scripts/cargo-admitted test -p perl-corpus --locked --test gold_repository_contract
```

Each focused run must execute its one named test, not zero/ignored tests. Retain the
complete full-target test identities and result counts; the earlier H report says 28
on Unix. Compile-only/exit-zero wrapper success is insufficient. For a newly needed
fault experiment, start a separate owner-approved H experiment, apply exactly one
patch below and run only its paired focused command. Do not combine faults or apply
them to the published candidate. Expect the named test to fail because invalid input
was accepted (`validation_error` returns `invalid fixture was accepted`), not because
of compilation, tool, setup or admission failure. Preserve that genuine failure.

All four experiments preserve the test-module SHA256
`52436b2c3b2d0468648e194ce37d3dd2c003c01465ec1460144074e43451d5d4` (bytes after the exact delimiter
`#[cfg(test)]\nmod tests {`). Retain the complete mutant tree/source identity and
executable identity as well; a file hash alone is not a full Rust proof subject.

## Copyable accepted test patch

Save the following block as `accepted-tests.patch`, with an LF after its last line.
Use only for inspection or owner-approved integration; it already exists in #18287.
Apply against B (or the byte-identical saved contract source), not again over H.

```diff
--- a/crates/perl-corpus/tests/gold_repository_contract.rs
+++ b/crates/perl-corpus/tests/gold_repository_contract.rs
@@ -1015,6 +1015,25 @@ mod tests {
         Ok(())
     }
 
+    #[test]
+    fn rejects_malformed_typed_named_assertion_members() -> Result<(), Box<dyn Error>> {
+        let directory = tempdir()?;
+        let sidecar = directory.path().join("expected_hover.json");
+        write_fixture_file(
+            &sidecar,
+            r#"{"version":1,"fixture":"fixture","assertions":[{"kind":"hover_contains","line":0,"character":0,"needle":"value"}]}"#,
+        )?;
+        validate_named_sidecar(&sidecar, "fixture")?;
+
+        write_fixture_file(&sidecar, r#"{"version":1,"fixture":"fixture","assertions":[{}]}"#)?;
+        let error = validation_error(validate_named_sidecar(&sidecar, "fixture"))?;
+        assert!(
+            error.contains("typed assertions") && error.contains("expected_hover.json"),
+            "valid envelope must reach typed assertion validation: {error}"
+        );
+        Ok(())
+    }
+
     #[test]
     fn named_sidecars_reject_unknown_fields() -> Result<(), Box<dyn Error>> {
         let directory = tempdir()?;
@@ -1138,6 +1157,48 @@ mod tests {
         Ok(())
     }
 
+    #[cfg(unix)]
+    #[test]
+    fn rejects_dangling_symlinked_fixture_members() -> Result<(), Box<dyn Error>> {
+        let root = tempdir()?;
+        let fixture = root.path().join("fixture");
+        fs::create_dir(&fixture)?;
+        write_fixture_file(&fixture.join("fixture.pl"), "use strict;\n")?;
+        validate_fixture_members(&fixture)?;
+
+        // A recognized sidecar name must be rejected as a link even when
+        // following it would report a missing target.
+        let link = fixture.join("expected_hover.json");
+        std::os::unix::fs::symlink(root.path().join("missing.json"), &link)?;
+        let error = validation_error(validate_fixture_members(&fixture))?;
+        assert!(
+            error.contains("fixture directory contains a symbolic link")
+                && error.contains("expected_hover.json"),
+            "dangling link must not be treated as an absent sidecar: {error}"
+        );
+        Ok(())
+    }
+
+    #[cfg(unix)]
+    #[test]
+    fn rejects_nested_module_payload_symlinks() -> Result<(), Box<dyn Error>> {
+        let root = tempdir()?;
+        let fixture = root.path().join("fixture");
+        let nested = fixture.join("lib").join("nested");
+        fs::create_dir_all(&nested)?;
+        write_fixture_file(&nested.join("Target.pm"), "package Target;\n1;\n")?;
+        validate_fixture_members(&fixture)?;
+
+        std::os::unix::fs::symlink(nested.join("Target.pm"), nested.join("Linked.pm"))?;
+        let error = validation_error(validate_fixture_members(&fixture))?;
+        assert!(
+            error.contains("module fixture payload contains a symbolic link")
+                && error.contains("Linked.pm"),
+            "nested payload must reject links instead of following or skipping them: {error}"
+        );
+        Ok(())
+    }
+
     #[cfg(windows)]
     #[test]
     fn rejects_windows_reparse_fixture_directory() -> Result<(), Box<dyn Error>> {
```

## Copyable isolated validator faults — NOT_EXECUTED

These edit validator helpers in the test target, never production source. Each patch
applies independently to H. Do not merge them.

### dangling-skip.patch

Paired test: `tests::rejects_dangling_symlinked_fixture_members`.
Patch SHA256: `f6b06617f7e2b5b8a7db60eb917fe3e92a1a6e9265240eae58f2812a30ea9dfe`.
Resulting whole-file Git blob: `e80f845f5bf43a4c64ad2e8336958872e66e624b`.
Resulting whole-file SHA256: `95d025f5376caa6fd91ecd6368e8b53fe9a1c8cf6a53e0d9e01dac7b2f4c989e`.
Apply/reconstruction PASS; Rust result NOT_EXECUTED.

```diff
--- a/crates/perl-corpus/tests/gold_repository_contract.rs
+++ b/crates/perl-corpus/tests/gold_repository_contract.rs
@@ -461,6 +461,9 @@
         let entry = entry?;
         let path = entry.path();
         let name = entry.file_name().to_string_lossy().into_owned();
+        if !path.exists() {
+            continue;
+        }
         let metadata = fs::symlink_metadata(&path)?;
 
         if metadata.file_type().is_symlink() {
```

### nested-follow.patch

Paired test: `tests::rejects_nested_module_payload_symlinks`.
Patch SHA256: `4985a35902772d0dd46e5332c69ca6e7899b5316dabd13bfc5764dd0ee55bd3d`.
Resulting whole-file Git blob: `0500c69ae1a7ff98c55344a1a9f0036c2fec5b1a`.
Resulting whole-file SHA256: `49d22ccf536ed61dbb4761fe80f46a10d1d611c31013621dbddc5710ccd07676`.
Apply/reconstruction PASS; Rust result NOT_EXECUTED.

```diff
--- a/crates/perl-corpus/tests/gold_repository_contract.rs
+++ b/crates/perl-corpus/tests/gold_repository_contract.rs
@@ -429,7 +429,7 @@
     for entry in fs::read_dir(path)? {
         let entry = entry?;
         let member = entry.path();
-        let metadata = fs::symlink_metadata(&member)?;
+        let metadata = fs::metadata(&member)?;
         let name = entry.file_name().to_string_lossy().into_owned();
 
         if metadata.file_type().is_symlink() {
```

### nested-no-recursion.patch

Paired test: `tests::rejects_nested_module_payload_symlinks`.
Patch SHA256: `579be0e7b8f17958d3f87e47a95760591a9f8973c4765f431f8aaccff16c7f04`.
Resulting whole-file Git blob: `f992a5409905a15622334fe28defc1f372df488d`.
Resulting whole-file SHA256: `934413d592b0ee8bb1523b956f09553f65f1fa0ff79f3f7f4171d4764c0e5d14`.
Apply/reconstruction PASS; Rust result NOT_EXECUTED.

```diff
--- a/crates/perl-corpus/tests/gold_repository_contract.rs
+++ b/crates/perl-corpus/tests/gold_repository_contract.rs
@@ -440,7 +440,6 @@
             .into());
         }
         if metadata.file_type().is_dir() {
-            validate_module_payload(&member)?;
             continue;
         }
         if !metadata.file_type().is_file()
```

### hover-untyped.patch

Paired test: `tests::rejects_malformed_typed_named_assertion_members`.
Patch SHA256: `1850c94565b2e817f91e941dd0cfc92c1a243a8123989f20cf946758b8322394`.
Resulting whole-file Git blob: `013a2701594fe136149c705878201bddbadbfbb8`.
Resulting whole-file SHA256: `e189bd25334b3a28da5bbfab8904c756043856b07f93ea1014399306dfe7a9a8`.
Apply/reconstruction PASS; Rust result NOT_EXECUTED.

```diff
--- a/crates/perl-corpus/tests/gold_repository_contract.rs
+++ b/crates/perl-corpus/tests/gold_repository_contract.rs
@@ -378,7 +378,6 @@
 
     match path.file_name().and_then(|name| name.to_str()) {
         Some("expected_hover.json") => {
-            validate_typed_named_sidecar::<HoverGoldExpected>(path, &document)?;
         }
         Some("expected_goto.json") => {
             validate_typed_named_sidecar::<GotoGoldExpected>(path, &document)?;
```

## Reproduce source-only inspection

With B and H available in the consumer's existing checkout:

```bash
git diff --name-status 3603efd4d5ba53b3115a4c0cc5a52b410bca65d5 3337c70f553c29ff50c0fc91af46467a3fb8a49f
git diff --numstat 3603efd4d5ba53b3115a4c0cc5a52b410bca65d5 3337c70f553c29ff50c0fc91af46467a3fb8a49f -- crates/perl-corpus/tests/gold_repository_contract.rs
git diff --check 3603efd4d5ba53b3115a4c0cc5a52b410bca65d5 3337c70f553c29ff50c0fc91af46467a3fb8a49f -- crates/perl-corpus/tests/gold_repository_contract.rs
git show 3337c70f553c29ff50c0fc91af46467a3fb8a49f:crates/perl-corpus/tests/gold_repository_contract.rs | sha256sum
```

Expected one modified file, 61 additions, zero deletions, no whitespace-error output,
and the candidate file SHA256 above. In disposable source-only B/H copies, respectively,
`git apply --check accepted-tests.patch` or `git apply --check <one-fault>.patch`
checks applicability without invoking Rust. The exact copyable patches are the
deliverable; local paths and hash references are supporting evidence only.
