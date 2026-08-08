//! The entry point of nash's type inference: a port of Elm's `Type.Solve`,
//! `Type.Unify`, and `Type.Occurs`, plus the `toAnnotation`/`toErrorType`
//! half of `Type.Type`.
//!
//! The driver runs `nash_constrain::constrain` to build a constraint tree
//! (filling a `UnionFind` store with fresh variables), then [`run`] to solve
//! it. On success the result is the solver's annotation per top-level value
//! — exactly the map `nash_can::from_module` needs to build an interface.

mod annotation;
mod occurs;
mod solve;
mod unify;

pub use crate::annotation::{to_annotation, to_error_type};
pub use crate::solve::run;
pub use crate::unify::{Answer, unify};
