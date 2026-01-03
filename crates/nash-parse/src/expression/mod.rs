//! Expression parsing for Nash.
//!
//! Ported from Elm's `Parse/Expression.hs`.

use nash_region::{Located, Position, Region};
use nash_source::Expr;

use crate::error;
use crate::Parser;

mod accessor;
mod case;
mod if_;
mod lambda;
mod list;
mod number;
mod record;
mod string;
mod tuple;
mod variable;

impl<'a> Parser<'a> {
    /// Parse a full expression, returning the expression and end position.
    ///
    /// Mirrors Elm's `expression`:
    /// ```haskell
    /// expression =
    ///   do  start <- getPosition
    ///       oneOf E.Start
    ///         [ let_ start
    ///         , if_ start
    ///         , case_ start
    ///         , function start
    ///         , do  expr <- possiblyNegativeTerm start
    ///               ...
    ///         ]
    /// ```
    ///
    /// Currently implements: lambda, possiblyNegativeTerm + function application.
    /// TODO: let, if, case, operators
    pub fn expression(
        &mut self,
    ) -> Result<(&'a Located<Expr<'a>>, Position), error::Expr<'a>> {
        let start = self.get_position();

        self.one_of(
            error::Expr::Start,
            vec![
                // Case: case expr of pattern -> branch ...
                Box::new(|p: &mut Parser<'a>| p.case_(start)),
                // If: if cond then expr else expr
                Box::new(|p: &mut Parser<'a>| p.if_(start)),
                // Lambda: \args -> body
                Box::new(|p: &mut Parser<'a>| p.lambda(start)),
                // Term (possibly negated) with function application
                Box::new(|p| {
                    let expr = p.possibly_negative_term(start)?;
                    let end = p.get_position();
                    p.chomp(error::Expr::Space)?;
                    p.chomp_expr_end(start, expr, vec![], end)
                }),
            ],
        )
    }

    /// Handle function application (and later, binary operators).
    ///
    /// Mirrors Elm's `chompExprEnd`:
    /// ```haskell
    /// chompExprEnd start (State ops expr args end) =
    ///   oneOfWithFallback
    ///     [ -- argument
    ///       do  Space.checkIndent end E.Start
    ///           arg <- term
    ///           ...
    ///     , -- operator (TODO)
    ///     ]
    ///     -- done: finalize with toCall
    /// ```
    fn chomp_expr_end(
        &mut self,
        start: Position,
        expr: &'a Located<Expr<'a>>,
        args: Vec<&'a Located<Expr<'a>>>,
        end: Position,
    ) -> Result<(&'a Located<Expr<'a>>, Position), error::Expr<'a>> {
        self.one_of_with_fallback(
            vec![
                // argument - function application
                Box::new(|p: &mut Parser<'a>| {
                    // Check if CURRENT position (after chomping whitespace) is past indent
                    let (row, col) = p.position();
                    p.check_indent(row, col, error::Expr::Start)?;
                    let arg = p.term()?;
                    let new_end = p.get_position();
                    p.chomp(error::Expr::Space)?;

                    let mut new_args = args.clone();
                    new_args.push(arg);

                    p.chomp_expr_end(start, expr, new_args, new_end)
                }),
                // TODO: operator handling
            ],
            // done: finalize expression
            (to_call(self, start, expr, args.clone()), end),
        )
    }

    /// Parse possibly negated term: `-term` or `term`.
    ///
    /// Mirrors Elm's `possiblyNegativeTerm`:
    /// ```haskell
    /// possiblyNegativeTerm start =
    ///   oneOf E.Start
    ///     [ do  word1 0x2D {---} E.Start
    ///           expr <- term
    ///           addEnd start (Src.Negate expr)
    ///     , term
    ///     ]
    /// ```
    fn possibly_negative_term(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.one_of(
            error::Expr::Start,
            vec![
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(b'-', error::Expr::Start)?;
                    let expr = p.term()?;
                    Ok(p.add_end(start, Expr::Negate(expr)))
                }),
                Box::new(|p| p.term()),
            ],
        )
    }

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
                Box::new(|p: &mut Parser<'a>| {
                    let expr = p.variable(start)?;
                    p.accessible(start, expr)
                }),
                Box::new(|p| p.string(start)),
                Box::new(|p| p.number(start)),
                Box::new(|p| p.list(start)),
                Box::new(|p| {
                    let expr = p.record(start)?;
                    p.accessible(start, expr)
                }),
                Box::new(|p| {
                    let expr = p.tuple(start)?;
                    p.accessible(start, expr)
                }),
                Box::new(|p| p.accessor(start)),
            ],
        )
    }
}

/// Convert function + args into a Call expression.
///
/// Mirrors Elm's `toCall`:
/// ```haskell
/// toCall func revArgs =
///   case revArgs of
///     [] -> func
///     lastArg : _ -> A.merge func lastArg (Src.Call func (reverse revArgs))
/// ```
fn to_call<'a>(
    parser: &Parser<'a>,
    _start: Position,
    func: &'a Located<Expr<'a>>,
    args: Vec<&'a Located<Expr<'a>>>,
) -> &'a Located<Expr<'a>> {
    if args.is_empty() {
        func
    } else {
        let last_arg = args.last().unwrap();
        let region = Region::span_across(&func.region, &last_arg.region);
        let args_slice = parser.alloc_slice_copy(&args);
        parser.alloc(Located::at(
            region,
            Expr::Call {
                function: func,
                arguments: args_slice,
            },
        ))
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

/// Snapshot test macro for full expression parsing.
#[cfg(test)]
macro_rules! assert_expression_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(indoc::indoc!($code));
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let (result, _end) = parser.expression().expect("expected successful parse");

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
#[cfg(test)]
pub(crate) use assert_expression_snapshot;

#[cfg(test)]
mod tests {
    use super::assert_expression_snapshot;

    #[test]
    fn negate_var() {
        assert_expression_snapshot!("-x");
    }

    #[test]
    fn negate_number() {
        assert_expression_snapshot!("-42");
    }

    #[test]
    fn negate_parens() {
        assert_expression_snapshot!("-(a)");
    }

    #[test]
    fn expr_simple_var() {
        assert_expression_snapshot!("foo");
    }

    #[test]
    fn expr_simple_number() {
        assert_expression_snapshot!("123");
    }

    #[test]
    fn application_single() {
        assert_expression_snapshot!("f x");
    }

    #[test]
    fn application_multiple() {
        assert_expression_snapshot!("f x y z");
    }

    #[test]
    fn application_nested() {
        assert_expression_snapshot!("f (g x)");
    }

    #[test]
    fn application_with_record() {
        assert_expression_snapshot!("f { x = 1 }");
    }
}
