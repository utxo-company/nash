//! Module header parsing for Nash.
//!
//! Ported from Elm's `Parse/Module.hs` (chompHeader).
//!
//! Parses module headers like:
//! - `module Main exposing (main)`
//! - `module Json.Decode exposing (..)`
//! - `module Platform.Cmd exposing (Cmd, map, batch)`

use nash_region::Located;
use nash_source::Exposing;

use crate::error;
use crate::Parser;

impl<'a> Parser<'a> {
    /// Parse a module header.
    ///
    /// Mirrors Elm's `chompHeader` (simplified - no port/effect modules):
    /// ```text
    /// module_header = 'module' module_name 'exposing' exposing_list
    /// ```
    ///
    /// Returns the module name and exports as a tuple.
    pub fn module_header(
        &mut self,
    ) -> Result<(&'a Located<&'a str>, &'a Located<Exposing<'a>>), error::Module<'a>> {
        // Match 'module' keyword
        self.keyword_module(error::Module::Problem)?;

        self.chomp_and_check_indent(
            |space, row, col| error::Module::Space(space, row, col),
            |row, col| error::Module::Name(row, col),
        )?;

        // Parse module name (e.g., "Json.Decode")
        let start = self.get_position();
        let name = self.module_name(error::Module::Name)?;
        let module_name = self.add_end(start, name);

        // Consume whitespace
        self.chomp(|space, row, col| error::Module::Space(space, row, col))?;

        // Must have 'exposing' keyword
        self.keyword_exposing(|row, col| error::Module::Exposing(self.bump.alloc(error::Exposing::Start(row, col)), row, col))?;

        self.chomp_and_check_indent(
            |space, row, col| error::Module::Space(space, row, col),
            |row, col| error::Module::Exposing(self.bump.alloc(error::Exposing::IndentValue(row, col)), row, col),
        )?;

        // Parse the exposing list, wrapping errors
        let exposing_start = self.get_position();
        let exposing = self.specialize(
            |bump, err, row, col| error::Module::Exposing(bump.alloc(err), row, col),
            |p| p.exposing(),
        )?;
        let exposing_located = self.add_end(exposing_start, exposing);

        Ok((module_name, exposing_located))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use indoc::indoc;

    macro_rules! assert_module_header_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let src = bump.alloc_str(input);
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result = parser.module_header();
            match result {
                Ok((name, exposing)) => {
                    insta::with_settings!({
                        description => format!("Code:\n\n{}", input),
                        omit_expression => true,
                    }, {
                        insta::assert_debug_snapshot!((name, exposing));
                    });
                }
                Err(e) => {
                    panic!("Expected successful parse, got error: {:?}", e);
                }
            }
        }};
    }

    #[test]
    fn module_header_simple_open() {
        assert_module_header_snapshot!("module Foo exposing (..)");
    }

    #[test]
    fn module_header_dotted() {
        assert_module_header_snapshot!("module Foo.Bar exposing (baz)");
    }

    #[test]
    fn module_header_mixed_exposing() {
        assert_module_header_snapshot!("module Main exposing (main, Msg(..))");
    }

    #[test]
    fn module_header_deeply_nested() {
        assert_module_header_snapshot!("module Platform.Cmd.Extra exposing (batch, none)");
    }
}
