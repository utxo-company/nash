use bumpalo::Bump;
use nash_ast::{Alias as CanAlias, Type as CanType, Union as CanUnion};
use nash_region::Located;
use nash_source::{Alias as SourceAlias, Infix, Union as SourceUnion, Value as SourceValue};

use super::{Binop, Ctor, Env, Info, Type, Var, dups};
use crate::Error;

pub fn add_union_types<'a>(
    env: &mut Env<'a>,
    unions: &'a [&'a Located<SourceUnion<'a>>],
    aliases: &'a [&'a Located<SourceAlias<'a>>],
) -> Result<(), Vec<Error<'a>>> {
    let items = aliases
        .iter()
        .map(|a| (a.value.name.value, a.value.name.region))
        .chain(
            unions
                .iter()
                .map(|u| (u.value.name.value, u.value.name.region)),
        );
    dups::detect(items, |name, first, second| Error::DuplicateType {
        name,
        first,
        second,
    })?;

    for union in unions {
        let typ = Type::Union {
            arity: union.value.arguments.len(),
            home: env.home,
        };
        env.insert_local_type(union.value.name.value, typ);
    }
    Ok(())
}

pub fn add_alias_type<'a>(bump: &'a Bump, env: &mut Env<'a>, can_alias: &CanAlias<'a>) {
    let typ = Type::Alias {
        arity: can_alias.parameters.len(),
        home: env.home,
        parameters: can_alias.parameters,
        typ: can_alias.typ,
    };
    env.insert_local_type(can_alias.name.value, typ);

    if let CanType::Record { fields, ext: None } = &can_alias.typ.value {
        let field_names = bump.alloc_slice_fill_iter(fields.iter().map(|f| f.field));
        let field_types = bump.alloc_slice_fill_iter(fields.iter().map(|f| f.typ));
        let info = Ctor::RecordCtor {
            home: env.home,
            alias_name: can_alias.name.value,
            type_vars: can_alias.parameters,
            field_names,
            field_types,
        };
        env.insert_local_ctor(can_alias.name.value, info);
    }
}

pub fn add_ctors<'a>(
    env: &mut Env<'a>,
    unions: &'a [&'a Located<CanUnion<'a>>],
    aliases: &'a [&'a Located<CanAlias<'a>>],
) -> Result<(), Vec<Error<'a>>> {
    let union_ctors = unions.iter().flat_map(|u| {
        u.value
            .ctors
            .iter()
            .map(move |c| (c.name, u.value.name.region))
    });
    let alias_ctors = aliases
        .iter()
        .filter(|a| matches!(&a.value.typ.value, CanType::Record { ext: None, .. }))
        .map(|a| (a.value.name.value, a.value.name.region));
    dups::detect(union_ctors.chain(alias_ctors), |name, first, second| {
        Error::DuplicateCtor {
            name,
            first,
            second,
        }
    })?;

    for union in unions {
        for ctor in union.value.ctors {
            let info = Ctor::Union {
                home: env.home,
                type_name: union.value.name.value,
                type_vars: union.value.parameters,
                union: &union.value,
                index: ctor.index,
                arity: ctor.arity,
                arguments: ctor.arguments,
                options: union.value.options,
                alternatives: union.value.alternatives,
            };
            env.insert_local_ctor(ctor.name, info);
        }
    }
    Ok(())
}

pub fn add_vars<'a>(
    env: &mut Env<'a>,
    values: &'a [&'a Located<SourceValue<'a>>],
) -> Result<(), Vec<Error<'a>>> {
    dups::detect(
        values
            .iter()
            .map(|v| (v.value.name.value, v.value.name.region)),
        |name, first, second| Error::DuplicateDecl {
            name,
            first,
            second,
        },
    )?;

    for value in values {
        env.vars.insert(
            value.value.name.value,
            Var::TopLevel(value.value.name.region),
        );
    }
    Ok(())
}

pub fn add_binops<'a>(
    env: &mut Env<'a>,
    binops: &'a [&'a Located<Infix<'a>>],
) -> Result<(), Vec<Error<'a>>> {
    dups::detect(
        binops.iter().map(|b| (b.value.op, b.region)),
        |name, first, second| Error::DuplicateBinop {
            name,
            first,
            second,
        },
    )?;

    for binop in binops {
        let info = Binop {
            symbol: binop.value.op,
            home: env.home,
            function: binop.value.name,
            associativity: binop.value.associativity,
            precedence: binop.value.precedence,
        };
        env.binops
            .insert(binop.value.op, Info::Specific(env.home, info));
    }
    Ok(())
}
