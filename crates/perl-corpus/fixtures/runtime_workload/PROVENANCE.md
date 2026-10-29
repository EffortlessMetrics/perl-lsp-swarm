# Provenance: spectralnorm.perl

Owner: #17536 under #17511 / #17509. Attributed real-corpus workload, not a
synthetic case, executable profile, or conformance result.

## Upstream identity

- Project: The Computer Language Benchmarks Game (maintained workload corpus)
- Program: `spectral-norm Perl #1`
- Contributors (from source header): Andrew Rodland; modified by R. Jelinek
- Repository: `https://salsa.debian.org/benchmarksgame-team/benchmarksgame`
- Commit: `40296663ed350d5fe4a6ab5e367bab61cb77c219`
  (master HEAD, 2025-03-07, site version 25.03)
- File: `public/program/spectralnorm-perl-1.html`
- Blob: `f41b3d7c06ddc809ee8e8b7c16fdcc7f377f7aa5` (page bytes: 8463)
- Byte range: `<pre>` source-code section, bytes 1904-7939
- Source page:
  `https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/spectralnorm-perl-1.html`

Raw `.perl` sources are absent from the upstream repo (verified 2026-10-12:
`bencher/programs/spectralnorm` and `bencher/programs/nbody` carry no perl
files), so the generated page section above is the pinned source. It is recorded
as such, not as a repo `.perl` file.

## Extraction transform (mechanical, diffable)

1. Take bytes 1904-7939 of the page (the `<pre>...</pre>` source section).
2. Strip syntax-highlight `<span ...>` / `</span>` tags.
3. Decode HTML entities (only `&#64;` to `@` occurs, in `@ARGV` and `@v`).
4. Ensure exactly one trailing newline (LF).

No reformatting, renaming, or semantic change. The vendored file is 985 bytes,
LF-only, with exactly one final newline.

## Integrity

- sha256 of `spectralnorm.perl`: `c7ad6bce4e8d44c5dd5892de3e480005d09f4d1eb54a3c3fd125e27facfdbc51`
- Verify: `sha256sum spectralnorm.perl`
  (Windows: `(Get-FileHash -Algorithm SHA256 spectralnorm.perl).Hash`)
- The `workload.t` driver recomputes this hash and fails when the bytes differ.

## License

- Grant: Revised BSD (3-Clause), confirmed at the project's `license.html` before copying.

Full text: `LICENSE-benchmarksgame` in this directory. Redistribution retains the
copyright notice, conditions, and disclaimer; no endorsement use.

## Reference observations

Upstream published output for input `5500`, recorded on the program page:

- Command: `perl spectralnorm.perl 5500`
- Output: `1.274224153`
- Perl: 5.40.0, x86_64-linux-thread-multi, 2025-01-16

Local frozen inputs and cross-cell observations are recorded in `README.md`.
