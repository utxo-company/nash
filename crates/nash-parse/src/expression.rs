//! Expression parsing for Nash.
//!
//! Ported from Elm's `Parse/Expression.hs`.

use nash_region::{Located, Position};
use nash_source::Expr;

use crate::Parser;
use crate::error;

impl<'a> Parser<'a> {
    /// Parse a term (atomic expression).
    ///
    /// Mirrors Elm's `term`:
    /// ```haskell
    /// term =
    ///   do  start <- getPosition
    ///       oneOf E.Start
    ///         [ variable start >>= accessible start
    ///         , string start
    ///         , number start
    ///         , ...
    ///         ]
    /// ```
    pub fn term(&mut self) -> Result<Located<Expr<'a>>, error::Expr<'a>> {
        let start = self.get_position();

        self.one_of(
            error::Expr::Start,
            vec![
                Box::new(|p: &mut Parser<'a>| p.string(start)),
                Box::new(|p| p.number(start)),
            ],
        )
    }

    /// Parse a string expression.
    ///
    /// Mirrors Elm's `string` helper:
    /// ```haskell
    /// string start =
    ///   do  str <- String.string E.Start E.String
    ///       addEnd start (Src.Str str)
    /// ```
    fn string(&mut self, start: Position) -> Result<Located<Expr<'a>>, error::Expr<'a>> {
        let s = self.string_literal(error::Expr::Start, error::Expr::String)?;
        Ok(self.add_end(start, Expr::Str(s)))
    }

    /// Parse a number expression.
    ///
    /// Mirrors Elm's `number` helper:
    /// ```haskell
    /// number start =
    ///   do  nmbr <- Number.number E.Start E.Number
    ///       addEnd start $
    ///         case nmbr of
    ///           Number.Int int -> Src.Int int
    ///           Number.Float float -> Src.Float float
    /// ```
    fn number(&mut self, start: Position) -> Result<Located<Expr<'a>>, error::Expr<'a>> {
        let n = self.number_literal(error::Expr::Start, error::Expr::Number)?;
        Ok(self.add_end(start, Expr::Int(n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    /// Snapshot test macro for successful expression parsing.
    macro_rules! assert_expr_snapshot {
        ($code:expr) => {{
            let bump = Bump::new();
            let src = bump.alloc_str(indoc::indoc!($code));
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result = parser.term().expect("expected successful parse");

            insta::with_settings!({
                description => format!("Code:\n\n{}", indoc::indoc!($code)),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    /// Snapshot test macro for expression parse errors.
    macro_rules! assert_expr_error_snapshot {
        ($code:expr) => {{
            let bump = Bump::new();
            let src = bump.alloc_str(indoc::indoc!($code));
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result = parser.term().expect_err("expected parse error");

            insta::with_settings!({
                description => format!("Code:\n\n{}", indoc::indoc!($code)),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    // =========================================================================
    // Success cases
    // =========================================================================

    #[test]
    fn expr_int_simple() {
        assert_expr_snapshot!("42");
    }

    #[test]
    fn expr_int_zero() {
        assert_expr_snapshot!("0");
    }

    #[test]
    fn expr_int_hex() {
        assert_expr_snapshot!("0xFF");
    }

    #[test]
    fn expr_int_large() {
        assert_expr_snapshot!("123456789");
    }

    #[test]
    fn expr_string_simple() {
        assert_expr_snapshot!(r#""hello""#);
    }

    #[test]
    fn expr_string_with_escape() {
        assert_expr_snapshot!(r#""hello\nworld""#);
    }

    #[test]
    fn expr_string_multi() {
        assert_expr_snapshot!(r#""""multi-line""""#);
    }

    // =========================================================================
    // Error cases
    // =========================================================================

    #[test]
    fn expr_error_not_a_term() {
        assert_expr_error_snapshot!("abc");
    }

    #[test]
    fn expr_error_empty() {
        assert_expr_error_snapshot!("");
    }

    #[test]
    fn expr_error_leading_zero() {
        assert_expr_error_snapshot!("007");
    }

    #[test]
    fn expr_error_hex_no_digits() {
        assert_expr_error_snapshot!("0x");
    }

    #[test]
    fn expr_error_dirty_end() {
        assert_expr_error_snapshot!("123abc");
    }
}
