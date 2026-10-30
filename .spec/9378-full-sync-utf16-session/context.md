# Context — #9378 full-sync UTF-16 initialize/session contract

Claim: one accepted initialized session contains one immutable text/position
contract — `sync_kind = full`, `position_encoding = utf-16` — and the
initialize response, stored state, serving boundary, and bounded evidence all
derive from that accepted value.

Parent: #8531. Decision gate: #8129 (`full_document_utf16`, recorded
2026-08-29, `selected_for_implementation`). First-RC controller: #13768.
Downstream train: #9380 → #9382 → #9383 → #9386 → #9388 → #9389.

## Current ruling

The initialization contract is closed:

```text
offer absent / null / empty
→ accept immutable FULL + UTF-16

offer is a valid string list containing UTF-16
→ accept immutable FULL + UTF-16

offer is a valid string list omitting UTF-16
→ accept immutable FULL + UTF-16
→ retain mandatory-utf16-fallback evidence

malformed capability shape or entry
→ InvalidParams (-32602)
→ no accepted session

first initialize attempt, accepted or rejected
→ consumes initialize-attempt authority

any later initialize, valid or malformed
→ InvalidRequest (-32600)
→ original accepted state unchanged or absent
```

A valid `[`utf-8`]`, `[`utf-32`]`, or unknown-string-only list does not create
`no-common-encoding`. The product still advertises and stores only UTF-16; it
does not claim UTF-8 or UTF-32 wire support.

## State authority

Attempted, accepted, and lifecycle-complete initialization are different facts:

```text
initialize_attempted
accepted_text_sync_session
initialized
```

The first request atomically owns `initialize_attempted` before its parameters
are classified. Exactly one concurrent first attempt wins. Every loser returns
`-32600` before parameter-specific validation.

A malformed first attempt leaves:

```text
initialize_attempted = true
accepted_text_sync_session = None
initialized = false
ordinary request admission = false
initialized-notification completion = false
watcher/index/bootstrap/startup side effects = 0
```

The attempt guard is not serving authority. Ordinary requests, formatting
interception, compatibility auto-initialize, initialized-notification
completion, and startup activation require the accepted session through
`initialization_accepted()`.

## Offer dispositions

- **Absent or null**: no client encoding constraint; select UTF-16 with
  `offer-absent`.
- **Present empty**: explicit empty list; select UTF-16 with `offer-empty`.
- **Present valid containing UTF-16**: select UTF-16 with
  `client-offered-utf16`.
- **Present valid omitting UTF-16**: select UTF-16 with
  `mandatory-utf16-fallback`; retain bounded offered entries and whether they
  are recognized.
- **Present malformed**: non-array value or any non-string entry; return typed
  `-32602` and install no accepted session.

Client name, offer order, duplicate entries, unknown strings, and later
initialize parameters cannot select another contract.

## Required falsifiers

1. malformed first → `-32602`; valid second → `-32600`; no session, no serving;
2. malformed first → `-32602`; malformed second → `-32600`;
3. accepted first → valid or malformed second → `-32600`; original contract
   byte-identical;
4. valid UTF-8-only, UTF-32-only, and unknown-string-only offers → successful
   FULL + UTF-16 with mandatory-fallback evidence;
5. concurrent valid/malformed attempts → one attempt owner, every loser
   `-32600`, no mixed accepted state;
6. rejected first followed by `initialized`, hover, didOpen, formatting, or
   compatibility preflight → remains non-serving with zero lifecycle effects;
7. response/state divergence → typed internal failure before acceptance.

The realistic red mutations are: classify before attempt ownership; restore a
valid-list no-common rejection; gate serving on attempted state; classify a
malformed second request before the one-shot guard; or let two concurrent
attempts both install/classify as first.

## Ownership boundary

This leaf owns initialize/session authority only. It does not implement full
source replacement, ranged-change refusal/desynchronization, outgoing range
closure, exact-process lifecycle, installed VSIX behavior, or public claim
projection. Those remain #9380, #9382, #9383, #9386, #9388, and #9389.

PR #14159 overlaps PR #12067 in runtime/public-API/generated-inventory
surfaces. Land #12067, restack #14159 once, regenerate only sanctioned outputs,
rerun focused proof and exact-head review, then merge #9378 before admitting
#9380.
