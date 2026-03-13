mod error;
mod interface;
mod module;

pub use crate::error::Error;
pub use crate::interface::{Interface, InterfaceAlias, InterfaceUnion};
pub use crate::module::{Context, canonicalize};
