# Checklist: #10712 + #11164 + #10333 route-preparation slice

One writer, one PR (`ux/issue-8087`). Conflict key: `distribution.train_control_plane`.

## Done in this slice

- [x] Encode the full #10712 required graph as `distribution_train.v1` (79 nodes).
- [x] Reject-list controllers (#10703 #8686 #5903 #6056 #8087 #7718 #7816 #6055 #4350
      #4351 #9146 #8970) encoded as `controller`/`release_handoff` with `do_not_build`.
- [x] Assign every node exactly one #11164 spec disposition.
- [x] Compile the eleven durable route packets (#10333, #11432, #11434, #11443, #11445,
      #11447, #11449, #11456, #11458, #11461, #11463) under `.spec/<issue>-<slug>/`.
- [x] `cargo xtask distribution-train check|graph|explain|next` (#10712).
- [x] `cargo xtask distribution-spec check|explain|index` (#11164).
- [x] `public_release_claims.v2` schema + typed Rust model/validator + fixtures +
      historical-v1 disposition (#10333).
- [x] Mutation fixtures: controller-as-implementation, cyclic hard edge, mutable field,
      satellite hard-dep, npm route, controller packet, packetless SPEC_COMPILED.

## Explicitly not in this slice

- Route population, evidence adapters (#11432 and later are the next writer's leaves).
- Eligibility/preference, renderer, guidance migration, parity gates, literal cutover.
- Standalone packet rail (#11485) and managed packet rail (#11161) — open sibling specs.
- Live observation (#10726); installer/package/channel/topology behavior changes.

## Next unblocked node

#11432 (R03B-E00 evidence-join substrate) — `ready` in the checked snapshot once this
PR lands, per `cargo xtask distribution-train next`.
