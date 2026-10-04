# Production unsafe allowance

The aggregate ceiling is ten counted production unsafe sites. The canonical
`perl-ci-hygiene check-unsafe-prod` checker and its production/test selection
remain authoritative; every counted site still needs adjacent SAFETY reasoning.
This policy changes the allowance, not the checker or required protection.

PR #16986 / issue #16979 allocate one site to the Windows trusted-root identity
query in `perl-dap::security::launch_authority::windows_directory_identity`.
The other nine sites retain their existing semantics: five Windows bounded-input
and owned-process-tree FFI boundaries, and four raw-pointer document-store
Send/Sync implementations. No site is excluded from counting by this allocation.

Accountable policy owner: repository maintainer @EffortlessSteven. Semantic owner:
perl-dap launch authority. Checker owner: perl-ci-hygiene. Review the source and
this policy together through ordinary current-candidate review and protection.

## Identity-query safety invariant

A zero-access OpenOptions handle uses explicit read/write/delete sharing,
BACKUP_SEMANTICS and OPEN_REPARSE_POINT for the final directory entry. File owns
and closes that handle. GetFileInformationByHandleEx receives a live handle and
an aligned, initialized, exclusively borrowed FILE_ID_INFO buffer whose exact
size matches FileIdInfo. The call retains neither pointer nor handle. Identity
is returned only on success with a nonzero volume and nonzero complete 128-bit
identifier. Opening/query failure or incomplete identity causes refusal.

Stable std lacks an equivalent full-128-bit query. Published file-id 0.2.3 lacks
a no-follow high-resolution entry point. Timestamp/size fallback, an unreviewed
upstream revision, and moving unsafe outside the count are not substitutes.

## Required proof and future constraint

Protected integration requires the canonical gate to report ten sites with no
missing SAFETY comments, current API verification, exact-candidate hosted Windows
replacement and unchanged-root controls, affected perl-dap library proof, and
selected Windows smoke. Historical receipts do not prove a changed candidate.
The replacement witness verifies rename stability and equal creation/last-write
timestamps set after writing its child, then admission and narrowing refusal.

New unsafe sites need a separate justified disposition; genuine reductions
should tighten the ceiling. If a stable safe equivalent preserves full IDs,
no-follow opening and fail-closed behavior, remove this FFI and its allowance.
