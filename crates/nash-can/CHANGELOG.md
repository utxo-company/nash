# nash-can

## 0.2.0 — 2026-03-15

### Minor changes

- [4276751](https://github.com/utxo-company/nash/commit/427675123bfd01ff43fa423cbafc108ba2e88994) Add module header canonicalization to `nash-can`.
  
  The new API builds canonical module names and exports from parsed module headers,
  preserves package context, and reports missing explicit headers. — Thanks @MicroProofs!
- [546fd0d](https://github.com/utxo-company/nash/commit/546fd0dfcb132c51530e15c23a9e2308cc9b0cda) Add imported interface support for canonical type resolution in `nash-can`.
  
  This slice threads imported module interfaces through canonicalization so alias and union types can resolve from exposed imports and qualified import prefixes while leaving value canonicalization and broader import/export resolution for later slices. — Thanks @MicroProofs!
- [4dfbdda](https://github.com/utxo-company/nash/commit/4dfbdda549ea5b18d85736962ca64df731c1bea4) Extend `nash-can` module canonicalization with real lowering for local unions, aliases, and local named types.
  
  This slice removes the temporary unsupported-content pseudo-errors, canonicalizes alias and union exports against local declarations, and supports unqualified and self-qualified references to local named types while leaving imports and value canonicalization for later slices. — Thanks @MicroProofs!

### Patch changes

- Updated dependencies: nash-ast@0.2.0

