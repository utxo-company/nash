//! Expression parsing for Nash.
//!
//! Ported from Elm's `Parse/Expression.hs`.

use nash_region::Located;
use nash_source::Expr;

use crate::Parser;
use crate::error;

mod list;
mod number;
mod string;
mod variable;

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
    pub fn term(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let start = self.get_position();

        self.one_of(
            error::Expr::Start,
            vec![
                Box::new(|p: &mut Parser<'a>| p.variable(start)),
                Box::new(|p| p.string(start)),
                Box::new(|p| p.number(start)),
                Box::new(|p| p.list(start)),
            ],
        )
    }
}

/// Snapshot test macro for successful expression parsing.
#[cfg(test)]
macro_rules! assert_expr_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(indoc::indoc!($code));
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
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
#[cfg(test)]
macro_rules! assert_expr_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(indoc::indoc!($code));
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser.term().expect_err("expected parse error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", indoc::indoc!($code)),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

#[cfg(test)]
pub(crate) use assert_expr_error_snapshot;
#[cfg(test)]
pub(crate) use assert_expr_snapshot;
