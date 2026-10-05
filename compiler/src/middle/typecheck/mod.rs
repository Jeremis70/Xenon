//! Type checking: resolves names and types in the AST and produces THIR.
//!
//! The checker runs in two passes. The first collects every function
//! signature, so calls may refer to functions defined later and duplicate
//! definitions are diagnosed deterministically. The second checks each body
//! with a lexical scope stack and builds its [`thir::Function`].
//!
//! The typing rules, in brief (see `docs/internals/thir.md` for details):
//!
//! - Untyped literals take their type from context: the other operand of a
//!   binary operator, the declared type of a variable, a parameter, the
//!   return type, ... Without context they default to `i64` / `f64`. Every
//!   typed integer literal is range-checked against its type.
//! - Arithmetic and bitwise operators require operands of one type.
//! - Comparisons accept integers of different types: the narrower operand
//!   is widened to the wider type (the unsigned one on a tie) by an explicit
//!   cast. Floats compare likewise.
//! - A variable of type `&T` denotes the `T` it refers to, both when read
//!   and when assigned to.
//! - `@place` builds a `&T` where a reference is expected, a `*T` otherwise.
//!
//! Module map:
//! - this module: the driver and the per-function context;
//! - `scope`: lexical scopes and name resolution;
//! - `literal`: classification of untyped literal expressions;
//! - `expr`: expressions and places;
//! - `operator`: unary, binary, and compound-assignment operators;
//! - `stmt`: statements, blocks, and loops.

mod expr;
mod literal;
mod operator;
mod scope;
mod stmt;

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast;
use crate::index::{Idx, IndexVec};
use crate::middle::ids::{BindingId, DefId};
use crate::middle::target::TargetSpec;
use crate::middle::thir::{self, BindingKind, ThirProgram};
use crate::source::Span;
use crate::types::Type;

use scope::Scopes;

/// Type-checks `program` for `target` and returns its THIR.
///
/// `target` resolves the width of `usize`, `isize`, and pointers, which
/// literal range checks depend on.
pub fn check_program(program: &ast::Program, target: &TargetSpec) -> SemanticResult<ThirProgram> {
    let globals = GlobalCtxt::collect(program, target)?;
    let mut functions = IndexVec::with_capacity(program.functions.len());
    for function in &program.functions {
        functions.push(FnCtxt::new(&globals, function).check_function(function)?);
    }
    let entry = program
        .functions
        .iter()
        .position(|function| function.attributes.iter().any(|attr| attr.name == "entry"))
        .map(DefId::new);
    Ok(ThirProgram { functions, entry })
}

/// The signature of a function, as seen by callers.
#[derive(Debug)]
struct Signature {
    params: Vec<Type>,
    return_ty: Type,
}

/// Program-wide facts shared by every function check.
#[derive(Debug)]
struct GlobalCtxt<'a> {
    target: &'a TargetSpec,
    signatures: IndexVec<DefId, Signature>,
    by_name: HashMap<&'a str, DefId>,
}

impl<'a> GlobalCtxt<'a> {
    /// Assigns a [`DefId`] to every function, in source order.
    fn collect(program: &'a ast::Program, target: &'a TargetSpec) -> SemanticResult<Self> {
        let mut signatures: IndexVec<DefId, Signature> =
            IndexVec::with_capacity(program.functions.len());
        let mut by_name = HashMap::with_capacity(program.functions.len());
        for function in &program.functions {
            let def_id = signatures.push(Signature {
                params: function
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
                return_ty: function.return_type.ty.clone(),
            });
            match by_name.entry(function.name.as_str()) {
                Entry::Vacant(entry) => {
                    entry.insert(def_id);
                }
                Entry::Occupied(entry) => {
                    let first: &ast::Function = &program.functions[entry.get().index()];
                    return Err(SemanticError::DuplicateFunction {
                        name: function.name.clone(),
                        first_span: first.span,
                        span: function.span,
                    });
                }
            }
        }
        Ok(Self {
            target,
            signatures,
            by_name,
        })
    }
}

/// The innermost-first stack entry of an enclosing loop.
#[derive(Debug)]
struct LoopFrame {
    /// The type the loop's context expects, if any.
    contextual: Option<Type>,
    /// The result type fixed by the first `break`.
    break_ty: Option<Type>,
}

/// The state of checking one function body.
#[derive(Debug)]
struct FnCtxt<'g, 'a> {
    globals: &'g GlobalCtxt<'a>,
    return_ty: Type,
    bindings: IndexVec<BindingId, thir::Binding>,
    scopes: Scopes,
    loops: Vec<LoopFrame>,
}

impl<'g, 'a> FnCtxt<'g, 'a> {
    fn new(globals: &'g GlobalCtxt<'a>, function: &ast::Function) -> Self {
        Self {
            globals,
            return_ty: function.return_type.ty.clone(),
            bindings: IndexVec::new(),
            scopes: Scopes::default(),
            loops: Vec::new(),
        }
    }

    fn target(&self) -> &TargetSpec {
        self.globals.target
    }

    fn check_function(mut self, function: &ast::Function) -> SemanticResult<thir::Function> {
        self.scopes.push();
        let params = function
            .params
            .iter()
            .map(|param| self.declare(param, BindingKind::Param))
            .collect();
        let named_return = function
            .return_type
            .name
            .is_some()
            .then(|| self.declare(&function.return_type, BindingKind::NamedReturn));
        let body = self.check_block(&function.body, function.span)?;
        self.scopes.pop();

        Ok(thir::Function {
            name: function.name.clone(),
            params,
            return_ty: self.return_ty,
            named_return,
            bindings: self.bindings,
            body,
            span: function.span,
        })
    }

    /// Creates a binding for `binding` and brings it into scope.
    fn declare(&mut self, binding: &ast::Binding, kind: BindingKind) -> BindingId {
        let id = self.bindings.push(thir::Binding {
            name: binding.name.clone(),
            ty: binding.ty.clone(),
            kind,
            span: binding.span,
        });
        if let Some(name) = &binding.name {
            self.scopes.insert(name.clone(), id);
        }
        id
    }

    /// Checks that `expr` has type `expected`.
    ///
    /// Literals already took their type from the expectation when they were
    /// checked, so no conversion happens here.
    fn coerce(&self, expr: thir::Expr, expected: &Type) -> SemanticResult<thir::Expr> {
        if &expr.ty == expected {
            Ok(expr)
        } else {
            Err(SemanticError::TypeMismatch {
                expected: expected.to_string(),
                found: expr.ty.to_string(),
                span: expr.span,
            })
        }
    }

    /// The span covering the statements of `block`, or `fallback` if it is
    /// empty. AST blocks do not record their own braces.
    fn block_span(block: &[ast::Stmt], fallback: Span) -> Span {
        match (block.first(), block.last()) {
            (Some(first), Some(last)) => first.span.to(last.span),
            _ => fallback,
        }
    }
}
