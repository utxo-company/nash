use bumpalo::Bump;
use nash_ast::{
    Alias as CanAlias, Associativity as CanAssociativity, Binop as CanBinop, Decls, Export,
    Exports, Module as CanModule, ModuleName, PackageName, Precedence as CanPrecedence,
    Union as CanUnion,
};
use nash_region::{Located, Region};
use nash_source::{
    Associativity as SourceAssociativity, Exposed, Exposing, Infix, Module as SourceModule,
    Precedence as SourcePrecedence, Privacy,
};

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
    module: &SourceModule<'a>,
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

pub fn canonicalize_module<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    module: &SourceModule<'a>,
) -> Result<CanModule<'a>, Error> {
    let header = canonicalize_header(bump, context, module)?;
    ensure_supported_module_items(module)?;

    let decls = bump.alloc(Decls::Empty);
    let unions: &'a [&'a Located<CanUnion<'a>>] =
        bump.alloc_slice_fill_iter(std::iter::empty::<&'a Located<CanUnion<'a>>>());
    let aliases: &'a [&'a Located<CanAlias<'a>>] =
        bump.alloc_slice_fill_iter(std::iter::empty::<&'a Located<CanAlias<'a>>>());
    let binops = canonicalize_binops(bump, module.binops);

    Ok(CanModule {
        name: header.name,
        exports: header.exports,
        docs: module.docs,
        decls,
        unions,
        aliases,
        binops,
    })
}

fn ensure_supported_module_items(module: &SourceModule<'_>) -> Result<(), Error> {
    if !module.imports.is_empty() {
        return Err(Error::UnsupportedImports);
    }

    if !module.values.is_empty() {
        return Err(Error::UnsupportedValues);
    }

    if !module.unions.is_empty() {
        return Err(Error::UnsupportedUnions);
    }

    if !module.aliases.is_empty() {
        return Err(Error::UnsupportedAliases);
    }

    Ok(())
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

fn canonicalize_binops<'a>(
    bump: &'a Bump,
    binops: &'a [&'a Located<Infix<'a>>],
) -> &'a [&'a Located<CanBinop<'a>>] {
    bump.alloc_slice_fill_iter(binops.iter().copied().map(|binop| {
        let binop = bump.alloc(Located::at(
            binop.region,
            CanBinop {
                symbol: binop.value.op,
                associativity: canonicalize_associativity(&binop.value.associativity),
                precedence: canonicalize_precedence(&binop.value.precedence),
                function: binop.value.name,
            },
        ));
        let binop: &'a Located<CanBinop<'a>> = binop;
        binop
    }))
}

fn canonicalize_associativity(associativity: &SourceAssociativity) -> CanAssociativity {
    match associativity {
        SourceAssociativity::Left => CanAssociativity::Left,
        SourceAssociativity::None => CanAssociativity::None,
        SourceAssociativity::Right => CanAssociativity::Right,
    }
}

fn canonicalize_precedence(precedence: &SourcePrecedence) -> CanPrecedence {
    CanPrecedence(precedence.0)
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

    use super::{Context, canonicalize_header, canonicalize_module};

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

    macro_rules! assert_module_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let src = bump.alloc_str(input);
            let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());
            let module = parser.module().expect("expected successful parse");
            let result = canonicalize_module(&bump, Context::default(), &module)
                .expect("expected successful canonicalization");

            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    macro_rules! assert_module_error_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let src = bump.alloc_str(input);
            let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());
            let module = parser.module().expect("expected successful parse");
            let result = canonicalize_module(&bump, Context::default(), &module)
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

    #[test]
    fn module_shell_header_only() {
        assert_module_snapshot!("module Main exposing (..)\n");
    }

    #[test]
    fn module_shell_with_infix_metadata() {
        assert_module_snapshot!(
            r#"
            module Main exposing ((|>))

            infix left 6 (|>) = apR
        "#
        );
    }

    #[test]
    fn module_shell_rejects_imports() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            import List
        "#
        );
    }

    #[test]
    fn module_shell_rejects_values() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            main = 42
        "#
        );
    }
}
