---
cargo/nash-source: minor
---

Derive `Clone` and `Copy` for `Associativity` and `Precedence`. These derives have existed in the workspace since March but were never released, so the published crate could not support dependents that embed these types in `Copy` structs (publishing `nash-can` failed with E0204 against the registry copy).
