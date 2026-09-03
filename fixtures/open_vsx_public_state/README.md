# Open VSX public-state fixtures

Instrument inputs for `xtask::open_vsx_public_state` (`#9923`, incident `#9129`).

**Every file here is synthetic.** None is a recorded observation of the live
Open VSX registry, and none should be cited as evidence of what the registry
returned at any moment. They exist to pin classifier behavior — in particular
that provider failure, instrument gaps and contradictory answers never collapse
into proven absence — against inputs a real probe could produce.

Digests are fabricated, well-formed placeholders. They prove that comparison
happens and that a mismatch is recorded as a `public_digest_mismatch` blocker;
they are not the digest of any real published package. No observation reaches
`available_exact` at all — this classifier holds no resolved candidate authority,
so a matching digest and a mismatching one both stop below it, for different
reasons the blockers name.

`incident_shape_listing_absent.json` encodes the *shape* described in `#9129` —
historical v0.17.0 publish references alongside an unreachable current listing —
so the classifier's answer to that shape is pinned. It is not a transcript of
the 2026-08-14 observation. Producing a real receipt for the live identity
requires running the probe against the network; that observation belongs to the
incident, not to this directory.
