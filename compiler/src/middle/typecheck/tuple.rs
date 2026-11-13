//! Checking of tuples: tuple expressions, element access, destructuring
//! `let`, parallel assignment, and bare `return`.
//!
//! Destructuring and parallel assignment are desugared here into ordinary
//! `Let`, `Assign`, and `Expr` statements over unnamed temporaries, so later
//! stages only ever see tuple values, element places, and plain statements.
//!
//! Evaluation order:
//!
//! - `let i32 a, i32 b = e1, e2;` evaluates `e1`, then `e2`, and only then
//!   declares `a` and `b`, so both initializers see the bindings they
//!   would shadow (`let i32 x, i32 y = y, x;` swaps into new variables).
//! - `a, b = e1, e2;` evaluates every value into a temporary first, left to
//!   right, then stores into each target, left to right. This is what makes
//!   `a, b = b, a` swap.

use std::collections::HashMap;

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast::{self, ExprKind as Ast, Pattern};
use crate::middle::ids::BindingId;
use crate::middle::thir::{BindingKind, Expr, ExprKind, Stmt, StmtKind};
use crate::source::Span;
use crate::types::Type;

use super::{Expectation, FnCtxt};

/// The destination of one value of a destructuring `let`.
enum Pending<'p> {
    /// A user variable, declared once every value has been evaluated.
    User(&'p ast::Binding),
    /// A temporary holding a whole tuple that is destructured further.
    Temp(BindingId),
    /// A value matched by `_`, evaluated only for its side effects.
    Discard,
}

/// The values of a destructuring `let`, in evaluation order.
type Entries<'p> = Vec<(Pending<'p>, Expr)>;

/// A checked target of a parallel assignment.
enum Target {
    /// A place to store into.
    Place(Expr),
    /// `_`: the value is evaluated and dropped.
    Discard,
    /// A nested list of targets.
    Tuple(Vec<Target>),
}

impl FnCtxt<'_, '_> {
    /// Checks a tuple expression. Each element takes its expected type from
    /// `expected` when that is a tuple of the same arity.
    pub(super) fn check_tuple(
        &mut self,
        elems: &[ast::Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> SemanticResult<Expr> {
        let expected = expected
            .and_then(Type::tuple_elems)
            .filter(|tys| tys.len() == elems.len());
        let elems = elems
            .iter()
            .enumerate()
            .map(|(index, elem)| self.check_expr(elem, expected.map(|tys| &tys[index])))
            .collect::<SemanticResult<Vec<_>>>()?;
        Ok(tuple_expr(elems, span))
    }

    /// Checks `base.index`, a place if `base` is one.
    ///
    /// An element of type `&T` denotes the `T` it refers to, exactly like a
    /// variable of type `&T`. Pointers are not dereferenced implicitly:
    /// `p.0` on a `*(i32, i32)` is an error; write `(*p).0`.
    pub(super) fn check_field(
        &mut self,
        base: &ast::Expr,
        index: u32,
        span: Span,
    ) -> SemanticResult<Expr> {
        let base = if base.is_place() {
            self.check_place(base)?
        } else {
            self.check_expr(base, None)?
        };
        Ok(auto_deref(field(base, index, span)?))
    }

    /// Checks `let <pattern> = <init>;`, appending the declarations it
    /// becomes to `out`.
    pub(super) fn check_destructure(
        &mut self,
        pattern: &Pattern,
        init: &ast::Expr,
        span: Span,
        out: &mut Vec<Stmt>,
    ) -> SemanticResult<()> {
        check_unique_names(pattern, &mut HashMap::new())?;
        let mut entries = Vec::new();
        self.destructure_init(pattern, init, &mut entries)?;
        // Every value has been checked, so names are only declared now.
        for (pending, init) in entries {
            let kind = match pending {
                Pending::User(binding) => StmtKind::Let {
                    binding: self.declare(binding, BindingKind::Local),
                    init: Some(init),
                },
                Pending::Temp(binding) => StmtKind::Let {
                    binding,
                    init: Some(init),
                },
                Pending::Discard => StmtKind::Expr(init),
            };
            out.push(Stmt { kind, span });
        }
        Ok(())
    }

    /// Matches `pattern` against the unchecked initializer `init`. A tuple
    /// literal is matched element by element, without building the tuple,
    /// so that each element takes the type of its pattern.
    fn destructure_init<'p>(
        &mut self,
        pattern: &'p Pattern,
        init: &ast::Expr,
        entries: &mut Entries<'p>,
    ) -> SemanticResult<()> {
        match (pattern, &init.kind) {
            (Pattern::Tuple(patterns, _), Ast::Tuple(elems)) => {
                if patterns.len() != elems.len() {
                    return Err(SemanticError::TupleArityMismatch {
                        expected: patterns.len(),
                        found: elems.len(),
                        span: init.span,
                    });
                }
                for (pattern, elem) in patterns.iter().zip(elems) {
                    self.destructure_init(pattern, elem, entries)?;
                }
                Ok(())
            }
            (Pattern::Binding(binding), _) => {
                let init = self.check_initializer(binding, init)?;
                entries.push((Pending::User(binding), init));
                Ok(())
            }
            (Pattern::Wildcard(_), _) => {
                let init = self.check_expr(init, None)?;
                entries.push((Pending::Discard, init));
                Ok(())
            }
            (Pattern::Tuple(..), _) => {
                let value = self.check_expr_expecting(init, &pattern_expectation(pattern))?;
                // A pattern of only `_` reads no element of a variable, so
                // read it whole, once: `let _, _ = r;` must not hide that
                // `r` may be uninitialized, just as `let _ = r;` does not.
                if is_stable(&value) && !binds_any(pattern) {
                    entries.push((Pending::Discard, value.clone()));
                }
                self.destructure_value(pattern, value, entries)
            }
        }
    }

    /// Matches `pattern` against the checked value `value`.
    fn destructure_value<'p>(
        &mut self,
        pattern: &'p Pattern,
        value: Expr,
        entries: &mut Entries<'p>,
    ) -> SemanticResult<()> {
        match pattern {
            Pattern::Binding(binding) => {
                require_type(&binding.ty, &value.ty, value.span)?;
                entries.push((Pending::User(binding), value));
            }
            // Only reached for elements of a stored tuple, whose reads have
            // no side effects.
            Pattern::Wildcard(_) => {}
            Pattern::Tuple(patterns, _) => {
                require_arity(patterns.len(), &value)?;
                let base = if is_stable(&value) {
                    value
                } else {
                    let temp = self.declare_temp(value.ty.clone(), value.span);
                    let base = binding_expr(temp, value.ty.clone(), value.span);
                    entries.push((Pending::Temp(temp), value));
                    base
                };
                for (index, pattern) in patterns.iter().enumerate() {
                    let elem = field(base.clone(), index as u32, base.span)?;
                    self.destructure_value(pattern, elem, entries)?;
                }
            }
        }
        Ok(())
    }

    /// Checks the parallel assignment `<target> = <value>;`, where `target`
    /// is `_` or a tuple of targets, appending the statements it becomes to
    /// `out`.
    pub(super) fn check_parallel_assign(
        &mut self,
        target: &ast::Expr,
        value: &ast::Expr,
        span: Span,
        out: &mut Vec<Stmt>,
    ) -> SemanticResult<()> {
        let target = self.check_target(target)?;
        self.check_distinct_targets(&target, &mut Vec::new())?;
        let mut stores = Vec::new();
        self.assign_init(target, value, out, &mut stores)?;
        out.extend(stores.into_iter().map(|(place, value)| Stmt {
            kind: StmtKind::Assign { place, value },
            span,
        }));
        Ok(())
    }

    fn check_target(&mut self, target: &ast::Expr) -> SemanticResult<Target> {
        match &target.kind {
            Ast::Wildcard => Ok(Target::Discard),
            Ast::Tuple(elems) => Ok(Target::Tuple(
                elems
                    .iter()
                    .map(|elem| self.check_target(elem))
                    .collect::<SemanticResult<_>>()?,
            )),
            _ => Ok(Target::Place(self.check_place(target)?)),
        }
    }

    /// Rejects a target assigned twice, or a target and an element of it,
    /// as in `a, a = ..` or `t, t.0 = ..`. Targets reached through a
    /// pointer are not compared: their addresses are only known at run
    /// time.
    fn check_distinct_targets(
        &self,
        target: &Target,
        seen: &mut Vec<(BindingId, Vec<u32>)>,
    ) -> SemanticResult<()> {
        match target {
            Target::Discard => Ok(()),
            Target::Tuple(targets) => targets
                .iter()
                .try_for_each(|target| self.check_distinct_targets(target, seen)),
            Target::Place(place) => {
                let Some((binding, path)) = static_path(place) else {
                    return Ok(());
                };
                let overlaps = seen.iter().any(|(other, other_path)| {
                    *other == binding
                        && (other_path.starts_with(&path) || path.starts_with(other_path))
                });
                if overlaps {
                    let name = self.bindings[binding].name.as_deref().unwrap_or("_");
                    let path: String = path.iter().map(|index| format!(".{index}")).collect();
                    return Err(SemanticError::DuplicateAssignTarget {
                        target: format!("{name}{path}"),
                        span: place.span,
                    });
                }
                seen.push((binding, path));
                Ok(())
            }
        }
    }

    /// Evaluates the unchecked `value` for `target` into temporaries,
    /// appending the evaluation to `out` and the stores to `stores`.
    fn assign_init(
        &mut self,
        target: Target,
        value: &ast::Expr,
        out: &mut Vec<Stmt>,
        stores: &mut Vec<(Expr, Expr)>,
    ) -> SemanticResult<()> {
        let span = value.span;
        match (target, &value.kind) {
            (Target::Tuple(targets), Ast::Tuple(elems)) => {
                if targets.len() != elems.len() {
                    return Err(SemanticError::TupleArityMismatch {
                        expected: targets.len(),
                        found: elems.len(),
                        span,
                    });
                }
                for (target, elem) in targets.into_iter().zip(elems) {
                    self.assign_init(target, elem, out, stores)?;
                }
            }
            (Target::Discard, _) => {
                let value = self.check_expr(value, None)?;
                out.push(Stmt {
                    kind: StmtKind::Expr(value),
                    span,
                });
            }
            (Target::Place(place), _) => {
                let value = self.check_expr_has_type(value, &place.ty)?;
                let temp = self.store_temp(value, out);
                stores.push((place, temp));
            }
            (target @ Target::Tuple(_), _) => {
                let value = self.check_expr_expecting(value, &target_expectation(&target))?;
                let temp = self.store_temp(value, out);
                self.assign_value(target, temp, stores)?;
            }
        }
        Ok(())
    }

    /// Matches `target` against the stored value `value`.
    fn assign_value(
        &self,
        target: Target,
        value: Expr,
        stores: &mut Vec<(Expr, Expr)>,
    ) -> SemanticResult<()> {
        match target {
            Target::Discard => {}
            Target::Place(place) => {
                require_type(&place.ty, &value.ty, value.span)?;
                stores.push((place, value));
            }
            Target::Tuple(targets) => {
                require_arity(targets.len(), &value)?;
                for (index, target) in targets.into_iter().enumerate() {
                    let elem = field(value.clone(), index as u32, value.span)?;
                    self.assign_value(target, elem, stores)?;
                }
            }
        }
        Ok(())
    }

    /// Stores `value` in a new temporary and returns a read of it.
    fn store_temp(&mut self, value: Expr, out: &mut Vec<Stmt>) -> Expr {
        let (ty, span) = (value.ty.clone(), value.span);
        let temp = self.declare_temp(ty.clone(), span);
        out.push(Stmt {
            kind: StmtKind::Let {
                binding: temp,
                init: Some(value),
            },
            span,
        });
        binding_expr(temp, ty, span)
    }

    /// The value of a bare `return;`: the named results, as a tuple if
    /// there are several, or `()` in a function returning `()`.
    ///
    /// A named result shadowed at the `return` is an error: the reader sees
    /// the inner variable, but the outer one would be returned.
    pub(super) fn bare_return_value(&self, span: Span) -> SemanticResult<Expr> {
        if self.named_returns.is_empty() {
            if self.return_ty.is_unit() {
                return Ok(unit(span));
            }
            return Err(SemanticError::BareReturnWithoutNamedResults {
                ty: self.return_ty.clone(),
                span,
            });
        }
        let mut values = Vec::with_capacity(self.named_returns.len());
        for &result in &self.named_returns {
            let binding = &self.bindings[result];
            if let Some(name) = &binding.name
                && let Some(visible) = self.scopes.lookup(name)
                && visible != result
            {
                return Err(SemanticError::ShadowedNamedResult {
                    name: name.clone(),
                    shadow_span: self.bindings[visible].span,
                    span,
                });
            }
            values.push(binding_expr(result, binding.ty.clone(), span));
        }
        if values.len() == 1 {
            return Ok(values.remove(0));
        }
        Ok(Expr {
            ty: self.return_ty.clone(),
            kind: ExprKind::Tuple(values),
            span,
        })
    }
}

/// The tuple of `elems`, typed by their types.
pub(super) fn tuple_expr(elems: Vec<Expr>, span: Span) -> Expr {
    Expr {
        ty: Type::Tuple(elems.iter().map(|elem| elem.ty.clone()).collect()),
        kind: ExprKind::Tuple(elems),
        span,
    }
}

/// The unit value `()`.
pub(super) fn unit(span: Span) -> Expr {
    Expr {
        kind: ExprKind::Tuple(Vec::new()),
        ty: Type::unit(),
        span,
    }
}

/// `base.index`, checking that `base` is a tuple with that element.
fn field(base: Expr, index: u32, span: Span) -> SemanticResult<Expr> {
    let Some(elems) = base.ty.tuple_elems() else {
        return Err(SemanticError::NotATuple {
            index,
            found: base.ty.to_string(),
            span,
        });
    };
    let ty =
        elems
            .get(index as usize)
            .cloned()
            .ok_or_else(|| SemanticError::TupleIndexOutOfRange {
                index,
                ty: base.ty.clone(),
                span,
            })?;
    Ok(Expr {
        kind: ExprKind::Field {
            base: Box::new(base),
            index,
        },
        ty,
        span,
    })
}

/// Wraps an expression of type `&T` in the dereference that denotes its `T`.
fn auto_deref(expr: Expr) -> Expr {
    match &expr.ty {
        Type::Reference(pointee) => Expr {
            ty: (**pointee).clone(),
            span: expr.span,
            kind: ExprKind::Deref(Box::new(expr)),
        },
        _ => expr,
    }
}

fn binding_expr(binding: BindingId, ty: Type, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Binding(binding),
        ty,
        span,
    }
}

/// Whether reading elements of `expr` repeatedly has no side effects and
/// always sees the same value: a variable or an element of one.
fn is_stable(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Binding(_) => true,
        ExprKind::Field { base, .. } => is_stable(base),
        _ => false,
    }
}

/// The variable and element path of a place not reached through a pointer.
fn static_path(place: &Expr) -> Option<(BindingId, Vec<u32>)> {
    match &place.kind {
        ExprKind::Binding(binding) => Some((*binding, Vec::new())),
        ExprKind::Field { base, index } => {
            let (binding, mut path) = static_path(base)?;
            path.push(*index);
            Some((binding, path))
        }
        _ => None,
    }
}

/// What a pattern expects of its value; `_` leaves its part unknown.
fn pattern_expectation(pattern: &Pattern) -> Expectation {
    match pattern {
        Pattern::Binding(binding) => Expectation::Type(binding.ty.clone()),
        Pattern::Wildcard(_) => Expectation::Unknown,
        Pattern::Tuple(patterns, _) => {
            Expectation::Tuple(patterns.iter().map(pattern_expectation).collect())
        }
    }
}

/// What a target expects of its value; `_` leaves its part unknown.
fn target_expectation(target: &Target) -> Expectation {
    match target {
        Target::Place(place) => Expectation::Type(place.ty.clone()),
        Target::Discard => Expectation::Unknown,
        Target::Tuple(targets) => {
            Expectation::Tuple(targets.iter().map(target_expectation).collect())
        }
    }
}

/// Whether `pattern` binds at least one variable.
fn binds_any(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Binding(_) => true,
        Pattern::Wildcard(_) => false,
        Pattern::Tuple(patterns, _) => patterns.iter().any(binds_any),
    }
}

/// Rejects a name bound twice in one pattern.
fn check_unique_names<'p>(
    pattern: &'p Pattern,
    seen: &mut HashMap<&'p str, Span>,
) -> SemanticResult<()> {
    match pattern {
        Pattern::Binding(binding) => {
            if let Some(name) = &binding.name
                && seen.insert(name, binding.span).is_some()
            {
                return Err(SemanticError::DuplicatePatternBinding {
                    name: name.clone(),
                    span: binding.span,
                });
            }
            Ok(())
        }
        Pattern::Wildcard(_) => Ok(()),
        Pattern::Tuple(patterns, _) => patterns
            .iter()
            .try_for_each(|pattern| check_unique_names(pattern, seen)),
    }
}

fn require_type(expected: &Type, found: &Type, span: Span) -> SemanticResult<()> {
    if expected == found {
        return Ok(());
    }
    Err(SemanticError::TypeMismatch {
        expected: expected.to_string(),
        found: found.to_string(),
        span,
    })
}

/// Requires `value` to be a tuple of `arity` elements.
fn require_arity(arity: usize, value: &Expr) -> SemanticResult<()> {
    match value.ty.tuple_elems() {
        Some(elems) if elems.len() == arity => Ok(()),
        Some(elems) => Err(SemanticError::TupleArityMismatch {
            expected: arity,
            found: elems.len(),
            span: value.span,
        }),
        None => Err(SemanticError::TypeMismatch {
            expected: format!("a tuple of {arity} values"),
            found: value.ty.to_string(),
            span: value.span,
        }),
    }
}
