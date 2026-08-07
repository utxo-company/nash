---
cargo/nash-ast: minor
cargo/nash-can: minor
cargo/nash-driver: minor
---

Restore Elm's pipeline invariant: interfaces only exist for type-solved modules, and foreign annotations are required, not optional.

- `nash-ast`: `VarForeign`, `VarOperator`, and `Binop` now carry a required `&Annotation`, matching `AST.Canonical` field-for-field.
- `nash-can`: `Interface::from_module(bump, module, annotations)` takes the solver's annotations map like Elm's `I.fromModule`; `InterfaceValue`/`InterfaceBinop`, `Env`'s `Var::Foreign`, `q_vars`, and `Binop` all carry required annotations. Local `infix` declarations no longer enter the env (matching Elm — the defining module calls the operator's function directly); they are validated instead: duplicate operators and operators whose function is not a top-level value are now real errors (`DuplicateBinop`, `BinopFunctionNotFound`), which Elm never needed because `infix` is kernel-only there. Nash keeps user-defined `infix` by simply not porting Elm's kernel-package parse gate.
- `nash-driver`: cross-module compilation is removed until `nash-constrain`/`nash-solve` exist — compiling dependents against unsolved dependencies was never sound. Modules now parse and canonicalize independently (in parallel), and the `Interface<'static>` transmute is gone.
