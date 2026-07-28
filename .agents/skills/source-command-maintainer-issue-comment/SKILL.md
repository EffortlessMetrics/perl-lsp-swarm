---
name: "source-command-maintainer-issue-comment"
description: "Maintainer vision (issue) step 3 — post alignment verdict as issue comment"
---

# source-command-maintainer-issue-comment

Use this skill when the user asks to run the migrated source command `maintainer-issue-comment`.

## Command Template

# Maintainer Issue: Comment

Post your project-alignment verdict and set the sign-off label.

## Steps

1. Post comment:
   ```bash
   gh issue comment <number> --body "$(cat <<'EOF'
   ## Maintainer Vision Review

   ### Roadmap alignment: [ALIGNED | TANGENTIAL | MISALIGNED]
   <how this relates to current priorities>

   ### User impact: [HIGH | MEDIUM | LOW | UNCLEAR]
   <who benefits, how many, how often>

   ### Scope fit: [IN SCOPE | BORDERLINE | OUT OF SCOPE]
   <does this belong in the LSP server?>

   ### Verdict: [ALIGNED | DEFERRED | OUT OF SCOPE | MISALIGNED]
   <one sentence justification>

   ---
   *Maintainer vision review (issue) — project alignment check.*
   EOF
   )"
   ```

2. Set sign-off label (verified apply — see `/label-apply-verified`):
   ```
   /label-apply-verified issue <number> "maintainer-issue-reviewed"
   ```
