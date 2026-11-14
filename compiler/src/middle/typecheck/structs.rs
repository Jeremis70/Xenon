//! Checking of struct construction and member access.
//!
//! Construction and member lookup each go through one function here
//! ([`FnCtxt::check_construct`] and [`GlobalCtxt::lookup_member`]), so that
//! whatever later changes what `Rect(..)` or `r.name` may mean changes it
//! in one place.

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast;
use crate::middle::thir::{Expr, ExprKind};
use crate::source::Span;
use crate::types::{StructId, StructTy, Type};

use super::items::Member;
use super::{FnCtxt, GlobalCtxt};

impl GlobalCtxt<'_> {
    /// What `name` denotes on a value of struct `id`, if anything.
    pub(super) fn lookup_member(&self, id: StructId, name: &str) -> Option<&Member> {
        self.structs.members.get(id)?.get(name)
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
        let base = if base.is_place() {
            self.check_place(base)?
        } else {
            self.check_expr(base, None)?
        };
        let no_such_member = || SemanticError::NoSuchMember {
            name: name.to_owned(),
            found: base.ty.to_string(),
            span,
        };
        let Type::Struct(struct_ty) = &base.ty else {
            return Err(no_such_member());
        };
        let globals = self.globals;
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
            None => Err(no_such_member()),
        }
    }
}
