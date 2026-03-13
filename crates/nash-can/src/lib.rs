mod error;
mod interface;
mod module;

pub use crate::error::{BadArityContext, Error, VarKind};
pub use crate::interface::{Interface, InterfaceAlias, InterfaceUnion};
pub use crate::module::{Context, canonicalize};
