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
use crate::error::{DuplicatePatternContext, VarKind};
use crate::expression;
use crate::pattern;
use crate::scc;
use crate::types;
use crate::warning::{Warning, WarningContext};
use crate::{Error, Interface};

#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    pub package: Option<PackageName<'a>>,
    pub interfaces: Option<&'a BTreeMap<&'a str, Interface<'a>>>,
}

#[derive(Debug)]
pub struct CanResult<'a> {
    pub module: CanModule<'a>,
    pub warnings: Vec<Warning<'a>>,
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
) -> Result<CanResult<'a>, Vec<Error<'a>>> {
    let home = canonicalize_header(context, module).map_err(|e| vec![e])?;

    let mut env =
        environment::foreign::create_initial_env(bump, home, context.interfaces, module.imports)?;

    environment::local::add_union_types(&mut env, module.unions, module.aliases)?;
    let aliases = canonicalize_aliases(bump, &mut env, module.aliases)?;
    let unions = canonicalize_unions(bump, &env, module.unions)?;

    environment::local::add_ctors(&mut env, unions, aliases)?;
    environment::local::add_vars(&mut env, module.values)?;
    environment::local::add_binops(&mut env, module.binops)?;

    let mut warnings = Vec::new();
    let decls = canonicalize_decls(bump, &env, module.values, &mut warnings)?;
    let binops = canonicalize_binops(bump, module.binops);
    let exports = canonicalize_exports(bump, &env, module)?;

    let can_module = CanModule {
        name: env.home,
        exports,
        docs: module.docs,
        decls,
        unions,
        aliases,
        binops,
    };

    // Check for unused imports
    let used_modules = collect_used_modules(&can_module);
    for import in module.imports {
        let module_name = import.import.value;
        if !used_modules.contains(module_name) {
            warnings.push(Warning::UnusedImport {
                region: import.import.region,
                module_name,
            });
        }
    }

    Ok(CanResult {
        module: can_module,
        warnings,
    })
}

fn canonicalize_decls<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    values: &'a [&'a Located<SourceValue<'a>>],
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Decls<'a>, Vec<Error<'a>>> {
    if values.is_empty() {
        return Ok(bump.alloc(Decls::Empty));
    }

    let mut errors = Vec::new();
    let mut nodes: Vec<NodeOne<'a>> = Vec::with_capacity(values.len());
    for value in values {
        match to_node_one(bump, env, value, warnings) {
            Ok(node) => nodes.push(node),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let top_level_names: BTreeSet<&str> = nodes.iter().map(|n| n.name).collect();

    // Phase 1: SCC on ALL dependencies
    let names: Vec<&str> = nodes.iter().map(|n| n.name).collect();
    let all_edges: BTreeMap<&str, Vec<&str>> = nodes
        .iter()
        .map(|node| {
            let deps: Vec<&str> = node
                .free_locals
                .keys()
                .filter(|k| top_level_names.contains(*k))
                .copied()
                .collect();
            (node.name, deps)
        })
        .collect();
    let phase1_sccs = scc::strongly_connected_components(&names, &all_edges);

    let node_map: BTreeMap<&str, &NodeOne<'a>> = nodes.iter().map(|n| (n.name, n)).collect();

    let mut decls: &'a Decls<'a> = bump.alloc(Decls::Empty);
    for scc_group in phase1_sccs.into_iter().rev() {
        match scc_group {
            scc::Scc::Acyclic(name) => {
                decls = bump.alloc(Decls::Declare {
                    definition: node_map[name].def,
                    next: decls,
                });
            }
            scc::Scc::Cyclic(group_names) => {
                // Phase 2: SCC on DIRECT deps within cyclic group
                let group_set: BTreeSet<&str> = group_names.iter().copied().collect();
                let direct_edges: BTreeMap<&str, Vec<&str>> = group_names
                    .iter()
                    .map(|&name| {
                        let node = node_map[name];
                        let deps = if node.has_args {
                            vec![] // functions: body is delayed
                        } else {
                            node.free_locals
                                .iter()
                                .filter(|(k, uses)| group_set.contains(*k) && uses.direct > 0)
                                .map(|(k, _)| *k)
                                .collect()
                        };
                        (name, deps)
                    })
                    .collect();

                let phase2_sccs = scc::strongly_connected_components(&group_names, &direct_edges);

                let mut rec_defs: Vec<&'a nash_ast::Def<'a>> = Vec::new();
                for sub_scc in &phase2_sccs {
                    match sub_scc {
                        scc::Scc::Acyclic(name) => {
                            rec_defs.push(node_map[name].def);
                        }
                        scc::Scc::Cyclic(bad_names) => {
                            let first = bad_names[0];
                            let def_name = def_located_name(node_map[first].def);
                            let others: Vec<&str> = bad_names[1..].to_vec();
                            return Err(vec![Error::RecursiveDecl {
                                name: def_name,
                                others: bump.alloc_slice_fill_iter(others),
                            }]);
                        }
                    }
                }

                if let Some((first, rest)) = rec_defs.split_first() {
                    decls = bump.alloc(Decls::DeclareRec {
                        definition: first,
                        following: bump.alloc_slice_fill_iter(rest.iter().copied()),
                        next: decls,
                    });
                }
            }
        }
    }
    Ok(decls)
}

fn def_located_name<'a>(def: &'a nash_ast::Def<'a>) -> &'a Located<&'a str> {
    match def {
        nash_ast::Def::Def { name, .. } | nash_ast::Def::TypedDef { name, .. } => name,
    }
}

struct NodeOne<'a> {
    def: &'a nash_ast::Def<'a>,
    name: &'a str,
    has_args: bool,
    free_locals: expression::FreeLocals<'a>,
}

fn to_node_one<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    value: &'a Located<SourceValue<'a>>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<NodeOne<'a>, Vec<Error<'a>>> {
    let src = &value.value;
    let mut arg_bindings = pattern::Bindings::new();
    let mut can_args = Vec::with_capacity(src.arguments.len());
    for arg in src.arguments {
        let (can_pat, bindings) = pattern::verify(
            bump,
            env,
            DuplicatePatternContext::FuncArgs(src.name.value),
            arg,
        )?;
        can_args.push(can_pat);
        arg_bindings.extend(bindings);
    }
    let body_env = env.add_locals(&arg_bindings)?;
    let mut free_locals = expression::FreeLocals::new();
    let can_body =
        expression::canonicalize_expr(bump, &body_env, src.body, &mut free_locals, warnings)?;

    let outer_free = expression::verify_bindings(
        WarningContext::Pattern,
        &arg_bindings,
        free_locals,
        warnings,
    );

    let def = if let Some(ann) = src.annotation {
        let annotation = types::to_annotation(bump, env, ann)?;
        let typed_args =
            expression::gather_typed_args(bump, src.name.value, &can_args, annotation)?;
        let result_type = expression::peel_result_type(annotation, can_args.len());
        bump.alloc(nash_ast::Def::TypedDef {
            name: src.name,
            free_vars: annotation.free_vars,
            args: typed_args,
            body: can_body,
            typ: result_type,
        })
    } else {
        bump.alloc(nash_ast::Def::Def {
            name: src.name,
            args: bump.alloc_slice_fill_iter(can_args),
            body: can_body,
        })
    };

    Ok(NodeOne {
        def,
        name: src.name.value,
        has_args: !src.arguments.is_empty(),
        free_locals: outer_free,
    })
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
    env: &Env<'a>,
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
                    .map(|e| canonicalize_exposed(bump, env, e)),
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
    env: &Env<'a>,
    exposed: &Exposed<'a>,
) -> Result<&'a Located<Export<'a>>, Vec<Error<'a>>> {
    Ok(bump.alloc(Located::at(
        exposed_region(exposed),
        canonicalize_export(env, exposed)?,
    )))
}

fn canonicalize_export<'a>(
    env: &Env<'a>,
    exposed: &Exposed<'a>,
) -> Result<Export<'a>, Vec<Error<'a>>> {
    Ok(match exposed {
        Exposed::Lower(name) => {
            if matches!(
                env.vars.get(name.value),
                Some(environment::Var::TopLevel(_))
            ) {
                Export::Value(name.value)
            } else {
                return Err(vec![Error::ExportNotFound {
                    region: name.region,
                    kind: VarKind::BadVar,
                    name: name.value,
                }]);
            }
        }
        Exposed::Upper { name, privacy } => match env.types.get(name.value) {
            Some(environment::Info::Specific(home, environment::Type::Union { .. }))
                if *home == env.home =>
            {
                match privacy {
                    Privacy::Public(_) => Export::UnionOpen(name.value),
                    Privacy::Private => Export::UnionClosed(name.value),
                }
            }
            Some(environment::Info::Specific(home, environment::Type::Alias { .. }))
                if *home == env.home =>
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
            }
            _ => {
                return Err(vec![Error::ExportNotFound {
                    region: name.region,
                    kind: VarKind::BadType,
                    name: name.value,
                }]);
            }
        },
        Exposed::Operator { region, op } => match env.binops.get(*op) {
            Some(environment::Info::Specific(home, _)) if *home == env.home => Export::Binop(op),
            _ => {
                return Err(vec![Error::ExportNotFound {
                    region: *region,
                    kind: VarKind::BadOp,
                    name: op,
                }]);
            }
        },
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

// --- Unused import detection ---

fn collect_used_modules<'a>(module: &CanModule<'a>) -> BTreeSet<&'a str> {
    let mut used = BTreeSet::new();
    let home = module.name;
    collect_from_decls(module.decls, home, &mut used);
    for union in module.unions {
        for ctor in union.value.ctors {
            for arg in ctor.arguments {
                collect_from_type(&arg.value, home, &mut used);
            }
        }
    }
    for alias in module.aliases {
        collect_from_type(&alias.value.typ.value, home, &mut used);
    }
    used
}

fn add_if_foreign<'a>(
    home: ModuleName<'a>,
    reference_home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    if reference_home != home {
        used.insert(reference_home.name);
    }
}

fn collect_from_decls<'a>(decls: &Decls<'a>, home: ModuleName<'a>, used: &mut BTreeSet<&'a str>) {
    match decls {
        Decls::Declare { definition, next } => {
            collect_from_def(definition, home, used);
            collect_from_decls(next, home, used);
        }
        Decls::DeclareRec {
            definition,
            following,
            next,
        } => {
            collect_from_def(definition, home, used);
            for def in *following {
                collect_from_def(def, home, used);
            }
            collect_from_decls(next, home, used);
        }
        Decls::Empty => {}
    }
}

fn collect_from_def<'a>(
    def: &nash_ast::Def<'a>,
    home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    match def {
        nash_ast::Def::Def { body, args, .. } => {
            for arg in *args {
                collect_from_pattern(&arg.value, home, used);
            }
            collect_from_expr(&body.value, home, used);
        }
        nash_ast::Def::TypedDef {
            args, body, typ, ..
        } => {
            for arg in *args {
                collect_from_pattern(&arg.pattern.value, home, used);
                collect_from_type(&arg.typ.value, home, used);
            }
            collect_from_expr(&body.value, home, used);
            collect_from_type(&typ.value, home, used);
        }
    }
}

fn collect_from_expr<'a>(
    expr: &nash_ast::Expr<'a>,
    home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    use nash_ast::Expr::*;
    match expr {
        VarLocal(_) | Str(_) | Int(_) | Accessor(_) | Unit => {}
        VarTopLevel(q) | VarForeign(q) => add_if_foreign(home, q.home, used),
        VarConstructor {
            reference,
            annotation,
            ..
        } => {
            add_if_foreign(home, reference.home, used);
            collect_from_annotation(annotation, home, used);
        }
        VarOperator {
            reference,
            annotation,
            ..
        } => {
            add_if_foreign(home, reference.home, used);
            if let Some(ann) = annotation {
                collect_from_annotation(ann, home, used);
            }
        }
        Binop {
            reference,
            left,
            right,
            annotation,
            ..
        } => {
            add_if_foreign(home, reference.home, used);
            if let Some(ann) = annotation {
                collect_from_annotation(ann, home, used);
            }
            collect_from_expr(&left.value, home, used);
            collect_from_expr(&right.value, home, used);
        }
        List(items) => {
            for item in *items {
                collect_from_expr(&item.value, home, used);
            }
        }
        Negate(e) => collect_from_expr(&e.value, home, used),
        Lambda { parameters, body } => {
            for p in *parameters {
                collect_from_pattern(&p.value, home, used);
            }
            collect_from_expr(&body.value, home, used);
        }
        Call {
            function,
            arguments,
        } => {
            collect_from_expr(&function.value, home, used);
            for a in *arguments {
                collect_from_expr(&a.value, home, used);
            }
        }
        If {
            branches,
            final_else,
        } => {
            for b in *branches {
                collect_from_expr(&b.condition.value, home, used);
                collect_from_expr(&b.then_branch.value, home, used);
            }
            collect_from_expr(&final_else.value, home, used);
        }
        Let { definition, body } => {
            collect_from_def(definition, home, used);
            collect_from_expr(&body.value, home, used);
        }
        LetRec { definitions, body } => {
            for d in *definitions {
                collect_from_def(d, home, used);
            }
            collect_from_expr(&body.value, home, used);
        }
        LetDestruct {
            pattern,
            value,
            body,
        } => {
            collect_from_pattern(&pattern.value, home, used);
            collect_from_expr(&value.value, home, used);
            collect_from_expr(&body.value, home, used);
        }
        Case {
            scrutinee,
            branches,
        } => {
            collect_from_expr(&scrutinee.value, home, used);
            for b in *branches {
                collect_from_pattern(&b.pattern.value, home, used);
                collect_from_expr(&b.body.value, home, used);
            }
        }
        Access { record, .. } => collect_from_expr(&record.value, home, used),
        Update { base, fields, .. } => {
            collect_from_expr(&base.value, home, used);
            for f in *fields {
                collect_from_expr(&f.value.value, home, used);
            }
        }
        Record(fields) => {
            for f in *fields {
                collect_from_expr(&f.value.value, home, used);
            }
        }
        Tuple {
            first,
            second,
            rest,
        } => {
            collect_from_expr(&first.value, home, used);
            collect_from_expr(&second.value, home, used);
            for r in *rest {
                collect_from_expr(&r.value, home, used);
            }
        }
    }
}

fn collect_from_pattern<'a>(
    pat: &nash_ast::Pattern<'a>,
    home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    use nash_ast::Pattern::*;
    match pat {
        Anything | Var(_) | Str(_) | Int(_) | Unit | Record(_) | Bool { .. } => {}
        Constructor(ctor) => {
            add_if_foreign(home, ctor.reference.home, used);
            for arg in ctor.arguments {
                collect_from_type(&arg.typ.value, home, used);
                collect_from_pattern(&arg.pattern.value, home, used);
            }
        }
        Alias { pattern, .. } => collect_from_pattern(&pattern.value, home, used),
        Tuple {
            first,
            second,
            rest,
        } => {
            collect_from_pattern(&first.value, home, used);
            collect_from_pattern(&second.value, home, used);
            for r in *rest {
                collect_from_pattern(&r.value, home, used);
            }
        }
        List(items) => {
            for item in *items {
                collect_from_pattern(&item.value, home, used);
            }
        }
        Cons { head, tail } => {
            collect_from_pattern(&head.value, home, used);
            collect_from_pattern(&tail.value, home, used);
        }
    }
}

fn collect_from_type<'a>(
    typ: &nash_ast::Type<'a>,
    home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    use nash_ast::Type::*;
    match typ {
        Var(_) | Unit => {}
        Lambda { from, to } => {
            collect_from_type(&from.value, home, used);
            collect_from_type(&to.value, home, used);
        }
        Named { reference, args } => {
            add_if_foreign(home, reference.home, used);
            for a in *args {
                collect_from_type(&a.value, home, used);
            }
        }
        Record { fields, .. } => {
            for f in *fields {
                collect_from_type(&f.typ.value, home, used);
            }
        }
        Tuple {
            first,
            second,
            rest,
        } => {
            collect_from_type(&first.value, home, used);
            collect_from_type(&second.value, home, used);
            for r in *rest {
                collect_from_type(&r.value, home, used);
            }
        }
        Alias {
            reference,
            arguments,
            target,
        } => {
            add_if_foreign(home, reference.home, used);
            for a in *arguments {
                collect_from_type(&a.typ.value, home, used);
            }
            match target {
                nash_ast::AliasType::Open(t) | nash_ast::AliasType::Filled(t) => {
                    collect_from_type(&t.value, home, used);
                }
            }
        }
    }
}

fn collect_from_annotation<'a>(
    ann: &nash_ast::Annotation<'a>,
    home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    collect_from_type(&ann.typ.value, home, used);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bumpalo::Bump;
    use indoc::indoc;
    use nash_ast::{
        Associativity, Ctor as CanCtor, CtorOpts, Module as CanModule, ModuleName, PackageName,
        Precedence, Type as CanType,
    };
    use nash_region::{Located, Region};

    use super::{Context, canonicalize};
    use crate::interface::{
        self, AliasVisibility, InterfaceAlias, InterfaceBinop, InterfaceUnion, InterfaceValue,
        UnionVisibility,
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
        canonicalize(bump, context, &module).map(|r| r.module)
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

    // === Expression canonicalization tests ===

    fn parse_and_canonicalize_with_warnings<'a>(
        bump: &'a Bump,
        input: &str,
        context: Context<'a>,
    ) -> Result<(CanModule<'a>, Vec<crate::Warning<'a>>), Vec<Error<'a>>> {
        let src = bump.alloc_str(input);
        let mut parser = nash_parse::Parser::new(bump, src.as_bytes());
        let module = parser.module().expect("expected successful parse");
        canonicalize(bump, context, &module).map(|r| (r.module, r.warnings))
    }

    macro_rules! assert_module_warning_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let (_, warnings) =
                parse_and_canonicalize_with_warnings(&bump, input, Context::default())
                    .expect("expected successful canonicalization");
            assert!(!warnings.is_empty(), "expected warnings but got none");
            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(warnings);
            });
        }};
    }

    // -- Positive expression tests --

    #[test]
    fn simple_value() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            x = 42
        "#
        );
    }

    #[test]
    fn function_def() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f a b = a
        "#
        );
    }

    #[test]
    fn lambda_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = \x -> x
        "#
        );
    }

    #[test]
    fn let_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = 1
                in
                x
        "#
        );
    }

    #[test]
    fn if_then_else() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = if x then 1 else 2
        "#
        );
    }

    #[test]
    fn record_literal() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = { x = 1, y = 2 }
        "#
        );
    }

    #[test]
    fn record_update() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f r = { r | x = 1 }
        "#
        );
    }

    #[test]
    fn list_literal() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = [ 1, 2, 3 ]
        "#
        );
    }

    #[test]
    fn accessor_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = .name
        "#
        );
    }

    #[test]
    fn field_access() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f r = r.name
        "#
        );
    }

    #[test]
    fn case_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x =
                case x of
                    1 -> "one"
                    _ -> "other"
        "#
        );
    }

    #[test]
    fn string_literal() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = "hello"
        "#
        );
    }

    #[test]
    fn negate_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = -x
        "#
        );
    }

    #[test]
    fn function_call() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = g x

            g y = y
        "#
        );
    }

    #[test]
    fn tuple_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = ( 1, 2, 3 )
        "#
        );
    }

    #[test]
    fn unit_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = ()
        "#
        );
    }

    #[test]
    fn let_recursive_function() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    go x = go x
                in
                go 1
        "#
        );
    }

    #[test]
    fn nested_let() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = 1
                    y = 2
                in
                x
        "#
        );
    }

    // -- Error expression tests --

    #[test]
    fn not_found_var() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f = nonexistent
        "#
        );
    }

    #[test]
    fn recursive_let_value() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = x
                in
                x
        "#
        );
    }

    #[test]
    fn shadowing_local() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f x =
                let
                    x = 1
                in
                x
        "#
        );
    }

    #[test]
    fn duplicate_let_bindings() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = 1
                    x = 2
                in
                x
        "#
        );
    }

    #[test]
    fn tuple_four_in_expr() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f = ( 1, 2, 3, 4 )
        "#
        );
    }

    #[test]
    fn duplicate_record_fields_in_expr() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f = { x = 1, x = 2 }
        "#
        );
    }

    // -- Warning tests --

    #[test]
    fn unused_lambda_arg() {
        assert_module_warning_snapshot!(
            r#"
            module Main exposing (..)

            f = \x -> 1
        "#
        );
    }

    #[test]
    fn unused_let_binding() {
        assert_module_warning_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = 1
                in
                2
        "#
        );
    }

    // === SCC tests ===

    #[test]
    fn mutual_recursion_functions() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = g x

            g x = f x
        "#
        );
    }

    #[test]
    fn self_recursive_function() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = f x
        "#
        );
    }

    #[test]
    fn mixed_recursive_and_non_recursive() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            x = 1

            f a = g a

            g a = f a

            y = 2
        "#
        );
    }

    #[test]
    fn dependency_ordering() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            b = a

            a = 1
        "#
        );
    }

    #[test]
    fn value_and_function_in_cycle() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            x = f 1

            f a = x
        "#
        );
    }

    // -- SCC error tests --

    #[test]
    fn recursive_decl_self_reference() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            x = x
        "#
        );
    }

    #[test]
    fn recursive_decl_mutual_values() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            x = y

            y = x
        "#
        );
    }

    // === Unused import warning tests ===

    fn value_interface<'a>(
        bump: &'a Bump,
        module_name: &'a str,
        val_name: &'a str,
    ) -> Interface<'a> {
        Interface {
            home: ModuleName {
                package: None,
                name: module_name,
            },
            values: bump.alloc_slice_fill_iter([crate::interface::InterfaceValue {
                name: val_name,
                annotation: None,
            }]),
            aliases: &[],
            unions: &[],
            binops: &[],
        }
    }

    #[test]
    fn unused_import_warning() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let (_, warnings) = parse_and_canonicalize_with_warnings(&bump, input, context)
            .expect("expected successful canonicalization");
        assert!(!warnings.is_empty(), "expected warnings but got none");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(warnings);
        });
    }

    #[test]
    fn used_import_no_warning() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo

            x = Foo.bar
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let (_, warnings) = parse_and_canonicalize_with_warnings(&bump, input, context)
            .expect("expected successful canonicalization");
        assert!(
            warnings.is_empty(),
            "expected no warnings but got: {warnings:?}"
        );
    }

    // === Helpers for interface-dependent tests ===

    fn maybe_with_ctors_interface<'a>(bump: &'a Bump) -> Interface<'a> {
        let just_arg = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let just_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "Just",
            index: 0,
            arity: 1,
            arguments: bump.alloc_slice_fill_iter([&*just_arg]),
        });
        let nothing_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "Nothing",
            index: 1,
            arity: 0,
            arguments: &[],
        });
        Interface {
            home: ModuleName {
                package: None,
                name: "Maybe",
            },
            values: &[],
            aliases: &[],
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                name: "Maybe",
                parameters: bump.alloc_slice_fill_iter(["a"]),
                ctors: bump.alloc_slice_fill_iter([just_ctor, nothing_ctor]),
                alternatives: 2,
                options: CtorOpts::Normal,
                visibility: UnionVisibility::Open,
            }]),
            binops: &[],
        }
    }

    fn basics_with_binops_interface<'a>(bump: &'a Bump) -> Interface<'a> {
        Interface {
            home: ModuleName {
                package: None,
                name: "Basics",
            },
            values: bump.alloc_slice_fill_iter([
                InterfaceValue {
                    name: "add",
                    annotation: None,
                },
                InterfaceValue {
                    name: "sub",
                    annotation: None,
                },
                InterfaceValue {
                    name: "mul",
                    annotation: None,
                },
                InterfaceValue {
                    name: "apR",
                    annotation: None,
                },
                InterfaceValue {
                    name: "apL",
                    annotation: None,
                },
            ]),
            aliases: &[],
            unions: &[],
            binops: bump.alloc_slice_fill_iter([
                InterfaceBinop {
                    symbol: "+",
                    associativity: Associativity::Left,
                    precedence: Precedence(6),
                    function: "add",
                },
                InterfaceBinop {
                    symbol: "-",
                    associativity: Associativity::Left,
                    precedence: Precedence(6),
                    function: "sub",
                },
                InterfaceBinop {
                    symbol: "*",
                    associativity: Associativity::Left,
                    precedence: Precedence(7),
                    function: "mul",
                },
                InterfaceBinop {
                    symbol: "|>",
                    associativity: Associativity::Left,
                    precedence: Precedence(0),
                    function: "apR",
                },
                InterfaceBinop {
                    symbol: "<|",
                    associativity: Associativity::Right,
                    precedence: Precedence(0),
                    function: "apL",
                },
            ]),
        }
    }

    // === Constructor / operator expression tests ===

    #[test]
    fn constructor_in_expression() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            x = Just
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
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
    fn record_ctor_in_expression() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type alias Pair a b = { first : a, second : b }

            p = Pair
        "#
        );
    }

    // qualified_ctor_in_expression skipped: parser doesn't yet produce
    // VarQual { kind: CapVar } for `Module.Ctor` syntax.

    #[test]
    fn op_as_value() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x = (+)
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Basics", basics_with_binops_interface(&bump))]);
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
    fn binop_expression() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x a b = a + b
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Basics", basics_with_binops_interface(&bump))]);
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
    fn binop_multi_precedence() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x a b c = a + b * c
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Basics", basics_with_binops_interface(&bump))]);
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
    fn binop_right_assoc() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x a b c = a <| b <| c
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Basics", basics_with_binops_interface(&bump))]);
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

    // === Typed def tests ===

    #[test]
    fn typed_def_top_level() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f : a -> a
            f x = x
        "#
        );
    }

    #[test]
    fn typed_def_in_let() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            g =
                let
                    f : a -> a
                    f x = x
                in
                f 1
        "#
        );
    }

    // === Let destruct ===

    #[test]
    fn let_destruct() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    (a, b) = (1, 2)
                in
                a
        "#
        );
    }

    #[test]
    fn let_mutual_recursion() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    go x = stop x
                    stop x = go x
                in
                go 1
        "#
        );
    }

    // === find_var gaps ===

    #[test]
    fn foreign_var_unqualified() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (bar)

            x = bar
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
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
    fn not_found_var_qualified() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo

            x = Foo.nonexistent
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
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

    // === verify_bindings ===

    #[test]
    fn underscore_prefix_no_warning() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = \_ -> 1
        "#
        );
    }

    // === canonicalize_if ===

    #[test]
    fn chained_if_else_if() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x =
                if x then
                    1
                else if x then
                    2
                else
                    3
        "#
        );
    }

    // === canonicalize_update ===

    #[test]
    fn update_missing_record_var() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f = { nonexistent | x = 1 }
        "#
        );
    }

    // === case branches ===

    #[test]
    fn case_with_ctor_patterns() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            f x =
                case x of
                    Just y -> y
                    Nothing -> 0
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
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

    // === Expression error tests ===

    #[test]
    fn not_found_binop() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            x = (+)
        "#
        );
    }

    #[test]
    fn ambiguous_var() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (..)

            import Bar exposing (..)

            x = baz
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([
            ("Foo", value_interface(&bump, "Foo", "baz")),
            ("Bar", value_interface(&bump, "Bar", "baz")),
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
    fn ambiguous_ctor_in_expr() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            import Option exposing (Option(..))

            x = Just
        "#
        );
        let bump = Bump::new();
        // Build two interfaces that both expose "Just"
        let just_arg = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let just_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "Just",
            index: 0,
            arity: 1,
            arguments: bump.alloc_slice_fill_iter([&*just_arg]),
        });
        let nothing_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "Nothing",
            index: 1,
            arity: 0,
            arguments: &[],
        });
        let maybe_interface = Interface {
            home: ModuleName {
                package: None,
                name: "Maybe",
            },
            values: &[],
            aliases: &[],
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                name: "Maybe",
                parameters: bump.alloc_slice_fill_iter(["a"]),
                ctors: bump.alloc_slice_fill_iter([just_ctor, nothing_ctor]),
                alternatives: 2,
                options: CtorOpts::Normal,
                visibility: UnionVisibility::Open,
            }]),
            binops: &[],
        };

        let just_arg2 = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let just_ctor2: &CanCtor = bump.alloc(CanCtor {
            name: "Just",
            index: 0,
            arity: 1,
            arguments: bump.alloc_slice_fill_iter([&*just_arg2]),
        });
        let none_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "None",
            index: 1,
            arity: 0,
            arguments: &[],
        });
        let option_interface = Interface {
            home: ModuleName {
                package: None,
                name: "Option",
            },
            values: &[],
            aliases: &[],
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                name: "Option",
                parameters: bump.alloc_slice_fill_iter(["a"]),
                ctors: bump.alloc_slice_fill_iter([just_ctor2, none_ctor]),
                alternatives: 2,
                options: CtorOpts::Normal,
                visibility: UnionVisibility::Open,
            }]),
            binops: &[],
        };

        let interfaces = BTreeMap::from([("Maybe", maybe_interface), ("Option", option_interface)]);
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
    fn ambiguous_binop() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            import MyMath exposing (..)

            x a b = a + b
        "#
        );
        let bump = Bump::new();
        let basics = basics_with_binops_interface(&bump);
        let mymath = Interface {
            home: ModuleName {
                package: None,
                name: "MyMath",
            },
            values: &[],
            aliases: &[],
            unions: &[],
            binops: bump.alloc_slice_fill_iter([InterfaceBinop {
                symbol: "+",
                associativity: Associativity::Left,
                precedence: Precedence(6),
                function: "myAdd",
            }]),
        };
        let interfaces = BTreeMap::from([("Basics", basics), ("MyMath", mymath)]);
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
    fn binop_non_assoc_conflict() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x a b c = a == b == c
        "#
        );
        let bump = Bump::new();
        let basics = Interface {
            home: ModuleName {
                package: None,
                name: "Basics",
            },
            values: bump.alloc_slice_fill_iter([InterfaceValue {
                name: "eq",
                annotation: None,
            }]),
            aliases: &[],
            unions: &[],
            binops: bump.alloc_slice_fill_iter([InterfaceBinop {
                symbol: "==",
                associativity: Associativity::None,
                precedence: Precedence(4),
                function: "eq",
            }]),
        };
        let interfaces = BTreeMap::from([("Basics", basics)]);
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
    fn annotation_too_short() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f : a
            f x = x
        "#
        );
    }

    #[test]
    fn duplicate_field_in_update() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f r = { r | x = 1, x = 2 }
        "#
        );
    }

    #[test]
    fn duplicate_pattern_lambda_args() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = \x x -> x
        "#
        );
    }

    #[test]
    fn duplicate_pattern_func_args() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x x = x
        "#
        );
    }

    #[test]
    fn duplicate_pattern_destruct() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    (x, x) = (1, 2)
                in
                x
        "#
        );
    }

    #[test]
    fn let_mutual_recursion_values_error() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    a = b
                    b = a
                in
                a
        "#
        );
    }

    #[test]
    fn shadowing_toplevel() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            x = 1

            f =
                let
                    x = 2
                in
                x
        "#
        );
    }

    // === Expression warning tests ===

    #[test]
    fn unused_func_arg_warning() {
        assert_module_warning_snapshot!(
            r#"
            module Main exposing (..)

            f x = 42
        "#
        );
    }

    #[test]
    fn unused_case_branch_var() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            f x =
                case x of
                    Just y -> 1
                    Nothing -> 0
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let (_, warnings) = parse_and_canonicalize_with_warnings(&bump, input, context)
            .expect("expected successful canonicalization");
        assert!(!warnings.is_empty(), "expected warnings but got none");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(warnings);
        });
    }

    // === Module-level tests ===

    #[test]
    fn duplicate_ctor_error() {
        // A record alias and union both produce a ctor named "Point"
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Point a = { x : a, y : a }

            type Shape a
                = Point a a
                | Circle a
        "#
        );
    }

    #[test]
    fn ctor_opts_unbox() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type Wrap a
                = Wrap a
        "#
        );
    }

    #[test]
    fn multiple_unbound_union_vars() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type Foo
                = Bar a b
        "#
        );
    }

    #[test]
    fn alias_both_unused_and_unbound() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Bad a = b
        "#
        );
    }

    #[test]
    fn export_explicit_value() {
        assert_module_snapshot!(
            r#"
            module Main exposing (foo)

            foo = 42
        "#
        );
    }

    #[test]
    fn infix_right_associativity() {
        assert_module_snapshot!(
            r#"
            module Main exposing ((<|))

            infix right 0 (<|) = apL
        "#
        );
    }

    #[test]
    fn infix_non_associativity() {
        assert_module_snapshot!(
            r#"
            module Main exposing ((==))

            infix non 4 (==) = eq
        "#
        );
    }

    #[test]
    fn local_record_alias_ctor() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type alias Point a = { x : a, y : a }

            p = Point
        "#
        );
    }

    // === Interface tests: non-exported binop ===

    #[test]
    fn interface_non_exported_binop() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (foo)

            infix left 6 (|>) = apR

            foo = 42
        "#
        );
    }

    // === Import validation tests ===

    #[test]
    fn import_exposing_not_found_value() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (nonexistent)

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
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
    fn import_exposing_not_found_type() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (Nonexistent)

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
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
    fn import_exposing_not_found_op() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing ((+))

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
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
    fn import_ctor_by_name() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Just)

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
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
    fn import_open_alias() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (MyAlias(..))

            x = 1
        "#
        );
        let bump = Bump::new();
        let alias_type = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let foo = Interface {
            home: ModuleName {
                package: None,
                name: "Foo",
            },
            values: &[],
            aliases: bump.alloc_slice_fill_iter([InterfaceAlias {
                name: "MyAlias",
                parameters: bump.alloc_slice_fill_iter(["a"]),
                typ: alias_type,
                visibility: AliasVisibility::Public,
            }]),
            unions: &[],
            binops: &[],
        };
        let interfaces = BTreeMap::from([("Foo", foo)]);
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

    // === collect_used_modules coverage ===

    #[test]
    fn collect_from_def_typed() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe)

            f : Maybe a -> Maybe a
            f x = x
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let (_, warnings) = parse_and_canonicalize_with_warnings(&bump, input, context)
            .expect("expected successful canonicalization");
        // Should not have an "unused import" warning for Maybe because
        // it is used in the type annotation.
        assert!(
            warnings.is_empty(),
            "expected no warnings but got: {warnings:?}"
        );
    }
}
