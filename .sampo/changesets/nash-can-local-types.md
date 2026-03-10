---
cargo/nash-can: minor
---

Extend `nash-can` module canonicalization with real lowering for local unions, aliases, and local named types.

This slice removes the temporary unsupported-content pseudo-errors, canonicalizes alias and union exports against local declarations, and supports unqualified and self-qualified references to local named types while leaving imports and value canonicalization for later slices.
