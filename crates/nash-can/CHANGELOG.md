# nash-can

## 0.3.0 — 2026-08-08

### Minor changes

- [41b8820](https://github.com/utxo-company/nash/commit/41b8820670da125b7974568e0977942ebea5c91a) Align `nash-can` module canonicalization more closely with Elm by tightening export validation, improving imported type ambiguity handling, and updating canonicalization errors and coverage. — Thanks @MicroProofs!
- [5b371ff](https://github.com/utxo-company/nash/commit/5b371ff3a3736619cc9d9a056b2f9dec21ba6b97) # Expand Interface model
  
  Add visibility, constructor metadata, and binop support for canonicalization. — Thanks @MicroProofs!
- [5b371ff](https://github.com/utxo-company/nash/commit/5b371ff3a3736619cc9d9a056b2f9dec21ba6b97) # Replace flat TypeContext with Env
  
  BTreeMap-based Env built from imports and local definitions, add
  InterfaceValue with annotation slot, auto-create RecordCtor for record
  aliases. — Thanks @MicroProofs!
- [ce5c411](https://github.com/utxo-company/nash/commit/ce5c4110ece5c7dea33bad51bafeafa9143ab38d) Restore Elm's pipeline invariant: interfaces only exist for type-solved modules, and foreign annotations are required, not optional.
  
  - `nash-ast`: `VarForeign`, `VarOperator`, and `Binop` now carry a required `&Annotation`, matching `AST.Canonical` field-for-field.
  - `nash-can`: `Interface::from_module(bump, module, annotations)` takes the solver's annotations map like Elm's `I.fromModule`; `InterfaceValue`/`InterfaceBinop`, `Env`'s `Var::Foreign`, `q_vars`, and `Binop` all carry required annotations. Local `infix` declarations no longer enter the env (matching Elm — the defining module calls the operator's function directly); they are validated instead: duplicate operators and operators whose function is not a top-level value are now real errors (`DuplicateBinop`, `BinopFunctionNotFound`), which Elm never needed because `infix` is kernel-only there. Nash keeps user-defined `infix` by simply not porting Elm's kernel-package parse gate.
  - `nash-driver`: cross-module compilation is removed until `nash-constrain`/`nash-solve` exist — compiling dependents against unsolved dependencies was never sound. Modules now parse and canonicalize independently (in parallel), and the `Interface<'static>` transmute is gone. — Thanks @rvcas!
- [ce5c411](https://github.com/utxo-company/nash/commit/ce5c4110ece5c7dea33bad51bafeafa9143ab38d) Fix semantic deviations from Elm's canonicalizer found in a full parity audit, and stop the driver from blocking the tokio executor during builds.
  
  nash-can:
  
  - Duplicate binders are now detected across sibling argument patterns (`f x x = x` and `\x x -> x` are rejected), with one duplicate-detection scope spanning all arguments like Elm's `Pattern.verify`.
  - Import privacy is enforced: closed unions no longer leak constructors and private unions/aliases are no longer importable (`toPublicUnion`/`toPublicAlias` are now applied when building the import environment).
  - `import Foo as F exposing (Bar(..))` exposes constructors again — the exposed-ctor tables are built directly from the interface instead of reading back through the alias-keyed qualified table.
  - Let-destructure cycle detection uses Elm's `_M$`-mangled node keys with an edge per bound name, so `let (a, b) = f a` reports `RecursiveLet` instead of silently dropping the destructure.
  - `let` destructures with constructor and list patterns now bind their names.
  - Record extension variables participate in union/alias free-variable checks: extensible-record aliases are accepted, unbound extension variables are rejected.
  - Bool constructor patterns are arity-checked (`True x` is a `BadArity` error).
  - `iterated_dealias` substitutes alias arguments, so typed definitions through parameterized aliases get the instantiated argument/result types.
  - Error fidelity now matches Elm: one duplicate error per name in name order, constructor/type lookup errors point at the name itself, `ExportNotFound` carries suggestions, `AnnotationTooShort` carries argument counts, `RecursiveAlias` carries the source type, export resolution runs before duplicate detection, cyclic aliases run the type-variable check first, and all bad top-level cycles in a group are reported together.
  - Determinism parity with Elm: SCC computation is an exact port of `Data.Graph.stronglyConnComp` (key-sorted Kosaraju), record fields and explicit exports are stored in Elm's canonical name order, explicit type/binop exposing overwrites instead of merging, and ambiguity tracking compares full canonical module names.
  
  nash-driver:
  
  - `build` now fetches sources asynchronously and runs all CPU-bound compilation inside `spawn_blocking`, compiling modules on a bounded pool of scoped worker threads instead of one OS thread per module and no longer stalling tokio executor workers. — Thanks @rvcas!
- [b5dce51](https://github.com/utxo-company/nash/commit/b5dce5118211878726683e9869b88f4baf69ee4e) # Add pattern and type annotation canonicalization
  
  Extract type canonicalization into `types.rs` with `to_annotation` and free
  variable collection. Add `pattern.rs` with constructor resolution, arity
  checks, and duplicate binder detection. Add `TupleLargerThanThree` validation
  to both type and pattern paths. — Thanks @MicroProofs!
- [4cb71ef](https://github.com/utxo-company/nash/commit/4cb71ef6032bc2abe0759679d45487f7ebe2697f) # Add local validation pass
  
  Duplicate detection, alias cycle detection via SCC, type variable binding
  checks, record field and export dup checks, and Elm-style error accumulation. — Thanks @MicroProofs!
- [57ea057](https://github.com/utxo-company/nash/commit/57ea05753fbd7876b600972f837bfeb05a958b50) # Add Ctor::Bool for PBool pattern synthesis, pre-seed List, replace unwrap — Thanks @MicroProofs!

### Patch changes

- [f68130a](https://github.com/utxo-company/nash/commit/f68130af319822d4785d7bd165f8af78caf0c6f3) Port Elm's type inference: constraint generation (`Type/Constrain/*`) and the rank-based solver (`Type/{Type,UnionFind,Solve,Unify,Occurs,Instantiate}.hs`).
  
  - `nash-constrain`: the shared inference vocabulary (index-based union-find with Elm's weight balancing, descriptors, inference `Type`, `Constraint`) plus constraint generation for expressions, patterns, and modules, carrying Elm's full `Expected`/`Category` error-context hierarchy. Type error data from `Type/Error.hs` and `Reporting/Error/Type.hs` is ported; rendering stays deferred with the rest of error reporting. Elm cases nash-ast has no expressions for (`Float`/`Chr` literals, shaders, kernel/debug vars, ports, effect managers) are omitted, and built-in type homes are package-less `Basics`/`List`/`String` pending a canonical core package.
  - `nash-solve`: unification with number/comparable/appendable/compappend supertypes, extensible records, and aliases; occurs checks; rank-based generalization with pools; and `to_annotation`/`to_error_type` with Elm's fresh-name scheme. One deliberate fix over Elm: `getVarNames` tracks visits per call instead of with persistent descriptor marks, so top-level values sharing generalized variables (unannotated mutual recursion) get complete `Forall`s — the same shape crashes Elm 0.19.1 with "Map.!: given key is not an element in the map" when used cross-module.
  - `nash-driver`: modules now run the full pipeline — parse, canonicalize, constrain, solve, `Interface::from_module` with the solver's annotations — in dependency order, and each solved module's interface is deep-copied into a build-wide arena for its dependents. Cross-module compilation is back, type-checked end to end.
  - `nash-can`: re-export `Annotations`; `nash-ast`: derive `Copy` for `FieldType`. — Thanks @rvcas!
- Updated dependencies: nash-ast@0.3.0, nash-parse@0.2.1

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

