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

- Keep interface types in `nash-can`, not `nash-ast`,
  unless a later consumer proves a better home.

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

pub fn from_module<'a>(
    bump: &'a Bump,
    module: &'a CanModule<'a>,
) -> Interface<'a> {
    // restrict to public exports here
}
```

### Diff 4: Environment scaffolding in `nash-can`

**Purpose**
Build a real canonicalization env from imports plus
locals, like Elm’s `Foreign.createInitialEnv` +
`Local.add`.

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
Port Elm’s structural validations for the local
declaration/type layer, then make any proven-necessary
AST cleanup.

**Files**:

- `crates/nash-can/src/environment/local.rs`
- `crates/nash-can/src/environment/dups.rs`
- `crates/nash-can/src/error.rs`
- maybe `crates/nash-ast/src/lib.rs` if validation
  proves current declaration names are misleading

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
- Only change `nash-ast` if this pass proves a real
  semantic mismatch. Default: keep AST stable.

**Important constraint**:

- Do not add Elm effects/ports AST nodes.
- Do not rename `Decls::Empty` just for parity. Rename
  only if the name lies about semantics.

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
- `crates/nash-ast/src/lib.rs` only if a concrete
  type/pattern gap appears during implementation

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
        SourcePattern::Ctor { name, args, .. } => {
            canonicalize_ctor_pattern(
                bump, env, *name, args, pattern.region,
            )?
        }
        SourcePattern::Var(name) => CanPattern::Var(name),
        _ => canonicalize_pattern_shape(bump, env, &pattern.value, pattern.region)?,
    };
    Ok(bump.alloc(Located::at(pattern.region, value)))
}
```

### Diff 7: Expression canonicalization and operator desugaring — DONE

Implemented expression canonicalization with:

- `expression.rs`: full `canonicalize_expr` for all 19 source expression variants
- Variable resolution (`find_var`/`find_var_qual`) with `PossibleNames` suggestions
- Constructor annotation synthesis (`to_var_ctor`) from `Ctor::Union` type vars
- Binop shunting-yard precedence parser (`build_tree_rec`)
- Let canonicalization with SCC cycle detection via `scc::strongly_connected_components`
- `FreeLocals`/`Uses` tracking with direct/delayed distinction for lambda bodies
- `verify_bindings` for unused-variable warnings
- `gather_typed_args`/`peel_result_type` for typed definitions
- `CanResult { module, warnings }` return type from `canonicalize`
- `Warning::UnusedVariable` with `WarningContext::{Pattern, Def}`
- `DuplicatePatternContext<'a>` with `FuncArgs(&'a str)` carrying function name
- `Var::Foreigns` for ambiguous import detection
- `Env::add_locals` for clone-on-scope-extension
- `Env::find_binop` with `NotFoundBinop`/`AmbiguousBinop` errors
- Error variants: `NotFoundVar`, `AmbiguousVar`, `BinopConflict`, `Shadowing`,
  `RecursiveLet`, `AnnotationTooShort`
- `PossibleNames` populated in `NotFoundType`, `NotFoundCtor`, `NotFoundVar`
- 26 new snapshot tests (18 positive, 6 error, 2 warning), 1423 total passing

Prereqs completed: `FieldUpdate.field`, optional `VarOperator`/`Binop` annotations,
`type_vars` on `Ctor::Union`, `symbol` on env `Binop`.

Deferred to type inference: `VarOperator.annotation`, `Binop.annotation`,
`Var::Foreign` annotation.
Deferred to prelude design: `List`/`Int`/`Bool`/`String` pre-seeding in env.

### Diff 8 detail (from Diff 4 gap analysis)

**Gap 4:** Two-phase SCC cycle detection.
Track `Uses { direct, delayed }` per free local.
Lambda bodies increment `delayed`; else `direct`.
Phase 1 SCC (all deps) -> `DeclareRec` groups.
Phase 2 SCC (direct-only, within cyclic groups) ->
`Error::RecursiveDecl` for zero-arg values in tight
cycles. Functions allowed because bodies are deferred.

### Diff 8: Value canonicalization, SCC grouping, and exported interface collection

**Purpose**
Finish the SPEC phase boundary: value
canonicalization, dependency ordering, and
public interface collection.

**Files**:

- `crates/nash-can/src/module.rs`
- likely `crates/nash-can/src/values.rs`
- `crates/nash-can/src/interface.rs`
- `crates/nash-can/src/error.rs`
- downstream follow-up: `crates/nash-driver/src/interface.rs`

**Changes**:

- Replace `canonicalize_decls(... todo!)` with Elm-style value canonicalization.
- Compute declaration SCCs and lower to `Decls::{Declare, DeclareRec, ...}`.
- Canonicalize explicit exports against the full
  env (values/binops/types/union privacy).
- Produce a trustworthy module interface from the finished canonical module.
- After this lands, plan the driver follow-up to
  serialize the richer interface instead of the
  current export-only placeholder.

**Verification**:

- Positive and negative snapshots for recursive
  groups, bad cycles, export validation,
  imported value use
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

1. ~~Diff 1: public API cutover~~ **done**
2. ~~Diff 2: error surface hardening~~ **done**
3. ~~Diff 3: interface model expansion~~ **done**
4. ~~Diff 4: environment scaffolding~~ **done**
5. ~~Diff 5: local validation~~ **done**
6. ~~Diff 6: pattern/type annotation canonicalization~~ **done**
7. ~~Diff 7: expression canonicalization~~ **done**
8. ~~Diff 8: values, SCCs, exports, interface collection~~ **done**

## What should intentionally stay different from Elm

- No `Effects`, `Port`, or `Manager` nodes in `nash-ast`.
- No effect/port canonicalization entrypoints in `nash-can`.
- Keep the arena parameter in public Rust APIs.
- Keep the smart-contract language scope:
  support Nash source, not Elm browser/runtime.
- Stop after every diff; once implemented and
  verified, do not begin the next in the same
  session or PR.

## Risks to watch

- Interface model duplication:
  `nash-can::Interface` and
  `nash-driver::Interface` describe different
  truths. Do not let both evolve independently.
- Error taxonomy drift: ad hoc errors during
  value/expr work produce a grab bag instead of
  a coherent canonicalization contract.
- AST churn without proof: remaining work is
  implementation in `nash-can`, not large new
  surface in `nash-ast`.

## Status

All 8 diffs are complete. Canonicalization is
feature-complete for the core language.
