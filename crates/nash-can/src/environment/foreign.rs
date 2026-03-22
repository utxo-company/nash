use std::collections::BTreeMap;

use bumpalo::Bump;
use nash_ast::{ModuleName, Type as CanType};
use nash_source::{Exposed, Exposing, Import as SourceImport, Privacy};

use super::{Binop, Ctor, Env, Type, Var, merge_exposed, merge_qualified};
use crate::error::Error;
use crate::interface::Interface;

pub fn create_initial_env<'a>(
    bump: &'a Bump,
    home: ModuleName<'a>,
    interfaces: Option<&'a BTreeMap<&'a str, Interface<'a>>>,
    imports: &'a [&'a SourceImport<'a>],
) -> Result<Env<'a>, Error<'a>> {
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
        let is_open = matches!(import.exposing, Exposing::Open);

        add_import_types(&mut env, interface, prefix, import, is_open);
        add_import_ctors(bump, &mut env, interface, prefix, import, is_open);
        add_import_values(&mut env, interface, prefix, import, is_open);
        add_import_binops(&mut env, interface, import, is_open);
    }

    Ok(env)
}

fn add_import_types<'a>(
    env: &mut Env<'a>,
    interface: &Interface<'a>,
    prefix: &'a str,
    import: &SourceImport<'a>,
    is_open: bool,
) {
    for alias in interface.aliases {
        let typ = Type::Alias {
            arity: alias.parameters.len(),
            home: interface.home,
            parameters: alias.parameters,
            typ: alias.typ,
        };
        merge_qualified(&mut env.q_types, prefix, alias.name, interface.home, typ);
        if is_open || import_exposes_type(import, alias.name) {
            merge_exposed(&mut env.types, alias.name, interface.home, typ);
        }
    }

    for union in interface.unions {
        let typ = Type::Union {
            arity: union.parameters.len(),
            home: interface.home,
        };
        merge_qualified(&mut env.q_types, prefix, union.name, interface.home, typ);
        if is_open || import_exposes_type(import, union.name) {
            merge_exposed(&mut env.types, union.name, interface.home, typ);
        }
    }
}

fn add_import_ctors<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    interface: &Interface<'a>,
    prefix: &'a str,
    import: &SourceImport<'a>,
    is_open: bool,
) {
    // Union constructors
    for union in interface.unions {
        let expose_ctors = is_open || import_exposes_union_ctors(import, union.name);
        for ctor in union.ctors {
            let info = Ctor::Union {
                home: interface.home,
                type_name: union.name,
                type_vars: union.parameters,
                index: ctor.index,
                arity: ctor.arity,
                arguments: ctor.arguments,
                options: union.options,
                alternatives: union.alternatives,
            };
            merge_qualified(&mut env.q_ctors, prefix, ctor.name, interface.home, info);
            if expose_ctors {
                merge_exposed(&mut env.ctors, ctor.name, interface.home, info);
            }
        }
    }

    // Record alias constructors (Elm's RecordCtor)
    for alias in interface.aliases {
        if let CanType::Record { fields, ext: None } = &alias.typ.value {
            let field_names = bump.alloc_slice_fill_iter(fields.iter().map(|f| f.field));
            let field_types = bump.alloc_slice_fill_iter(fields.iter().map(|f| f.typ));
            let info = Ctor::RecordCtor {
                home: interface.home,
                field_names,
                field_types,
            };
            merge_qualified(&mut env.q_ctors, prefix, alias.name, interface.home, info);
            if is_open || import_exposes_type(import, alias.name) {
                merge_exposed(&mut env.ctors, alias.name, interface.home, info);
            }
        }
    }
}

fn add_import_values<'a>(
    env: &mut Env<'a>,
    interface: &Interface<'a>,
    prefix: &'a str,
    import: &SourceImport<'a>,
    is_open: bool,
) {
    for iv in interface.values {
        let inner = env.q_vars.entry(prefix).or_default();
        merge_exposed(inner, iv.name, interface.home, ());
        if is_open || import_exposes_value(import, iv.name) {
            use std::collections::btree_map::Entry;
            match env.vars.entry(iv.name) {
                Entry::Vacant(e) => {
                    e.insert(Var::Foreign(interface.home));
                }
                Entry::Occupied(mut e) => match e.get() {
                    Var::Foreign(existing) if existing.name != interface.home.name => {
                        let first = *existing;
                        e.insert(Var::Foreigns(first, vec![interface.home]));
                    }
                    Var::Foreigns(_, others) => {
                        if !others.iter().any(|h| h.name == interface.home.name)
                            && let Var::Foreigns(_, others) = e.get_mut()
                        {
                            others.push(interface.home);
                        }
                    }
                    _ => {}
                },
            }
        }
    }
}

fn add_import_binops<'a>(
    env: &mut Env<'a>,
    interface: &Interface<'a>,
    import: &SourceImport<'a>,
    is_open: bool,
) {
    for binop in interface.binops {
        let info = Binop {
            symbol: binop.symbol,
            home: interface.home,
            function: binop.function,
            associativity: binop.associativity,
            precedence: binop.precedence,
        };
        // Binops are only unqualified (you don't write Module.(+))
        if is_open || import_exposes_operator(import, binop.symbol) {
            merge_exposed(&mut env.binops, binop.symbol, interface.home, info);
        }
    }
}

fn find_interface<'a>(
    interfaces: Option<&'a BTreeMap<&'a str, Interface<'a>>>,
    import: &SourceImport<'a>,
) -> Result<&'a Interface<'a>, Error<'a>> {
    interfaces
        .and_then(|m| m.get(import.import.value))
        .ok_or(Error::ImportNotFound {
            region: import.import.region,
            module: import.import.value,
        })
}

fn import_prefix<'a>(import: &SourceImport<'a>) -> &'a str {
    import.alias.unwrap_or(import.import.value)
}

fn import_exposes_type(import: &SourceImport<'_>, name: &str) -> bool {
    match import.exposing {
        Exposing::Open => true,
        Exposing::Explicit(exposed) => exposed.iter().any(|e| match e {
            Exposed::Upper { name: n, .. } => n.value == name,
            _ => false,
        }),
    }
}

fn import_exposes_union_ctors(import: &SourceImport<'_>, union_name: &str) -> bool {
    match import.exposing {
        Exposing::Open => true,
        Exposing::Explicit(exposed) => exposed.iter().any(|e| match e {
            Exposed::Upper {
                name,
                privacy: Privacy::Public(_),
            } => name.value == union_name,
            _ => false,
        }),
    }
}

fn import_exposes_value(import: &SourceImport<'_>, name: &str) -> bool {
    match import.exposing {
        Exposing::Open => true,
        Exposing::Explicit(exposed) => exposed.iter().any(|e| match e {
            Exposed::Lower(n) => n.value == name,
            _ => false,
        }),
    }
}

fn import_exposes_operator(import: &SourceImport<'_>, symbol: &str) -> bool {
    match import.exposing {
        Exposing::Open => true,
        Exposing::Explicit(exposed) => exposed.iter().any(|e| match e {
            Exposed::Operator { op, .. } => *op == symbol,
            _ => false,
        }),
    }
}
