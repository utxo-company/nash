# Nash Language Specification

## Current State

**Next task:** Implement integer literal parsing with proper error types

Key files:
- `crates/nash-parse/src/lib.rs` - Parser struct, stub `parse_int`
- `crates/nash-parse/src/error.rs` - Error type hierarchy (ported from Elm)
- `crates/nash-source/src/lib.rs` - AST types
- `elm/compiler/src/Reporting/Error/Syntax.hs` - Reference for error types

## Implementation Progress

### Parser Infrastructure
- [x] Parser struct with state tracking (`Parser<'a>` in lib.rs)
- [x] Basic methods (peek, advance, position)
- [x] Snapshot test infrastructure (insta macros)
- [x] Error type hierarchy (from Elm's Syntax.hs) - `error.rs`

### Literals
- [ ] Integer literals
- [ ] String literals (single-line)
- [ ] String literals (multi-line)
- [ ] Char literals

### Identifiers
- [ ] Lowercase variables
- [ ] Uppercase variables (constructors)
- [ ] Qualified names (Module.name)
- [ ] Operators

### Basic Expressions
- [ ] Unit `()`
- [ ] Tuples
- [ ] Lists
- [ ] Records
- [ ] Record update

### Patterns
- [ ] Wildcard `_`
- [ ] Variable binding
- [ ] Constructor patterns
- [ ] Tuple patterns
- [ ] List patterns
- [ ] Record patterns
- [ ] As-patterns

### Types
- [ ] Type variables
- [ ] Named types
- [ ] Function types
- [ ] Tuple types
- [ ] Record types

### Expressions
- [ ] Variables
- [ ] Function application
- [ ] Lambda expressions
- [ ] Let expressions
- [ ] If expressions
- [ ] Case expressions
- [ ] Binary operators

### Declarations
- [ ] Value definitions
- [ ] Type annotations
- [ ] Type aliases
- [ ] Custom types (unions)
- [ ] Infix declarations

### Module Structure
- [ ] Module header
- [ ] Exposing list
- [ ] Imports
- [ ] Full module parsing

---

## Grammar (EBNF)

The grammar is built up incrementally as features are implemented.

### Notation
```
rule      = definition ;
( ... )   = grouping
[ ... ]   = optional
{ ... }   = zero or more
|         = alternation
"..."     = terminal string
'...'     = terminal char
```

### Lexical

```ebnf
digit     = '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' ;
lower     = 'a' | ... | 'z' ;
upper     = 'A' | ... | 'Z' ;
```

### Literals

```ebnf
(* To be filled in as implemented *)
```

### Expressions

```ebnf
(* To be filled in as implemented *)
```

### Patterns

```ebnf
(* To be filled in as implemented *)
```

### Types

```ebnf
(* To be filled in as implemented *)
```

### Declarations

```ebnf
(* To be filled in as implemented *)
```

### Module

```ebnf
(* To be filled in as implemented *)
```
