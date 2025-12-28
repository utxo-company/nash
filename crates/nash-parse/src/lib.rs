use bumpalo::Bump;
use nash_region::{Located, Position, Region};

pub mod error;
mod expression;
mod number;

pub type Row = u16;
pub type Col = u16;

/// Saved parser state for backtracking.
#[derive(Clone, Copy)]
struct ParserState {
    pos: usize,
    indent: u16,
    row: Row,
    col: Col,
}

/// Parser for Nash source code.
///
/// Combines the arena allocator with parsing state for a unified API.
/// All parsed AST nodes are allocated in the provided bump arena.
///
/// The source bytes should already be allocated in the arena (via `bump.alloc_str`),
/// so all string slices in the resulting AST share the `'a` lifetime.
pub struct Parser<'a> {
    /// Arena allocator for AST nodes
    bump: &'a Bump,
    /// Source bytes (UTF-8, already in arena)
    src: &'a [u8],
    /// Current byte position
    pos: usize,
    /// Current indentation level (for layout-sensitive parsing)
    indent: u16,
    /// Current row (1-indexed)
    row: Row,
    /// Current column (1-indexed)
    col: Col,
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given source bytes.
    ///
    /// The source should already be allocated in the arena.
    pub fn new(bump: &'a Bump, src: &'a [u8]) -> Self {
        Parser {
            bump,
            src,
            pos: 0,
            indent: 0,
            row: 1,
            col: 1,
        }
    }

    // -------------------------------------------------------------------------
    // Position & State
    // -------------------------------------------------------------------------

    /// Current position as (row, col).
    #[inline]
    pub fn position(&self) -> (Row, Col) {
        (self.row, self.col)
    }

    /// Get the current position as a `Position`.
    #[inline]
    pub fn get_position(&self) -> Position {
        Position::new(self.row, self.col)
    }

    /// Create a `Located` value spanning from `start` to the current position.
    #[inline]
    pub fn add_end<T>(&self, start: Position, value: T) -> Located<T> {
        let end = self.get_position();
        Located::at(Region::new(start, end), value)
    }

    /// Current row (1-indexed).
    #[inline]
    pub fn row(&self) -> Row {
        self.row
    }

    /// Current column (1-indexed).
    #[inline]
    pub fn col(&self) -> Col {
        self.col
    }

    /// Current indentation level.
    #[inline]
    pub fn indent(&self) -> u16 {
        self.indent
    }

    /// Set the indentation level.
    #[inline]
    pub fn set_indent(&mut self, indent: u16) {
        self.indent = indent;
    }

    /// Check if we've reached the end of input.
    #[inline]
    pub fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    /// Save current parser state for backtracking.
    #[inline]
    fn save_state(&self) -> ParserState {
        ParserState {
            pos: self.pos,
            indent: self.indent,
            row: self.row,
            col: self.col,
        }
    }

    /// Restore parser state for backtracking.
    #[inline]
    fn restore_state(&mut self, state: ParserState) {
        self.pos = state.pos;
        self.indent = state.indent;
        self.row = state.row;
        self.col = state.col;
    }

    // -------------------------------------------------------------------------
    // Combinators
    // -------------------------------------------------------------------------

    /// Try multiple parsers in order, returning the first success.
    ///
    /// Mirrors Elm's `oneOf`:
    /// ```haskell
    /// oneOf :: (Row -> Col -> x) -> [Parser x a] -> Parser x a
    /// ```
    ///
    /// Key semantics:
    /// - If a parser fails without consuming input, try the next one
    /// - If a parser fails after consuming input, propagate the error (committed)
    /// - If all parsers fail without consuming, call `to_error(row, col)`
    ///
    /// # Example
    /// ```ignore
    /// parser.one_of(
    ///     error::Expr::Start,  // Constructor: (Row, Col) -> Expr
    ///     [
    ///         |p| p.number(start),
    ///         |p| p.string(start),
    ///     ],
    /// )
    /// ```
    pub fn one_of<T, E, F, const N: usize>(
        &mut self,
        to_error: impl FnOnce(Row, Col) -> E,
        parsers: [F; N],
    ) -> Result<T, E>
    where
        F: FnOnce(&mut Self) -> Result<T, E>,
    {
        let initial_state = self.save_state();

        for parser in parsers {
            let before = self.save_state();
            match parser(self) {
                Ok(value) => return Ok(value),
                Err(e) => {
                    // Did we consume any input?
                    if self.pos != before.pos {
                        // Committed - propagate error
                        return Err(e);
                    }
                    // No input consumed - restore and try next
                    self.restore_state(before);
                }
            }
        }

        // All parsers failed without consuming - restore to initial and return error
        self.restore_state(initial_state);
        let (row, col) = self.position();
        Err(to_error(row, col))
    }

    /// Like `one_of` but returns a fallback value if nothing matches.
    ///
    /// Mirrors Elm's `oneOfWithFallback`:
    /// ```haskell
    /// oneOfWithFallback :: [Parser x a] -> a -> Parser x a
    /// ```
    pub fn one_of_with_fallback<T, E, F, const N: usize>(
        &mut self,
        parsers: [F; N],
        fallback: T,
    ) -> Result<T, E>
    where
        F: FnOnce(&mut Self) -> Result<T, E>,
    {
        let initial_state = self.save_state();

        for parser in parsers {
            let before = self.save_state();
            match parser(self) {
                Ok(value) => return Ok(value),
                Err(e) => {
                    // Did we consume any input?
                    if self.pos != before.pos {
                        // Committed - propagate error
                        return Err(e);
                    }
                    // No input consumed - restore and try next
                    self.restore_state(before);
                }
            }
        }

        // All parsers failed without consuming - return fallback
        self.restore_state(initial_state);
        Ok(fallback)
    }

    // -------------------------------------------------------------------------
    // Peeking
    // -------------------------------------------------------------------------

    /// Peek at the current byte without consuming it.
    #[inline]
    pub fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// Peek at a byte at the given offset from current position.
    #[inline]
    pub fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    /// Get the remaining bytes from current position.
    #[inline]
    pub fn remaining(&self) -> &'a [u8] {
        &self.src[self.pos..]
    }

    // -------------------------------------------------------------------------
    // Advancing
    // -------------------------------------------------------------------------

    /// Advance by one byte, updating row/col for newlines.
    #[inline]
    pub fn advance(&mut self) {
        if let Some(b) = self.peek() {
            self.pos += 1;
            if b == b'\n' {
                self.row += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
    }

    /// Advance by n bytes, tracking newlines.
    #[inline]
    pub fn advance_by(&mut self, n: usize) {
        for _ in 0..n {
            self.advance();
        }
    }

    // -------------------------------------------------------------------------
    // Allocation helpers
    // -------------------------------------------------------------------------

    /// Allocate a value in the arena.
    #[inline]
    pub fn alloc<T>(&self, value: T) -> &'a T {
        self.bump.alloc(value)
    }

    /// Allocate a slice in the arena by copying.
    #[inline]
    pub fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &'a [T] {
        self.bump.alloc_slice_copy(slice)
    }

    /// Allocate a string in the arena (for constructed strings like escape sequences).
    #[inline]
    pub fn alloc_str(&self, s: &str) -> &'a str {
        self.bump.alloc_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_new() {
        let bump = Bump::new();
        let src = bump.alloc_str("hello");
        let parser = Parser::new(&bump, src.as_bytes());

        assert_eq!(parser.row(), 1);
        assert_eq!(parser.col(), 1);
        assert_eq!(parser.peek(), Some(b'h'));
        assert!(!parser.is_eof());
    }

    #[test]
    fn test_parser_advance() {
        let bump = Bump::new();
        let src = bump.alloc_str("ab\ncd");
        let mut parser = Parser::new(&bump, src.as_bytes());

        assert_eq!(parser.position(), (1, 1));
        parser.advance(); // 'a'
        assert_eq!(parser.position(), (1, 2));
        parser.advance(); // 'b'
        assert_eq!(parser.position(), (1, 3));
        parser.advance(); // '\n'
        assert_eq!(parser.position(), (2, 1));
        parser.advance(); // 'c'
        assert_eq!(parser.position(), (2, 2));
    }

    #[test]
    fn test_parser_eof() {
        let bump = Bump::new();
        let src = bump.alloc_str("x");
        let mut parser = Parser::new(&bump, src.as_bytes());

        assert!(!parser.is_eof());
        parser.advance();
        assert!(parser.is_eof());
        assert_eq!(parser.peek(), None);
    }
}
