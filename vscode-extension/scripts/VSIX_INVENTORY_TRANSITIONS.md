# Review a VSIX inventory transition

Commit tracked package sources and build the candidate VSIX first. Then measure
that exact archive against the accepted base baseline, using full commit SHAs:

```sh
node scripts/propose-vsix-inventory-transition.js \
  --base <accepted-base-sha> --candidate <current-head-sha> \
  --vsix <produced-vsix-path> --owner-issue <issue-number> \
  --reason "Specific cause and intent of this package change"
```

The default prints a proposal without changing files. Inspect the archive digest,
file additions and removals, size changes, and `package_policy_class`. An unexpected
structural change requires its own disposition; a proposal does not accept it.

Repeat the command with `--write` to prepare the canonical baseline and transition
declaration for review. It refuses a moved HEAD, an unrelated base, an independently
edited baseline or declaration, dirty tracked sources, a changed archive, or a
no-change proposal. The existing transition
validator remains the gate. Run it against the same exact VSIX after reviewing the
written diff:

```sh
node scripts/check-vsix-inventory-transition.js --base <accepted-base-sha> \
  --vsix <produced-vsix-path>
```

The proposal measures package inventory. It does not establish installed behavior,
native payload identity, or that the supplied archive was produced from the current
source commit; preserve independent build provenance and installed proof.
