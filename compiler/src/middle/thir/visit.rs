//! Read-only traversal of THIR.
//!
//! Implement [`Visitor`] and override the `visit_*` methods for the nodes of
//! interest; call the matching `walk_*` function to continue into children.
//! New THIR nodes only need to be added to the `walk_*` functions for every
//! existing visitor to traverse them.

use super::{Block, Expr, ExprKind, Function, Stmt, StmtKind};

/// A read-only THIR visitor.
pub trait Visitor<'thir>: Sized {
    /// Visits a function.
    fn visit_function(&mut self, function: &'thir Function) {
        walk_function(self, function);
    }

    /// Visits a block.
    fn visit_block(&mut self, block: &'thir Block) {
        walk_block(self, block);
    }

    /// Visits a statement.
    fn visit_stmt(&mut self, stmt: &'thir Stmt) {
        walk_stmt(self, stmt);
    }

    /// Visits an expression.
    fn visit_expr(&mut self, expr: &'thir Expr) {
        walk_expr(self, expr);
    }
}

/// Visits the body of `function`.
pub fn walk_function<'thir, V: Visitor<'thir>>(visitor: &mut V, function: &'thir Function) {
    visitor.visit_block(&function.body);
}

/// Visits every statement of `block`.
pub fn walk_block<'thir, V: Visitor<'thir>>(visitor: &mut V, block: &'thir Block) {
    for stmt in &block.stmts {
        visitor.visit_stmt(stmt);
    }
}

/// Visits the children of `stmt`, in evaluation order.
pub fn walk_stmt<'thir, V: Visitor<'thir>>(visitor: &mut V, stmt: &'thir Stmt) {
    match &stmt.kind {
        StmtKind::Let { init, .. } => {
            if let Some(init) = init {
                visitor.visit_expr(init);
            }
        }
        StmtKind::Expr(expr) | StmtKind::Return(expr) => visitor.visit_expr(expr),
        StmtKind::Assign { place, value } | StmtKind::CompoundAssign { place, value, .. } => {
            visitor.visit_expr(place);
            visitor.visit_expr(value);
        }
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            visitor.visit_expr(condition);
            visitor.visit_block(then_block);
            if let Some(else_block) = else_block {
                visitor.visit_block(else_block);
            }
        }
        StmtKind::Break(value) => {
            if let Some(value) = value {
                visitor.visit_expr(value);
            }
        }
        StmtKind::Continue => {}
    }
}

/// Visits the children of `expr`, in evaluation order.
///
/// For a loop with a post-condition, the body is visited before the
/// condition.
pub fn walk_expr<'thir, V: Visitor<'thir>>(visitor: &mut V, expr: &'thir Expr) {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Binding(_) => {}
        ExprKind::Deref(operand)
        | ExprKind::Unary { operand, .. }
        | ExprKind::Cast { operand, .. }
        | ExprKind::AddressOf { place: operand, .. } => visitor.visit_expr(operand),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Logical { lhs, rhs, .. } => {
            visitor.visit_expr(lhs);
            visitor.visit_expr(rhs);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                visitor.visit_expr(arg);
            }
        }
        ExprKind::If {
            condition,
            then_expr,
            else_expr,
        } => {
            visitor.visit_expr(condition);
            visitor.visit_expr(then_expr);
            visitor.visit_expr(else_expr);
        }
        ExprKind::Loop { condition, body } => match condition {
            Some(condition) if condition.placement == super::ConditionPlacement::Before => {
                visitor.visit_expr(&condition.expr);
                visitor.visit_block(body);
            }
            Some(condition) => {
                visitor.visit_block(body);
                visitor.visit_expr(&condition.expr);
            }
            None => visitor.visit_block(body),
        },
    }
}
