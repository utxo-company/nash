//! Number parsing for Nash.
//!
//! Ported from Elm's `Parse/Number.hs`.
//! Currently only supports integers (no floats).

use crate::error;
use crate::{Col, Parser, Row};

impl<'a> Parser<'a> {
    /// Parse an integer literal with custom error constructors.
    ///
    /// Mirrors Elm's `Number.number`:
    /// ```haskell
    /// number :: (Row -> Col -> x) -> (E.Number -> Row -> Col -> x) -> Parser x Number
    /// ```
    ///
    /// Takes two error constructors:
    /// - `to_expectation`: called when no digit is found (empty error, no input consumed)
    /// - `to_error`: called when parsing fails after consuming input
    ///
    /// Handles:
    /// - Decimal integers: `42`, `123`
    /// - Hex integers: `0xFF`, `0x1A2B`
    ///
    /// # Example
    /// ```ignore
    /// // From expression parsing:
    /// self.number_literal(error::Expr::Start, error::Expr::Number)
    /// ```
    pub fn number_literal<E>(
        &mut self,
        to_expectation: impl FnOnce(Row, Col) -> E,
        to_error: impl FnOnce(error::Number, Row, Col) -> E,
    ) -> Result<i128, E> {
        let (row, col) = self.position();

        // Check first - if not a digit, return expectation error WITHOUT consuming
        let first = match self.peek() {
            Some(b) if b.is_ascii_digit() => b,
            _ => return Err(to_expectation(row, col)),
        };

        // Now we're committed - consume the first digit
        self.advance();

        let result = if first == b'0' {
            self.chomp_zero()
        } else {
            self.chomp_int((first - b'0') as i128)
        };

        result.map_err(|e| to_error(e, self.row(), self.col()))
    }

    /// Continue parsing after seeing a leading '0'.
    fn chomp_zero(&mut self) -> Result<i128, error::Number> {
        match self.peek() {
            None => Ok(0),

            Some(b'x') | Some(b'X') => {
                self.advance();
                self.chomp_hex()
            }

            Some(b) if b.is_ascii_digit() => {
                // Leading zeros not allowed: 007, 00, etc.
                Err(error::Number::NoLeadingZero)
            }

            Some(b) if is_ident_inner(b) => {
                // 0abc - dirty end
                Err(error::Number::End)
            }

            Some(_) => Ok(0),
        }
    }

    /// Parse remaining decimal digits after the first non-zero digit.
    fn chomp_int(&mut self, mut n: i128) -> Result<i128, error::Number> {
        loop {
            match self.peek() {
                Some(b) if b.is_ascii_digit() => {
                    n = n * 10 + (b - b'0') as i128;
                    self.advance();
                }

                Some(b) if is_ident_inner(b) => {
                    // 123abc - dirty end
                    return Err(error::Number::End);
                }

                _ => return Ok(n),
            }
        }
    }

    /// Parse hex digits after `0x`.
    fn chomp_hex(&mut self) -> Result<i128, error::Number> {
        let mut n: i128 = 0;
        let mut has_digits = false;

        loop {
            match self.peek() {
                Some(b) if b.is_ascii_hexdigit() => {
                    has_digits = true;
                    n = n * 16 + hex_value(b) as i128;
                    self.advance();
                }

                Some(b) if is_ident_inner(b) => {
                    // 0xGG or 0x1G - invalid hex followed by ident char
                    return Err(error::Number::HexDigit);
                }

                _ => {
                    if has_digits {
                        return Ok(n);
                    } else {
                        // 0x without any hex digits
                        return Err(error::Number::HexDigit);
                    }
                }
            }
        }
    }
}

/// Check if a byte is a valid inner identifier character.
/// Used to detect "dirty end" like `123abc`.
#[inline]
fn is_ident_inner(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// Get the numeric value of a hex digit.
#[inline]
fn hex_value(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    /// Test error type that mirrors the two-constructor pattern.
    #[derive(Debug)]
    enum TestError {
        Expected(u16, u16),
        Number(error::Number, u16, u16),
    }

    /// Snapshot test macro for successful number literal parsing.
    macro_rules! assert_number_snapshot {
        ($code:expr) => {{
            let bump = Bump::new();
            let src = bump.alloc_str(indoc::indoc!($code));
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result: Result<i128, TestError> =
                parser.number_literal(TestError::Expected, TestError::Number);
            let value = result.expect("expected successful parse");

            insta::with_settings!({
                description => format!("Code:\n\n{}", indoc::indoc!($code)),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(value);
            });
        }};
    }

    /// Snapshot test macro for number literal parse errors.
    macro_rules! assert_number_error_snapshot {
        ($code:expr) => {{
            let bump = Bump::new();
            let src = bump.alloc_str(indoc::indoc!($code));
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result: Result<i128, TestError> =
                parser.number_literal(TestError::Expected, TestError::Number);
            let err = result.expect_err("expected parse error");

            insta::with_settings!({
                description => format!("Code:\n\n{}", indoc::indoc!($code)),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(err);
            });
        }};
    }

    // =========================================================================
    // Success cases
    // =========================================================================

    #[test]
    fn number_simple() {
        assert_number_snapshot!("42");
    }

    #[test]
    fn number_zero() {
        assert_number_snapshot!("0");
    }

    #[test]
    fn number_large() {
        assert_number_snapshot!("123456789");
    }

    #[test]
    fn number_hex_lower() {
        assert_number_snapshot!("0xff");
    }

    #[test]
    fn number_hex_upper() {
        assert_number_snapshot!("0xFF");
    }

    #[test]
    fn number_hex_mixed() {
        assert_number_snapshot!("0xDeAdBeEf");
    }

    #[test]
    fn number_hex_capital_x() {
        assert_number_snapshot!("0X1A");
    }

    // =========================================================================
    // Error cases
    // =========================================================================

    #[test]
    fn error_leading_zero() {
        assert_number_error_snapshot!("007");
    }

    #[test]
    fn error_leading_zeros() {
        assert_number_error_snapshot!("00");
    }

    #[test]
    fn error_dirty_end() {
        assert_number_error_snapshot!("123abc");
    }

    #[test]
    fn error_zero_dirty_end() {
        assert_number_error_snapshot!("0abc");
    }

    #[test]
    fn error_hex_no_digits() {
        assert_number_error_snapshot!("0x");
    }

    #[test]
    fn error_hex_bad_digit() {
        assert_number_error_snapshot!("0xGG");
    }

    #[test]
    fn error_hex_dirty_end() {
        assert_number_error_snapshot!("0x1G");
    }

    #[test]
    fn error_not_a_number() {
        assert_number_error_snapshot!("abc");
    }
}
