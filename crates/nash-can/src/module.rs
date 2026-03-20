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
    Exposing, FieldType as SourceFieldType, Infix, Module as SourceModule,
    Precedence as SourcePrecedence, Privacy, Type as SourceType, Union as SourceUnion,
    Value as SourceValue,
};

use crate::environment::{self, Env, Info};
use crate::error::{BadArityContext, VarKind};
use crate::{Error, Interface};

#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    pub package: Option<PackageName<'a>>,
    pub interfaces: Option<&'a BTreeMap<&'a str, Interface<'a>>>,
}

trait BumpExt<'a> {
    fn alloc_slice_fill_results<T, E>(
        &'a self,
        iter: impl IntoIterator<Item = Result<T, E>>,
    ) -> Result<&'a [T], E>;
}

impl<'a> BumpExt<'a> for Bump {
    fn alloc_slice_fill_results<T, E>(
        &'a self,
        iter: impl IntoIterator<Item = Result<T, E>>,
    ) -> Result<&'a [T], E> {
        let items = iter.into_iter().collect::<Result<Vec<_>, _>>()?;
        Ok(self.alloc_slice_fill_iter(items))
    }
}

fn canonicalize_header<'a>(
    context: Context<'a>,
    module: &SourceModule<'a>,
) -> Result<ModuleName<'a>, Error<'a>> {
    let name = module.name.ok_or(Error::MissingModuleHeader)?;

    Ok(ModuleName {
        package: context.package,
        name: name.value,
    })
}

pub fn canonicalize<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    module: &SourceModule<'a>,
) -> Result<CanModule<'a>, Error<'a>> {
    let home = canonicalize_header(context, module)?;

    // Phase 1: Build env from imports
    let mut env =
        environment::foreign::create_initial_env(bump, home, context.interfaces, module.imports)?;

    // Phase 2: Add local union type names
    environment::local::add_union_types(&mut env, module.unions);

    // Phase 3: Canonicalize aliases incrementally, adding each to env
    let aliases = canonicalize_aliases(bump, &mut env, module.aliases)?;

    // Phase 4: Canonicalize union ctor types (all types now in env)
    let unions = canonicalize_unions(bump, &env, module.aliases, module.unions)?;

    // Phase 5: Add remaining local definitions to env
    environment::local::add_ctors(&mut env, unions);
    environment::local::add_vars(&mut env, module.values);
    environment::local::add_binops(&mut env, module.binops);

    // Phase 6: Canonicalize remaining (values still todo)
    let decls = canonicalize_decls(bump, module.values);
    let binops = canonicalize_binops(bump, module.binops);
    let exports = canonicalize_exports(bump, module)?;

    Ok(CanModule {
        name: env.home,
        exports,
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
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    unions: &'a [&'a Located<SourceUnion<'a>>],
) -> Result<&'a [&'a Located<CanUnion<'a>>], Error<'a>> {
    bump.alloc_slice_fill_results(unions.iter().copied().map(|union| {
        Ok(&*bump.alloc(Located::at(
            union.region,
            canonicalize_union(bump, env, source_aliases, &union.value)?,
        )))
    }))
}

fn canonicalize_union<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    union: &SourceUnion<'a>,
) -> Result<CanUnion<'a>, Error<'a>> {
    let parameters =
        bump.alloc_slice_fill_iter(union.arguments.iter().copied().map(|arg| arg.value));
    let ctors = canonicalize_ctors(bump, env, source_aliases, union.ctors)?;
    let alternatives = union
        .ctors
        .len()
        .try_into()
        .expect("union alternatives exceed u16");
    let options = if union.ctors.len() == 1 && union.ctors[0].arguments.len() == 1 {
        CtorOpts::Unbox
    } else if union.ctors.iter().all(|ctor| ctor.arguments.is_empty()) {
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
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    ctors: &'a [&'a SourceCtor<'a>],
) -> Result<&'a [&'a CanCtor<'a>], Error<'a>> {
    bump.alloc_slice_fill_results(ctors.iter().copied().enumerate().map(|(index, ctor)| {
        let arguments = canonicalize_type_arguments(bump, env, source_aliases, ctor.arguments)?;
        Ok(&*bump.alloc(CanCtor {
            name: ctor.name.value,
            index: index.try_into().expect("constructor index exceeds u16"),
            arity: ctor
                .arguments
                .len()
                .try_into()
                .expect("constructor arity exceeds u16"),
            arguments,
        }))
    }))
}

fn canonicalize_aliases<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
) -> Result<&'a [&'a Located<CanAlias<'a>>], Error<'a>> {
    let mut results = Vec::new();

    for source_alias in source_aliases {
        let alias = &source_alias.value;
        let parameters =
            bump.alloc_slice_fill_iter(alias.arguments.iter().copied().map(|arg| arg.value));
        let typ = canonicalize_type(bump, env, source_aliases, alias.typ)?;

        let can_alias = CanAlias {
            name: alias.name,
            parameters,
            typ,
        };

        // Add to env so subsequent aliases can reference it
        // (also creates RecordCtor if alias body is a plain record)
        environment::local::add_alias_type(bump, env, &can_alias);

        results.push(&*bump.alloc(Located::at(source_alias.region, can_alias)));
    }

    Ok(bump.alloc_slice_fill_iter(results))
}

fn canonicalize_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    typ: &'a Located<SourceType<'a>>,
) -> Result<&'a Located<CanType<'a>>, Error<'a>> {
    Ok(bump.alloc(Located::at(
        typ.region,
        canonicalize_type_value(bump, env, source_aliases, typ.region, &typ.value)?,
    )))
}

fn canonicalize_type_value<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    region: Region,
    typ: &SourceType<'a>,
) -> Result<CanType<'a>, Error<'a>> {
    Ok(match typ {
        SourceType::Lambda { from, to } => CanType::Lambda {
            from: canonicalize_type(bump, env, source_aliases, from)?,
            to: canonicalize_type(bump, env, source_aliases, to)?,
        },
        SourceType::Var(name) => CanType::Var(name),
        SourceType::Type { name, args, .. } => {
            canonicalize_named_type(bump, env, source_aliases, region, name, args)?
        }
        SourceType::TypeQual {
            module: type_module,
            name,
            args,
            ..
        } => {
            if *type_module == env.home.name {
                canonicalize_named_type(bump, env, source_aliases, region, name, args)?
            } else {
                canonicalize_qualified_named_type(
                    bump,
                    env,
                    source_aliases,
                    region,
                    type_module,
                    name,
                    args,
                )?
            }
        }
        SourceType::Record { fields, ext } => CanType::Record {
            fields: bump.alloc_slice_fill_results(fields.iter().copied().enumerate().map(
                |(index, field)| {
                    canonicalize_field_type(
                        bump,
                        env,
                        source_aliases,
                        index.try_into().expect("record field index exceeds u16"),
                        field,
                    )
                },
            ))?,
            ext: ext.map(|name| name.value),
        },
        SourceType::Unit => CanType::Unit,
        SourceType::Tuple {
            first,
            second,
            rest,
        } => CanType::Tuple {
            first: canonicalize_type(bump, env, source_aliases, first)?,
            second: canonicalize_type(bump, env, source_aliases, second)?,
            rest: canonicalize_type_arguments(bump, env, source_aliases, rest)?,
        },
    })
}

fn canonicalize_named_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    region: Region,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<CanType<'a>, Error<'a>> {
    // Check env (imported types + local unions + already-processed aliases)
    if let Some(info) = env.types.get(name) {
        return match info {
            Info::Specific(_, typ) => {
                canonicalize_env_type(bump, env, source_aliases, region, name, args, *typ)
            }
            Info::Ambiguous(first, others) => Err(Error::AmbiguousType {
                region,
                prefix: None,
                name,
                first_module: *first,
                other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
            }),
        };
    }

    // Fallback: source aliases not yet in env (forward references during alias processing)
    if let Some(alias) = find_source_alias(source_aliases, name) {
        check_arity(region, name, alias.arguments.len(), args.len())?;
        let arguments =
            canonicalize_alias_arguments(bump, env, source_aliases, alias.arguments, args)?;
        let target = CanAliasType::Open(canonicalize_type(bump, env, source_aliases, alias.typ)?);

        return Ok(CanType::Alias {
            reference: QualifiedName {
                home: env.home,
                name,
            },
            arguments,
            target,
        });
    }

    Err(Error::NotFoundType {
        region,
        prefix: None,
        name,
    })
}

fn canonicalize_qualified_named_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    region: Region,
    prefix: &'a str,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<CanType<'a>, Error<'a>> {
    let info = env
        .q_types
        .get(prefix)
        .and_then(|m| m.get(name))
        .ok_or(Error::NotFoundType {
            region,
            prefix: Some(prefix),
            name,
        })?;

    match info {
        Info::Specific(_, typ) => {
            canonicalize_env_type(bump, env, source_aliases, region, name, args, *typ)
        }
        Info::Ambiguous(first, others) => Err(Error::AmbiguousType {
            region,
            prefix: Some(prefix),
            name,
            first_module: *first,
            other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
        }),
    }
}

fn canonicalize_env_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    region: Region,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
    typ: environment::Type<'a>,
) -> Result<CanType<'a>, Error<'a>> {
    match typ {
        environment::Type::Alias {
            arity,
            home,
            parameters,
            typ: alias_typ,
        } => {
            check_arity(region, name, arity, args.len())?;
            let arguments =
                canonicalize_env_alias_arguments(bump, env, source_aliases, parameters, args)?;
            Ok(CanType::Alias {
                reference: QualifiedName { home, name },
                arguments,
                target: CanAliasType::Open(alias_typ),
            })
        }
        environment::Type::Union { arity, home } => {
            check_arity(region, name, arity, args.len())?;
            let args = canonicalize_type_arguments(bump, env, source_aliases, args)?;
            Ok(CanType::Named {
                reference: QualifiedName { home, name },
                args,
            })
        }
    }
}

fn canonicalize_alias_arguments<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    parameters: &'a [&'a Located<&'a str>],
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [CanAliasArgument<'a>], Error<'a>> {
    bump.alloc_slice_fill_results(parameters.iter().copied().zip(args.iter().copied()).map(
        |(parameter, arg)| {
            Ok(CanAliasArgument {
                name: parameter.value,
                typ: canonicalize_type(bump, env, source_aliases, arg)?,
            })
        },
    ))
}

fn canonicalize_env_alias_arguments<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    parameters: &'a [&'a str],
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [CanAliasArgument<'a>], Error<'a>> {
    bump.alloc_slice_fill_results(parameters.iter().copied().zip(args.iter().copied()).map(
        |(parameter, arg)| {
            Ok(CanAliasArgument {
                name: parameter,
                typ: canonicalize_type(bump, env, source_aliases, arg)?,
            })
        },
    ))
}

fn canonicalize_type_arguments<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [&'a Located<CanType<'a>>], Error<'a>> {
    bump.alloc_slice_fill_results(
        args.iter()
            .copied()
            .map(|arg| canonicalize_type(bump, env, source_aliases, arg)),
    )
}

fn check_arity<'a>(
    region: Region,
    name: &'a str,
    expected: usize,
    actual: usize,
) -> Result<(), Error<'a>> {
    if expected == actual {
        Ok(())
    } else {
        Err(Error::BadArity {
            region,
            context: BadArityContext::TypeArity,
            name,
            expected,
            actual,
        })
    }
}

fn find_source_alias<'a>(
    aliases: &'a [&'a Located<SourceAlias<'a>>],
    name: &str,
) -> Option<&'a SourceAlias<'a>> {
    aliases
        .iter()
        .find(|alias| alias.value.name.value == name)
        .map(|alias| &alias.value)
}

fn canonicalize_field_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
    index: u16,
    field: &SourceFieldType<'a>,
) -> Result<CanFieldType<'a>, Error<'a>> {
    Ok(CanFieldType {
        index,
        field: field.field.value,
        typ: canonicalize_type(bump, env, source_aliases, field.typ)?,
    })
}

fn canonicalize_exports<'a>(
    bump: &'a Bump,
    module: &SourceModule<'a>,
) -> Result<Exports<'a>, Error<'a>> {
    match module.exports.value {
        Exposing::Open => Ok(Exports::Everything(module.exports.region)),
        Exposing::Explicit(exposed) => {
            let exposed: &'a [&'a Located<Export<'a>>] = bump.alloc_slice_fill_results(
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
        &*bump.alloc(Located::at(
            binop.region,
            CanBinop {
                symbol: binop.value.op,
                associativity: canonicalize_associativity(binop.value.associativity),
                precedence: canonicalize_precedence(binop.value.precedence),
                function: binop.value.name,
            },
        ))
    }))
}

fn canonicalize_associativity(associativity: SourceAssociativity) -> CanAssociativity {
    match associativity {
        SourceAssociativity::Left => CanAssociativity::Left,
        SourceAssociativity::None => CanAssociativity::None,
        SourceAssociativity::Right => CanAssociativity::Right,
    }
}

fn canonicalize_precedence(precedence: SourcePrecedence) -> CanPrecedence {
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
        Exposed::Lower(name) => {
            if module
                .values
                .iter()
                .any(|value| value.value.name.value == name.value)
            {
                Export::Value(name.value)
            } else {
                return Err(Error::ExportNotFound {
                    region: name.region,
                    kind: VarKind::BadVar,
                    name: name.value,
                });
            }
        }
        Exposed::Upper { name, privacy } => {
            if module
                .unions
                .iter()
                .any(|union| union.value.name.value == name.value)
            {
                match privacy {
                    Privacy::Public(_) => Export::UnionOpen(name.value),
                    Privacy::Private => Export::UnionClosed(name.value),
                }
            } else if module
                .aliases
                .iter()
                .any(|alias| alias.value.name.value == name.value)
            {
                match privacy {
                    Privacy::Public(region) => {
                        return Err(Error::ExportOpenAlias {
                            region: *region,
                            name: name.value,
                        });
                    }
                    Privacy::Private => Export::Alias(name.value),
                }
            } else {
                return Err(Error::ExportNotFound {
                    region: name.region,
                    kind: VarKind::BadType,
                    name: name.value,
                });
            }
        }
        Exposed::Operator { region, op } => {
            if module.binops.iter().any(|binop| binop.value.op == *op) {
                Export::Binop(op)
            } else {
                return Err(Error::ExportNotFound {
                    region: *region,
                    kind: VarKind::BadOp,
                    name: op,
                });
            }
        }
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
    use nash_ast::{
        Ctor as CanCtor, CtorOpts, Module as CanModule, ModuleName, PackageName, Type as CanType,
    };
    use nash_region::{Located, Region};

    use super::{Context, canonicalize};
    use crate::interface::{
        self, AliasVisibility, InterfaceAlias, InterfaceUnion, UnionVisibility,
    };
    use crate::{Error, Interface};

    fn parse_and_canonicalize<'a>(
        bump: &'a Bump,
        input: &str,
        context: Context<'a>,
    ) -> Result<CanModule<'a>, Error<'a>> {
        let src = bump.alloc_str(input);
        let mut parser = nash_parse::Parser::new(bump, src.as_bytes());
        let module = parser.module().expect("expected successful parse");
        canonicalize(bump, context, &module)
    }

    macro_rules! assert_module_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let result = parse_and_canonicalize(&bump, input, Context::default())
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
            let result = parse_and_canonicalize(&bump, input, Context::default())
                .expect_err("expected canonicalization error");

            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    macro_rules! assert_interface_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let can_module = parse_and_canonicalize(&bump, input, Context::default())
                .expect("expected successful canonicalization");
            let result = interface::from_module(&bump, &can_module);
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
            values: &[],
            aliases: &[],
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                name: union_name,
                parameters,
                ctors: &[],
                alternatives: 0,
                options: CtorOpts::Normal,
                visibility: UnionVisibility::Open,
            }]),
            binops: &[],
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
            values: &[],
            aliases: bump.alloc_slice_fill_iter([InterfaceAlias {
                name: alias_name,
                parameters,
                typ,
                visibility: AliasVisibility::Public,
            }]),
            unions: &[],
            binops: &[],
        }
    }

    // === Module tests ===

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
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_reports_ambiguous_explicit_and_open_imported_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Maybe exposing (Maybe)

            import Option exposing (..)

            type alias Wrapped a = Maybe a
        "#
        );
        let bump = Bump::new();
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
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_reports_ambiguous_qualified_imported_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Json.Decode as Decode

            import Html.Decode as Decode

            type alias Wrapped msg = Decode.Decoder msg
        "#
        );
        let bump = Bump::new();
        let parameters = bump.alloc_slice_fill_iter(["msg"]);
        let interfaces = BTreeMap::from([
            (
                "Json.Decode",
                alias_interface(
                    &bump,
                    "Json.Decode",
                    "Decoder",
                    parameters,
                    var_type(&bump, "msg"),
                ),
            ),
            (
                "Html.Decode",
                alias_interface(
                    &bump,
                    "Html.Decode",
                    "Decoder",
                    parameters,
                    var_type(&bump, "msg"),
                ),
            ),
        ]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");

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
    fn module_shell_reports_missing_exported_values() {
        assert_module_error_snapshot!("module Main exposing (main)\n");
    }

    #[test]
    fn module_shell_reports_missing_exported_operators() {
        assert_module_error_snapshot!("module Main exposing ((|>))\n");
    }

    #[test]
    fn module_shell_reports_ambiguous_imported_types_with_multiple_modules() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Maybe exposing (..)

            import Option exposing (..)

            import Choice exposing (..)

            type alias Wrapped a = Maybe a
        "#
        );
        let bump = Bump::new();
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
            (
                "Choice",
                union_interface(&bump, "Choice", "Maybe", parameters),
            ),
        ]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
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
        let parameters = bump.alloc_slice_fill_iter(["a"]);
        let interfaces = BTreeMap::from([(
            "Maybe",
            union_interface(&bump, "Maybe", "Maybe", parameters),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");

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
        let parameters = bump.alloc_slice_fill_iter(["e", "a"]);
        let interfaces = BTreeMap::from([(
            "Result",
            union_interface(&bump, "Result", "Result", parameters),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");

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
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    // === Converted tests ===

    #[test]
    fn module_shell_requires_explicit_header() {
        assert_module_error_snapshot!("main = 42\n");
    }

    #[test]
    fn module_shell_keeps_package_context() {
        let input = indoc!("module Json.Decode exposing (..)\n");
        let bump = Bump::new();
        let context = Context {
            package: Some(PackageName {
                author: "nash",
                project: "compiler",
            }),
            interfaces: None,
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_reports_unresolved_exported_upper_names() {
        assert_module_error_snapshot!("module Main exposing (Missing)\n");
    }

    #[test]
    fn module_shell_reports_export_open_alias() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Pair(..))

            type alias Pair a b = (a, b)
        "#
        );
    }

    // === Interface tests ===

    #[test]
    fn interface_from_module_empty() {
        assert_interface_snapshot!("module Main exposing (..)\n");
    }

    #[test]
    fn interface_from_module_open_exports() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (..)

            type alias Pair a b = (a, b)

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn interface_from_module_open_union() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (Bool(..))

            type Bool
                = True
                | False
        "#
        );
    }

    #[test]
    fn interface_from_module_closed_union() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (Bool)

            type Bool
                = True
                | False
        "#
        );
    }

    #[test]
    fn interface_from_module_mixed_visibility() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (PublicAlias)

            type alias PublicAlias a = a

            type alias PrivateAlias a = a

            type PrivateUnion
                = Foo
                | Bar
        "#
        );
    }

    #[test]
    fn interface_from_module_with_binops() {
        assert_interface_snapshot!(
            r#"
            module Main exposing ((|>))

            infix left 6 (|>) = apR
        "#
        );
    }

    // === to_public tests ===

    #[test]
    fn to_public_union_open_passes_through() {
        let union = InterfaceUnion {
            name: "Bool",
            parameters: &[],
            ctors: &[],
            alternatives: 2,
            options: CtorOpts::Enum,
            visibility: UnionVisibility::Open,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(union.to_public());
        });
    }

    #[test]
    fn to_public_union_closed_strips_ctors() {
        let bump = Bump::new();
        let ctor: &CanCtor = bump.alloc(CanCtor {
            name: "True",
            index: 0,
            arity: 0,
            arguments: &[],
        });
        let union = InterfaceUnion {
            name: "Bool",
            parameters: &[],
            ctors: bump.alloc_slice_fill_iter([ctor]),
            alternatives: 2,
            options: CtorOpts::Enum,
            visibility: UnionVisibility::Closed,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(union.to_public());
        });
    }

    #[test]
    fn to_public_union_private_returns_none() {
        let union = InterfaceUnion {
            name: "Internal",
            parameters: &[],
            ctors: &[],
            alternatives: 0,
            options: CtorOpts::Normal,
            visibility: UnionVisibility::Private,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(union.to_public());
        });
    }

    #[test]
    fn to_public_alias_public_passes_through() {
        let bump = Bump::new();
        let typ = bump.alloc(Located::at(Region::zero(), CanType::Unit));
        let alias = InterfaceAlias {
            name: "Pair",
            parameters: &["a", "b"],
            typ,
            visibility: AliasVisibility::Public,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(alias.to_public());
        });
    }

    #[test]
    fn to_public_alias_private_returns_none() {
        let bump = Bump::new();
        let typ = bump.alloc(Located::at(Region::zero(), CanType::Unit));
        let alias = InterfaceAlias {
            name: "Internal",
            parameters: &[],
            typ,
            visibility: AliasVisibility::Private,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(alias.to_public());
        });
    }
}
