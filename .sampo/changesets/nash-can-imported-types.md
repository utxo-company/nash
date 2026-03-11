---
cargo/nash-can: minor
---

Add imported interface support for canonical type resolution in `nash-can`.

This slice threads imported module interfaces through canonicalization so alias and union types can resolve from exposed imports and qualified import prefixes while leaving value canonicalization and broader import/export resolution for later slices.
