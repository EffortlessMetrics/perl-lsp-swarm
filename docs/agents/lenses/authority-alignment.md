# Authority alignment lens

Determine who owns the fact, who consumes it, and whether the candidate creates or bypasses another authority.

Check:

- canonical owner and public interface;
- downstream consumers and generated artifacts;
- source ordering and freshness;
- compatibility and migration boundaries;
- duplicate caches, registries, validators, or state machines;
- whether the proposed location is the smallest correct owner.

Return the current owner, intended owner, consumers, migration/deletion boundary, and unresolved contradictions.
