//! Identifiers for program-level definitions.
//!
//! These indices are shared by every middle-end IR (THIR, MIR) so that a
//! definition keeps the same identity as it is lowered from one IR to the
//! next, and so that backends can refer to functions without knowing names.

use crate::index::newtype_index;

newtype_index! {
    /// Identifies a function definition within a compiled program.
    pub struct DefId = "fn{}";
}
