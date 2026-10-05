//! Lexical scopes and name resolution.

use std::collections::HashMap;

use crate::middle::ids::BindingId;

/// A stack of lexical scopes mapping names to bindings.
///
/// A declaration shadows any earlier binding of the same name, in the same
/// or an enclosing scope, from the point of declaration onward. Leaving a
/// scope makes its bindings unreachable and uncovers the shadowed ones.
#[derive(Debug, Default)]
pub(super) struct Scopes {
    stack: Vec<HashMap<String, BindingId>>,
}

impl Scopes {
    /// Enters a new innermost scope.
    pub(super) fn push(&mut self) {
        self.stack.push(HashMap::new());
    }

    /// Leaves the innermost scope.
    pub(super) fn pop(&mut self) {
        self.stack.pop();
    }

    /// Binds `name` in the innermost scope.
    pub(super) fn insert(&mut self, name: String, binding: BindingId) {
        if let Some(scope) = self.stack.last_mut() {
            scope.insert(name, binding);
        }
    }

    /// Resolves `name` to the innermost visible binding.
    pub(super) fn lookup(&self, name: &str) -> Option<BindingId> {
        self.stack
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }
}
