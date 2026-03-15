---
cargo/nash-plutus: minor
---

Integrate `nash-plutus` (UPLC CEK machine) into the workspace.

Workspace-ify dependencies, rename `uplc_turbo` to `nash_plutus`, upgrade
thiserror v1 to v2, and replace the proc-macro test generator with a dedicated
task crate using `quote` and `cargo fmt`.
