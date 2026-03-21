mod accumulate;
pub mod environment;
mod error;
mod interface;
mod module;
pub mod pattern;
mod scc;
pub mod types;

pub use crate::error::{BadArityContext, DuplicatePatternContext, Error, VarKind};
pub use crate::interface::{
    AliasVisibility, Interface, InterfaceAlias, InterfaceBinop, InterfaceUnion, InterfaceValue,
    UnionVisibility, from_module,
};
pub use crate::module::{Context, canonicalize};
