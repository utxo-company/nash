# Nash Language Specification

## Current State

**Next task:** Unit `()`, Tuples, Lists

Current working files:
- `crates/nash-parse/src/lib.rs` - Parser struct, `one_of` combinator (boxed closures)
- `crates/nash-parse/src/number.rs` - `number_literal` primitive
- `crates/nash-parse/src/string.rs` - `string_literal` primitive
- `crates/nash-parse/src/keyword.rs` - reserved words
- `crates/nash-parse/src/expression/mod.rs` - `term` (uses `one_of`)
- `crates/nash-parse/src/expression/number.rs` - `number` expression + tests
- `crates/nash-parse/src/expression/string.rs` - `string` expression + tests
- `crates/nash-parse/src/expression/variable.rs` - `variable`, `foreign_alpha` + tests
- `crates/nash-parse/src/error.rs` - Error hierarchy

## File Mappings

Elm parser modules → Nash parser modules:

| Elm (`elm/compiler/src/Parse/`) | Nash (`crates/nash-parse/src/`) |
|---------------------------------|---------------------------------|
| `Primitives.hs`                 | `lib.rs` (Parser struct)        |
| `Module.hs`                     | `module.rs`                     |
| `Declaration.hs`                | `declaration.rs`                |
| `Expression.hs`                 | `expression/`                   |
| `Pattern.hs`                    | `pattern.rs`                    |
| `Type.hs`                       | `type.rs`                       |
| `Number.hs`                     | `number.rs`                     |
| `String.hs`                     | `string.rs`                     |
| `Variable.hs`                   | `expression/variable.rs`        |
| `Symbol.hs`                     | `symbol.rs`                     |
| `Keyword.hs`                    | `keyword.rs`                    |
| `Space.hs`                      | `space.rs`                      |
| `Reporting/Error/Syntax.hs`     | `error.rs`                      |

AST types: `crates/nash-source/src/lib.rs`

## Implementation Progress

### Parser Infrastructure
- [x] Parser struct with state tracking (`Parser<'a>` in lib.rs)
- [x] Basic methods (peek, advance, position)
- [x] Snapshot test infrastructure (insta macros)
- [x] Error type hierarchy (from Elm's Syntax.hs) - `error.rs`
- [x] `one_of` / `one_of_with_fallback` combinators

### Literals
- [x] Integer literals (`number.rs`)
- [x] String literals (single-line) (`string.rs`)
- [x] String literals (multi-line) (`string.rs`)

### Identifiers
- [x] Lowercase variables (`expression/variable.rs`)
- [x] Uppercase variables (constructors)
- [x] Qualified names (Module.name)
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
- [x] term / number (`expression/number.rs`)
- [x] term / string (`expression/string.rs`)
- [x] term / variable (`expression/variable.rs`)
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
number_literal = decimal_int | hex_int ;
decimal_int    = nonzero_digit { digit } | '0' ;
hex_int        = '0' ( 'x' | 'X' ) hex_digit { hex_digit } ;
nonzero_digit  = '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' ;
hex_digit      = digit | 'a' | 'b' | 'c' | 'd' | 'e' | 'f'
                       | 'A' | 'B' | 'C' | 'D' | 'E' | 'F' ;

string_literal = single_string | multi_string ;
single_string  = '"' { string_char | escape } '"' ;
multi_string   = '"""' { any_char | escape } '"""' ;
string_char    = (* any char except '"', '\', newline *) ;
escape         = '\' ( 'n' | 'r' | 't' | '"' | '\'' | '\' | unicode_escape ) ;
unicode_escape = 'u' '{' hex_digit hex_digit hex_digit hex_digit [ hex_digit [ hex_digit ] ] '}' ;
```

### Expressions

```ebnf
term           = variable | string | number | ... ;
variable       = lower_var | upper_var | qualified_var ;
lower_var      = lower { inner_char } ;
upper_var      = upper { inner_char } ;
qualified_var  = upper { inner_char } '.' ( lower_var | upper_var | qualified_var ) ;
inner_char     = lower | upper | digit | '_' ;
string         = string_literal ;
number         = number_literal ;  (* no floats in Nash *)
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
