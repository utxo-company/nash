# Nash canonicalization parity rollout

## Goal
Continue `nash-can` and `nash-ast` toward Elm-style canonicalization for the core language while preserving intentional Nash differences for a smart-contract language.

## Ground rules from research
- Preserve Nash-specific omissions: no ports, effects, or managers. `nash-source` and `nash-parse` explicitly omit them.
- Treat Elm as the reference for the core canonicalization contract: imports, exports, env construction, duplicates, name resolution, value/expr/pattern/type canonicalization, operator desugaring, and cycle detection.
- Do full cutover on public API cleanup. There are currently no external consumers of `nash-can::{canonicalize_header, canonicalize_module}` beyond `nash-can` re-exports and crate-local tests, so boundary cleanup is low blast radius.
- Avoid speculative `nash-ast` churn. Change AST only where the canonicalizer proves a real gap.

## Distinct diffs

### Diff 1: Public API boundary cutover in `nash-can`
**Purpose**
Match Elm’s phase boundary more closely by exposing one public module-level entrypoint instead of a public header-only slice.

**Files**
- `crates/nash-can/src/lib.rs`
- `crates/nash-can/src/module.rs`
- `crates/nash-can/src/snapshots/*` as needed

**Changes**
- Replace public `canonicalize_module(...)` with public `canonicalize(...) -> Result<CanModule, Error>`.
- Stop exporting `Header` and `canonicalize_header` publicly.
- Keep header lowering as a private helper inside `module.rs` if it still reduces duplication.
- Update tests to drive the public API through `canonicalize` rather than the split API.

**Notes**
- Keep the Rust-specific `bump: &Bump` parameter.
- Keep `Context` for now, but treat it as transitional input plumbing rather than the final environment model.

**Verification**
- `cargo test -p nash-can`

### Diff 2: Error surface and panic removal for already-implemented paths
**Purpose**
Replace `todo!` panics in the existing type/export/import slice with typed canonicalization errors so callers can trust failures.

**Files**
- `crates/nash-can/src/error.rs`
- `crates/nash-can/src/module.rs`
- `crates/nash-can/src/snapshots/*`

**Changes**
- Expand `Error` beyond `MissingModuleHeader` to cover the paths the crate already reaches today:
  - unresolved named type
  - unresolved qualified named type
  - missing imported interface
  - ambiguous imported type
  - bad type arity
  - unresolved exported upper name
- Convert all currently reachable `todo!`s in those paths into `Result` returns.
- Add negative snapshot tests for each new error.

**Why before larger implementation**
- It hardens the current slice before adding more moving parts.
- It prevents later value/env work from building on panic-driven control flow.

**Verification**
- `cargo test -p nash-can`

### Diff 3: Canonical interface model expansion
**Purpose**
Make imported-module information truthful enough for Elm-style canonicalization.

**Files**
- `crates/nash-can/src/interface.rs`
- `crates/nash-can/src/lib.rs`
- `crates/nash-can/src/module.rs`
- follow-up impact only: `crates/nash-driver/src/interface.rs`

**Changes**
- Extend `nash-can::Interface` beyond aliases/unions to include at least:
  - values
  - unions
  - aliases
  - binops
  - enough constructor visibility information to distinguish open vs closed union exports
- Add an interface extraction helper analogous to Elm’s `Elm.Interface.fromModule`, but scoped to Nash’s current language surface.
- Do not touch ports/effects.

**Design choice**
- Keep interface types in `nash-can`, not `nash-ast`, unless a later consumer proves a better home. Elm keeps interface data separate from canonical AST.

**Verification**
- Unit tests for interface extraction from canonical modules
- `cargo test -p nash-can`

### Diff 4: Environment scaffolding in `nash-can`
**Purpose**
Stop treating `Context` as the environment. Build a real canonicalization env from imports plus locals, like Elm’s `Foreign.createInitialEnv` + `Local.add`.

**Files**
- `crates/nash-can/src/module.rs`
- new files, likely:
  - `crates/nash-can/src/environment.rs`
  - `crates/nash-can/src/environment/foreign.rs`
  - `crates/nash-can/src/environment/local.rs`
  - `crates/nash-can/src/environment/dups.rs`
- `crates/nash-can/src/error.rs`

**Changes**
- Build exposed and qualified lookup tables for:
  - values
  - types
  - constructors
  - binops
- Validate imports while building the env:
  - imported module exists
  - exposed items exist
  - alias/qualification conflicts
  - ambiguity from multiple open imports
- Add local declarations/types/ctors/aliases into the env before value canonicalization.

**Why separate from value canonicalization**
- This is the contract the rest of canonicalization consumes.
- It shrinks later diffs by giving expression/pattern/type code one lookup surface.

**Verification**
- Targeted env/import snapshot tests
- `cargo test -p nash-can`

### Diff 5: Local validation pass and minimal `nash-ast` truthfulness cleanup
**Purpose**
Port Elm’s structural validations that belong to the local declaration/type layer, and only then make any AST cleanup proven necessary.

**Files**
- `crates/nash-can/src/environment/local.rs`
- `crates/nash-can/src/environment/dups.rs`
- `crates/nash-can/src/error.rs`
- maybe `crates/nash-ast/src/lib.rs` if validation work proves the current declaration tail or metadata names are misleading

**Changes**
- Detect and report duplicates for:
  - value names
  - type names
  - constructors
  - record fields
  - pattern binders
  - export lists / explicit import exposure lists
- Check alias cycles.
- Check type-variable binding and arity rules.
- Only change `nash-ast` if this pass proves a real semantic mismatch. Current default is to keep the AST stable and not chase superficial Elm naming.

**Important constraint**
- Do not add Elm effects/ports AST nodes.
- Do not rename `Decls::Empty` just for parity. Rename only if the implementation proves that the current name lies about semantics.

**Verification**
- New negative snapshots for each duplicate/cycle/binding class
- `cargo test -p nash-can`

### Diff 6: Pattern and type annotation canonicalization
**Purpose**
Finish the type/pattern layer so value bodies can be canonicalized correctly.

**Files**
- `crates/nash-can/src/module.rs` or split-out modules:
  - `crates/nash-can/src/pattern.rs`
  - `crates/nash-can/src/types.rs`
- `crates/nash-can/src/error.rs`
- `crates/nash-ast/src/lib.rs` only if a concrete type/pattern gap appears during implementation

**Changes**
- Canonicalize annotations into `Annotation { free_vars, typ }`.
- Canonicalize constructor patterns through the env, including:
  - qualified vs unqualified lookup
  - arity checks
  - tuple/record/list/cons forms
  - duplicate binder detection
- Reuse the env introduced in Diff 4 instead of ad hoc lookup helpers in `module.rs`.

**Verification**
- Snapshot tests for typed defs, constructor patterns, bad arity, duplicate binders
- `cargo test -p nash-can`

### Diff 7: Expression canonicalization and operator desugaring
**Purpose**
Port the core expression layer so `nash-can` actually resolves names and desugars syntax, as SPEC Phase 3 promises.

**Files**
- new file, likely `crates/nash-can/src/expression.rs`
- `crates/nash-can/src/module.rs`
- `crates/nash-can/src/error.rs`
- `crates/nash-can/src/snapshots/*`

**Changes**
- Canonicalize vars to local vs top-level vs constructor vs operator references.
- Resolve qualified and unqualified names through the env.
- Desugar source `Expr::BinOps` chains into canonical `Expr::Binop` trees using precedence/associativity.
- Canonicalize let/case/access/update/record/tuple/list/call/lambda/if.
- Preserve region information.

**Verification**
- Targeted snapshots for operator precedence/associativity, name resolution, let/case lowering
- `cargo test -p nash-can`

### Diff 8: Value canonicalization, SCC grouping, and exported interface collection
**Purpose**
Finish the phase boundary promised by SPEC: value canonicalization, dependency ordering, and public interface collection.

**Files**
- `crates/nash-can/src/module.rs`
- likely `crates/nash-can/src/values.rs`
- `crates/nash-can/src/interface.rs`
- `crates/nash-can/src/error.rs`
- downstream follow-up: `crates/nash-driver/src/interface.rs`

**Changes**
- Replace `canonicalize_decls(... todo!)` with Elm-style value canonicalization.
- Compute declaration SCCs and lower to `Decls::{Declare, DeclareRec, ...}`.
- Canonicalize explicit exports against the full env, including values/binops/types/union privacy.
- Produce a trustworthy module interface from the finished canonical module.
- After this lands, plan the driver follow-up to serialize the richer interface instead of the current export-only placeholder.

**Verification**
- Positive and negative snapshots for recursive groups, bad recursive cycles, export validation, imported value use
- `cargo test -p nash-can`

## Recommended order
1. Diff 1: public API cutover
2. Diff 2: error surface hardening
3. Diff 3: interface model expansion
4. Diff 4: environment scaffolding
5. Diff 5: local validation
6. Diff 6: pattern/type annotation canonicalization
7. Diff 7: expression canonicalization
8. Diff 8: values, SCCs, exports, interface collection

## What should intentionally stay different from Elm
- No `Effects`, `Port`, or `Manager` nodes in `nash-ast`.
- No effect/port canonicalization entrypoints in `nash-can`.
- Keep the arena parameter in public Rust APIs.
- Keep the smart-contract language scope: canonicalization should support the current Nash source language, not Elm browser/runtime features.

## Risks to watch
- Interface model duplication: `nash-can::Interface` and `nash-driver::Interface` currently describe different truths. Do not let both evolve independently for long.
- Error taxonomy drift: if errors are added ad hoc while implementing values/exprs, the result will be a grab bag instead of a coherent canonicalization contract.
- AST churn without proof: most of the remaining work is implementation in `nash-can`, not large new surface in `nash-ast`.

## Immediate next diff
Start with Diff 1. It is low blast radius, fixes a real Elm boundary drift, and makes all following work happen behind the correct public entrypoint.
