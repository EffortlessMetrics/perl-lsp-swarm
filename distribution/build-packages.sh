#!/usr/bin/env bash
set -euo pipefail
cat >&2 <<'MESSAGE'
distribution/build-packages.sh is retired historical scaffolding.
It encoded an independent 1.0.0/amd64/server-only DEB, RPM, tarball,
man-page, and systemd product model outside current release topology.
Reintroduction requires a new topology-backed owner and public-channel evidence.
MESSAGE
exit 64
