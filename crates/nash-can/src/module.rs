use bumpalo::Bump;
use nash_ast::{
    Alias as CanAlias, Associativity as CanAssociativity, Binop as CanBinop, Ctor as CanCtor,
    CtorOpts, Decls, Export, Exports, FieldType as CanFieldType, Module as CanModule, ModuleName,
    PackageName, Precedence as CanPrecedence, Type as CanType, Union as CanUnion,
};
use nash_region::{Located, Region};
use nash_source::{
    Alias as SourceAlias, Associativity as SourceAssociativity, Ctor as SourceCtor, Exposed,
    Exposing, FieldType as SourceFieldType, Infix, Module as SourceModule,
    Precedence as SourcePrecedence, Privacy, Type as SourceType, Union as SourceUnion,
    Value as SourceValue,
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
        exports: canonicalize_exports(bump, module),
    })
}

pub fn canonicalize_module<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    module: &SourceModule<'a>,
) -> Result<CanModule<'a>, Error> {
    let header = canonicalize_header(bump, context, module)?;
    let decls = canonicalize_decls(bump, module.values);
    let unions = canonicalize_unions(bump, module.unions);
    let aliases = canonicalize_aliases(bump, module.aliases);
    let binops = canonicalize_binops(bump, module.binops);

    if !module.imports.is_empty() {
        todo!("canonicalize imported interfaces and build the foreign environment");
    }

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

fn canonicalize_decls<'a>(
    bump: &'a Bump,
    values: &'a [&'a Located<SourceValue<'a>>],
) -> &'a Decls<'a> {
    if values.is_empty() {
        bump.alloc(Decls::Empty)
    } else {
        todo!("canonicalize values and dependency ordering");
    }
}

fn canonicalize_unions<'a>(
    bump: &'a Bump,
    unions: &'a [&'a Located<SourceUnion<'a>>],
) -> &'a [&'a Located<CanUnion<'a>>] {
    bump.alloc_slice_fill_iter(unions.iter().copied().map(|union| {
        let union = bump.alloc(Located::at(
            union.region,
            canonicalize_union(bump, &union.value),
        ));
        let union: &'a Located<CanUnion<'a>> = union;
        union
    }))
}

fn canonicalize_union<'a>(bump: &'a Bump, union: &SourceUnion<'a>) -> CanUnion<'a> {
    let parameters =
        bump.alloc_slice_fill_iter(union.arguments.iter().copied().map(|arg| arg.value));
    let ctors = canonicalize_ctors(bump, union.ctors);
    let alternatives = union
        .ctors
        .len()
        .try_into()
        .expect("union alternatives exceed u16");
    let options = if union.ctors.iter().all(|ctor| ctor.arguments.is_empty()) {
        CtorOpts::Enum
    } else {
        CtorOpts::Normal
    };

    CanUnion {
        name: union.name,
        parameters,
        ctors,
        alternatives,
        options,
    }
}

fn canonicalize_ctors<'a>(
    bump: &'a Bump,
    ctors: &'a [&'a SourceCtor<'a>],
) -> &'a [&'a CanCtor<'a>] {
    bump.alloc_slice_fill_iter(ctors.iter().copied().enumerate().map(|(index, ctor)| {
        let ctor = bump.alloc(CanCtor {
            name: ctor.name.value,
            index: index.try_into().expect("constructor index exceeds u16"),
            arity: ctor
                .arguments
                .len()
                .try_into()
                .expect("constructor arity exceeds u16"),
            arguments: bump.alloc_slice_fill_iter(
                ctor.arguments
                    .iter()
                    .copied()
                    .map(|argument| canonicalize_type(bump, argument)),
            ),
        });
        let ctor: &'a CanCtor<'a> = ctor;
        ctor
    }))
}

fn canonicalize_aliases<'a>(
    bump: &'a Bump,
    aliases: &'a [&'a Located<SourceAlias<'a>>],
) -> &'a [&'a Located<CanAlias<'a>>] {
    bump.alloc_slice_fill_iter(aliases.iter().copied().map(|alias| {
        let alias = bump.alloc(Located::at(
            alias.region,
            canonicalize_alias(bump, &alias.value),
        ));
        let alias: &'a Located<CanAlias<'a>> = alias;
        alias
    }))
}

fn canonicalize_alias<'a>(bump: &'a Bump, alias: &SourceAlias<'a>) -> CanAlias<'a> {
    let parameters =
        bump.alloc_slice_fill_iter(alias.arguments.iter().copied().map(|arg| arg.value));

    CanAlias {
        name: alias.name,
        parameters,
        typ: canonicalize_type(bump, alias.typ),
    }
}

fn canonicalize_type<'a>(
    bump: &'a Bump,
    typ: &'a Located<SourceType<'a>>,
) -> &'a Located<CanType<'a>> {
    let typ = bump.alloc(Located::at(
        typ.region,
        canonicalize_type_value(bump, &typ.value),
    ));
    let typ: &'a Located<CanType<'a>> = typ;
    typ
}

fn canonicalize_type_value<'a>(bump: &'a Bump, typ: &SourceType<'a>) -> CanType<'a> {
    match typ {
        SourceType::Lambda { from, to } => CanType::Lambda {
            from: canonicalize_type(bump, from),
            to: canonicalize_type(bump, to),
        },
        SourceType::Var(name) => CanType::Var(name),
        SourceType::Type { name, .. } => todo!("canonicalize unqualified named type `{name}`"),
        SourceType::TypeQual { module, name, .. } => {
            todo!("canonicalize qualified named type `{module}.{name}`")
        }
        SourceType::Record { fields, ext } => CanType::Record {
            fields: bump.alloc_slice_fill_iter(fields.iter().copied().enumerate().map(
                |(index, field)| {
                    canonicalize_field_type(
                        bump,
                        index.try_into().expect("record field index exceeds u16"),
                        field,
                    )
                },
            )),
            ext: ext.map(|name| name.value),
        },
        SourceType::Unit => CanType::Unit,
        SourceType::Tuple {
            first,
            second,
            rest,
        } => CanType::Tuple {
            first: canonicalize_type(bump, first),
            second: canonicalize_type(bump, second),
            rest: bump.alloc_slice_fill_iter(
                rest.iter()
                    .copied()
                    .map(|item| canonicalize_type(bump, item)),
            ),
        },
    }
}

fn canonicalize_field_type<'a>(
    bump: &'a Bump,
    index: u16,
    field: &SourceFieldType<'a>,
) -> CanFieldType<'a> {
    CanFieldType {
        index,
        field: field.field.value,
        typ: canonicalize_type(bump, field.typ),
    }
}

fn canonicalize_exports<'a>(bump: &'a Bump, module: &SourceModule<'a>) -> Exports<'a> {
    match module.exports.value {
        Exposing::Open => Exports::Everything(module.exports.region),
        Exposing::Explicit(exposed) => {
            let exposed: &'a [&'a Located<Export<'a>>] =
                bump.alloc_slice_fill_iter(exposed.iter().copied().map(|exposed| {
                    let export = bump.alloc(canonicalize_exposed(module, exposed));
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

fn canonicalize_exposed<'a>(
    module: &SourceModule<'a>,
    exposed: &Exposed<'a>,
) -> Located<Export<'a>> {
    Located::at(exposed_region(exposed), canonical_export(module, exposed))
}

fn canonical_export<'a>(module: &SourceModule<'a>, exposed: &Exposed<'a>) -> Export<'a> {
    match exposed {
        Exposed::Lower(name) => Export::Value(name.value),
        Exposed::Upper { name, privacy } => {
            if module
                .aliases
                .iter()
                .any(|alias| alias.value.name.value == name.value)
            {
                Export::Alias(name.value)
            } else if module
                .unions
                .iter()
                .any(|union| union.value.name.value == name.value)
            {
                match privacy {
                    Privacy::Public(_) => Export::UnionOpen(name.value),
                    Privacy::Private => Export::UnionClosed(name.value),
                }
            } else {
                todo!(
                    "resolve exported upper name `{}` against local/imported declarations",
                    name.value
                )
            }
        }
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

    #[test]
    fn module_header_open_exports() {
        assert_header_snapshot!("module Main exposing (..)\n");
    }

    #[test]
    fn module_header_explicit_exports() {
        assert_header_snapshot!("module Main exposing (main, (+))\n");
    }

    #[test]
    fn module_header_requires_explicit_header() {
        assert_header_error_snapshot!("main = 42\n");
    }

    #[test]
    fn module_header_keeps_package_context() {
        let input = indoc!("module Json.Decode exposing (decodeString)\n");
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
    fn module_shell_with_enum_union() {
        assert_module_snapshot!(
            r#"
            module Main exposing (Bool(..))

            type Bool
                = True
                | False
        "#
        );
    }

    #[test]
    fn module_shell_with_aliases_and_unions() {
        assert_module_snapshot!(
            r#"
            module Main exposing (Pair, Maybe(..))

            type alias Pair a b = (a, b)

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }
}
