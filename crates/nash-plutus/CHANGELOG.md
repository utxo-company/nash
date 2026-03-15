# nash-plutus

## 0.1.0 — 2026-03-15

### Minor changes

- [001497d](https://github.com/utxo-company/nash/commit/001497df610aa7cd599ba20d699d519862c835ea) Integrate `nash-plutus` (UPLC CEK machine) into the workspace.
  
  Workspace-ify dependencies, rename `uplc_turbo` to `nash_plutus`, upgrade
  thiserror v1 to v2, and replace the proc-macro test generator with a dedicated
  task crate using `quote` and `cargo fmt`. — Thanks @rvcas!

