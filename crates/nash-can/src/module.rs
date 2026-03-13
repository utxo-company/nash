use std::collections::BTreeMap;

use bumpalo::Bump;
use nash_ast::{
    Alias as CanAlias, AliasArgument as CanAliasArgument, AliasType as CanAliasType,
    Associativity as CanAssociativity, Binop as CanBinop, Ctor as CanCtor, CtorOpts, Decls, Export,
    Exports, FieldType as CanFieldType, Module as CanModule, ModuleName, PackageName,
    Precedence as CanPrecedence, QualifiedName, Type as CanType, Union as CanUnion,
};
use nash_region::{Located, Region};
use nash_source::{
    Alias as SourceAlias, Associativity as SourceAssociativity, Ctor as SourceCtor, Exposed,
    Exposing, FieldType as SourceFieldType, Import as SourceImport, Infix, Module as SourceModule,
    Precedence as SourcePrecedence, Privacy, Type as SourceType, Union as SourceUnion,
    Value as SourceValue,
};

use crate::error::TypeKind;
use crate::{Error, Interface, InterfaceAlias, InterfaceUnion};

#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    pub package: Option<PackageName<'a>>,
    pub interfaces: Option<&'a BTreeMap<&'a str, Interface<'a>>>,
}

#[derive(Debug)]
struct Header<'a> {
    pub name: ModuleName<'a>,
    pub exports: Exports<'a>,
}

#[derive(Clone, Copy)]
enum ResolvedType<'a> {
    Alias {
        home: ModuleName<'a>,
        alias: &'a InterfaceAlias<'a>,
    },
    Union {
        home: ModuleName<'a>,
        union: &'a InterfaceUnion<'a>,
    },
}

fn try_alloc_slice_fill_iter<'a, T, E>(
    bump: &'a Bump,
    iter: impl IntoIterator<Item = Result<T, E>>,
) -> Result<&'a [T], E> {
    let mut items = Vec::new();

    for item in iter {
        items.push(item?);
    }

    Ok(bump.alloc_slice_fill_iter(items))
}

fn canonicalize_header<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    module: &SourceModule<'a>,
) -> Result<Header<'a>, Error<'a>> {
    let name = module.name.ok_or(Error::MissingModuleHeader)?;

    Ok(Header {
        name: ModuleName {
            package: context.package,
            name: name.value,
        },
        exports: canonicalize_exports(bump, module)?,
    })
}

pub fn canonicalize<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    module: &SourceModule<'a>,
) -> Result<CanModule<'a>, Error<'a>> {
    let header = canonicalize_header(bump, context, module)?;
    let home = header.name;
    let decls = canonicalize_decls(bump, module.values);
    let unions = canonicalize_unions(bump, context, home, module, module.unions)?;
    let aliases = canonicalize_aliases(bump, context, home, module, module.aliases)?;
    let binops = canonicalize_binops(bump, module.binops);

    Ok(CanModule {
        name: home,
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
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    unions: &'a [&'a Located<SourceUnion<'a>>],
) -> Result<&'a [&'a Located<CanUnion<'a>>], Error<'a>> {
    try_alloc_slice_fill_iter(
        bump,
        unions.iter().copied().map(|union| {
            let union = bump.alloc(Located::at(
                union.region,
                canonicalize_union(bump, context, home, module, &union.value)?,
            ));
            let union: &'a Located<CanUnion<'a>> = union;

            Ok(union)
        }),
    )
}

fn canonicalize_union<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    union: &SourceUnion<'a>,
) -> Result<CanUnion<'a>, Error<'a>> {
    let parameters =
        bump.alloc_slice_fill_iter(union.arguments.iter().copied().map(|arg| arg.value));
    let ctors = canonicalize_ctors(bump, context, home, module, union.ctors)?;
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

    Ok(CanUnion {
        name: union.name,
        parameters,
        ctors,
        alternatives,
        options,
    })
}

fn canonicalize_ctors<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    ctors: &'a [&'a SourceCtor<'a>],
) -> Result<&'a [&'a CanCtor<'a>], Error<'a>> {
    try_alloc_slice_fill_iter(
        bump,
        ctors.iter().copied().enumerate().map(|(index, ctor)| {
            let arguments =
                canonicalize_type_arguments(bump, context, home, module, ctor.arguments)?;
            let ctor = bump.alloc(CanCtor {
                name: ctor.name.value,
                index: index.try_into().expect("constructor index exceeds u16"),
                arity: ctor
                    .arguments
                    .len()
                    .try_into()
                    .expect("constructor arity exceeds u16"),
                arguments,
            });
            let ctor: &'a CanCtor<'a> = ctor;

            Ok(ctor)
        }),
    )
}

fn canonicalize_aliases<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    aliases: &'a [&'a Located<SourceAlias<'a>>],
) -> Result<&'a [&'a Located<CanAlias<'a>>], Error<'a>> {
    try_alloc_slice_fill_iter(
        bump,
        aliases.iter().copied().map(|alias| {
            let alias = bump.alloc(Located::at(
                alias.region,
                canonicalize_alias(bump, context, home, module, &alias.value)?,
            ));
            let alias: &'a Located<CanAlias<'a>> = alias;

            Ok(alias)
        }),
    )
}

fn canonicalize_alias<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    alias: &SourceAlias<'a>,
) -> Result<CanAlias<'a>, Error<'a>> {
    let parameters =
        bump.alloc_slice_fill_iter(alias.arguments.iter().copied().map(|arg| arg.value));

    Ok(CanAlias {
        name: alias.name,
        parameters,
        typ: canonicalize_type(bump, context, home, module, alias.typ)?,
    })
}

fn canonicalize_type<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    typ: &'a Located<SourceType<'a>>,
) -> Result<&'a Located<CanType<'a>>, Error<'a>> {
    let typ = bump.alloc(Located::at(
        typ.region,
        canonicalize_type_value(bump, context, home, module, typ.region, &typ.value)?,
    ));
    let typ: &'a Located<CanType<'a>> = typ;

    Ok(typ)
}

fn canonicalize_type_value<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    region: Region,
    typ: &SourceType<'a>,
) -> Result<CanType<'a>, Error<'a>> {
    Ok(match typ {
        SourceType::Lambda { from, to } => CanType::Lambda {
            from: canonicalize_type(bump, context, home, module, from)?,
            to: canonicalize_type(bump, context, home, module, to)?,
        },
        SourceType::Var(name) => CanType::Var(name),
        SourceType::Type { name, args, .. } => {
            canonicalize_named_type(bump, context, home, module, region, name, args)?
        }
        SourceType::TypeQual {
            module: type_module,
            name,
            args,
            ..
        } => {
            if *type_module == home.name {
                canonicalize_named_type(bump, context, home, module, region, name, args)?
            } else {
                canonicalize_qualified_named_type(
                    bump,
                    context,
                    home,
                    module,
                    region,
                    type_module,
                    name,
                    args,
                )?
            }
        }
        SourceType::Record { fields, ext } => CanType::Record {
            fields: try_alloc_slice_fill_iter(
                bump,
                fields.iter().copied().enumerate().map(|(index, field)| {
                    canonicalize_field_type(
                        bump,
                        context,
                        home,
                        module,
                        index.try_into().expect("record field index exceeds u16"),
                        field,
                    )
                }),
            )?,
            ext: ext.map(|name| name.value),
        },
        SourceType::Unit => CanType::Unit,
        SourceType::Tuple {
            first,
            second,
            rest,
        } => CanType::Tuple {
            first: canonicalize_type(bump, context, home, module, first)?,
            second: canonicalize_type(bump, context, home, module, second)?,
            rest: canonicalize_type_arguments(bump, context, home, module, rest)?,
        },
    })
}

fn canonicalize_named_type<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    region: Region,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<CanType<'a>, Error<'a>> {
    if let Some(alias) = find_alias(module.aliases, name) {
        ensure_type_arity(
            region,
            TypeKind::Alias,
            name,
            alias.arguments.len(),
            args.len(),
        )?;
        let arguments =
            canonicalize_alias_arguments(bump, context, home, module, alias.arguments, args)?;
        let target = CanAliasType::Open(canonicalize_type(bump, context, home, module, alias.typ)?);

        Ok(CanType::Alias {
            reference: QualifiedName { home, name },
            arguments,
            target,
        })
    } else if let Some(union) = find_union(module.unions, name) {
        ensure_type_arity(
            region,
            TypeKind::Union,
            name,
            union.arguments.len(),
            args.len(),
        )?;
        let args = canonicalize_type_arguments(bump, context, home, module, args)?;

        Ok(CanType::Named {
            reference: QualifiedName { home, name },
            args,
        })
    } else if let Some(resolved) = find_imported_named_type(context, module, region, name)? {
        canonicalize_resolved_type(bump, context, home, module, region, name, args, resolved)
    } else {
        Err(Error::UnresolvedNamedType { region, name })
    }
}

fn canonicalize_qualified_named_type<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    region: Region,
    type_module: &'a str,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<CanType<'a>, Error<'a>> {
    let resolved = find_imported_qualified_named_type(context, module, type_module, name)?.ok_or(
        Error::UnresolvedQualifiedNamedType {
            region,
            module: type_module,
            name,
        },
    )?;

    canonicalize_resolved_type(bump, context, home, module, region, name, args, resolved)
}

fn canonicalize_resolved_type<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    region: Region,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
    resolved: ResolvedType<'a>,
) -> Result<CanType<'a>, Error<'a>> {
    match resolved {
        ResolvedType::Alias {
            home: imported_home,
            alias,
        } => {
            ensure_type_arity(
                region,
                TypeKind::Alias,
                name,
                alias.parameters.len(),
                args.len(),
            )?;
            let arguments = canonicalize_interface_alias_arguments(
                bump,
                context,
                home,
                module,
                alias.parameters,
                args,
            )?;

            Ok(CanType::Alias {
                reference: QualifiedName {
                    home: imported_home,
                    name: alias.name,
                },
                arguments,
                target: CanAliasType::Open(alias.typ),
            })
        }
        ResolvedType::Union {
            home: imported_home,
            union,
        } => {
            ensure_type_arity(
                region,
                TypeKind::Union,
                name,
                union.parameters.len(),
                args.len(),
            )?;
            let args = canonicalize_type_arguments(bump, context, home, module, args)?;

            Ok(CanType::Named {
                reference: QualifiedName {
                    home: imported_home,
                    name: union.name,
                },
                args,
            })
        }
    }
}

fn canonicalize_alias_arguments<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    parameters: &'a [&'a Located<&'a str>],
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [CanAliasArgument<'a>], Error<'a>> {
    try_alloc_slice_fill_iter(
        bump,
        parameters
            .iter()
            .copied()
            .zip(args.iter().copied())
            .map(|(parameter, arg)| {
                Ok(CanAliasArgument {
                    name: parameter.value,
                    typ: canonicalize_type(bump, context, home, module, arg)?,
                })
            }),
    )
}

fn canonicalize_interface_alias_arguments<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    parameters: &'a [&'a str],
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [CanAliasArgument<'a>], Error<'a>> {
    try_alloc_slice_fill_iter(
        bump,
        parameters
            .iter()
            .copied()
            .zip(args.iter().copied())
            .map(|(parameter, arg)| {
                Ok(CanAliasArgument {
                    name: parameter,
                    typ: canonicalize_type(bump, context, home, module, arg)?,
                })
            }),
    )
}

fn canonicalize_type_arguments<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [&'a Located<CanType<'a>>], Error<'a>> {
    try_alloc_slice_fill_iter(
        bump,
        args.iter()
            .copied()
            .map(|arg| canonicalize_type(bump, context, home, module, arg)),
    )
}

fn ensure_type_arity<'a>(
    region: Region,
    kind: TypeKind,
    name: &'a str,
    expected: usize,
    actual: usize,
) -> Result<(), Error<'a>> {
    if expected == actual {
        Ok(())
    } else {
        Err(Error::BadTypeArity {
            region,
            kind,
            name,
            expected,
            actual,
        })
    }
}

fn find_alias<'a>(
    aliases: &'a [&'a Located<SourceAlias<'a>>],
    name: &str,
) -> Option<&'a SourceAlias<'a>> {
    aliases
        .iter()
        .find(|alias| alias.value.name.value == name)
        .map(|alias| &alias.value)
}

fn find_union<'a>(
    unions: &'a [&'a Located<SourceUnion<'a>>],
    name: &str,
) -> Option<&'a SourceUnion<'a>> {
    unions
        .iter()
        .find(|union| union.value.name.value == name)
        .map(|union| &union.value)
}

fn find_imported_named_type<'a>(
    context: Context<'a>,
    module: &SourceModule<'a>,
    region: Region,
    name: &'a str,
) -> Result<Option<ResolvedType<'a>>, Error<'a>> {
    let mut explicit_resolved = None;
    let mut open_resolved = None;

    for import in module.imports {
        if !import_exposes_type(import, name) {
            continue;
        }

        let interface = imported_interface(context, import)?;
        let Some(candidate) = resolve_interface_type(interface, name) else {
            continue;
        };

        match import.exposing {
            Exposing::Open => {
                if let Some((_, first_module)) = open_resolved {
                    return Err(Error::AmbiguousImportedType {
                        region,
                        name,
                        first_module,
                        second_module: import.import.value,
                    });
                }

                open_resolved = Some((candidate, import.import.value));
            }
            Exposing::Explicit(_) => {
                if explicit_resolved.is_none() {
                    explicit_resolved = Some(candidate);
                }
            }
        }
    }

    Ok(explicit_resolved.or_else(|| open_resolved.map(|(candidate, _)| candidate)))
}

fn find_imported_qualified_named_type<'a>(
    context: Context<'a>,
    module: &SourceModule<'a>,
    prefix: &str,
    name: &'a str,
) -> Result<Option<ResolvedType<'a>>, Error<'a>> {
    let Some(import) = module
        .imports
        .iter()
        .copied()
        .find(|import| import_prefix(import) == prefix)
    else {
        return Ok(None);
    };
    let interface = imported_interface(context, import)?;

    Ok(resolve_interface_type(interface, name))
}

fn imported_interface<'a>(
    context: Context<'a>,
    import: &'a SourceImport<'a>,
) -> Result<&'a Interface<'a>, Error<'a>> {
    find_interface(context, import.import.value).ok_or(Error::MissingImportedInterface {
        region: import.import.region,
        module: import.import.value,
    })
}

fn find_interface<'a>(context: Context<'a>, module_name: &str) -> Option<&'a Interface<'a>> {
    context.interfaces?.get(module_name)
}

fn resolve_interface_type<'a>(
    interface: &'a Interface<'a>,
    name: &str,
) -> Option<ResolvedType<'a>> {
    if let Some(alias) = interface.aliases.iter().find(|alias| alias.name == name) {
        Some(ResolvedType::Alias {
            home: interface.home,
            alias,
        })
    } else {
        interface
            .unions
            .iter()
            .find(|union| union.name == name)
            .map(|union| ResolvedType::Union {
                home: interface.home,
                union,
            })
    }
}

fn import_exposes_type(import: &SourceImport<'_>, name: &str) -> bool {
    match import.exposing {
        Exposing::Open => true,
        Exposing::Explicit(exposed) => exposed.iter().any(|exposed| match exposed {
            Exposed::Upper {
                name: exposed_name, ..
            } => exposed_name.value == name,
            Exposed::Lower(_) | Exposed::Operator { .. } => false,
        }),
    }
}

fn import_prefix<'a>(import: &SourceImport<'a>) -> &'a str {
    import.alias.unwrap_or(import.import.value)
}

fn canonicalize_field_type<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    home: ModuleName<'a>,
    module: &SourceModule<'a>,
    index: u16,
    field: &SourceFieldType<'a>,
) -> Result<CanFieldType<'a>, Error<'a>> {
    Ok(CanFieldType {
        index,
        field: field.field.value,
        typ: canonicalize_type(bump, context, home, module, field.typ)?,
    })
}

fn canonicalize_exports<'a>(
    bump: &'a Bump,
    module: &SourceModule<'a>,
) -> Result<Exports<'a>, Error<'a>> {
    match module.exports.value {
        Exposing::Open => Ok(Exports::Everything(module.exports.region)),
        Exposing::Explicit(exposed) => {
            let exposed: &'a [&'a Located<Export<'a>>] = try_alloc_slice_fill_iter(
                bump,
                exposed
                    .iter()
                    .copied()
                    .map(|exposed| canonicalize_exposed(bump, module, exposed)),
            )?;

            Ok(Exports::Explicit(exposed))
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
    bump: &'a Bump,
    module: &SourceModule<'a>,
    exposed: &Exposed<'a>,
) -> Result<&'a Located<Export<'a>>, Error<'a>> {
    Ok(bump.alloc(Located::at(
        exposed_region(exposed),
        canonicalize_export(module, exposed)?,
    )))
}

fn canonicalize_export<'a>(
    module: &SourceModule<'a>,
    exposed: &Exposed<'a>,
) -> Result<Export<'a>, Error<'a>> {
    Ok(match exposed {
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
                return Err(Error::UnresolvedExportedUpperName {
                    region: exposed_region(exposed),
                    name: name.value,
                });
            }
        }
        Exposed::Operator { op, .. } => Export::Binop(op),
    })
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
    use std::collections::BTreeMap;

    use bumpalo::Bump;
    use indoc::indoc;
    use nash_ast::{ModuleName, PackageName, Type as CanType};
    use nash_region::{Located, Region};

    use super::{Context, canonicalize, canonicalize_header};
    use crate::{Interface, InterfaceAlias, InterfaceUnion};

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
            let result = canonicalize(&bump, Context::default(), &module)
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
            let result = canonicalize(&bump, Context::default(), &module)
                .expect_err("expected canonicalization error");

            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    fn var_type<'a>(bump: &'a Bump, name: &'a str) -> &'a Located<CanType<'a>> {
        bump.alloc(Located::at(Region::zero(), CanType::Var(name)))
    }

    fn union_interface<'a>(
        bump: &'a Bump,
        module_name: &'a str,
        union_name: &'a str,
        parameters: &'a [&'a str],
    ) -> Interface<'a> {
        Interface {
            home: ModuleName {
                package: None,
                name: module_name,
            },
            aliases: bump.alloc_slice_fill_iter(std::iter::empty::<InterfaceAlias<'a>>()),
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                name: union_name,
                parameters,
            }]),
        }
    }

    fn alias_interface<'a>(
        bump: &'a Bump,
        module_name: &'a str,
        alias_name: &'a str,
        parameters: &'a [&'a str],
        typ: &'a Located<CanType<'a>>,
    ) -> Interface<'a> {
        Interface {
            home: ModuleName {
                package: None,
                name: module_name,
            },
            aliases: bump.alloc_slice_fill_iter([InterfaceAlias {
                name: alias_name,
                parameters,
                typ,
            }]),
            unions: bump.alloc_slice_fill_iter(std::iter::empty::<InterfaceUnion<'a>>()),
        }
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
            interfaces: None,
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

    #[test]
    fn module_shell_with_local_named_types() {
        assert_module_snapshot!(
            r#"
            module Main exposing (Pair, WrappedPair, WrappedMaybe, Maybe(..))

            type alias Pair a b = (a, b)

            type alias WrappedPair a b = Pair a b

            type alias WrappedMaybe a = Maybe a

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn module_shell_with_self_qualified_named_types() {
        assert_module_snapshot!(
            r#"
            module Main exposing (Maybe(..), Wrapped)

            type alias Wrapped a = Main.Maybe a

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn module_shell_reports_unresolved_named_types() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Wrapped)

            type alias Wrapped = Missing
        "#
        );
    }

    #[test]
    fn module_shell_reports_unresolved_qualified_named_types() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Wrapped)

            type alias Wrapped = Missing.Maybe
        "#
        );
    }

    #[test]
    fn module_shell_reports_missing_imported_interfaces() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Wrapped)

            import Result

            type alias Wrapped e a = Result.Result e a
        "#
        );
    }

    #[test]
    fn module_shell_reports_ambiguous_open_imported_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Maybe exposing (..)

            import Option exposing (..)

            type alias Wrapped a = Maybe a
        "#
        );
        let bump = Bump::new();
        let src = bump.alloc_str(input);
        let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());
        let module = parser.module().expect("expected successful parse");
        let parameters = bump.alloc_slice_fill_iter(["a"]);
        let interfaces = BTreeMap::from([
            (
                "Maybe",
                union_interface(&bump, "Maybe", "Maybe", parameters),
            ),
            (
                "Option",
                union_interface(&bump, "Option", "Maybe", parameters),
            ),
        ]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result =
            canonicalize(&bump, context, &module).expect_err("expected canonicalization error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_reports_bad_type_arity() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Wrapped, Maybe(..))

            type alias Wrapped = Maybe

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn module_header_reports_unresolved_exported_upper_names() {
        assert_header_error_snapshot!("module Main exposing (Missing)\n");
    }

    #[test]
    fn module_shell_with_imported_exposed_union_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Maybe exposing (Maybe)

            type alias Wrapped a = Maybe a
        "#
        );
        let bump = Bump::new();
        let src = bump.alloc_str(input);
        let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());
        let module = parser.module().expect("expected successful parse");
        let parameters = bump.alloc_slice_fill_iter(["a"]);
        let interfaces = BTreeMap::from([(
            "Maybe",
            union_interface(&bump, "Maybe", "Maybe", parameters),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result =
            canonicalize(&bump, context, &module).expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_with_imported_qualified_union_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Result

            type alias Wrapped e a = Result.Result e a
        "#
        );
        let bump = Bump::new();
        let src = bump.alloc_str(input);
        let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());
        let module = parser.module().expect("expected successful parse");
        let parameters = bump.alloc_slice_fill_iter(["e", "a"]);
        let interfaces = BTreeMap::from([(
            "Result",
            union_interface(&bump, "Result", "Result", parameters),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result =
            canonicalize(&bump, context, &module).expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_with_imported_aliased_alias_types() {
        let input = indoc!(
            r#"
            module Main exposing (Decoder)

            import Json.Decode as Decode

            type alias Decoder msg = Decode.Decoder msg
        "#
        );
        let bump = Bump::new();
        let src = bump.alloc_str(input);
        let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());
        let module = parser.module().expect("expected successful parse");
        let parameters = bump.alloc_slice_fill_iter(["msg"]);
        let interfaces = BTreeMap::from([(
            "Json.Decode",
            alias_interface(
                &bump,
                "Json.Decode",
                "Decoder",
                parameters,
                var_type(&bump, "msg"),
            ),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result =
            canonicalize(&bump, context, &module).expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }
}
