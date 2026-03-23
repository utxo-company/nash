use std::collections::BTreeMap;

use bumpalo::Bump;
use nash_ast::{ModuleName, Type as CanType};
use nash_region::{Located, Region};
use nash_source::{Exposed, Exposing, Import as SourceImport, Privacy};

use super::{Binop, Ctor, Env, Type, Var, merge_exposed, merge_qualified};
use crate::error::Error;
use crate::interface::Interface;

pub fn create_initial_env<'a>(
    bump: &'a Bump,
    home: ModuleName<'a>,
    interfaces: Option<&'a BTreeMap<&'a str, Interface<'a>>>,
    imports: &'a [&'a SourceImport<'a>],
) -> Result<Env<'a>, Vec<Error<'a>>> {
    let mut env = Env {
        home,
        vars: BTreeMap::new(),
        types: BTreeMap::new(),
        ctors: BTreeMap::new(),
        binops: BTreeMap::new(),
        q_vars: BTreeMap::new(),
        q_types: BTreeMap::new(),
        q_ctors: BTreeMap::new(),
    };

    for import in imports {
        let interface = find_interface(interfaces, import)?;
        let prefix = import_prefix(import);

        // Qualified lookups are always available
        add_qualified_types(&mut env, interface, prefix);
        add_qualified_ctors(bump, &mut env, interface, prefix);
        add_qualified_values(&mut env, interface, prefix);

        // Unqualified exposure depends on the exposing clause
        match &import.exposing {
            Exposing::Open => {
                add_open_types(&mut env, interface);
                add_open_ctors(&mut env, interface);
                add_open_values(&mut env, interface);
                add_open_binops(&mut env, interface);
            }
            Exposing::Explicit(exposed) => {
                validate_explicit_exposing(bump, &mut env, interface, exposed)?;
            }
        }
    }

    Ok(env)
}

// --- Qualified (always) ---

fn add_qualified_types<'a>(env: &mut Env<'a>, interface: &Interface<'a>, prefix: &'a str) {
    for alias in interface.aliases {
        let typ = Type::Alias {
            arity: alias.parameters.len(),
            home: interface.home,
            parameters: alias.parameters,
            typ: alias.typ,
        };
        merge_qualified(&mut env.q_types, prefix, alias.name, interface.home, typ);
    }
    for union in interface.unions {
        let typ = Type::Union {
            arity: union.parameters.len(),
            home: interface.home,
        };
        merge_qualified(&mut env.q_types, prefix, union.name, interface.home, typ);
    }
}

fn add_qualified_ctors<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    interface: &Interface<'a>,
    prefix: &'a str,
) {
    for union in interface.unions {
        let can_union = make_can_union(bump, union);
        for ctor in union.ctors {
            let info = make_union_ctor(interface.home, union, can_union, ctor);
            merge_qualified(&mut env.q_ctors, prefix, ctor.name, interface.home, info);
        }
    }
    for alias in interface.aliases {
        if let Some(info) = make_record_ctor(bump, interface.home, alias) {
            merge_qualified(&mut env.q_ctors, prefix, alias.name, interface.home, info);
        }
    }
}

fn add_qualified_values<'a>(env: &mut Env<'a>, interface: &Interface<'a>, prefix: &'a str) {
    for iv in interface.values {
        let inner = env.q_vars.entry(prefix).or_default();
        merge_exposed(inner, iv.name, interface.home, ());
    }
}

// --- Open (expose everything) ---

fn add_open_types<'a>(env: &mut Env<'a>, interface: &Interface<'a>) {
    for alias in interface.aliases {
        let typ = Type::Alias {
            arity: alias.parameters.len(),
            home: interface.home,
            parameters: alias.parameters,
            typ: alias.typ,
        };
        merge_exposed(&mut env.types, alias.name, interface.home, typ);
    }
    for union in interface.unions {
        let typ = Type::Union {
            arity: union.parameters.len(),
            home: interface.home,
        };
        merge_exposed(&mut env.types, union.name, interface.home, typ);
    }
}

fn add_open_ctors<'a>(env: &mut Env<'a>, interface: &Interface<'a>) {
    for union in interface.unions {
        for ctor in union.ctors {
            if let Some(ctor_info) = lookup_qualified_ctor(env, interface.home.name, ctor.name) {
                merge_exposed(&mut env.ctors, ctor.name, interface.home, ctor_info);
            }
        }
    }
    for alias in interface.aliases {
        if matches!(&alias.typ.value, CanType::Record { ext: None, .. })
            && let Some(ctor_info) = lookup_qualified_ctor(env, interface.home.name, alias.name)
        {
            merge_exposed(&mut env.ctors, alias.name, interface.home, ctor_info);
        }
    }
}

fn add_open_values<'a>(env: &mut Env<'a>, interface: &Interface<'a>) {
    for iv in interface.values {
        add_single_value(env, interface.home, iv.name);
    }
}

fn add_open_binops<'a>(env: &mut Env<'a>, interface: &Interface<'a>) {
    for binop in interface.binops {
        let info = Binop {
            symbol: binop.symbol,
            home: interface.home,
            function: binop.function,
            associativity: binop.associativity,
            precedence: binop.precedence,
        };
        merge_exposed(&mut env.binops, binop.symbol, interface.home, info);
    }
}

// --- Explicit exposing validation ---

fn validate_explicit_exposing<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    interface: &Interface<'a>,
    exposed: &[&'a Exposed<'a>],
) -> Result<(), Vec<Error<'a>>> {
    let mut errors = Vec::new();

    for item in exposed {
        match item {
            Exposed::Lower(name) => {
                if find_value(interface, name.value) {
                    add_single_value(env, interface.home, name.value);
                } else {
                    errors.push(Error::ImportExposingNotFound {
                        region: name.region,
                        module: interface.home,
                        name: name.value,
                        available: available_values(bump, interface),
                    });
                }
            }
            Exposed::Upper { name, privacy } => match privacy {
                Privacy::Private => {
                    // `import Foo exposing (Bar)` — expose the type (alias or union)
                    // but not union constructors
                    if let Some(()) = find_and_expose_type(env, interface, name.value) {
                        // Also expose record alias ctor if applicable
                        expose_record_ctor_if_applicable(env, interface, name.value);
                    } else if check_for_ctor_mistake(interface, name.value) {
                        errors.push(Error::ImportCtorByName {
                            region: name.region,
                            name: name.value,
                            type_name: find_ctor_type_name(interface, name.value)
                                .unwrap_or(name.value),
                        });
                    } else {
                        errors.push(Error::ImportExposingNotFound {
                            region: name.region,
                            module: interface.home,
                            name: name.value,
                            available: available_types(bump, interface),
                        });
                    }
                }
                Privacy::Public(_) => {
                    // `import Foo exposing (Bar(..))` — must be a union, not an alias
                    if find_union(interface, name.value) {
                        find_and_expose_type(env, interface, name.value);
                        expose_union_ctors(env, interface, name.value);
                    } else if find_alias(interface, name.value) {
                        errors.push(Error::ImportOpenAlias {
                            region: name.region,
                            name: name.value,
                        });
                    } else {
                        errors.push(Error::ImportExposingNotFound {
                            region: name.region,
                            module: interface.home,
                            name: name.value,
                            available: available_types(bump, interface),
                        });
                    }
                }
            },
            Exposed::Operator { region, op } => {
                if let Some(binop) = find_binop(interface, op) {
                    let info = Binop {
                        symbol: binop.symbol,
                        home: interface.home,
                        function: binop.function,
                        associativity: binop.associativity,
                        precedence: binop.precedence,
                    };
                    merge_exposed(&mut env.binops, binop.symbol, interface.home, info);
                } else {
                    errors.push(Error::ImportExposingNotFound {
                        region: *region,
                        module: interface.home,
                        name: op,
                        available: available_binops(bump, interface),
                    });
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// --- Single-item helpers ---

fn add_single_value<'a>(env: &mut Env<'a>, home: ModuleName<'a>, name: &'a str) {
    use std::collections::btree_map::Entry;
    match env.vars.entry(name) {
        Entry::Vacant(e) => {
            e.insert(Var::Foreign(home));
        }
        Entry::Occupied(mut e) => match e.get() {
            Var::Foreign(existing) if existing.name != home.name => {
                let first = *existing;
                e.insert(Var::Foreigns(first, vec![home]));
            }
            Var::Foreigns(_, others) => {
                if !others.iter().any(|h| h.name == home.name)
                    && let Var::Foreigns(_, others) = e.get_mut()
                {
                    others.push(home);
                }
            }
            _ => {}
        },
    }
}

fn find_and_expose_type<'a>(
    env: &mut Env<'a>,
    interface: &Interface<'a>,
    name: &'a str,
) -> Option<()> {
    for alias in interface.aliases {
        if alias.name == name {
            let typ = Type::Alias {
                arity: alias.parameters.len(),
                home: interface.home,
                parameters: alias.parameters,
                typ: alias.typ,
            };
            merge_exposed(&mut env.types, name, interface.home, typ);
            return Some(());
        }
    }
    for union in interface.unions {
        if union.name == name {
            let typ = Type::Union {
                arity: union.parameters.len(),
                home: interface.home,
            };
            merge_exposed(&mut env.types, name, interface.home, typ);
            return Some(());
        }
    }
    None
}

fn expose_union_ctors<'a>(env: &mut Env<'a>, interface: &Interface<'a>, union_name: &'a str) {
    for union in interface.unions {
        if union.name == union_name {
            for ctor in union.ctors {
                if let Some(super::Info::Specific(_, ctor_info)) = env
                    .q_ctors
                    .get(interface.home.name)
                    .and_then(|m| m.get(ctor.name))
                {
                    merge_exposed(&mut env.ctors, ctor.name, interface.home, *ctor_info);
                }
            }
        }
    }
}

fn expose_record_ctor_if_applicable<'a>(
    env: &mut Env<'a>,
    interface: &Interface<'a>,
    name: &'a str,
) {
    for alias in interface.aliases {
        if alias.name == name
            && matches!(&alias.typ.value, CanType::Record { ext: None, .. })
            && let Some(super::Info::Specific(_, ctor_info)) = env
                .q_ctors
                .get(interface.home.name)
                .and_then(|m| m.get(name))
        {
            merge_exposed(&mut env.ctors, name, interface.home, *ctor_info);
        }
    }
}

// --- Lookup helpers ---

fn lookup_qualified_ctor<'a>(env: &Env<'a>, module: &str, name: &str) -> Option<Ctor<'a>> {
    if let Some(super::Info::Specific(_, ctor)) = env.q_ctors.get(module).and_then(|m| m.get(name))
    {
        Some(*ctor)
    } else {
        None
    }
}

fn find_value(interface: &Interface<'_>, name: &str) -> bool {
    interface.values.iter().any(|v| v.name == name)
}

fn find_alias(interface: &Interface<'_>, name: &str) -> bool {
    interface.aliases.iter().any(|a| a.name == name)
}

fn find_union(interface: &Interface<'_>, name: &str) -> bool {
    interface.unions.iter().any(|u| u.name == name)
}

fn find_binop<'a>(
    interface: &Interface<'a>,
    symbol: &str,
) -> Option<&'a crate::interface::InterfaceBinop<'a>> {
    interface.binops.iter().find(|b| b.symbol == symbol)
}

fn check_for_ctor_mistake(interface: &Interface<'_>, name: &str) -> bool {
    interface
        .unions
        .iter()
        .any(|u| u.ctors.iter().any(|c| c.name == name))
}

fn find_ctor_type_name<'a>(interface: &Interface<'a>, ctor_name: &str) -> Option<&'a str> {
    interface
        .unions
        .iter()
        .find(|u| u.ctors.iter().any(|c| c.name == ctor_name))
        .map(|u| u.name)
}

fn available_values<'a>(bump: &'a Bump, interface: &Interface<'a>) -> &'a [&'a str] {
    bump.alloc_slice_fill_iter(interface.values.iter().map(|v| v.name))
}

fn available_types<'a>(bump: &'a Bump, interface: &Interface<'a>) -> &'a [&'a str] {
    let names: Vec<&'a str> = interface
        .aliases
        .iter()
        .map(|a| a.name)
        .chain(interface.unions.iter().map(|u| u.name))
        .collect();
    bump.alloc_slice_fill_iter(names)
}

fn available_binops<'a>(bump: &'a Bump, interface: &Interface<'a>) -> &'a [&'a str] {
    bump.alloc_slice_fill_iter(interface.binops.iter().map(|b| b.symbol))
}

// --- Construction helpers ---

fn make_can_union<'a>(
    bump: &'a Bump,
    union: &crate::interface::InterfaceUnion<'a>,
) -> &'a nash_ast::Union<'a> {
    bump.alloc(nash_ast::Union {
        name: bump.alloc(Located::at(Region::zero(), union.name)),
        parameters: union.parameters,
        ctors: union.ctors,
        alternatives: union.alternatives,
        options: union.options,
    })
}

fn make_union_ctor<'a>(
    home: ModuleName<'a>,
    union: &crate::interface::InterfaceUnion<'a>,
    can_union: &'a nash_ast::Union<'a>,
    ctor: &nash_ast::Ctor<'a>,
) -> Ctor<'a> {
    Ctor::Union {
        home,
        type_name: union.name,
        type_vars: union.parameters,
        union: can_union,
        index: ctor.index,
        arity: ctor.arity,
        arguments: ctor.arguments,
        options: union.options,
        alternatives: union.alternatives,
    }
}

fn make_record_ctor<'a>(
    bump: &'a Bump,
    home: ModuleName<'a>,
    alias: &crate::interface::InterfaceAlias<'a>,
) -> Option<Ctor<'a>> {
    if let CanType::Record { fields, ext: None } = &alias.typ.value {
        let field_names = bump.alloc_slice_fill_iter(fields.iter().map(|f| f.field));
        let field_types = bump.alloc_slice_fill_iter(fields.iter().map(|f| f.typ));
        Some(Ctor::RecordCtor {
            home,
            alias_name: alias.name,
            type_vars: alias.parameters,
            field_names,
            field_types,
        })
    } else {
        None
    }
}

fn find_interface<'a>(
    interfaces: Option<&'a BTreeMap<&'a str, Interface<'a>>>,
    import: &SourceImport<'a>,
) -> Result<&'a Interface<'a>, Vec<Error<'a>>> {
    interfaces
        .and_then(|m| m.get(import.import.value))
        .ok_or_else(|| {
            vec![Error::ImportNotFound {
                region: import.import.region,
                module: import.import.value,
            }]
        })
}

fn import_prefix<'a>(import: &SourceImport<'a>) -> &'a str {
    import.alias.unwrap_or(import.import.value)
}
