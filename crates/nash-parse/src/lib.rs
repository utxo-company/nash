use bumpalo::Bump;

pub mod error;

pub type Row = u16;
pub type Col = u16;

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

    // -------------------------------------------------------------------------
    // Parsing (stub to test snapshot infrastructure)
    // -------------------------------------------------------------------------

    /// Parse an integer literal.
    ///
    /// Minimal stub to test snapshot infrastructure.
    /// Error type will be replaced with proper Elm-style errors.
    pub fn parse_int(&mut self) -> Result<i128, ()> {
        let start_pos = self.pos;

        // Must start with a digit
        if !matches!(self.peek(), Some(b) if b.is_ascii_digit()) {
            return Err(());
        }

        // Consume all digits
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }

        // Parse the integer
        let slice = &self.src[start_pos..self.pos];
        let s = std::str::from_utf8(slice).expect("digits are valid utf8");
        let value = s.parse::<i128>().expect("valid integer");

        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Snapshot Test Macros
    // =========================================================================
    //
    // These macros provide a consistent way to write snapshot tests.
    // Each macro:
    // 1. Creates a bump arena
    // 2. Allocates the source code in the arena
    // 3. Creates a parser
    // 4. Calls the relevant parse method
    // 5. Snapshots the result with the source code in the description
    //
    // We have two variants per parse target:
    // - `assert_X_snapshot!` - expects success, unwraps Ok, panics on Err
    // - `assert_X_error_snapshot!` - expects failure, unwraps Err, panics on Ok
    //
    // As we add more modules (expr.rs, pattern.rs, etc.), each will define
    // its own macros in its test submodule for proper namespacing.

    /// Snapshot test macro for successful integer literal parsing.
    /// Expects Ok, panics on Err.
    macro_rules! assert_int_snapshot {
        ($code:expr) => {{
            let bump = Bump::new();
            let src = bump.alloc_str(indoc::indoc!($code));
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result = parser.parse_int().expect("expected successful parse");

            insta::with_settings!({
                description => indoc::indoc!($code),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    /// Snapshot test macro for integer literal parse errors.
    /// Expects Err, panics on Ok.
    macro_rules! assert_int_error_snapshot {
        ($code:expr) => {{
            let bump = Bump::new();
            let src = bump.alloc_str(indoc::indoc!($code));
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result = parser.parse_int().expect_err("expected parse error");

            insta::with_settings!({
                description => indoc::indoc!($code),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    // =========================================================================
    // Unit Tests (non-snapshot)
    // =========================================================================

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

    // =========================================================================
    // Snapshot Tests
    // =========================================================================

    #[test]
    fn int_simple() {
        assert_int_snapshot!("42");
    }

    #[test]
    fn int_zero() {
        assert_int_snapshot!("0");
    }

    #[test]
    fn int_large() {
        assert_int_snapshot!("123456789");
    }
}
