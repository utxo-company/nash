pub mod dups;
pub mod foreign;
pub mod local;

use std::collections::BTreeMap;

use nash_ast::{Associativity, CtorOpts, ModuleName, Precedence, Type as CanType};
use nash_region::{Located, Region};

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
        index: u16,
        arity: u16,
        arguments: &'a [&'a Located<CanType<'a>>],
        options: CtorOpts,
        alternatives: u16,
    },
    /// Record alias ctor (e.g., `Point` from `type alias Point = { x : Int, y : Int }`).
    /// Elm creates these automatically for non-extensible record aliases.
    RecordCtor {
        home: ModuleName<'a>,
        field_names: &'a [&'a str],
        field_types: &'a [&'a Located<CanType<'a>>],
    },
}

/// A binary operator in scope.
#[derive(Clone, Copy, Debug)]
pub struct Binop<'a> {
    pub home: ModuleName<'a>,
    pub function: &'a str,
    pub associativity: Associativity,
    pub precedence: Precedence,
}

/// The canonicalization environment.
///
/// Built from imports (foreign) then augmented with local definitions.
/// Consumed by type, pattern, and expression canonicalization.
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
        Entry::Occupied(mut e) => {
            let make_ambiguous = match e.get() {
                Info::Specific(existing, _) if existing.name != home.name => Some(*existing),
                _ => None,
            };

            if let Some(first) = make_ambiguous {
                e.insert(Info::Ambiguous(first, vec![home]));
            } else {
                match e.get_mut() {
                    Info::Ambiguous(_, others) if !others.iter().any(|h| h.name == home.name) => {
                        others.push(home);
                    }
                    _ => {}
                }
            }
        }
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
