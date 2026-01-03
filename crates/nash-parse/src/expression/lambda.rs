//! Lambda expression parsing for Nash.
//!
//! Ported from Elm's `Parse/Expression.hs` (function, chompArgs).
//!
//! Parses: `\arg1 arg2 -> body`

use bumpalo::collections::Vec as BumpVec;
use nash_region::{Located, Position, Region};
use nash_source::Expr;

use crate::error::{self, Func};
use crate::Parser;

impl<'a> Parser<'a> {
    /// Parse a lambda expression: `\arg1 arg2 -> body`.
    ///
    /// Mirrors Elm's `function`:
    /// ```haskell
    /// function start =
    ///   inContext E.Func (word1 0x5C {-\-} E.Start) $
    ///     do  Space.chompAndCheckIndent E.FuncSpace E.FuncIndentArg
    ///         arg <- specialize E.FuncArg Pattern.term
    ///         Space.chompAndCheckIndent E.FuncSpace E.FuncIndentArrow
    ///         revArgs <- chompArgs [arg]
    ///         Space.chompAndCheckIndent E.FuncSpace E.FuncIndentBody
    ///         (body, end) <- specialize E.FuncBody expression
    ///         let funcExpr = Src.Lambda (reverse revArgs) body
    ///         return (A.at start end funcExpr, end)
    /// ```
    pub(crate) fn lambda(
        &mut self,
        start: Position,
    ) -> Result<(&'a Located<Expr<'a>>, Position), error::Expr<'a>> {
        self.in_context(
            |bump, e, row, col| error::Expr::Func(bump.alloc(e), row, col),
            |p| p.word1(b'\\', error::Expr::Start),
            |p| {
                p.chomp_and_check_indent(Func::Space, Func::IndentArg)?;

                let arg = p.specialize(
                    |bump, e, r, c| Func::Arg(bump.alloc(e), r, c),
                    |p| p.pattern_term(),
                )?;

                p.chomp_and_check_indent(Func::Space, Func::IndentArrow)?;

                let rev_args = p.chomp_lambda_args(vec![arg])?;

                p.chomp_and_check_indent(Func::Space, Func::IndentBody)?;

                let (body, end) = p.specialize(
                    |bump, e, r, c| Func::Body(bump.alloc(e), r, c),
                    |p| p.expression(),
                )?;

                // Reverse args and convert to bump slice
                let mut params: BumpVec<'a, &'a Located<_>> = BumpVec::new_in(p.bump);
                for arg in rev_args.into_iter().rev() {
                    params.push(arg);
                }
                let params_slice = params.into_bump_slice();

                let lambda = Expr::Lambda {
                    parameters: params_slice,
                    body,
                };

                Ok((p.alloc(Located::at(Region::new(start, end), lambda)), end))
            },
        )
    }

    /// Chomp additional lambda arguments until we hit `->`.
    ///
    /// Mirrors Elm's `chompArgs`:
    /// ```haskell
    /// chompArgs revArgs =
    ///   oneOf E.FuncArrow
    ///     [ do  arg <- specialize E.FuncArg Pattern.term
    ///           Space.chompAndCheckIndent E.FuncSpace E.FuncIndentArrow
    ///           chompArgs (arg:revArgs)
    ///     , do  word2 0x2D 0x3E {-->-} E.FuncArrow
    ///           return revArgs
    ///     ]
    /// ```
    fn chomp_lambda_args(
        &mut self,
        mut rev_args: Vec<&'a Located<nash_source::Pattern<'a>>>,
    ) -> Result<Vec<&'a Located<nash_source::Pattern<'a>>>, Func<'a>> {
        loop {
            // Try to parse arrow first
            if self.word2(b'-', b'>', Func::Arrow).is_ok() {
                return Ok(rev_args);
            }

            // Otherwise parse another pattern arg
            let arg = self.specialize(
                |bump, e, r, c| Func::Arg(bump.alloc(e), r, c),
                |p| p.pattern_term(),
            )?;
            rev_args.push(arg);

            self.chomp_and_check_indent(Func::Space, Func::IndentArrow)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::assert_expression_snapshot;

    #[test]
    fn lambda_single_arg() {
        assert_expression_snapshot!(r"\x -> x");
    }

    #[test]
    fn lambda_multiple_args() {
        assert_expression_snapshot!(r"\x y -> x");
    }

    #[test]
    fn lambda_pattern_arg() {
        assert_expression_snapshot!(r"\(a, b) -> a");
    }

    #[test]
    fn lambda_wildcard() {
        assert_expression_snapshot!(r"\_ -> 42");
    }

    #[test]
    fn lambda_with_body() {
        assert_expression_snapshot!(r"\x -> f x");
    }
}
