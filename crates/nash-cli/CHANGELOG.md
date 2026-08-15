# nash-cli

## 0.2.1 — 2026-08-08

### Patch changes

- [1c5f4a8](https://github.com/utxo-company/nash/commit/1c5f4a803a5f6f7f8178fc02d8edbd89bf234c32) Fix the compiler version proxy: guard against a cached binary whose version doesn't match its folder name exec'ing itself in a silent infinite loop (now a clear error via the `NASH_PROXY_VERSION` env var handshake), and fix the release asset name to match what cargo-dist actually publishes (`nash-cli-{target}` instead of `nash-{target}`), which made every proxy download fail. — Thanks @rvcas!
- Updated dependencies: nash-driver@0.2.0

## 0.2.0 — 2026-03-10

### Minor changes

- [45b79ae](https://github.com/utxo-company/nash/commit/45b79ae1c94701b4f16a612e381a57c986b099e0) Add the minimal Nash language server skeleton and CLI entry point for running it over stdio.
  
  Includes commit `f2b7657c1fa70a22dc2ac450ec54cc0623bd2f3f` and the follow-up cleanup to the initial LSP scaffolding. — Thanks @MicroProofs!

### Patch changes

- Updated dependencies: nash-language-server@0.2.0

## 0.1.3 — 2026-02-20

### Patch changes

- [8a40dbe](https://github.com/nash-script/compiler/commit/8a40dbe43869de6453b6a7bb5dce04d3b900efcf) Add module-level doc comments to lib.rs. — Thanks @rvcas!

## 0.1.2 — 2026-02-20

### Patch changes

- [5cc62cc](https://github.com/nash-script/compiler/commit/5cc62cc9ce5568bb779f542b872909dc71942578) Add explicit about text to CLI help output. — Thanks @rvcas!

## 0.1.1 — 2026-02-20

### Patch changes

- [c46f722](https://github.com/nash-script/compiler/commit/c46f72228fb027163b0590fa9075edc8eeff39c7) Unify into a single `nash` binary with clap for argument parsing, octocrab for downloading missing compiler versions from GitHub Releases, and automatic version proxying before clap runs. — Thanks @rvcas!
- Updated dependencies: nash-config@0.3.0, nash-driver@0.1.1

