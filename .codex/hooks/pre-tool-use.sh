#!/usr/bin/env bash
# PreToolUse hook: block dangerous bash commands before execution
# Exit 2 = block with feedback
# Exit 0 = allow

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

if echo "$CMD" | grep -qE 'git push --force($|[[:space:]])|git push -f |git checkout \.|git reset --hard|cargo publish|git clean -fd'; then
  echo "Blocked: dangerous command '$CMD'. Use safer alternatives." >&2
  exit 2
fi

# Block recursive deletes that target the filesystem root or a whole top-level
# system directory. Subpath deletes (for example /tmp/foo or ./target) remain allowed.
if echo "$CMD" | grep -qE 'rm +([^ ]+ +)*/(\*?|(bin|boot|dev|etc|home|lib|lib32|lib64|libexec|lost\+found|media|mnt|opt|proc|root|run|sbin|snap|srv|sys|tmp|usr|var)/?\*?)([[:space:];&|<>]|$)'; then
  echo "Blocked: refusing to recursively delete the filesystem root or a whole system directory." >&2
  echo "Deleting a specific subpath (for example /tmp/foo or ./target) is allowed." >&2
  exit 2
fi

# Refspecs beginning with + force non-fast-forward updates.
if echo "$CMD" | grep -qE 'git +push( +[^ ]+)*[[:space:]]\+[^[:space:]]'; then
  echo "Blocked: refspec '+<ref>' forces a non-fast-forward update." >&2
  echo "Remove the leading '+' or use a safer alternative." >&2
  exit 2
fi

# Stash is shared by all worktrees.
if echo "$CMD" | grep -qE 'git stash( |$)'; then
  echo "Blocked: git stash is shared across worktrees and risks cross-contamination." >&2
  echo "Use git restore for a scoped discard or a WIP commit to preserve work." >&2
  exit 2
fi

# Linked-worktree guard: do not create nested worktrees or switch the current
# linked worktree to another existing branch.
git_dir=$(git rev-parse --git-dir 2>/dev/null)
common_dir=$(git rev-parse --git-common-dir 2>/dev/null)
if [ -n "$git_dir" ] && [ -n "$common_dir" ] && [ "$git_dir" != "$common_dir" ] && [ "$git_dir" != ".git" ]; then
  if echo "$CMD" | grep -qE 'git +worktree +add( |$)'; then
    echo "Blocked: git worktree add inside a linked worktree creates nested worktrees." >&2
    echo "Run it from the main checkout instead." >&2
    exit 2
  fi

  if echo "$CMD" | grep -qE 'git +switch( +-[-[:alnum:]]+)* +[^-]'; then
    if ! echo "$CMD" | grep -qE 'git +switch +(-c|-C|--create|--force-create)( |$)'; then
      echo "Blocked: git switch <branch> inside a linked worktree changes its assigned branch." >&2
      echo "Run it from the main checkout or create a new branch with git switch -c." >&2
      exit 2
    fi
  fi

  if echo "$CMD" | grep -qE 'git +checkout( |$)'; then
    if ! echo "$CMD" | grep -qE 'git +checkout +(-b|-B|--force|-f |--ours|--theirs|--detach|-- +|[a-f0-9]+ +-- +|HEAD +-- +)'; then
      echo "Blocked: git checkout <branch> inside a linked worktree changes its assigned branch." >&2
      echo "Run it from the main checkout; path restore and conflict-resolution forms remain allowed." >&2
      exit 2
    fi
  fi
fi

exit 0
