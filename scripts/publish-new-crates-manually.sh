#!/usr/bin/env bash
# Retired ambient publisher (#16802). No argument or environment enables it.
printf '%s\n' \
  'Direct local crate publication is disabled.' \
  'Follow the reviewed release transaction (#13768 / #6055) and docs/release/RUNBOOK.md.' >&2
exit 1
