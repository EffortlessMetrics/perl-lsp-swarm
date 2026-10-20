#!/usr/bin/env bash
cd /h/Code/Muse/perl-lsp-swarm-worktrees/issue-18111-dup-has || exit 1
DEVPLANE=/h/devplane scripts/cargo-admitted test -p perl-lsp-rs-core --locked --lib > /tmp/t18192c.log 2>&1
echo "exit=$?"
grep -E 'test result|FAILED|^error' /tmp/t18192c.log | tail -5
