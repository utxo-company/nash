---
cargo/nash-cli: patch
---

Fix the compiler version proxy: guard against a cached binary whose version doesn't match its folder name exec'ing itself in a silent infinite loop (now a clear error via the `NASH_PROXY_VERSION` env var handshake), and fix the release asset name to match what cargo-dist actually publishes (`nash-cli-{target}` instead of `nash-{target}`), which made every proxy download fail.
