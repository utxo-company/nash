# nash-source

## 0.3.0 — 2026-08-15

### Minor changes

- [c56025b](https://github.com/utxo-company/nash/commit/c56025bbfa8da9b042add37ceab2de17e0ffaf35) Derive `Clone` and `Copy` for `Associativity` and `Precedence`. These derives have existed in the workspace since March but were never released, so the published crate could not support dependents that embed these types in `Copy` structs (publishing `nash-can` failed with E0204 against the registry copy). — Thanks @rvcas!

