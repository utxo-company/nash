# nash-parse

## 0.2.2 — 2026-08-15

### Patch changes

- Updated dependencies: nash-source@0.3.0

## 0.2.1 — 2026-08-08

### Patch changes

- [ce5c411](https://github.com/utxo-company/nash/commit/ce5c4110ece5c7dea33bad51bafeafa9143ab38d) Fix `check_indent` to match Elm's `Space.checkIndent`: the check now runs on the parser's current column (`col > indent && col > 1`) instead of the previous token's end position. Previously a declaration starting at column 1 could be swallowed by the preceding construct — most visibly, a union's variant list would absorb a following value definition as extra constructor arguments (`type Msg = Increment | Decrement` followed by `main = 0` parsed `main` as a constructor argument and dropped the definition entirely). Multiline snapshot tests now use `assert_indented_*_snapshot!` variants that lay fragments out as they appear inside a definition, since column-1 continuation lines are not valid layout. — Thanks @rvcas!

