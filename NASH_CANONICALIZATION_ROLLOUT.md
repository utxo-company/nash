# Nash canonicalization parity rollout

## Goal

Continue `nash-can` and `nash-ast` toward Elm-style
canonicalization for the core language while preserving
intentional Nash differences for a smart-contract language.

## Ground rules from research

- Preserve Nash-specific omissions: no ports, effects,
  or managers. `nash-source` and `nash-parse` omit them.
- Treat Elm as the reference for the core canonicalization
  contract: imports, exports, env construction, duplicates,
  name resolution, value/expr/pattern/type canonicalization,
  operator desugaring, and cycle detection.
- Do full cutover on public API cleanup. No external
  consumers of `nash-can::{canonicalize_header,
  canonicalize_module}` beyond re-exports and crate-local
  tests, so boundary cleanup is low blast radius.
- Avoid speculative `nash-ast` churn. Change AST only
  where the canonicalizer proves a real gap.

## Evidence for no `Effects`, `Port`, or `Manager` in Nash scope

Source AST only models imports, values, unions, aliases, and binops:

```rust
pub struct Module<'a> {
    pub name: Option<&'a Located<&'a str>>,
    pub exports: &'a Located<Exposing<'a>>,
    pub docs: &'a Docs<'a>,
    pub imports: &'a [&'a Import<'a>],
    pub values: &'a [&'a Located<Value<'a>>],
    pub unions: &'a [&'a Located<Union<'a>>],
    pub aliases: &'a [&'a Located<Alias<'a>>],
    pub binops: &'a [&'a Located<Infix<'a>>],
}
```

`crates/nash-source/src/lib.rs` has no source nodes for effects, ports, or managers.

Parser docs explicitly narrow the language surface:

```rust
/// Mirrors Elm's `chompHeader` (simplified - no port/effect modules):
```

and

```rust
///         , portDecl maybeDocs  -- skipped in Nash
```

So these omissions are intentional language-scope
differences, not missing parity work. We still use Elm as
the reference for the core canonicalization contract, but
not for Elm runtime/web-module features.

## Distinct diffs

### Diff 1: Public API boundary cutover in `nash-can`

**Purpose**
Match Elm’s phase boundary more closely by exposing one
public module-level entrypoint instead of a public
header-only slice.

#### Files

- `crates/nash-can/src/lib.rs`
- `crates/nash-can/src/module.rs`
- `crates/nash-can/src/snapshots/*` as needed

#### Changes

- Replace public `canonicalize_module(...)` with
  `canonicalize(...) -> Result<CanModule, Error>`.
- Stop exporting `Header` and `canonicalize_header`.
- Keep header lowering as a private helper inside
  `module.rs` if it still reduces duplication.
- Update tests to drive through `canonicalize`
  rather than the split API.

#### Notes

- Keep the Rust-specific `bump: &Bump` parameter.
- Keep `Context` for now, but treat it as transitional
  input plumbing rather than the final environment model.

#### Verification

- `cargo test -p nash-can`

#### Representative snippet

```rust
pub use crate::module::{Context, canonicalize};

pub fn canonicalize<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    module: &SourceModule<'a>,
) -> Result<CanModule<'a>, Error> {
    let header = canonicalize_header(context, module)?;
    lower_module(bump, context, header, module)
}
```

### Diff 2: Error surface and panic removal for already-implemented paths

**Purpose**
Replace `todo!` panics in the existing
type/export/import slice with typed canonicalization
errors so callers can trust failures.

**Files**:

- `crates/nash-can/src/error.rs`
- `crates/nash-can/src/module.rs`
- `crates/nash-can/src/snapshots/*`

**Changes**:

- Expand `Error` beyond `MissingModuleHeader` to cover
  the paths the crate already reaches today:
  - unresolved named type
  - unresolved qualified named type
  - missing imported interface
  - ambiguous imported type
  - bad type arity
  - unresolved exported upper name
- Convert all currently reachable `todo!`s in those paths into `Result` returns.
- Add negative snapshot tests for each new error.

#### Why before larger implementation

- It hardens the current slice before adding more moving parts.
- It prevents later value/env work from building on panic-driven control flow.

**Verification**::

- `cargo test -p nash-can`

**Representative snippet**::

```rust
pub enum Error<'a> {
    MissingModuleHeader,
    UnknownNamedType { name: &'a str, region: Region },
    UnknownQualifiedType { module: &'a str, name: &'a str, region: Region },
    MissingImportedInterface { module: &'a str, region: Region },
    AmbiguousImportedType { name: &'a str, region: Region },
    BadTypeArity {
        kind: &'static str, name: &'a str,
        expected: usize, actual: usize, region: Region,
    },
    UnknownExport { name: &'a str, region: Region },
}
```

### Diff 3: Canonical interface model expansion

**Purpose**
Make imported-module information truthful enough for Elm-style canonicalization.

**Files**:

- `crates/nash-can/src/interface.rs`
- `crates/nash-can/src/lib.rs`
- `crates/nash-can/src/module.rs`
- follow-up impact only: `crates/nash-driver/src/interface.rs`

**Changes**:

- Extend `nash-can::Interface` beyond aliases/unions to include at least:
  - values
  - unions
  - aliases
  - binops
  - enough constructor visibility to distinguish
    open vs closed union exports
- Add an interface extraction helper analogous to
  Elm’s `Elm.Interface.fromModule`, scoped to Nash.
- Do not touch ports/effects.

**Design choice**:

- Keep interface types in `nash-can`, not `nash-ast`, unless a later consumer proves a better home. Elm keeps interface data separate from canonical AST.

**Verification**:

- Unit tests for interface extraction from canonical modules
- `cargo test -p nash-can`

**Representative snippet**:

```rust
pub struct Interface<'a> {
    pub home: ModuleName<'a>,
    pub values: &'a [InterfaceValue<'a>],
    pub aliases: &'a [InterfaceAlias<'a>],
    pub unions: &'a [InterfaceUnion<'a>],
    pub binops: &'a [InterfaceBinop<'a>],
}

pub fn from_module<'a>(bump: &'a Bump, module: &'a CanModule<'a>) -> Interface<'a> {
    // restrict to public exports here
}
```

### Diff 4: Environment scaffolding in `nash-can`

**Purpose**
Stop treating `Context` as the environment. Build a real canonicalization env from imports plus locals, like Elm’s `Foreign.createInitialEnv` + `Local.add`.

**Files**:

- `crates/nash-can/src/module.rs`
- new files, likely:
  - `crates/nash-can/src/environment.rs`
  - `crates/nash-can/src/environment/foreign.rs`
  - `crates/nash-can/src/environment/local.rs`
  - `crates/nash-can/src/environment/dups.rs`
- `crates/nash-can/src/error.rs`

**Changes**:

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

**Why separate from value canonicalization**:

- This is the contract the rest of canonicalization consumes.
- It shrinks later diffs by giving expression/pattern/type code one lookup surface.

**Verification**:

- Targeted env/import snapshot tests
- `cargo test -p nash-can`

**Representative snippet**:

```rust
struct Env<'a> {
    values: ValueEnv<'a>,
    types: TypeEnv<'a>,
    ctors: CtorEnv<'a>,
    binops: BinopEnv<'a>,
}

fn create_initial_env<'a>(
    interfaces: &'a [Interface<'a>],
    imports: &'a [&'a SourceImport<'a>],
) -> Result<Env<'a>, Error> {
    // import validation and exposed/qualified tables live here
}
```

### Diff 5: Local validation pass and minimal `nash-ast` truthfulness cleanup

**Purpose**
Port Elm’s structural validations that belong to the local declaration/type layer, and only then make any AST cleanup proven necessary.

**Files**:

- `crates/nash-can/src/environment/local.rs`
- `crates/nash-can/src/environment/dups.rs`
- `crates/nash-can/src/error.rs`
- maybe `crates/nash-ast/src/lib.rs` if validation work proves the current declaration tail or metadata names are misleading

**Changes**:

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

**Important constraint**:

- Do not add Elm effects/ports AST nodes.
- Do not rename `Decls::Empty` just for parity. Rename only if the implementation proves that the current name lies about semantics.

**Verification**:

- New negative snapshots for each duplicate/cycle/binding class
- `cargo test -p nash-can`

**Representative snippet**:

```rust
fn insert_unique<'a, T>(
    seen: &mut BTreeMap<&'a str, Region>,
    name: &'a str,
    region: Region,
    mk_err: impl FnOnce(Region) -> Error,
 ) -> Result<(), Error> {
    if let Some(previous) = seen.insert(name, region) {
        return Err(mk_err(previous));
    }
    Ok(())
}
```

### Diff 6: Pattern and type annotation canonicalization

**Purpose**
Finish the type/pattern layer so value bodies can be canonicalized correctly.

**Files**:

- `crates/nash-can/src/module.rs` or split-out modules:
  - `crates/nash-can/src/pattern.rs`
  - `crates/nash-can/src/types.rs`
- `crates/nash-can/src/error.rs`
- `crates/nash-ast/src/lib.rs` only if a concrete type/pattern gap appears during implementation

**Changes**:

- Canonicalize annotations into `Annotation { free_vars, typ }`.
- Canonicalize constructor patterns through the env, including:
  - qualified vs unqualified lookup
  - arity checks
  - tuple/record/list/cons forms
  - duplicate binder detection
- Reuse the env introduced in Diff 4 instead of ad hoc lookup helpers in `module.rs`.

**Verification**:

- Snapshot tests for typed defs, constructor patterns, bad arity, duplicate binders
- `cargo test -p nash-can`

**Representative snippet**:

```rust
fn canonicalize_pattern<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    pattern: &'a Located<SourcePattern<'a>>,
) -> Result<&'a Located<CanPattern<'a>>, Error> {
    let value = match &pattern.value {
        SourcePattern::Ctor { name, args, .. } => canonicalize_ctor_pattern(bump, env, *name, args, pattern.region)?,
        SourcePattern::Var(name) => CanPattern::Var(name),
        _ => canonicalize_pattern_shape(bump, env, &pattern.value, pattern.region)?,
    };
    Ok(bump.alloc(Located::at(pattern.region, value)))
}
```

### Diff 7 prereqs (from Diff 4 gap analysis)

**Gap 2:** Add `field: &'a str` to `nash_ast::FieldUpdate` so record updates carry field identity.

**Gap 5:** Add warnings channel — `CanResult { module, warnings: Vec<Warning> }` as return type of `canonicalize`. Emit `UnusedVariable`, `UnusedImport`, `MissingTypeAnnotation` during expression canonicalization.

### Diff 7: Expression canonicalization and operator desugaring

**Purpose**
Port the core expression layer so `nash-can` actually resolves names and desugars syntax, as SPEC Phase 3 promises.

**Files**:

- new file, likely `crates/nash-can/src/expression.rs`
- `crates/nash-can/src/module.rs`
- `crates/nash-can/src/error.rs`
- `crates/nash-can/src/snapshots/*`

**Changes**:

- Canonicalize vars to local vs top-level vs constructor vs operator references.
- Resolve qualified and unqualified names through the env.
- Desugar source `Expr::BinOps` chains into canonical `Expr::Binop` trees using precedence/associativity.
- Canonicalize let/case/access/update/record/tuple/list/call/lambda/if.
- Preserve region information.

**Verification**:

- Targeted snapshots for operator precedence/associativity, name resolution, let/case lowering
- `cargo test -p nash-can`

**Representative snippet**:

```rust
match &expr.value {
    SourceExpr::BinOps { operands, last } => {
        desugar_binops(bump, env, operands, last)
    }
    SourceExpr::Var { kind: _, name } => resolve_var(env, *name, expr.region),
    SourceExpr::VarQual { module, name, .. } => resolve_qualified_var(env, module, name, expr.region),
    _ => canonicalize_expr_node(bump, env, expr),
}
```

### Diff 8 detail (from Diff 4 gap analysis)

**Gap 4:** Two-phase SCC cycle detection. Track `Uses { direct: u32, delayed: u32 }` per free local. Lambda bodies increment `delayed`; else increments `direct`. Phase 1 SCC (all deps) -> `DeclareRec` groups. Phase 2 SCC (direct-only, within cyclic groups) -> `Error::RecursiveDecl` for zero-arg values in tight cycles. Functions allowed in cycles because their bodies are deferred.

### Diff 8: Value canonicalization, SCC grouping, and exported interface collection

**Purpose**
Finish the phase boundary promised by SPEC: value canonicalization, dependency ordering, and public interface collection.

**Files**:

- `crates/nash-can/src/module.rs`
- likely `crates/nash-can/src/values.rs`
- `crates/nash-can/src/interface.rs`
- `crates/nash-can/src/error.rs`
- downstream follow-up: `crates/nash-driver/src/interface.rs`

**Changes**:

- Replace `canonicalize_decls(... todo!)` with Elm-style value canonicalization.
- Compute declaration SCCs and lower to `Decls::{Declare, DeclareRec, ...}`.
- Canonicalize explicit exports against the full env, including values/binops/types/union privacy.
- Produce a trustworthy module interface from the finished canonical module.
- After this lands, plan the driver follow-up to serialize the richer interface instead of the current export-only placeholder.

**Verification**:

- Positive and negative snapshots for recursive groups, bad recursive cycles, export validation, imported value use
- `cargo test -p nash-can`

**Representative snippet**:

```rust
fn canonicalize_decls<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    values: &'a [&'a Located<SourceValue<'a>>],
) -> Result<&'a Decls<'a>, Error> {
    let groups = values_to_sccs(env, values)?;
    lower_sccs(bump, groups)
}
```

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
- Stop after every diff no matter what; once one diff is implemented and verified, pause and do not begin the next diff in the same session or PR.

## Risks to watch

- Interface model duplication: `nash-can::Interface` and `nash-driver::Interface` currently describe different truths. Do not let both evolve independently for long.
- Error taxonomy drift: if errors are added ad hoc while implementing values/exprs, the result will be a grab bag instead of a coherent canonicalization contract.
- AST churn without proof: most of the remaining work is implementation in `nash-can`, not large new surface in `nash-ast`.

## Immediate next diff

Start with Diff 1. It is low blast radius, fixes a real Elm boundary drift, and makes all following work happen behind the correct public entrypoint.
