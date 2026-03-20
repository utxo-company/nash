pub mod environment;
mod error;
mod interface;
mod module;

pub use crate::error::{BadArityContext, Error, VarKind};
pub use crate::interface::{
    AliasVisibility, Interface, InterfaceAlias, InterfaceBinop, InterfaceUnion, InterfaceValue,
    UnionVisibility, from_module,
};
pub use crate::module::{Context, canonicalize};
