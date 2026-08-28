# Badge endpoints

This directory contains generated Shields endpoint JSON used by README badges.

Regenerate:

```bash
python3 scripts/generate-badges.py
```

Check drift:

```bash
python3 scripts/generate-badges.py --check
```

Only committed `*.json` endpoint files are public badge surfaces. Detailed reports stay in CI artifacts and `target/`.
