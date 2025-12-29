//! Keyword handling for Nash.
//!
//! Ported from Elm's `Parse/Variable.hs` (reservedWords).

/// Reserved words that cannot be used as variable names.
pub const RESERVED: &[&str] = &[
    "if", "then", "else", "case", "of", "let", "in", "type", "module", "where", "import",
    "exposing", "as", "port",
];

/// Check if a name is a reserved keyword.
#[inline]
pub fn is_reserved(name: &str) -> bool {
    RESERVED.contains(&name)
}
