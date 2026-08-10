# Production-path lens

Verify that a real user request can reach the changed behavior through current production wiring.

Trace:

```text
entry request
→ routing and capability admission
→ semantic owner
→ changed implementation
→ emitted fact or response
→ packaging/runtime boundary
→ visible result
```

Component tests do not establish production reachability. Identify fallback, stale-generation, disabled-capability, packaging, and extension-host boundaries. Return the proven path and every remaining gap.
