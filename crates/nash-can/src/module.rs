use std::collections::{BTreeMap, BTreeSet};

use bumpalo::Bump;
use nash_ast::{
    Alias as CanAlias, Associativity as CanAssociativity, Binop as CanBinop, Ctor as CanCtor,
    CtorOpts, Decls, Export, Exports, Module as CanModule, ModuleName, PackageName,
    Precedence as CanPrecedence, Union as CanUnion,
};
use nash_region::{Located, Region};
use nash_source::{
    Alias as SourceAlias, Associativity as SourceAssociativity, Ctor as SourceCtor, Exposed,
    Exposing, Infix, Module as SourceModule, Precedence as SourcePrecedence, Privacy,
    Type as SourceType, Union as SourceUnion, Value as SourceValue,
};

use crate::accumulate;
use crate::environment::{self, Env, dups};
use crate::error::VarKind;
use crate::scc;
use crate::types;
use crate::{Error, Interface};

#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    pub package: Option<PackageName<'a>>,
    pub interfaces: Option<&'a BTreeMap<&'a str, Interface<'a>>>,
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
) -> Result<CanModule<'a>, Vec<Error<'a>>> {
    let home = canonicalize_header(context, module).map_err(|e| vec![e])?;

    let mut env =
        environment::foreign::create_initial_env(bump, home, context.interfaces, module.imports)
            .map_err(|e| vec![e])?;

    environment::local::add_union_types(&mut env, module.unions, module.aliases)?;
    let aliases = canonicalize_aliases(bump, &mut env, module.aliases)?;
    let unions = canonicalize_unions(bump, &env, module.unions)?;

    environment::local::add_ctors(&mut env, unions, aliases)?;
    environment::local::add_vars(&mut env, module.values)?;
    environment::local::add_binops(&mut env, module.binops)?;

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
    unions: &'a [&'a Located<SourceUnion<'a>>],
) -> Result<&'a [&'a Located<CanUnion<'a>>], Vec<Error<'a>>> {
    accumulate::try_all_alloc_ref(
        bump,
        unions.iter().copied().map(|union| {
            let can = canonicalize_union(bump, env, union)?;
            Ok(&*bump.alloc(Located::at(union.region, can)))
        }),
    )
}

fn canonicalize_union<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_union: &'a Located<SourceUnion<'a>>,
) -> Result<CanUnion<'a>, Vec<Error<'a>>> {
    check_union_free_vars(bump, source_union)?;

    let union = &source_union.value;
    let parameters =
        bump.alloc_slice_fill_iter(union.arguments.iter().copied().map(|arg| arg.value));
    let ctors = canonicalize_ctors(bump, env, union.ctors)?;
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
    ctors: &'a [&'a SourceCtor<'a>],
) -> Result<&'a [&'a CanCtor<'a>], Vec<Error<'a>>> {
    accumulate::try_all_alloc_ref(
        bump,
        ctors.iter().copied().enumerate().map(|(index, ctor)| {
            let arguments = types::canonicalize_type_arguments(bump, env, ctor.arguments)?;
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
        }),
    )
}

fn canonicalize_aliases<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
) -> Result<&'a [&'a Located<CanAlias<'a>>], Vec<Error<'a>>> {
    let alias_names: BTreeSet<&str> = source_aliases.iter().map(|a| a.value.name.value).collect();

    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for alias in source_aliases {
        let mut deps = Vec::new();
        collect_type_edges(&alias.value.typ.value, &alias_names, &mut deps);
        edges.insert(alias.value.name.value, deps);
    }

    let names: Vec<&str> = source_aliases.iter().map(|a| a.value.name.value).collect();
    let sccs = scc::strongly_connected_components(&names, &edges);

    let mut results: BTreeMap<&str, &Located<CanAlias>> = BTreeMap::new();
    for component in &sccs {
        match component {
            scc::Scc::Acyclic(name) => {
                let source = source_aliases
                    .iter()
                    .find(|a| a.value.name.value == *name)
                    .unwrap();
                check_alias_free_vars(bump, source)?;
                let alias = canonicalize_single_alias(bump, env, source)?;
                environment::local::add_alias_type(bump, env, &alias.value);
                results.insert(name, alias);
            }
            scc::Scc::Cyclic(cycle_names) => {
                let first = source_aliases
                    .iter()
                    .find(|a| a.value.name.value == cycle_names[0])
                    .unwrap();
                return Err(vec![Error::RecursiveAlias {
                    region: first.value.name.region,
                    name: cycle_names[0],
                    args: bump.alloc_slice_fill_iter(first.value.arguments.iter().map(|a| a.value)),
                    others: bump.alloc_slice_fill_iter(cycle_names[1..].iter().copied()),
                }]);
            }
        }
    }

    Ok(bump.alloc_slice_fill_iter(
        source_aliases
            .iter()
            .map(|a| *results.get(a.value.name.value).unwrap()),
    ))
}

fn canonicalize_single_alias<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_alias: &'a Located<SourceAlias<'a>>,
) -> Result<&'a Located<CanAlias<'a>>, Vec<Error<'a>>> {
    let alias = &source_alias.value;
    let parameters =
        bump.alloc_slice_fill_iter(alias.arguments.iter().copied().map(|arg| arg.value));
    let typ = types::canonicalize_type(bump, env, alias.typ)?;

    let can_alias = CanAlias {
        name: alias.name,
        parameters,
        typ,
    };

    Ok(&*bump.alloc(Located::at(source_alias.region, can_alias)))
}

fn check_union_free_vars<'a>(
    bump: &'a Bump,
    union: &'a Located<SourceUnion<'a>>,
) -> Result<(), Vec<Error<'a>>> {
    let u = &union.value;

    dups::detect(
        u.arguments.iter().map(|a| (a.value, a.region)),
        |arg_name, first, second| Error::DuplicateUnionArg {
            type_name: u.name.value,
            arg_name,
            first,
            second,
        },
    )?;

    let bound: BTreeSet<&str> = u.arguments.iter().map(|a| a.value).collect();

    let mut free_vars: BTreeMap<&str, Region> = BTreeMap::new();
    for ctor in u.ctors {
        for arg in ctor.arguments {
            collect_free_type_vars(arg, &mut free_vars);
        }
    }

    let unbound: Vec<(&str, Region)> = free_vars
        .into_iter()
        .filter(|(name, _)| !bound.contains(name))
        .collect();

    if unbound.is_empty() {
        Ok(())
    } else {
        let args = bump.alloc_slice_fill_iter(u.arguments.iter().map(|a| a.value));
        let (first_unbound, rest_unbound) = unbound.split_first().unwrap();
        Err(vec![Error::TypeVarsUnboundInUnion {
            region: union.region,
            name: u.name.value,
            args,
            unbound: *first_unbound,
            more_unbound: bump.alloc_slice_fill_iter(rest_unbound.iter().copied()),
        }])
    }
}

fn check_alias_free_vars<'a>(
    bump: &'a Bump,
    alias: &'a Located<SourceAlias<'a>>,
) -> Result<(), Vec<Error<'a>>> {
    let a = &alias.value;

    dups::detect(
        a.arguments.iter().map(|arg| (arg.value, arg.region)),
        |arg_name, first, second| Error::DuplicateAliasArg {
            type_name: a.name.value,
            arg_name,
            first,
            second,
        },
    )?;

    let bound: BTreeSet<&str> = a.arguments.iter().map(|arg| arg.value).collect();

    let mut free_vars: BTreeMap<&str, Region> = BTreeMap::new();
    collect_free_type_vars(a.typ, &mut free_vars);

    let unused: Vec<(&str, Region)> = a
        .arguments
        .iter()
        .filter(|arg| !free_vars.contains_key(arg.value))
        .map(|arg| (arg.value, arg.region))
        .collect();

    let unbound: Vec<(&str, Region)> = free_vars
        .into_iter()
        .filter(|(name, _)| !bound.contains(name))
        .collect();

    if unused.is_empty() && unbound.is_empty() {
        Ok(())
    } else {
        let args = bump.alloc_slice_fill_iter(a.arguments.iter().map(|arg| arg.value));
        Err(vec![Error::TypeVarsMessedUpInAlias {
            region: alias.region,
            name: a.name.value,
            args,
            unused: bump.alloc_slice_fill_iter(unused),
            unbound: bump.alloc_slice_fill_iter(unbound),
        }])
    }
}

fn collect_type_edges<'a>(
    typ: &SourceType<'a>,
    alias_names: &BTreeSet<&'a str>,
    edges: &mut Vec<&'a str>,
) {
    match typ {
        SourceType::Lambda { from, to } => {
            collect_type_edges(&from.value, alias_names, edges);
            collect_type_edges(&to.value, alias_names, edges);
        }
        SourceType::Var(_) => {}
        SourceType::Type { name, args, .. } => {
            if alias_names.contains(name) && !edges.contains(name) {
                edges.push(name);
            }
            for arg in *args {
                collect_type_edges(&arg.value, alias_names, edges);
            }
        }
        SourceType::TypeQual { args, .. } => {
            // Qualified refs are external, not local alias deps
            for arg in *args {
                collect_type_edges(&arg.value, alias_names, edges);
            }
        }
        SourceType::Record { fields, ext: _ } => {
            for field in *fields {
                collect_type_edges(&field.typ.value, alias_names, edges);
            }
        }
        SourceType::Unit => {}
        SourceType::Tuple {
            first,
            second,
            rest,
        } => {
            collect_type_edges(&first.value, alias_names, edges);
            collect_type_edges(&second.value, alias_names, edges);
            for r in *rest {
                collect_type_edges(&r.value, alias_names, edges);
            }
        }
    }
}

fn collect_free_type_vars<'a>(typ: &Located<SourceType<'a>>, vars: &mut BTreeMap<&'a str, Region>) {
    match &typ.value {
        SourceType::Var(name) => {
            vars.entry(name).or_insert(typ.region);
        }
        SourceType::Lambda { from, to } => {
            collect_free_type_vars(from, vars);
            collect_free_type_vars(to, vars);
        }
        SourceType::Type { args, .. } | SourceType::TypeQual { args, .. } => {
            for arg in *args {
                collect_free_type_vars(arg, vars);
            }
        }
        SourceType::Record { fields, .. } => {
            for field in *fields {
                collect_free_type_vars(field.typ, vars);
            }
        }
        SourceType::Unit => {}
        SourceType::Tuple {
            first,
            second,
            rest,
        } => {
            collect_free_type_vars(first, vars);
            collect_free_type_vars(second, vars);
            for r in *rest {
                collect_free_type_vars(r, vars);
            }
        }
    }
}

fn canonicalize_exports<'a>(
    bump: &'a Bump,
    module: &SourceModule<'a>,
) -> Result<Exports<'a>, Vec<Error<'a>>> {
    match module.exports.value {
        Exposing::Open => Ok(Exports::Everything(module.exports.region)),
        Exposing::Explicit(exposed) => {
            dups::detect(
                exposed.iter().map(|e| exposed_name_and_region(e)),
                |name, first, second| Error::ExportDuplicate {
                    name,
                    first,
                    second,
                },
            )?;
            let exports = accumulate::try_all_alloc_ref(
                bump,
                exposed
                    .iter()
                    .copied()
                    .map(|e| canonicalize_exposed(bump, module, e)),
            )?;
            Ok(Exports::Explicit(exports))
        }
    }
}

fn exposed_name_and_region<'a>(exposed: &Exposed<'a>) -> (&'a str, Region) {
    match exposed {
        Exposed::Lower(name) => (name.value, name.region),
        Exposed::Upper { name, .. } => (name.value, name.region),
        Exposed::Operator { region, op } => (op, *region),
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
) -> Result<&'a Located<Export<'a>>, Vec<Error<'a>>> {
    Ok(bump.alloc(Located::at(
        exposed_region(exposed),
        canonicalize_export(module, exposed)?,
    )))
}

fn canonicalize_export<'a>(
    module: &SourceModule<'a>,
    exposed: &Exposed<'a>,
) -> Result<Export<'a>, Vec<Error<'a>>> {
    Ok(match exposed {
        Exposed::Lower(name) => {
            if module
                .values
                .iter()
                .any(|value| value.value.name.value == name.value)
            {
                Export::Value(name.value)
            } else {
                return Err(vec![Error::ExportNotFound {
                    region: name.region,
                    kind: VarKind::BadVar,
                    name: name.value,
                }]);
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
                        return Err(vec![Error::ExportOpenAlias {
                            region: *region,
                            name: name.value,
                        }]);
                    }
                    Privacy::Private => Export::Alias(name.value),
                }
            } else {
                return Err(vec![Error::ExportNotFound {
                    region: name.region,
                    kind: VarKind::BadType,
                    name: name.value,
                }]);
            }
        }
        Exposed::Operator { region, op } => {
            if module.binops.iter().any(|binop| binop.value.op == *op) {
                Export::Binop(op)
            } else {
                return Err(vec![Error::ExportNotFound {
                    region: *region,
                    kind: VarKind::BadOp,
                    name: op,
                }]);
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
    ) -> Result<CanModule<'a>, Vec<Error<'a>>> {
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

    // === Validation tests ===

    #[test]
    fn duplicate_type_alias_and_union() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Foo = Int

            type Foo
                = Bar
        "#
        );
    }

    #[test]
    fn duplicate_type_two_aliases() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Foo = Int

            type alias Foo = String
        "#
        );
    }

    #[test]
    fn duplicate_value_decl() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            foo = 1

            foo = 2
        "#
        );
    }

    #[test]
    fn duplicate_binop() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            infix left 6 (|>) = apR

            infix left 6 (|>) = apR2
        "#
        );
    }

    #[test]
    fn record_type_duplicate_field() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias R = { x : Int, x : String }
        "#
        );
    }

    #[test]
    fn recursive_alias_self() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Loop = Loop
        "#
        );
    }

    #[test]
    fn recursive_alias_cycle() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias A = B

            type alias B = A
        "#
        );
    }

    #[test]
    fn union_unbound_type_var() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type MyType a
                = MyTag b
        "#
        );
    }

    #[test]
    fn alias_unused_type_param() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Phantom a = Int
        "#
        );
    }

    #[test]
    fn alias_unbound_type_var() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Bad = List a
        "#
        );
    }

    #[test]
    fn union_duplicate_type_param() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type Bad a a
                = Foo
        "#
        );
    }

    #[test]
    fn alias_duplicate_type_param() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Bad a a = a
        "#
        );
    }

    #[test]
    fn multiple_duplicate_types_all_reported() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Foo = Int

            type alias Foo = String

            type alias Bar = Int

            type alias Bar = String
        "#
        );
    }

    #[test]
    fn record_multiple_duplicate_fields_all_reported() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias R = { x : Int, x : String, y : Int, y : String }
        "#
        );
    }

    #[test]
    fn duplicate_export() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Foo, Foo)

            type alias Foo = Int
        "#
        );
    }
}
