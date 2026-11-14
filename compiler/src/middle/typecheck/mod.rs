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
//! - Comparisons widen integers of the same signedness to the wider type.
//!   Mixed-sign operands widen to a signed type that holds both ranges:
//!   at least one bit wider than the unsigned operand. Floats compare at
//!   the wider width.
//! - Every other conversion is an explicit `x as T`, which is exact or
//!   traps (see [`crate::middle::cast`]).
//! - A variable of type `&T` denotes the `T` it refers to, both when read
//!   and when assigned to.
//! - `@place` builds a `&T` where a reference is expected, a `*T` otherwise.
//!
//! Module map:
//! - this module: the driver, item collection (signatures, attributes, the
//!   entry point), and the per-function context;
//! - `cast`: explicit conversions `x as T`;
//! - `entry`: validation of the executable entry point on THIR;
//! - `scope`: lexical scopes and name resolution;
//! - `literal`: classification of untyped literal expressions;
//! - `expr`: expressions and places;
//! - `operator`: unary, binary, and compound-assignment operators;
//! - `stmt`: statements, blocks, and loops;
//! - `tuple`: tuple expressions, element access, destructuring `let`,
//!   parallel assignment, and bare `return`.

mod cast;
mod entry;
mod expr;
mod items;
mod literal;
mod operator;
mod scope;
mod stmt;
mod structs;
mod tuple;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast;
use crate::index::{Idx, IndexVec};
use crate::middle::ids::{BindingId, DefId};
use crate::middle::target::TargetSpec;
use crate::middle::thir::{self, BindingKind, ThirProgram};
use crate::source::Span;
use crate::types::Type;

pub use entry::{is_entry_return_type, validate_entry_point};
use items::StructItems;
use scope::Scopes;

/// Type-checks `program` for `target` and returns its THIR.
///
/// `target` resolves the width of `usize`, `isize`, and pointers, which
/// literal range checks depend on.
pub fn check_program(program: &ast::Program, target: &TargetSpec) -> SemanticResult<ThirProgram> {
    let globals = GlobalCtxt::collect(program, target)?;
    let mut functions = IndexVec::with_capacity(program.functions.len());
    for (index, function) in program.functions.iter().enumerate() {
        let def_id = DefId::new(index);
        functions.push(FnCtxt::new(&globals, def_id).check_function(function)?);
    }
    Ok(ThirProgram {
        functions,
        entry: globals.entry,
        structs: Arc::new(globals.structs.table),
    })
}

/// The attribute marking the program entry point.
const ENTRY_ATTRIBUTE: &str = "entry";

/// The signature of a function, as seen by callers.
#[derive(Debug)]
struct Signature {
    params: Vec<Type>,
    /// The types after `->`, in order.
    results: Vec<Type>,
    /// `()` without results, the type of a single result, or the tuple of
    /// several results' types.
    return_ty: Type,
}

/// Program-wide facts shared by every function check.
#[derive(Debug)]
struct GlobalCtxt<'a> {
    target: &'a TargetSpec,
    signatures: IndexVec<DefId, Signature>,
    by_name: HashMap<&'a str, DefId>,
    /// The function marked `#[entry]`, if any.
    entry: Option<DefId>,
    structs: StructItems<'a>,
}

impl<'a> GlobalCtxt<'a> {
    /// Assigns a [`DefId`] to every function, in source order, and checks
    /// its attributes.
    fn collect(program: &'a ast::Program, target: &'a TargetSpec) -> SemanticResult<Self> {
        let mut signatures: IndexVec<DefId, Signature> =
            IndexVec::with_capacity(program.functions.len());
        let mut by_name = HashMap::with_capacity(program.functions.len());
        let mut entry: Option<DefId> = None;
        let structs = StructItems::collect(program)?;
        for function in &program.functions {
            // A call `Name(..)` must mean one thing.
            if let Some(id) = structs.ids.get(function.name.as_str()) {
                let decl = &program.structs[id.as_u32() as usize];
                let (first, second) = if decl.span.start < function.span.start {
                    (decl.name_span, function.span)
                } else {
                    (function.span, decl.name_span)
                };
                return Err(SemanticError::NameConflict {
                    name: function.name.clone(),
                    first_span: first,
                    span: second,
                });
            }
            let resolve_all = |bindings: &[ast::Binding]| -> SemanticResult<Vec<Type>> {
                bindings.iter().map(|b| structs.resolve(&b.ty)).collect()
            };
            let results = resolve_all(&function.returns)?;
            let def_id = signatures.push(Signature {
                params: resolve_all(&function.params)?,
                return_ty: match results.as_slice() {
                    [] => Type::unit(),
                    [single] => single.clone(),
                    results => Type::Tuple(results.to_vec()),
                },
                results,
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
            for attribute in &function.attributes {
                if attribute.name != ENTRY_ATTRIBUTE {
                    return Err(SemanticError::UnknownAttribute {
                        name: attribute.name.clone(),
                        span: attribute.span,
                    });
                }
                if let Some(first) = entry
                    && first != def_id
                {
                    return Err(SemanticError::MultipleEntryPoints {
                        first_span: program.functions[first.index()].span,
                        span: function.span,
                    });
                }
                entry = Some(def_id);
            }
        }
        Ok(Self {
            target,
            signatures,
            by_name,
            entry,
            structs,
        })
    }

    /// Resolves a type written in the source to the type it names.
    fn resolve(&self, ty: &ast::TypeExpr) -> SemanticResult<Type> {
        self.structs.resolve(ty)
    }
}

/// What a context knows about the type of an expression it checks.
///
/// Unlike a plain `Option<&Type>`, it can describe a tuple whose elements
/// are only partly known, as for `let u8 a, _ = e;`: the first element is
/// expected to be `u8`, the second is unconstrained. Like any expectation,
/// it only guides context-dependent expressions such as literals; callers
/// still check the resulting type.
#[derive(Debug, Clone, PartialEq)]
enum Expectation {
    /// Nothing is known.
    Unknown,
    /// The whole type is known.
    Type(Type),
    /// A tuple of this arity, each element with its own expectation.
    Tuple(Vec<Expectation>),
}

impl Expectation {
    fn from_type(expected: Option<&Type>) -> Self {
        expected.map_or(Expectation::Unknown, |ty| Expectation::Type(ty.clone()))
    }

    /// The expected type, if every part of it is known.
    fn to_type(&self) -> Option<Type> {
        match self {
            Expectation::Unknown => None,
            Expectation::Type(ty) => Some(ty.clone()),
            Expectation::Tuple(elems) => elems
                .iter()
                .map(Expectation::to_type)
                .collect::<Option<_>>()
                .map(Type::Tuple),
        }
    }
}

/// The innermost-first stack entry of an enclosing loop.
#[derive(Debug)]
struct LoopFrame {
    /// What the loop's context expects of its value.
    contextual: Expectation,
    /// The result type fixed by the first `break`.
    break_ty: Option<Type>,
}

/// The state of checking one function body.
#[derive(Debug)]
struct FnCtxt<'g, 'a> {
    globals: &'g GlobalCtxt<'a>,
    /// The function being checked.
    def_id: DefId,
    return_ty: Type,
    /// The bindings of the named results, in order.
    named_returns: Vec<BindingId>,
    bindings: IndexVec<BindingId, thir::Binding>,
    scopes: Scopes,
    loops: Vec<LoopFrame>,
}

impl<'g, 'a> FnCtxt<'g, 'a> {
    fn new(globals: &'g GlobalCtxt<'a>, def_id: DefId) -> Self {
        Self {
            globals,
            def_id,
            return_ty: globals.signatures[def_id].return_ty.clone(),
            named_returns: Vec::new(),
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
        let globals = self.globals;
        let signature = &globals.signatures[self.def_id];
        let params = function
            .params
            .iter()
            .zip(&signature.params)
            .map(|(param, ty)| self.declare(param, ty.clone(), BindingKind::Param))
            .collect();
        let results = function.returns.iter().zip(&signature.results);
        for (result, ty) in results.filter(|(r, _)| r.name.is_some()) {
            // A reference can only be bound at its declaration, and a named
            // result has none: it could never refer to anything.
            if matches!(ty, Type::Reference(_)) {
                return Err(SemanticError::ReferenceNamedReturn {
                    ty: ty.clone(),
                    span: result.span,
                });
            }
            let id = self.declare(result, ty.clone(), BindingKind::NamedReturn);
            self.named_returns.push(id);
        }
        let body = self.check_block(&function.body, function.span)?;
        self.scopes.pop();

        Ok(thir::Function {
            name: function.name.clone(),
            params,
            return_ty: self.return_ty,
            named_returns: self.named_returns,
            bindings: self.bindings,
            body,
            span: function.span,
        })
    }

    /// Creates a binding for `binding`, whose resolved type is `ty`, and
    /// brings it into scope.
    fn declare(&mut self, binding: &ast::Binding, ty: Type, kind: BindingKind) -> BindingId {
        let id = self.bindings.push(thir::Binding {
            name: binding.name.clone(),
            ty,
            kind,
            span: binding.span,
        });
        if let Some(name) = &binding.name {
            self.scopes.insert(name.clone(), id);
        }
        id
    }

    /// Creates an unnamed binding of type `ty` for a compiler temporary.
    /// It is never in scope, so it cannot shadow anything.
    fn declare_temp(&mut self, ty: Type, span: Span) -> BindingId {
        self.bindings.push(thir::Binding {
            name: None,
            ty,
            kind: BindingKind::Local,
            span,
        })
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
