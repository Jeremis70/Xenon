//! Identifiers for definitions and bindings.
//!
//! These indices are shared by every middle-end IR (THIR, MIR) so that an
//! entity keeps the same identity as it is lowered from one IR to the next,
//! and so that later stages never resolve anything by name again.

use crate::index::newtype_index;

newtype_index! {
    /// Identifies a function definition within a compiled program.
    pub struct DefId = "fn{}";
}

newtype_index! {
    /// Identifies a variable binding (parameter, named return, or `let`)
    /// within one function.
    ///
    /// Every declaration gets a fresh id, so shadowed variables that share a
    /// name remain distinct.
    pub struct BindingId = "b{}";
}
