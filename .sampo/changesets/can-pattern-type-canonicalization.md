---
cargo/nash-can: minor
---

# Add pattern and type annotation canonicalization

Extract type canonicalization into `types.rs` with `to_annotation` and free
variable collection. Add `pattern.rs` with constructor resolution, arity
checks, and duplicate binder detection. Add `TupleLargerThanThree` validation
to both type and pattern paths.
