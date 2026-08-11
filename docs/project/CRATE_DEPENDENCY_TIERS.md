# Crate dependency boundaries — current-source pointer

This page previously presented a frozen numeric tier model and a 116-member inventory. That topology is no longer current: the workspace has absorbed multiple former microcrates and the surviving package boundaries have changed.

Use these authorities instead:

- [Cargo.toml](../Cargo.toml) for exact workspace membership, exclusions, workspace dependencies, and the publish allowlist;
- [Architecture Overview](../reference/ARCHITECTURE.md) for current crate families and contributor seam selection;
- package-local READMEs and manifests for the dependency contract of a specific crate;
- [the publishing guide](PUBLISHING_AFTER_COLLAPSE.md) for the current release and publish topology.

The publish allowlist is intentionally ordered for publication dependencies, but it is not a complete architectural tier model. Do not infer current crate existence, dependency direction, build parallelism, or API stability from the retired tables below this pointer. Historical tier assignments remain useful only as migration archaeology.

When documenting a dependency boundary, name the current owning package and link to its manifest or README. When a former crate has been absorbed, use the absorption comment in Cargo.toml and the surviving module path as the authority.
