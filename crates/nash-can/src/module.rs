use bumpalo::Bump;
use nash_ast::{Export, Exports, ModuleName, PackageName};
use nash_region::{Located, Region};
use nash_source::{Exposed, Exposing, Module, Privacy};

use crate::Error;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context<'a> {
    pub package: Option<PackageName<'a>>,
}

#[derive(Debug)]
pub struct Header<'a> {
    pub name: ModuleName<'a>,
    pub exports: Exports<'a>,
}

pub fn canonicalize_header<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    module: &Module<'a>,
) -> Result<Header<'a>, Error> {
    let name = module.name.ok_or(Error::MissingModuleHeader)?;

    Ok(Header {
        name: ModuleName {
            package: context.package,
            name: name.value,
        },
        exports: canonicalize_exports(bump, module.exports),
    })
}

fn canonicalize_exports<'a>(bump: &'a Bump, exports: &Located<Exposing<'a>>) -> Exports<'a> {
    match exports.value {
        Exposing::Open => Exports::Everything(exports.region),
        Exposing::Explicit(exposed) => {
            let exposed: &'a [&'a Located<Export<'a>>] =
                bump.alloc_slice_fill_iter(exposed.iter().copied().map(|exposed| {
                    let export = bump.alloc(canonicalize_exposed(exposed));
                    let export: &'a Located<Export<'a>> = export;
                    export
                }));

            Exports::Explicit(exposed)
        }
    }
}

fn canonicalize_exposed<'a>(exposed: &Exposed<'a>) -> Located<Export<'a>> {
    Located::at(exposed_region(exposed), canonical_export(exposed))
}

fn canonical_export<'a>(exposed: &Exposed<'a>) -> Export<'a> {
    match exposed {
        Exposed::Lower(name) => Export::Value(name.value),
        Exposed::Upper {
            name,
            privacy: Privacy::Public(_),
        } => Export::UnionOpen(name.value),
        Exposed::Upper {
            name,
            privacy: Privacy::Private,
        } => Export::UnionClosed(name.value),
        Exposed::Operator { op, .. } => Export::Binop(op),
    }
}

fn exposed_region(exposed: &Exposed<'_>) -> Region {
    match exposed {
        Exposed::Lower(name) => name.region,
        Exposed::Upper {
            name,
            privacy: Privacy::Public(region),
        } => Region::span_across(&name.region, region),
        Exposed::Upper {
            name,
            privacy: Privacy::Private,
        } => name.region,
        Exposed::Operator { region, .. } => *region,
    }
}

#[cfg(test)]
mod tests {
    use bumpalo::Bump;
    use indoc::indoc;
    use nash_ast::PackageName;

    use super::{Context, canonicalize_header};

    macro_rules! assert_header_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let src = bump.alloc_str(input);
            let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());
            let module = parser.module().expect("expected successful parse");
            let result = canonicalize_header(&bump, Context::default(), &module)
                .expect("expected successful canonicalization");

            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    macro_rules! assert_header_error_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let src = bump.alloc_str(input);
            let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());
            let module = parser.module().expect("expected successful parse");
            let result = canonicalize_header(&bump, Context::default(), &module)
                .expect_err("expected canonicalization error");

            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    #[test]
    fn module_header_open_exports() {
        assert_header_snapshot!("module Main exposing (..)\n");
    }

    #[test]
    fn module_header_explicit_exports() {
        assert_header_snapshot!("module Main exposing (main, Model, Msg(..), (+))\n");
    }

    #[test]
    fn module_header_requires_explicit_header() {
        assert_header_error_snapshot!("main = 42\n");
    }

    #[test]
    fn module_header_keeps_package_context() {
        let input = indoc!("module Json.Decode exposing (Decoder)\n");
        let bump = Bump::new();
        let src = bump.alloc_str(input);
        let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());
        let module = parser.module().expect("expected successful parse");
        let context = Context {
            package: Some(PackageName {
                author: "nash",
                project: "compiler",
            }),
        };
        let result = canonicalize_header(&bump, context, &module)
            .expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }
}
