# Explore the Perl LSP demo

Open `main.pl` and wait for the workspace index to become ready. The example runs
without CPAN dependencies: `perl main.pl` prints a summary of five numbers.

- Place the cursor after `Utils::` or `Database::` in `main.pl` to see functions
  defined by this project. Use `Utils::` for `load_data` and `process_data`, and
  `Database::` for `save`.
- Use Go to Definition on `Utils::process_data`, then Find All References to
  follow the call back to `main.pl`.
- Hover over `Utils::load_data` or `Database::save` to inspect the symbol.
- To try diagnostics, temporarily remove the closing `)` from
  `Utils::process_data($data)`. Restore it to clear the syntax error.

The files open without deliberate diagnostics. The temporary error is an exercise;
restore the original call before running the program again.
