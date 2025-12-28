# Claude Code Guidelines for Nash Compiler

## Project Overview

Nash is a programming language compiler being ported from the Elm compiler. We are translating Haskell to Rust while adapting to Rust idioms where appropriate.

## Reference Material

The Elm compiler source is cloned locally at `elm/` (gitignored). Use this as the primary reference for:
- Parser structure: `elm/compiler/src/Parse/`
- Error types: `elm/compiler/src/Reporting/Error/Syntax.hs`
- AST definitions: `elm/compiler/src/AST/`

## Memory Management

We use **bumpalo** for arena allocation:
- One `Bump` arena per module
- All AST nodes are allocated in the arena
- Source text is loaded into the arena first, so `&'a str` slices point into arena memory
- Unified `'a` lifetime throughout - drop the arena, everything is freed

```rust
let bump = Bump::new();
let src: &str = bump.alloc_str(&file_contents);
let mut parser = Parser::new(&bump, src.as_bytes());
```

## Testing Strategy

We use **insta** for snapshot testing with extremely granular tests:
- Separate macros for success vs error cases
- `assert_X_snapshot!` - expects Ok, panics on Err, snapshots the parsed value
- `assert_X_error_snapshot!` - expects Err, panics on Ok, snapshots the error

Macros are defined in each module's test submodule for proper namespacing.

Example snapshot output:
```
---
source: crates/nash-parse/src/lib.rs
description: "42"
---
42
```

Run tests with:
```bash
cargo insta test -p nash-parse
cargo insta accept  # to accept new snapshots
```

## Progress Tracking

**SPEC.md** at the repo root tracks:
- Implementation progress with checkboxes
- Grammar definitions in EBNF notation

Update SPEC.md as features are implemented.

## Development Workflow

1. Add grammar rule to SPEC.md
2. Write snapshot test(s) first
3. Implement parser code
4. Run `cargo insta test`, review snapshots
5. Mark complete in SPEC.md

## Error Messages

We are porting Elm's full error hierarchy from `Reporting/Error/Syntax.hs`. Error types are nested (Module contains Decl, Decl contains Expr, etc.) to enable Elm-quality error messages.

## Code Style

- Recursive descent parser (not parser combinators)
- Single `Parser<'a>` struct combining arena + state
- Incremental implementation - test each feature before moving on
- No premature abstraction
