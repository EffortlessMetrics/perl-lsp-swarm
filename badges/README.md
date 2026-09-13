# Badge endpoints

This directory contains generated Shields endpoint JSON used by README badges.

## Automatic refresh

On normal default-branch pushes, `.github/workflows/badge-endpoints.yml`
consumes the exact-SHA RIPR receipt produced by `.github/workflows/ripr.yml`.
It renders the public endpoint from that receipt and does not start a second
repository-wide RIPR scan.

## Manual or local regeneration

The canonical generator is:

```bash
python3 scripts/generate-badges.py
```

Check committed endpoint drift without modifying committed endpoint files:

```bash
python3 scripts/generate-badges.py --check
```

The check still writes its computed payload to
`target/xtask/badges/ripr-plus.json`; it leaves the committed `badges/*.json`
endpoint files untouched.

Without a supplied receipt, manual generation runs RIPR directly.
`cargo xtask badges` and `cargo xtask badges --check` are deprecated
compatibility entrypoints: they delegate to the Python generator and do not
own badge semantics.

Committed `*.json` files are the public badge endpoint payloads. The automatic
workflow currently stages the entire `badges/` directory, including this
maintainer README, and its write-enabled path publishes that directory as a
whole. Documentation here is therefore part of the operational publication
surface even though it is not an endpoint. Detailed reports stay in CI artifacts
and `target/`.
