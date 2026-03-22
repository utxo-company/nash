use std::collections::BTreeSet;

use bumpalo::Bump;
use nash_ast::{
    AliasArgument as CanAliasArgument, AliasType as CanAliasType, Annotation,
    FieldType as CanFieldType, FreeVars, QualifiedName, Type as CanType,
};
use nash_region::{Located, Region};
use nash_source::Type as SourceType;

use crate::Error;
use crate::accumulate;
use crate::environment::{self, Env, Info, dups};
use crate::error::BadArityContext;

/// Canonicalize a source type and wrap it in an Annotation with free type variables.
/// Mirrors Elm's `Type.toAnnotation`.
pub fn to_annotation<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_type: &'a Located<SourceType<'a>>,
) -> Result<&'a Annotation<'a>, Vec<Error<'a>>> {
    let typ = canonicalize_type(bump, env, source_type)?;
    let mut free_var_set: BTreeSet<&'a str> = BTreeSet::new();
    collect_free_vars(&typ.value, &mut free_var_set);
    let free_vars: FreeVars<'a> = bump.alloc_slice_fill_iter(free_var_set);
    Ok(bump.alloc(Annotation { free_vars, typ }))
}

/// Canonicalize a source type using the environment.
/// Mirrors Elm's `Type.canonicalize`.
pub fn canonicalize_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    typ: &'a Located<SourceType<'a>>,
) -> Result<&'a Located<CanType<'a>>, Vec<Error<'a>>> {
    Ok(bump.alloc(Located::at(
        typ.region,
        canonicalize_type_value(bump, env, typ.region, &typ.value)?,
    )))
}

/// Canonicalize a slice of type arguments.
pub fn canonicalize_type_arguments<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [&'a Located<CanType<'a>>], Vec<Error<'a>>> {
    accumulate::try_all_alloc_ref(
        bump,
        args.iter()
            .copied()
            .map(|arg| canonicalize_type(bump, env, arg)),
    )
}

fn canonicalize_type_value<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    typ: &SourceType<'a>,
) -> Result<CanType<'a>, Vec<Error<'a>>> {
    Ok(match typ {
        SourceType::Lambda { from, to } => {
            let (from, to) = accumulate::accumulate2(
                canonicalize_type(bump, env, from),
                canonicalize_type(bump, env, to),
            )?;
            CanType::Lambda { from, to }
        }
        SourceType::Var(name) => CanType::Var(name),
        SourceType::Type { name, args, .. } => {
            canonicalize_named_type(bump, env, region, name, args)?
        }
        SourceType::TypeQual {
            module: type_module,
            name,
            args,
            ..
        } => {
            if *type_module == env.home.name {
                canonicalize_named_type(bump, env, region, name, args)?
            } else {
                canonicalize_qualified_named_type(bump, env, region, type_module, name, args)?
            }
        }
        SourceType::Record { fields, ext } => {
            dups::detect(
                fields.iter().map(|f| (f.field.value, f.field.region)),
                |name, first, second| Error::DuplicateField {
                    name,
                    first,
                    second,
                },
            )?;
            let can_fields = accumulate::try_all_alloc(
                bump,
                fields.iter().copied().enumerate().map(|(index, field)| {
                    canonicalize_field_type(
                        bump,
                        env,
                        index.try_into().expect("record field index exceeds u16"),
                        field,
                    )
                }),
            )?;
            CanType::Record {
                fields: can_fields,
                ext: ext.map(|name| name.value),
            }
        }
        SourceType::Unit => CanType::Unit,
        SourceType::Tuple {
            first,
            second,
            rest,
        } => {
            if rest.len() > 1 {
                return Err(vec![Error::TupleLargerThanThree { region }]);
            }
            let (first, second, rest) = accumulate::accumulate3(
                canonicalize_type(bump, env, first),
                canonicalize_type(bump, env, second),
                canonicalize_type_arguments(bump, env, rest),
            )?;
            CanType::Tuple {
                first,
                second,
                rest,
            }
        }
    })
}

fn canonicalize_named_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<CanType<'a>, Vec<Error<'a>>> {
    if let Some(info) = env.types.get(name) {
        return match info {
            Info::Specific(_, typ) => canonicalize_env_type(bump, env, region, name, args, *typ),
            Info::Ambiguous(first, others) => Err(vec![Error::AmbiguousType {
                region,
                prefix: None,
                name,
                first_module: *first,
                other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
            }]),
        };
    }

    Err(vec![Error::NotFoundType {
        region,
        prefix: None,
        name,
        suggestions: env.possible_type_names(bump),
    }])
}

fn canonicalize_qualified_named_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    prefix: &'a str,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<CanType<'a>, Vec<Error<'a>>> {
    let info = env
        .q_types
        .get(prefix)
        .and_then(|m| m.get(name))
        .ok_or_else(|| {
            vec![Error::NotFoundType {
                region,
                prefix: Some(prefix),
                name,
                suggestions: env.possible_type_names(bump),
            }]
        })?;

    match info {
        Info::Specific(_, typ) => canonicalize_env_type(bump, env, region, name, args, *typ),
        Info::Ambiguous(first, others) => Err(vec![Error::AmbiguousType {
            region,
            prefix: Some(prefix),
            name,
            first_module: *first,
            other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
        }]),
    }
}

fn canonicalize_env_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
    typ: environment::Type<'a>,
) -> Result<CanType<'a>, Vec<Error<'a>>> {
    match typ {
        environment::Type::Alias {
            arity,
            home,
            parameters,
            typ: alias_typ,
        } => {
            check_arity(region, name, arity, args.len())?;
            let arguments = canonicalize_env_alias_arguments(bump, env, parameters, args)?;
            Ok(CanType::Alias {
                reference: QualifiedName { home, name },
                arguments,
                target: CanAliasType::Open(alias_typ),
            })
        }
        environment::Type::Union { arity, home } => {
            check_arity(region, name, arity, args.len())?;
            let args = canonicalize_type_arguments(bump, env, args)?;
            Ok(CanType::Named {
                reference: QualifiedName { home, name },
                args,
            })
        }
    }
}

fn canonicalize_env_alias_arguments<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    parameters: &'a [&'a str],
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [CanAliasArgument<'a>], Vec<Error<'a>>> {
    accumulate::try_all_alloc(
        bump,
        parameters
            .iter()
            .copied()
            .zip(args.iter().copied())
            .map(|(parameter, arg)| {
                Ok(CanAliasArgument {
                    name: parameter,
                    typ: canonicalize_type(bump, env, arg)?,
                })
            }),
    )
}

fn check_arity<'a>(
    region: Region,
    name: &'a str,
    expected: usize,
    actual: usize,
) -> Result<(), Vec<Error<'a>>> {
    if expected == actual {
        Ok(())
    } else {
        Err(vec![Error::BadArity {
            region,
            context: BadArityContext::TypeArity,
            name,
            expected,
            actual,
        }])
    }
}

fn canonicalize_field_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    index: u16,
    field: &nash_source::FieldType<'a>,
) -> Result<CanFieldType<'a>, Vec<Error<'a>>> {
    Ok(CanFieldType {
        index,
        field: field.field.value,
        typ: canonicalize_type(bump, env, field.typ)?,
    })
}

pub fn collect_free_vars<'a>(typ: &CanType<'a>, vars: &mut BTreeSet<&'a str>) {
    match typ {
        CanType::Var(name) => {
            vars.insert(name);
        }
        CanType::Lambda { from, to } => {
            collect_free_vars(&from.value, vars);
            collect_free_vars(&to.value, vars);
        }
        CanType::Named { args, .. } => {
            for arg in *args {
                collect_free_vars(&arg.value, vars);
            }
        }
        CanType::Record { fields, ext } => {
            if let Some(name) = ext {
                vars.insert(name);
            }
            for f in *fields {
                collect_free_vars(&f.typ.value, vars);
            }
        }
        CanType::Alias { arguments, .. } => {
            for arg in *arguments {
                collect_free_vars(&arg.typ.value, vars);
            }
        }
        CanType::Unit => {}
        CanType::Tuple {
            first,
            second,
            rest,
        } => {
            collect_free_vars(&first.value, vars);
            collect_free_vars(&second.value, vars);
            for r in *rest {
                collect_free_vars(&r.value, vars);
            }
        }
    }
}

/// Mirrors Elm's `Type.iteratedDealias`.
pub fn iterated_dealias<'a>(typ: &'a Located<CanType<'a>>) -> &'a Located<CanType<'a>> {
    match &typ.value {
        CanType::Alias {
            target: CanAliasType::Open(inner),
            ..
        }
        | CanType::Alias {
            target: CanAliasType::Filled(inner),
            ..
        } => iterated_dealias(inner),
        _ => typ,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use nash_ast::ModuleName;

    use crate::environment::{Env, Info, Type as EnvType};

    fn empty_env<'a>(bump: &'a Bump) -> Env<'a> {
        let _ = bump;
        Env {
            home: ModuleName {
                package: None,
                name: "Main",
            },
            vars: Default::default(),
            types: Default::default(),
            ctors: Default::default(),
            binops: Default::default(),
            q_vars: Default::default(),
            q_types: Default::default(),
            q_ctors: Default::default(),
        }
    }

    fn env_with_int<'a>(bump: &'a Bump) -> Env<'a> {
        let home = ModuleName {
            package: None,
            name: "Basics",
        };
        let mut env = empty_env(bump);
        env.types.insert(
            "Int",
            Info::Specific(home, EnvType::Union { arity: 0, home }),
        );
        env
    }

    fn env_with_list_and_int<'a>(bump: &'a Bump) -> Env<'a> {
        let basics = ModuleName {
            package: None,
            name: "Basics",
        };
        let list_mod = ModuleName {
            package: None,
            name: "List",
        };
        let mut env = empty_env(bump);
        env.types.insert(
            "Int",
            Info::Specific(
                basics,
                EnvType::Union {
                    arity: 0,
                    home: basics,
                },
            ),
        );
        env.types.insert(
            "List",
            Info::Specific(
                list_mod,
                EnvType::Union {
                    arity: 1,
                    home: list_mod,
                },
            ),
        );
        env
    }

    fn env_with_maybe_alias<'a>(bump: &'a Bump) -> Env<'a> {
        let home = ModuleName {
            package: None,
            name: "Maybe",
        };
        let mut env = empty_env(bump);
        let alias_type = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        env.types.insert(
            "Maybe",
            Info::Specific(
                home,
                EnvType::Alias {
                    arity: 1,
                    home,
                    parameters: bump.alloc_slice_fill_iter(["a"]),
                    typ: alias_type,
                },
            ),
        );
        env
    }

    fn parse_type<'a>(bump: &'a Bump, input: &str) -> &'a Located<SourceType<'a>> {
        let src = bump.alloc_str(input);
        let mut parser = nash_parse::Parser::new(bump, src.as_bytes());
        let (typ, _end) = parser.type_expr().expect("expected successful parse");
        typ
    }

    macro_rules! assert_type_snapshot {
        ($input:expr, $env_fn:ident) => {{
            let bump = Bump::new();
            let env = $env_fn(&bump);
            let typ = parse_type(&bump, $input);
            let result = canonicalize_type(&bump, &env, typ);
            insta::with_settings!({
                description => $input,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result.unwrap());
            });
        }};
    }

    macro_rules! assert_type_error_snapshot {
        ($input:expr, $env_fn:ident) => {{
            let bump = Bump::new();
            let env = $env_fn(&bump);
            let typ = parse_type(&bump, $input);
            let result = canonicalize_type(&bump, &env, typ);
            insta::with_settings!({
                description => $input,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result.unwrap_err());
            });
        }};
    }

    macro_rules! assert_annotation_snapshot {
        ($input:expr, $env_fn:ident) => {{
            let bump = Bump::new();
            let env = $env_fn(&bump);
            let typ = parse_type(&bump, $input);
            let result = to_annotation(&bump, &env, typ);
            insta::with_settings!({
                description => $input,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result.unwrap());
            });
        }};
    }

    #[test]
    fn annotation_simple_var() {
        assert_annotation_snapshot!("a", empty_env);
    }

    #[test]
    fn annotation_function() {
        assert_annotation_snapshot!("a -> b -> a", empty_env);
    }

    #[test]
    fn annotation_no_free_vars() {
        assert_annotation_snapshot!("Int", env_with_int);
    }

    #[test]
    fn annotation_mixed() {
        assert_annotation_snapshot!("a -> List a", env_with_list_and_int);
    }

    #[test]
    fn annotation_record_ext() {
        assert_annotation_snapshot!("{ a | x : Int }", env_with_int);
    }

    #[test]
    fn type_tuple_three() {
        assert_type_snapshot!("( a, b, c )", empty_env);
    }

    #[test]
    fn type_tuple_four_errors() {
        assert_type_error_snapshot!("( a, b, c, d )", empty_env);
    }

    #[test]
    fn type_alias_expansion() {
        assert_type_snapshot!("Maybe a", env_with_maybe_alias);
    }

    #[test]
    fn type_union_reference() {
        assert_type_snapshot!("List a", env_with_list_and_int);
    }
}
