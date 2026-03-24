use bumpalo::Bump;
use nash_ast::{
    Alias as CanAlias, Associativity, Binop as CanBinop, Ctor as CanCtor, CtorOpts, Decls, Def,
    Export, Exports, Module as CanModule, ModuleName, Precedence, Type as CanType,
    Union as CanUnion,
};
use nash_region::Located;

/// An exported value with optional type annotation.
#[derive(Clone, Copy, Debug)]
pub struct InterfaceValue<'a> {
    pub name: &'a str,
    /// None until type inference is implemented.
    pub annotation: Option<&'a Located<CanType<'a>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnionVisibility {
    Open,
    Closed,
    Private,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AliasVisibility {
    Public,
    Private,
}

#[derive(Clone, Copy, Debug)]
pub struct Interface<'a> {
    pub home: ModuleName<'a>,
    pub values: &'a [InterfaceValue<'a>],
    pub aliases: &'a [InterfaceAlias<'a>],
    pub unions: &'a [InterfaceUnion<'a>],
    pub binops: &'a [InterfaceBinop<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceUnion<'a> {
    pub name: &'a str,
    pub parameters: &'a [&'a str],
    pub ctors: &'a [&'a CanCtor<'a>],
    pub alternatives: u16,
    pub options: CtorOpts,
    pub visibility: UnionVisibility,
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceAlias<'a> {
    pub name: &'a str,
    pub parameters: &'a [&'a str],
    pub typ: &'a Located<CanType<'a>>,
    pub visibility: AliasVisibility,
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceBinop<'a> {
    pub symbol: &'a str,
    pub associativity: Associativity,
    pub precedence: Precedence,
    pub function: &'a str,
}

pub fn from_module<'a>(bump: &'a Bump, module: &CanModule<'a>) -> Interface<'a> {
    Interface {
        home: module.name,
        values: extract_values(bump, &module.exports, module.decls),
        unions: extract_unions(bump, &module.exports, module.unions),
        aliases: extract_aliases(bump, &module.exports, module.aliases),
        binops: extract_binops(bump, &module.exports, module.binops),
    }
}

impl<'a> InterfaceUnion<'a> {
    pub fn to_public(&self) -> Option<InterfaceUnion<'a>> {
        match self.visibility {
            UnionVisibility::Open => Some(*self),
            UnionVisibility::Closed => Some(InterfaceUnion {
                ctors: &[],
                alternatives: 0,
                ..*self
            }),
            UnionVisibility::Private => None,
        }
    }
}

impl<'a> InterfaceAlias<'a> {
    pub fn to_public(&self) -> Option<InterfaceAlias<'a>> {
        match self.visibility {
            AliasVisibility::Public => Some(*self),
            AliasVisibility::Private => None,
        }
    }
}

fn extract_values<'a>(
    bump: &'a Bump,
    exports: &Exports<'a>,
    decls: &Decls<'a>,
) -> &'a [InterfaceValue<'a>] {
    let mut names = Vec::new();
    collect_decl_names(decls, &mut names);

    match exports {
        Exports::Everything(_) => {
            bump.alloc_slice_fill_iter(names.into_iter().map(|name| InterfaceValue {
                name,
                annotation: None,
            }))
        }
        Exports::Explicit(exports) => bump.alloc_slice_fill_iter(
            names
                .into_iter()
                .filter(|name| is_exported_value(exports, name))
                .map(|name| InterfaceValue {
                    name,
                    annotation: None,
                })
                .collect::<Vec<_>>(),
        ),
    }
}

fn collect_decl_names<'a>(decls: &Decls<'a>, names: &mut Vec<&'a str>) {
    match decls {
        Decls::Declare { definition, next } => {
            names.push(def_name(definition));
            collect_decl_names(next, names);
        }
        Decls::DeclareRec {
            definition,
            following,
            next,
        } => {
            names.push(def_name(definition));
            for def in *following {
                names.push(def_name(def));
            }
            collect_decl_names(next, names);
        }
        Decls::Empty => {}
    }
}

fn def_name<'a>(def: &Def<'a>) -> &'a str {
    match def {
        Def::Def { name, .. } | Def::TypedDef { name, .. } => name.value,
    }
}

fn is_exported_value(exports: &[&Located<Export<'_>>], name: &str) -> bool {
    exports
        .iter()
        .any(|export| matches!(&export.value, Export::Value(n) if *n == name))
}

fn extract_unions<'a>(
    bump: &'a Bump,
    exports: &Exports<'a>,
    unions: &'a [&'a Located<CanUnion<'a>>],
) -> &'a [InterfaceUnion<'a>] {
    bump.alloc_slice_fill_iter(unions.iter().map(|union| {
        let name = union.value.name.value;
        InterfaceUnion {
            name,
            parameters: union.value.parameters,
            ctors: union.value.ctors,
            alternatives: union.value.alternatives,
            options: union.value.options,
            visibility: union_visibility(exports, name),
        }
    }))
}

fn extract_aliases<'a>(
    bump: &'a Bump,
    exports: &Exports<'a>,
    aliases: &'a [&'a Located<CanAlias<'a>>],
) -> &'a [InterfaceAlias<'a>] {
    bump.alloc_slice_fill_iter(aliases.iter().map(|alias| {
        let name = alias.value.name.value;
        InterfaceAlias {
            name,
            parameters: alias.value.parameters,
            typ: alias.value.typ,
            visibility: alias_visibility(exports, name),
        }
    }))
}

fn extract_binops<'a>(
    bump: &'a Bump,
    exports: &Exports<'a>,
    binops: &'a [&'a Located<CanBinop<'a>>],
) -> &'a [InterfaceBinop<'a>] {
    bump.alloc_slice_fill_iter(
        binops
            .iter()
            .filter_map(|binop| {
                if is_exported_binop(exports, binop.value.symbol) {
                    Some(InterfaceBinop {
                        symbol: binop.value.symbol,
                        associativity: binop.value.associativity,
                        precedence: binop.value.precedence,
                        function: binop.value.function,
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
    )
}

fn union_visibility(exports: &Exports<'_>, name: &str) -> UnionVisibility {
    match exports {
        Exports::Everything(_) => UnionVisibility::Open,
        Exports::Explicit(exports) => {
            for export in exports.iter() {
                match &export.value {
                    Export::UnionOpen(n) if *n == name => return UnionVisibility::Open,
                    Export::UnionClosed(n) if *n == name => return UnionVisibility::Closed,
                    _ => {}
                }
            }
            UnionVisibility::Private
        }
    }
}

fn alias_visibility(exports: &Exports<'_>, name: &str) -> AliasVisibility {
    match exports {
        Exports::Everything(_) => AliasVisibility::Public,
        Exports::Explicit(exports) => {
            if exports
                .iter()
                .any(|e| matches!(&e.value, Export::Alias(n) if *n == name))
            {
                AliasVisibility::Public
            } else {
                AliasVisibility::Private
            }
        }
    }
}

fn is_exported_binop(exports: &Exports<'_>, symbol: &str) -> bool {
    match exports {
        Exports::Everything(_) => true,
        Exports::Explicit(exports) => exports
            .iter()
            .any(|export| matches!(&export.value, Export::Binop(s) if *s == symbol)),
    }
}
