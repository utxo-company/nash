# Nash Language Specification

## Current State

**Next task:** Declarations (Value definitions)

Current working files:
- `crates/nash-parse/src/lib.rs` - Parser struct, combinators (`one_of`, `in_context`, `specialize`, `word1`, `word2`)
- `crates/nash-parse/src/number.rs` - `number_literal` primitive
- `crates/nash-parse/src/string.rs` - `string_literal` primitive
- `crates/nash-parse/src/keyword.rs` - reserved words, keyword parsers (`keyword_if`, `keyword_then`, etc.)
- `crates/nash-parse/src/space.rs` - whitespace, comments, indentation
- `crates/nash-parse/src/expression/mod.rs` - `expression`, `term`, `possibly_negative_term`, `chomp_expr_end`
- `crates/nash-parse/src/expression/accessor.rs` - `.field` accessor, `foo.bar` field access chains
- `crates/nash-parse/src/expression/lambda.rs` - `\args -> body` lambda expressions
- `crates/nash-parse/src/expression/if_.rs` - `if/then/else` expressions
- `crates/nash-parse/src/expression/case.rs` - `case/of` expressions
- `crates/nash-parse/src/expression/let_.rs` - `let/in` expressions
- `crates/nash-parse/src/expression/number.rs` - `number` expression + tests
- `crates/nash-parse/src/expression/string.rs` - `string` expression + tests
- `crates/nash-parse/src/expression/variable.rs` - `variable`, `lower_name`, `upper_name`, `foreign_alpha` + tests
- `crates/nash-parse/src/expression/list.rs` - `list` expression + tests
- `crates/nash-parse/src/expression/tuple.rs` - `tuple`, unit, parens + tests
- `crates/nash-parse/src/expression/record.rs` - `record`, record update + tests
- `crates/nash-parse/src/pattern/mod.rs` - `pattern_term`, `pattern_expr` (cons, as, ctor args)
- `crates/nash-parse/src/pattern/term.rs` - wildcard, var, ctor, number, string
- `crates/nash-parse/src/pattern/record.rs` - `{ x, y, z }`
- `crates/nash-parse/src/pattern/tuple.rs` - `()`, `(a, b)`
- `crates/nash-parse/src/pattern/list.rs` - `[]`, `[a, b, c]`
- `crates/nash-parse/src/type_.rs` - `type_term`, `type_expr` (variables, named, function, tuple, record)
- `crates/nash-parse/src/error.rs` - Error hierarchy

## File Mappings

Elm parser modules → Nash parser modules:

| Elm (`elm/compiler/src/Parse/`) | Nash (`crates/nash-parse/src/`) |
|---------------------------------|---------------------------------|
| `Primitives.hs`                 | `lib.rs` (Parser struct)        |
| `Module.hs`                     | `module.rs`                     |
| `Declaration.hs`                | `declaration.rs`                |
| `Expression.hs`                 | `expression/`                   |
| `Pattern.hs`                    | `pattern/`                      |
| `Type.hs`                       | `type_.rs`                      |
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
- [x] `in_context` / `specialize` for error wrapping
- [x] `word1` / `word2` for byte matching
- [x] Whitespace and comment handling (`space.rs`)
- [x] Line comments (`--`)
- [x] Multi-line comments (`{- -}`) with nesting
- [x] Indentation checking

### Literals
- [x] Integer literals (`number.rs`)
- [x] String literals (single-line) (`string.rs`)
- [x] String literals (multi-line) (`string.rs`)

### Identifiers
- [x] Lowercase variables (`expression/variable.rs`)
- [x] Uppercase variables (constructors)
- [x] Qualified names (Module.name)
- [x] Operators (`symbol.rs`)

### Basic Expressions
- [x] Unit `()` (`expression/tuple.rs`)
- [x] Tuples (`expression/tuple.rs`)
- [x] Lists (`expression/list.rs`)
- [x] Records (`expression/record.rs`)
- [x] Record update (`expression/record.rs`)

### Patterns
- [x] Wildcard `_` (`pattern/term.rs`)
- [x] Variable binding (`pattern/term.rs`)
- [x] Constructor patterns (`pattern/term.rs`, `pattern/mod.rs`)
- [x] Tuple patterns (`pattern/tuple.rs`)
- [x] List patterns (`pattern/list.rs`)
- [x] Record patterns (`pattern/record.rs`)
- [x] As-patterns (`pattern/mod.rs`)
- [x] Cons patterns (`pattern/mod.rs`)

### Types
- [x] Type variables (`type_.rs`)
- [x] Named types (`type_.rs`)
- [x] Function types (`type_.rs`)
- [x] Tuple types (`type_.rs`)
- [x] Record types (`type_.rs`)

### Expressions
- [x] term / number (`expression/number.rs`)
- [x] term / string (`expression/string.rs`)
- [x] term / variable (`expression/variable.rs`)
- [x] Accessor `.field` (`expression/accessor.rs`)
- [x] Field access `foo.bar.baz` (`expression/accessor.rs`)
- [x] Negation `-expr` (`expression/mod.rs`)
- [x] Function application (`expression/mod.rs`)
- [x] Lambda expressions (`expression/lambda.rs`)
- [x] If expressions (`expression/if_.rs`)
- [x] Case expressions (`expression/case.rs`)
- [x] Let expressions (`expression/let_.rs`)
- [x] Binary operators (`expression/mod.rs`, `symbol.rs`)
- [x] Operator sections (`expression/tuple.rs`)

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
expression     = let_expr | case_expr | if_expr | lambda | binop_expr ;
binop_expr     = possibly_neg_term { term } { operator binop_rhs } ;
binop_rhs      = possibly_neg_term { term } | let_expr | case_expr | if_expr | lambda ;
let_expr       = 'let' let_def { let_def } 'in' expression ;
let_def        = definition | destructure ;
definition     = lower_var [ ':' type_expr ] { pattern } '=' expression ;
destructure    = pattern '=' expression ;
case_expr      = 'case' expression 'of' case_branch { case_branch } ;
case_branch    = pattern '->' expression ;
if_expr        = 'if' expression 'then' expression 'else' expression ;
lambda         = '\' pattern { pattern } '->' expression ;
possibly_neg_term = '-' term | term ;
term           = ( variable | record | tuple ) { '.' lower_var }
               | string | number | list | accessor ;
accessor       = '.' lower_var ;
variable       = lower_var | upper_var | qualified_var ;
lower_var      = lower { inner_char } ;
upper_var      = upper { inner_char } ;
qualified_var  = upper { inner_char } '.' ( lower_var | upper_var | qualified_var ) ;
inner_char     = lower | upper | digit | '_' ;
string         = string_literal ;
number         = number_literal ;  (* no floats in Nash *)
list           = '[' [ expr { ',' expr } ] ']' ;
tuple          = '(' ')' | '(' operator ')' | '(' expr ')' | '(' expr ',' expr { ',' expr } ')' ;
record         = '{' '}' | '{' lower_var '=' expr { ',' field } '}'
               | '{' lower_var '|' field { ',' field } '}' ;
field          = lower_var '=' expr ;
operator       = op_char { op_char } ;  (* except reserved: . | -> = : *)
op_char        = '+' | '-' | '*' | '/' | '=' | '.' | '<' | '>' | ':' | '&' | '|' | '^' | '?' | '%' | '!' ;
```

### Patterns

```ebnf
pattern_expr   = pattern_part { '::' pattern_part } [ 'as' lower_var ] ;
pattern_part   = ctor_pattern | pattern_term ;
ctor_pattern   = ( upper_var | qualified_upper ) { pattern_term } ;
pattern_term   = wildcard | lower_var | ctor_no_args | number | string
               | pattern_record | pattern_tuple | pattern_list ;
wildcard       = '_' ;
ctor_no_args   = upper_var | qualified_upper ;
pattern_record = '{' '}' | '{' lower_var { ',' lower_var } '}' ;
pattern_tuple  = '(' ')' | '(' pattern_expr ')'
               | '(' pattern_expr ',' pattern_expr { ',' pattern_expr } ')' ;
pattern_list   = '[' ']' | '[' pattern_expr { ',' pattern_expr } ']' ;
```

### Types

```ebnf
type_expr      = type_app [ '->' type_expr ] ;
type_app       = upper_var { type_term } ;
type_term      = type_var | type_named | type_tuple | type_record ;
type_var       = lower_var ;
type_named     = upper_var | qualified_upper ;
type_tuple     = '(' ')' | '(' type_expr ')' | '(' type_expr ',' type_expr { ',' type_expr } ')' ;
type_record    = '{' '}' | '{' lower_var ':' type_expr { ',' type_field } '}'
               | '{' lower_var '|' type_field { ',' type_field } '}' ;
type_field     = lower_var ':' type_expr ;
```

### Declarations

```ebnf
(* To be filled in as implemented *)
```

### Module

```ebnf
(* To be filled in as implemented *)
```
