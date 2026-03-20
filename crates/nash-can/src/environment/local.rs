use bumpalo::Bump;
use nash_ast::{Alias as CanAlias, Type as CanType, Union as CanUnion};
use nash_region::Located;
use nash_source::{Infix, Union as SourceUnion, Value as SourceValue};

use super::{Binop, Ctor, Env, Info, Type, Var};

/// Add local union type names to the environment (before canonicalization).
pub fn add_union_types<'a>(env: &mut Env<'a>, unions: &'a [&'a Located<SourceUnion<'a>>]) {
    for union in unions {
        let typ = Type::Union {
            arity: union.value.arguments.len(),
            home: env.home,
        };
        env.insert_local_type(union.value.name.value, typ);
    }
}

/// Add a single canonicalized alias to the environment.
/// Called incrementally as each alias is canonicalized so subsequent aliases
/// can reference previously processed ones through the env.
/// If the alias body is a non-extensible record, also adds a RecordCtor.
pub fn add_alias_type<'a>(bump: &'a Bump, env: &mut Env<'a>, can_alias: &CanAlias<'a>) {
    let typ = Type::Alias {
        arity: can_alias.parameters.len(),
        home: env.home,
        parameters: can_alias.parameters,
        typ: can_alias.typ,
    };
    env.insert_local_type(can_alias.name.value, typ);

    // Record alias constructor (Elm's RecordCtor)
    if let CanType::Record { fields, ext: None } = &can_alias.typ.value {
        let field_names = bump.alloc_slice_fill_iter(fields.iter().map(|f| f.field));
        let field_types = bump.alloc_slice_fill_iter(fields.iter().map(|f| f.typ));
        let info = Ctor::RecordCtor {
            home: env.home,
            field_names,
            field_types,
        };
        env.insert_local_ctor(can_alias.name.value, info);
    }
}

/// Add local constructors to the environment (after union canonicalization).
pub fn add_ctors<'a>(env: &mut Env<'a>, unions: &'a [&'a Located<CanUnion<'a>>]) {
    for union in unions {
        for ctor in union.value.ctors {
            let info = Ctor::Union {
                home: env.home,
                type_name: union.value.name.value,
                index: ctor.index,
                arity: ctor.arity,
                arguments: ctor.arguments,
                options: union.value.options,
                alternatives: union.value.alternatives,
            };
            env.insert_local_ctor(ctor.name, info);
        }
    }
}

/// Add local top-level value names to the environment.
pub fn add_vars<'a>(env: &mut Env<'a>, values: &'a [&'a Located<SourceValue<'a>>]) {
    for value in values {
        env.vars.insert(
            value.value.name.value,
            Var::TopLevel(value.value.name.region),
        );
    }
}

/// Add local binops to the environment.
pub fn add_binops<'a>(env: &mut Env<'a>, binops: &'a [&'a Located<Infix<'a>>]) {
    for binop in binops {
        let info = Binop {
            home: env.home,
            function: binop.value.name,
            associativity: binop.value.associativity,
            precedence: binop.value.precedence,
        };
        env.binops
            .insert(binop.value.op, Info::Specific(env.home, info));
    }
}
