#!/usr/bin/env bash
# PreToolUse hook: block dangerous bash commands before execution
# Exit 2 = block with feedback
# Exit 0 = allow

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty')
# Normalize CRLF -> LF (#4006 fix-forward): on Windows Git Bash, piping a
# multi-line command through `jq -r` was observed to reintroduce a trailing
# \r on every embedded newline, even though the JSON itself only encodes
# \n. That \r broke the heredoc-closing-delimiter EXACT match below (a
# closer line "EOF\r" != the parsed delimiter "EOF"), which silently
# swallowed everything after an unclosed-looking heredoc as body -- including
# a real trailing hazard like `rm -rf /etc`. Stripping \r here is safe: the
# existing regex guards already treat \r as whitespace via [:space:], so no
# real command's matching semantics change; only the new exact-match heredoc
# comparison needed this to be robust.
CMD="${CMD//$'\r'/}"

# Narrative-stripping preprocessor (#4006): the guards below run grep -qE
# against an unanchored, substring-matched view of the command text. That
# means trigger phrases INSIDE narrative -- a commit message, a heredoc body
# documenting a dangerous command, a quoted argument -- fire the same guard
# as an actual invocation of that command. Observed false blocks:
#   - `git commit -m "removed the redundant git worktree add line"` tripped
#     the worktree-add guard because the message text contains the phrase.
#   - a heredoc body documenting `rm -rf /etc` (e.g. a PR description written
#     via `gh pr create --body-file -` with a `<<EOF ... EOF` heredoc) tripped
#     the rm-rf-root guard.
#   - `git commit -m "fixed: git push --force is now blocked"` tripped the
#     dangerous-command guard.
#
# strip_narrative() derives a command-structure-only view by removing, in
# order: (1) heredoc bodies (opener line kept, body + closing delimiter line
# dropped), (2) `#`-to-end-of-line comments outside quotes, (3) the CONTENTS
# of '...' and "..." (quote characters are kept so surrounding structure like
# `-m ""` still reads as a flag+empty-arg; only what's INSIDE is blanked).
# All six guard blocks below match against this stripped view instead of the
# raw $CMD, so a trigger phrase can no longer fire from inside narrative --
# while an UNQUOTED real invocation (`git worktree add x`, `rm -rf /etc`,
# `git push --force`) is untouched by stripping and still matches. This is a
# heuristic lexer, not a full shell parser (deliberately out of scope per
# #4006) -- it tracks quote/heredoc state to blank narrative content, it does
# not resolve variable expansion, command substitution, or nested quoting
# edge cases. Known accepted limitation carried over unchanged from the
# pre-existing rm-rf guard: a quoted dangerous path (`rm -rf "/etc"`) is not
# matched, because its content is blanked same as any other quoted content --
# this was already true before this change (the guard comment above the
# rm-rf pattern documents it) and is not a new hole.
#
# Narrow exception (adversarial review on #4041): a quoted argument
# immediately after a known shell interpreter's `-c` flag (`bash -c "..."`,
# `sh -c '...'`, etc.) is not narrative -- it is a second shell script that
# actually executes. Blindly blanking it (as the general quoted-content rule
# above does) hides a real hazard (`bash -c "... && rm -rf /etc"`) from the
# same six guards. _shell_c_context() detects this one well-known,
# unambiguous code-execution idiom so that specific quoted region is passed
# through unblanked instead (see SQ_PASS/DQ_PASS below); this is a narrower,
# not a general, exception -- it does not attempt to recognize every way a
# quoted string can end up executed (eval, ssh remote-command, `perl -e`,
# command substitution, etc. are still out of scope, consistent with "not a
# full shell parser" above).
_shell_c_context() {
  [[ "$1" =~ (^|[[:space:]])(bash|sh|zsh|ksh|dash|csh|tcsh)[[:space:]]+-c[[:space:]]*$ ]]
}

strip_narrative() {
  local input="$1"
  local -a LINES=()
  mapfile -t LINES <<<"$input"
  local out="" state="NORMAL" first_line=1
  local -a delim_queue=() striptabs_queue=()
  local heredoc_delim="" heredoc_striptabs=0
  local line

  for line in "${LINES[@]}"; do
    if [ "$first_line" = "1" ]; then
      first_line=0
    else
      out+=$'\n'
    fi

    if [ "$state" = "HEREDOC_SKIP" ]; then
      local check_line="$line"
      if [ "$heredoc_striptabs" = "1" ]; then
        while [ "${check_line:0:1}" = $'\t' ]; do
          check_line="${check_line:1}"
        done
      fi
      if [ "$check_line" = "$heredoc_delim" ]; then
        if [ "${#delim_queue[@]}" -gt 0 ]; then
          heredoc_delim="${delim_queue[0]}"
          heredoc_striptabs="${striptabs_queue[0]}"
          delim_queue=("${delim_queue[@]:1}")
          striptabs_queue=("${striptabs_queue[@]:1}")
        else
          state="NORMAL"
        fi
      fi
      # Closing-delimiter line and body lines alike are dropped (not appended).
      continue
    fi

    local n=${#line} i=0 lineout="" c
    while [ "$i" -lt "$n" ]; do
      c="${line:$i:1}"
      case "$state" in
        SQ)
          if [ "$c" = "'" ]; then
            lineout+="'"
            state="NORMAL"
          fi
          i=$((i + 1))
          ;;
        DQ)
          if [ "$c" = '"' ]; then
            lineout+='"'
            state="NORMAL"
            i=$((i + 1))
          elif [ "$c" = '\' ]; then
            i=$((i + 2))
          else
            i=$((i + 1))
          fi
          ;;
        SQ_PASS)
          # Mirrors SQ (single-quote has no escape semantics in real bash)
          # but PASSES CONTENT THROUGH instead of blanking it, AND drops the
          # wrapping quote chars themselves (dequotes) so the interior reads
          # like plain top-level command text to the guards below -- e.g. a
          # trailing dangerous path must be followed by whitespace/EOL to
          # match the rm-rf guard's terminator class; a literal closing `"`
          # right after it would NOT match, silently reintroducing the
          # bypass. Entered only when the quote opened immediately after a
          # known shell interpreter's `-c` flag (see _shell_c_context
          # below): that quoted argument is not narrative, it is a second
          # shell script that will actually execute -- blanking OR
          # re-quoting it would hide a real hazard from the existing guards
          # (adversarial review on #4041).
          if [ "$c" = "'" ]; then
            state="NORMAL"
          else
            lineout+="$c"
          fi
          i=$((i + 1))
          ;;
        DQ_PASS)
          # Mirrors DQ's escape handling but passes content through and
          # drops the wrapping quote chars (see SQ_PASS above for why).
          if [ "$c" = '"' ]; then
            state="NORMAL"
            i=$((i + 1))
          elif [ "$c" = '\' ]; then
            lineout+="$c"
            local pnc="${line:$((i + 1)):1}"
            if [ -n "$pnc" ]; then
              lineout+="$pnc"
              i=$((i + 2))
            else
              i=$((i + 1))
            fi
          else
            lineout+="$c"
            i=$((i + 1))
          fi
          ;;
        NORMAL)
          if [ "$c" = '\' ]; then
            # Backslash OUTSIDE any quote makes the following character
            # literal -- it does NOT start an escape sequence to interpret
            # later, and critically it must NOT let a following `"`/`'`
            # open SQ/DQ (adversarial review on #4041: `\"` in NORMAL was
            # wrongly treated as a bare quote-open, which then desynced the
            # rest of the line/buffer into DQ and silently absorbed a real
            # trailing hazard as if it were quoted content -- a false
            # ALLOW). Consume both the backslash and the escaped char here,
            # emitted verbatim, with no state transition. This must run
            # before the `#`/quote/heredoc checks below so none of them see
            # an escaped char as if it were bare.
            lineout+="$c"
            local nc="${line:$((i + 1)):1}"
            if [ -n "$nc" ]; then
              lineout+="$nc"
              i=$((i + 2))
            else
              i=$((i + 1))
            fi
          elif [ "$c" = "#" ]; then
            i=$n
          elif [ "$c" = "'" ]; then
            # Lookback MUST happen before appending the quote char itself --
            # _shell_c_context matches a trailing `-c` + optional whitespace
            # at the END of the buffer, which the quote char would break.
            # The opening quote char is only appended for the general
            # (blanking) SQ case; SQ_PASS dequotes, so it is dropped here.
            if _shell_c_context "$lineout"; then
              state="SQ_PASS"
            else
              state="SQ"
              lineout+="'"
            fi
            i=$((i + 1))
          elif [ "$c" = '"' ]; then
            # As above: the opening quote char is only appended for the
            # general (blanking) DQ case; DQ_PASS dequotes, so it's dropped.
            if _shell_c_context "$lineout"; then
              state="DQ_PASS"
            else
              state="DQ"
              lineout+='"'
            fi
            i=$((i + 1))
          elif [ "$c" = "<" ] && [ "${line:$((i + 1)):1}" = "<" ] && [ "${line:$((i + 2)):1}" = "<" ]; then
            # here-string `<<<` -- not a heredoc, leave as literal structure.
            lineout+="<<<"
            i=$((i + 3))
          elif [ "$c" = "<" ] && [ "${line:$((i + 1)):1}" = "<" ]; then
            local j=$((i + 2)) mod="" ch="${line:$j:1}"
            if [ "$ch" = "-" ] || [ "$ch" = "~" ]; then
              mod="$ch"
              j=$((j + 1))
            fi
            while [ "${line:$j:1}" = " " ] || [ "${line:$j:1}" = $'\t' ]; do
              j=$((j + 1))
            done
            local qch="${line:$j:1}" quoted=0 qc=""
            if [ "$qch" = "'" ] || [ "$qch" = '"' ]; then
              quoted=1
              qc="$qch"
              j=$((j + 1))
            fi
            local word="" wc
            while :; do
              wc="${line:$j:1}"
              if [ -z "$wc" ]; then
                break
              fi
              if [ "$quoted" = "1" ]; then
                if [ "$wc" = "$qc" ]; then
                  j=$((j + 1))
                  break
                fi
              else
                case "$wc" in
                  [A-Za-z0-9_]) : ;;
                  *) break ;;
                esac
              fi
              word+="$wc"
              j=$((j + 1))
            done
            if [ -n "$word" ]; then
              lineout+="${line:$i:$((j - i))}"
              delim_queue+=("$word")
              if [ "$mod" = "-" ] || [ "$mod" = "~" ]; then
                striptabs_queue+=("1")
              else
                striptabs_queue+=("0")
              fi
              i=$j
            else
              lineout+="$c"
              i=$((i + 1))
            fi
          else
            lineout+="$c"
            i=$((i + 1))
          fi
          ;;
      esac
    done
    out+="$lineout"

    if [ "$state" = "NORMAL" ] && [ "${#delim_queue[@]}" -gt 0 ]; then
      heredoc_delim="${delim_queue[0]}"
      heredoc_striptabs="${striptabs_queue[0]}"
      delim_queue=("${delim_queue[@]:1}")
      striptabs_queue=("${striptabs_queue[@]:1}")
      state="HEREDOC_SKIP"
    fi
  done

  # Fail-safe (adversarial review on #4041): a lexer desync must never
  # produce a false ALLOW. If parsing ends inside an unterminated
  # quote/heredoc (state != NORMAL), $out cannot be trusted -- narrative
  # blanking may have silently absorbed real, unquoted command text (this
  # is exactly the shape a lexer bug or a construct we don't model
  # produces). Rather than ship a possibly-corrupted stripped view, fall
  # back to the RAW input so every guard still sees the untouched command.
  # A string that genuinely leaves a quote/heredoc unterminated is not
  # valid, executable bash in the first place, so the worst case of this
  # fallback is a narrative false-block on malformed input -- never a
  # hazard false-allow. The invariant: strip_narrative may only DROP text
  # it is confident is narrative; when uncertain, it must keep text so a
  # guard can still fire.
  if [ "$state" != "NORMAL" ]; then
    printf '%s' "$input"
    return
  fi

  printf '%s' "$out"
}

CMD_STRIPPED=$(strip_narrative "$CMD")

# Shared regex fragments (#3763 deep-review hardening, 2026-07-11): the
# read-only-shell block below matches a git/gh subcommand immediately after
# the binary name. Two bypasses were found and fixed:
#   1. A bare `([[:space:]]|$)` terminator lets a shell separator ride
#      straight through: `git push;echo`, `git push&&x`, `(git push)` all
#      matched "push" followed by a non-whitespace, non-EOL character that
#      the old class didn't cover. CMD_TERMINATOR adds `; & | < >` (shell
#      separators, mirroring the `rm -rf` guard further down this file) and
#      `)` (a subshell-wrapped command still needs to be caught).
#   2. Requiring the subcommand immediately after `git`/`gh` lets global
#      options hide it: `git -C /repo push`, `git -c user.name=x push`,
#      `git --no-pager push`, `git --git-dir=... push`, and
#      `gh --repo owner/name pr merge 42` all bypassed. GIT_GLOBAL_OPT /
#      GH_GLOBAL_OPT tolerate a repeated run of the common global flags
#      between the binary and its subcommand.
GIT_GLOBAL_OPT='(-C[[:space:]]+[^[:space:]]+|-c[[:space:]]+[^[:space:]]+|--git-dir=[^[:space:]]+|--work-tree=[^[:space:]]+|--no-pager|--paginate|-p)'
GH_GLOBAL_OPT='(--repo[[:space:]]+[^[:space:]]+|-R[[:space:]]+[^[:space:]]+|--hostname[[:space:]]+[^[:space:]]+)'
CMD_TERMINATOR='([[:space:];&|<>)]|$)'

# M4b read-only shell (#3763): review/audit agents are mechanically read-only.
# Their tool allowlist already excludes Edit/Write/NotebookEdit/Agent (see
# `.claude/agents/*.md` `tools:` + `cargo xtask check-agent-capabilities`).
# This block closes the residual mutating-Bash vector so the boundary is a
# TRUE read-only shell: when the invoking agent is spawned with
# CLAUDE_AGENT_READONLY=1, mutating git/gh/filesystem commands are rejected
# BEFORE execution, while read-only inspection (git diff/log/show/status,
# gh pr view/diff/list/checks, cargo check, cat, grep) passes. Review/audit
# agents return findings to an orchestrator; a writer/publisher performs the
# writes. Enable by exporting CLAUDE_AGENT_READONLY=1 in the review-agent spawn.
if [ "${CLAUDE_AGENT_READONLY:-0}" = "1" ] && [ -n "$CMD" ]; then
  # Terminator + global-option hardening (#3763 deep-review, 2026-07-11):
  # CMD_TERMINATOR / GIT_GLOBAL_OPT / GH_GLOBAL_OPT above close bypasses
  # like `git -C /x commit`, `git push;echo`, `(git push)`, and
  # `git --no-pager push` that a bare `([[:space:]]|$)` terminator missed.
  if echo "$CMD_STRIPPED" | grep -qE "(^|[^[:alnum:]_])git([[:space:]]+${GIT_GLOBAL_OPT})*[[:space:]]+(commit|push|merge|rebase|reset|revert|cherry-pick|am|apply|restore|stash|clean|add|rm|mv)${CMD_TERMINATOR}"; then
    echo "Blocked (read-only agent): mutating git command. Review/audit agents return findings; a writer/publisher commits. See #3763 (M4b)." >&2
    exit 2
  fi
  if echo "$CMD_STRIPPED" | grep -qE '(^|[^[:alnum:]_])git[[:space:]]+(checkout[[:space:]]+(-[bB]|--)|switch[[:space:]]+-[cC]|branch[[:space:]]+(-[dDmMcC]|[^-[:space:]])|worktree[[:space:]]+add|tag[[:space:]]+[^-[:space:]])'; then
    echo "Blocked (read-only agent): mutating git command (branch/worktree/tag). See #3763 (M4b)." >&2
    exit 2
  fi
  if echo "$CMD_STRIPPED" | grep -qE "(^|[^[:alnum:]_])gh([[:space:]]+${GH_GLOBAL_OPT})*[[:space:]]+(pr[[:space:]]+(merge|comment|review|edit|close|create|ready|lock|reopen)|issue[[:space:]]+(create|edit|comment|close|delete|lock|reopen|pin|transfer|develop)|label[[:space:]]+(create|edit|delete|clone)|release[[:space:]]+(create|edit|delete|upload)|secret|variable|cache[[:space:]]+delete|workflow[[:space:]]+(run|enable|disable)|run[[:space:]]+(rerun|cancel|delete))${CMD_TERMINATOR}"; then
    echo "Blocked (read-only agent): mutating gh command. Review/audit agents post no comments/labels; a publisher/ops does. See #3763 (M4b)." >&2
    exit 2
  fi
  if echo "$CMD_STRIPPED" | grep -qE '(^|[^[:alnum:]_])gh[[:space:]]+.*--(add|remove)-label'; then
    echo "Blocked (read-only agent): gh label mutation. See #3763 (M4b)." >&2
    exit 2
  fi
  if echo "$CMD_STRIPPED" | grep -qE '(^|[^[:alnum:]_])gh[[:space:]]+api[[:space:]]+.*(-X[[:space:]]*|--method[[:space:]]+)(POST|PATCH|PUT|DELETE)'; then
    echo "Blocked (read-only agent): gh api write request. See #3763 (M4b)." >&2
    exit 2
  fi
  # Filesystem writes: redirection (`>`, `>>` — but not `2>`/`2>&1`), and
  # mutating coreutils. Read-only agents do not modify the filesystem.
  if echo "$CMD_STRIPPED" | grep -qE '>>|[^0-9&>]>[^&>]|(^|[^[:alnum:]_])(tee|dd|truncate|install)[[:space:]]|(^|[^[:alnum:]_])(cp|mv|touch|mkdir|rmdir|ln)[[:space:]]|(^|[^[:alnum:]_])sed[[:space:]]+-[^[:space:]]*i'; then
    echo "Blocked (read-only agent): file-writing command. Review/audit agents do not modify the filesystem. See #3763 (M4b)." >&2
    exit 2
  fi
fi

if echo "$CMD_STRIPPED" | grep -qE 'git push --force($|[[:space:]])|git push -f |git checkout \.|git reset --hard|cargo publish|git clean -fd'; then
  echo "Blocked: dangerous command '$CMD'. Use safer alternatives." >&2
  exit 2
fi

# Block recursive deletes that target the filesystem root or a whole top-level
# system directory. Subpath deletes (e.g. `rm -rf /tmp/foo`, `rm -rf /home/user/proj/target`)
# are allowed — the previous bare `rm -rf /` substring match blocked every absolute path.
# Matched: `rm -rf /`, `rm -rf /*`, `rm -rf /etc`, `rm -rf /home/`, `rm -rf /usr/*`,
#          `rm -rf /run`, `rm -rf /tmp`, `rm -rf /tmp/*` (whole shared-temp wipe removes
#          active build files/sockets in container sessions — per Codex review on PR #1952),
#          separator-terminated forms `rm -rf /etc;`, `rm -rf /etc&`, `rm -rf /etc||`,
#          and a dangerous dir at ANY argument position, e.g. `rm -rf /home/foo /etc`
#          (both per factory-droid review on PR #1952).
# Not matched: `rm -rf /tmp/x`, `rm -rf /home/user/...`, `rm -rf ./target`, `rm -rf /etc-backup`.
# The dir alternation enumerates the FHS / common-Linux top-levels worth protecting (incl.
# /run, /media, /libexec, /snap, /lost+found — added per factory-droid review on PR #1952).
# It is a curated allow-list, not byte-for-byte parity with the old substring match: the goal
# is to catch whole-directory wipes of system top-levels, not every string the old rule caught.
# The `([^ ]+ +)*` prefix lets any earlier whitespace-delimited argument be skipped so the
# dangerous dir is found wherever it appears; the trailing class (whitespace, EOL, or a
# shell separator ; & | < >) ensures only a *whole* dir is matched, not a deeper subpath.
#
# Known, accepted limitations (maintainer-acknowledged on PR #1952):
#   - Quoted dangerous paths are NOT matched: `rm -rf "/etc"` slips through, because the
#     hook does no shell-quote stripping. The old substring match missed this too.
#   - The unanchored `rm ` match can false-positive on command names ending in `rm`:
#     `git rm /etc` is blocked. Anchoring `^rm ` was rejected because it would lose
#     coverage for chained commands like `cmd && rm /etc`. A rare false-block is preferred
#     over a coverage hole for a safety guard.
#   - Subpath deletes under a system dir are intentionally allowed (e.g. `rm -rf /run/docker.sock`):
#     this guard is a coarse net against whole-dir/root wipes, not a per-file protector;
#     re-blocking subpaths would reintroduce the over-blocking this change fixes.
if echo "$CMD_STRIPPED" | grep -qE 'rm +([^ ]+ +)*/(\*?|(bin|boot|dev|etc|home|lib|lib32|lib64|libexec|lost\+found|media|mnt|opt|proc|root|run|sbin|snap|srv|sys|tmp|usr|var)/?\*?)([[:space:];&|<>]|$)'; then
  echo "Blocked: refusing to recursively delete the filesystem root or a whole system directory." >&2
  echo "Deleting a specific subpath (e.g. /tmp/foo, ./target) is allowed." >&2
  exit 2
fi

# Block refspec force-push forms: `git push <remote> +branch`, `git push origin +refs/heads/main`,
# `git push <remote> +HEAD:branch`. The `+` prefix on a refspec bypasses non-fast-forward checks
# the same way --force does.
if echo "$CMD_STRIPPED" | grep -qE 'git +push( +[^ ]+)*[[:space:]]\+[^[:space:]]'; then
  echo "Blocked: refspec '+<ref>' in git push forces non-fast-forward the same as --force." >&2
  echo "Remove the leading '+' or use a safer alternative." >&2
  exit 2
fi

# Block git stash commands -- stash is shared across all worktrees and causes cross-contamination.
# Use git restore <file> to discard changes, or git commit -m wip to save work in progress.
if echo "$CMD_STRIPPED" | grep -qE 'git stash( |$)'; then
  echo "Blocked: git stash is shared across all worktrees and risks cross-contamination." >&2
  echo "Use git restore <file> to discard changes or git commit -m wip to save work." >&2
  exit 2
fi

# Worktree guard (#4464): when CWD is inside a linked worktree, block branch-mutating
# commands that anchor to the wrong location and cause nested-worktree contamination.
# Detection: git-dir != git-common-dir means we're inside a linked worktree, not main.
git_dir=$(git rev-parse --git-dir 2>/dev/null)
common_dir=$(git rev-parse --git-common-dir 2>/dev/null)
if [ -n "$git_dir" ] && [ -n "$common_dir" ] && [ "$git_dir" != "$common_dir" ] && [ "$git_dir" != ".git" ]; then
  # In a linked worktree. Block the subset of git ops that anchor wrong.

  # git worktree add: always block (creates nested worktrees)
  if echo "$CMD_STRIPPED" | grep -qE 'git +worktree +add( |$)'; then
    echo "Blocked: 'git worktree add' inside a linked worktree creates nested worktrees." >&2
    echo "Recovery: cd to main checkout first, then re-run. Main checkout: $(git rev-parse --git-common-dir)/.." >&2
    echo "See #4456 for context." >&2
    exit 2
  fi

  # git switch <branch> (without -c/-C): block branch-switch form
  if echo "$CMD_STRIPPED" | grep -qE 'git +switch( +-[-[:alnum:]]+)* +[^-]'; then
    # Allow -c/-C (create) forms
    if ! echo "$CMD_STRIPPED" | grep -qE 'git +switch +(-c|-C|--create|--force-create)( |$)'; then
      echo "Blocked: 'git switch <branch>' inside a linked worktree changes the worktree's branch." >&2
      echo "Recovery: cd to main checkout first, or use 'git switch -c <new-branch>' to create from current." >&2
      echo "See #4456 for context." >&2
      exit 2
    fi
  fi

  # git checkout <branch>: block branch-switch form (not -b/-B, not -- <file>, not --ours/--theirs)
  if echo "$CMD_STRIPPED" | grep -qE 'git +checkout( |$)'; then
    # Allow safe forms:
    #   git checkout -b / -B / --force        → create-and-switch
    #   git checkout -- <path>                → restore file
    #   git checkout --ours / --theirs        → rebase conflict
    #   git checkout HEAD -- <path>           → restore from HEAD
    if ! echo "$CMD_STRIPPED" | grep -qE 'git +checkout +(-b|-B|--force|-f |--ours|--theirs|--detach|-- +|[a-f0-9]+ +-- +|HEAD +-- +)'; then
      echo "Blocked: 'git checkout <branch>' inside a linked worktree changes the worktree's branch." >&2
      echo "Recovery: cd to main checkout first. Allowed here: 'git checkout -b/-B <new>', 'git checkout -- <file>', 'git checkout --ours/--theirs'." >&2
      echo "See #4456 for context." >&2
      exit 2
    fi
  fi
fi

exit 0
