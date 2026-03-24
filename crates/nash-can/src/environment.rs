pub mod dups;
pub mod foreign;
pub mod local;

use std::collections::BTreeMap;

use bumpalo::Bump;
use nash_ast::{Associativity, CtorOpts, ModuleName, Precedence, Type as CanType};
use nash_region::{Located, Region};

use crate::Error;

/// A resolved name: either uniquely identified or ambiguous across modules.
#[derive(Clone, Debug)]
pub enum Info<'a, T> {
    Specific(ModuleName<'a>, T),
    Ambiguous(ModuleName<'a>, Vec<ModuleName<'a>>),
}

/// Unqualified name -> resolution.
pub type Exposed<'a, T> = BTreeMap<&'a str, Info<'a, T>>;

/// Module prefix -> name -> resolution (for `Module.name` lookups).
pub type Qualified<'a, T> = BTreeMap<&'a str, BTreeMap<&'a str, Info<'a, T>>>;

/// A value variable in scope.
#[derive(Clone, Debug)]
pub enum Var<'a> {
    Local(Region),
    TopLevel(Region),
    /// Imported from another module. Annotation deferred.
    Foreign(ModuleName<'a>),
    /// Ambiguous import: same name imported from multiple modules.
    Foreigns(ModuleName<'a>, Vec<ModuleName<'a>>),
}

/// A type in scope (alias or union).
#[derive(Clone, Copy, Debug)]
pub enum Type<'a> {
    Alias {
        arity: usize,
        home: ModuleName<'a>,
        parameters: &'a [&'a str],
        typ: &'a Located<CanType<'a>>,
    },
    Union {
        arity: usize,
        home: ModuleName<'a>,
    },
}

/// A data constructor in scope.
#[derive(Clone, Copy, Debug)]
pub enum Ctor<'a> {
    /// Union constructor (e.g., `Just` from `type Maybe a = Just a | Nothing`).
    Union {
        home: ModuleName<'a>,
        type_name: &'a str,
        type_vars: &'a [&'a str],
        union: &'a nash_ast::Union<'a>,
        index: u16,
        arity: u16,
        arguments: &'a [&'a Located<CanType<'a>>],
        options: CtorOpts,
        alternatives: u16,
    },
    /// Built-in Bool constructor (True or False from Basics).
    /// Separated from `Union` so pattern/expression canonicalization can
    /// emit `CanPattern::Bool` / synthesize the annotation without string checks.
    Bool {
        home: ModuleName<'a>,
        union: &'a nash_ast::Union<'a>,
        index: u16,
    },
    /// Record alias ctor (e.g., `Point` from `type alias Point = { x : Int, y : Int }`).
    /// Elm creates these automatically for non-extensible record aliases.
    RecordCtor {
        home: ModuleName<'a>,
        alias_name: &'a str,
        type_vars: &'a [&'a str],
        field_names: &'a [&'a str],
        field_types: &'a [&'a Located<CanType<'a>>],
    },
}

/// A binary operator in scope.
#[derive(Clone, Copy, Debug)]
pub struct Binop<'a> {
    pub symbol: &'a str,
    pub home: ModuleName<'a>,
    pub function: &'a str,
    pub associativity: Associativity,
    pub precedence: Precedence,
}

/// The canonicalization environment.
///
/// Built from imports (foreign) then augmented with local definitions.
/// Consumed by type, pattern, and expression canonicalization.
#[derive(Clone)]
pub struct Env<'a> {
    pub home: ModuleName<'a>,
    pub vars: BTreeMap<&'a str, Var<'a>>,
    pub types: Exposed<'a, Type<'a>>,
    pub ctors: Exposed<'a, Ctor<'a>>,
    pub binops: Exposed<'a, Binop<'a>>,
    pub q_vars: Qualified<'a, ()>,
    pub q_types: Qualified<'a, Type<'a>>,
    pub q_ctors: Qualified<'a, Ctor<'a>>,
}

impl<'a> Env<'a> {
    /// Insert into both the unqualified and self-qualified tables.
    /// Used by local.rs — local definitions overwrite imported ones.
    pub fn insert_local_type(&mut self, name: &'a str, typ: Type<'a>) {
        self.types.insert(name, Info::Specific(self.home, typ));
        self.q_types
            .entry(self.home.name)
            .or_default()
            .insert(name, Info::Specific(self.home, typ));
    }

    /// Insert into both the unqualified and self-qualified ctor tables.
    pub fn insert_local_ctor(&mut self, name: &'a str, ctor: Ctor<'a>) {
        self.ctors.insert(name, Info::Specific(self.home, ctor));
        self.q_ctors
            .entry(self.home.name)
            .or_default()
            .insert(name, Info::Specific(self.home, ctor));
    }

    /// Look up an unqualified constructor. Mirrors Elm's `Env.findCtor`.
    pub fn find_ctor(
        &self,
        bump: &'a Bump,
        region: Region,
        name: &'a str,
    ) -> Result<Ctor<'a>, Vec<Error<'a>>> {
        match self.ctors.get(name) {
            Some(Info::Specific(_, ctor)) => Ok(*ctor),
            Some(Info::Ambiguous(first, others)) => Err(vec![Error::AmbiguousCtor {
                region,
                prefix: None,
                name,
                first_module: *first,
                other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
            }]),
            None => Err(vec![Error::NotFoundCtor {
                region,
                prefix: None,
                name,
                suggestions: self.possible_ctor_names(bump),
            }]),
        }
    }

    /// Look up a qualified constructor. Mirrors Elm's `Env.findCtorQual`.
    pub fn find_ctor_qual(
        &self,
        bump: &'a Bump,
        region: Region,
        prefix: &'a str,
        name: &'a str,
    ) -> Result<Ctor<'a>, Vec<Error<'a>>> {
        let info = self
            .q_ctors
            .get(prefix)
            .and_then(|m| m.get(name))
            .ok_or_else(|| {
                vec![Error::NotFoundCtor {
                    region,
                    prefix: Some(prefix),
                    name,
                    suggestions: self.possible_ctor_names(bump),
                }]
            })?;
        match info {
            Info::Specific(_, ctor) => Ok(*ctor),
            Info::Ambiguous(first, others) => Err(vec![Error::AmbiguousCtor {
                region,
                prefix: Some(prefix),
                name,
                first_module: *first,
                other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
            }]),
        }
    }

    /// Extend env with local bindings (clone-on-scope-extension).
    /// Shadows foreign imports silently.
    /// Errors on re-shadowing a local/top-level.
    pub fn add_locals(
        &self,
        bindings: &std::collections::BTreeMap<&'a str, Region>,
    ) -> Result<Env<'a>, Vec<Error<'a>>> {
        let mut new_env = self.clone();
        let mut errors = Vec::new();

        for (&name, &region) in bindings {
            match new_env.vars.get(name) {
                Some(Var::Local(original)) | Some(Var::TopLevel(original)) => {
                    errors.push(Error::Shadowing {
                        name,
                        original: *original,
                        new: region,
                    });
                }
                _ => {
                    new_env.vars.insert(name, Var::Local(region));
                }
            }
        }

        if errors.is_empty() {
            Ok(new_env)
        } else {
            Err(errors)
        }
    }

    /// Look up a binop by symbol. Mirrors Elm's `Env.findBinop`.
    pub fn find_binop(
        &self,
        bump: &'a Bump,
        region: Region,
        symbol: &'a str,
    ) -> Result<Binop<'a>, Vec<Error<'a>>> {
        match self.binops.get(symbol) {
            Some(Info::Specific(_, binop)) => Ok(*binop),
            Some(Info::Ambiguous(first, others)) => Err(vec![Error::AmbiguousBinop {
                region,
                name: symbol,
                first_module: *first,
                other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
            }]),
            None => Err(vec![Error::NotFoundBinop {
                region,
                name: symbol,
                available: self.available_binops(bump),
            }]),
        }
    }

    fn available_binops(&self, bump: &'a Bump) -> &'a [&'a str] {
        bump.alloc_slice_fill_iter(self.binops.keys().copied())
    }

    pub fn possible_var_names(&self, bump: &'a Bump) -> crate::error::PossibleNames<'a> {
        let locals = bump.alloc_slice_fill_iter(self.vars.keys().copied());
        let qualified = bump.alloc_slice_fill_iter(self.q_vars.iter().map(|(prefix, inner)| {
            let names = bump.alloc_slice_fill_iter(inner.keys().copied());
            (*prefix, names as &[&str])
        }));
        crate::error::PossibleNames { locals, qualified }
    }

    pub fn possible_type_names(&self, bump: &'a Bump) -> crate::error::PossibleNames<'a> {
        let locals = bump.alloc_slice_fill_iter(self.types.keys().copied());
        let qualified = bump.alloc_slice_fill_iter(self.q_types.iter().map(|(prefix, inner)| {
            let names = bump.alloc_slice_fill_iter(inner.keys().copied());
            (*prefix, names as &[&str])
        }));
        crate::error::PossibleNames { locals, qualified }
    }

    pub fn possible_ctor_names(&self, bump: &'a Bump) -> crate::error::PossibleNames<'a> {
        let locals = bump.alloc_slice_fill_iter(self.ctors.keys().copied());
        let qualified = bump.alloc_slice_fill_iter(self.q_ctors.iter().map(|(prefix, inner)| {
            let names = bump.alloc_slice_fill_iter(inner.keys().copied());
            (*prefix, names as &[&str])
        }));
        crate::error::PossibleNames { locals, qualified }
    }
}

// --- Merge helpers (Elm's mergeInfo) ---

pub fn merge_exposed<'a, T: Clone>(
    table: &mut Exposed<'a, T>,
    name: &'a str,
    home: ModuleName<'a>,
    value: T,
) {
    use std::collections::btree_map::Entry;
    match table.entry(name) {
        Entry::Vacant(e) => {
            e.insert(Info::Specific(home, value));
        }
        Entry::Occupied(mut e) => match e.get() {
            Info::Specific(existing, _) if existing.name != home.name => {
                let first = *existing;
                e.insert(Info::Ambiguous(first, vec![home]));
            }
            Info::Ambiguous(_, others) if !others.iter().any(|h| h.name == home.name) => {
                if let Info::Ambiguous(_, others) = e.get_mut() {
                    others.push(home);
                }
            }
            _ => {}
        },
    }
}

pub fn merge_qualified<'a, T: Clone>(
    table: &mut Qualified<'a, T>,
    prefix: &'a str,
    name: &'a str,
    home: ModuleName<'a>,
    value: T,
) {
    let inner = table.entry(prefix).or_default();
    merge_exposed(inner, name, home, value);
}
