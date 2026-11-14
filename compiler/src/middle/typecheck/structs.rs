//! Checking of struct construction and member access.
//!
//! Construction and member lookup each go through one function here
//! ([`FnCtxt::check_construct`] and [`GlobalCtxt::lookup_member`]), so that
//! whatever later changes what `Rect(..)` or `r.name` may mean changes it
//! in one place.

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast;
use crate::middle::ops::IndirectionKind;
use crate::middle::thir::{Expr, ExprKind};
use crate::source::Span;
use crate::types::{StructId, StructTy, Type};

use super::items::{Member, ReceiverKind};
use super::{FnCtxt, GlobalCtxt};

impl GlobalCtxt<'_> {
    /// What `name` denotes on a value of struct `id`, if anything.
    pub(super) fn lookup_member(&self, id: StructId, name: &str) -> Option<&Member> {
        let (member, _) = self.structs.members.get(id)?.get(name)?;
        Some(member)
    }

    /// The struct named `name`, if the program defines one.
    pub(super) fn struct_named(&self, name: &str) -> Option<StructTy> {
        let id = *self.structs.ids.get(name)?;
        match self.structs.table.ty(id) {
            Type::Struct(struct_ty) => Some(struct_ty),
            _ => None,
        }
    }
}

impl FnCtxt<'_, '_> {
    /// Checks `Name(field = value, ..)`, which builds a struct from all of
    /// its fields, named in any order.
    ///
    /// The values are evaluated in the order they are written, whatever
    /// the order of the fields.
    pub(super) fn check_construct(
        &mut self,
        ty: StructTy,
        args: &[ast::NamedArg],
        span: Span,
    ) -> SemanticResult<Expr> {
        let globals = self.globals;
        let def = globals
            .structs
            .table
            .get(ty.id)
            .expect("a resolved struct type is defined");
        let mut given: Vec<Option<Span>> = vec![None; def.fields.len()];
        let mut fields = Vec::with_capacity(args.len());
        for arg in args {
            let Some(index) = def.fields.iter().position(|field| field.name == arg.name) else {
                return Err(SemanticError::UnknownField {
                    name: arg.name.clone(),
                    ty: ty.name.to_string(),
                    span: arg.name_span,
                });
            };
            if let Some(first_span) = given[index].replace(arg.name_span) {
                return Err(SemanticError::DuplicateFieldInit {
                    name: arg.name.clone(),
                    first_span,
                    span: arg.name_span,
                });
            }
            let value = self.check_expr_has_type(&arg.value, &def.fields[index].ty)?;
            fields.push((index as u32, value));
        }
        if let Some(missing) = given.iter().position(Option::is_none) {
            return Err(SemanticError::MissingField {
                name: def.fields[missing].name.clone(),
                ty: ty.name.to_string(),
                span,
            });
        }
        Ok(Expr {
            kind: ExprKind::Struct { fields },
            ty: Type::Struct(ty),
            span,
        })
    }

    /// Checks `base.name`, a place if `base` is one.
    ///
    /// Like tuple elements, members are not reached through a pointer:
    /// `p.width` on a `*Rect` is an error; write `(*p).width`.
    pub(super) fn check_member(
        &mut self,
        base: &ast::Expr,
        name: &str,
        span: Span,
    ) -> SemanticResult<Expr> {
        let globals = self.globals;
        if let Some(struct_ty) = self.named_struct(base) {
            // Only functions are reached through the name of a struct, and
            // a function is called.
            return Err(match globals.lookup_member(struct_ty.id, name) {
                Some(Member::AssocFn(_) | Member::Method { .. }) => {
                    not_called(name, &struct_ty, span)
                }
                _ => no_such_member(name, &struct_ty.name, span),
            });
        }
        let base = self.check_receiver(base)?;
        let Type::Struct(struct_ty) = &base.ty else {
            return Err(no_such_member(name, &base.ty.to_string(), span));
        };
        match globals.lookup_member(struct_ty.id, name) {
            Some(Member::Field(index)) => {
                let ty = globals
                    .structs
                    .table
                    .field_ty(&base.ty, *index)
                    .expect("a field member has a definition")
                    .clone();
                Ok(Expr {
                    kind: ExprKind::Field {
                        base: Box::new(base),
                        index: *index,
                    },
                    ty,
                    span,
                })
            }
            Some(Member::Method { .. } | Member::AssocFn(_)) => {
                Err(not_called(name, struct_ty, span))
            }
            None => Err(no_such_member(name, &struct_ty.name, span)),
        }
    }

    /// Checks `receiver.name(args)`: a method called on a value, or an
    /// associated function called on the name of its struct.
    ///
    /// A method is an ordinary function whose first argument is the value
    /// it is called on, or a reference to it for a `&self` method.
    pub(super) fn check_method_call(
        &mut self,
        receiver: &ast::Expr,
        name: &str,
        args: &[ast::Expr],
        span: Span,
    ) -> SemanticResult<Expr> {
        let globals = self.globals;
        if let Some(struct_ty) = self.named_struct(receiver) {
            return match globals.lookup_member(struct_ty.id, name) {
                Some(Member::AssocFn(def)) => self.call(*def, name, None, args, span),
                Some(Member::Method { .. }) => Err(SemanticError::MethodNeedsReceiver {
                    name: name.to_owned(),
                    ty: struct_ty.name.to_string(),
                    span,
                }),
                _ => Err(no_such_member(name, &struct_ty.name, span)),
            };
        }
        let receiver = self.check_receiver(receiver)?;
        let Type::Struct(struct_ty) = &receiver.ty else {
            return Err(no_such_member(name, &receiver.ty.to_string(), span));
        };
        match globals.lookup_member(struct_ty.id, name) {
            Some(Member::Method {
                def,
                receiver: kind,
            }) => {
                let receiver = match kind {
                    ReceiverKind::Value => receiver,
                    // The receiver need not be a place: MIR construction
                    // gives a value a temporary to refer to.
                    ReceiverKind::Reference => Expr {
                        ty: IndirectionKind::Reference.pointer_to(receiver.ty.clone()),
                        span: receiver.span,
                        kind: ExprKind::AddressOf {
                            kind: IndirectionKind::Reference,
                            place: Box::new(receiver),
                        },
                    },
                };
                self.call(*def, name, Some(receiver), args, span)
            }
            Some(Member::Field(_) | Member::AssocFn(_)) => Err(SemanticError::NotAMethod {
                name: name.to_owned(),
                ty: struct_ty.name.to_string(),
                span,
            }),
            None => Err(no_such_member(name, &struct_ty.name, span)),
        }
    }

    /// The struct `expr` names, if it is the name of a struct and not of
    /// a variable: a variable hides a struct of the same name.
    fn named_struct(&self, expr: &ast::Expr) -> Option<StructTy> {
        match &expr.kind {
            ast::ExprKind::Ident(name) if self.scopes.lookup(name).is_none() => {
                self.globals.struct_named(name)
            }
            _ => None,
        }
    }

    /// Checks the expression before a `.name`, as a place if it is one.
    fn check_receiver(&mut self, base: &ast::Expr) -> SemanticResult<Expr> {
        if base.is_place() {
            self.check_place(base)
        } else {
            self.check_expr(base, None)
        }
    }
}

/// The error for `.name` on something of type `found` that has no such
/// member.
fn no_such_member(name: &str, found: &str, span: Span) -> SemanticError {
    SemanticError::NoSuchMember {
        name: name.to_owned(),
        found: found.to_owned(),
        span,
    }
}

/// The error for a method or associated function named without a call.
fn not_called(name: &str, ty: &StructTy, span: Span) -> SemanticError {
    SemanticError::MethodNotCalled {
        name: name.to_owned(),
        ty: ty.name.to_string(),
        span,
    }
}
