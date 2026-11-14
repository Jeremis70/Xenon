//! Collection of the program's struct definitions, before any function is
//! checked.
//!
//! Structs are collected in three steps, so that their declaration order
//! does not matter: every name gets a [`StructId`], then field types are
//! resolved against those names, then structs that would contain themselves
//! are rejected.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast;
use crate::index::IndexVec;
use crate::types::{FieldDef, StructDef, StructId, StructTable, Type};

/// What a name after `.` denotes on a value of a struct type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Member {
    /// The stored field with this index, in declaration order.
    Field(u32),
}

/// The members of one struct, by name. Fields, and later methods and
/// properties, share this one namespace.
pub(super) type Members<'a> = HashMap<&'a str, Member>;

/// Every struct of a program.
#[derive(Debug, Default)]
pub(super) struct StructItems<'a> {
    pub(super) table: StructTable,
    pub(super) ids: HashMap<&'a str, StructId>,
    pub(super) members: IndexVec<StructId, Members<'a>>,
}

impl<'a> StructItems<'a> {
    /// Collects the structs of `program`.
    pub(super) fn collect(program: &'a ast::Program) -> SemanticResult<Self> {
        let mut items = Self::default();
        for decl in &program.structs {
            items.declare(decl, program)?;
        }
        for (index, decl) in program.structs.iter().enumerate() {
            items.define_fields(StructId::from_u32(index as u32), decl)?;
        }
        for (index, decl) in program.structs.iter().enumerate() {
            items.check_not_recursive(StructId::from_u32(index as u32), decl)?;
        }
        Ok(items)
    }

    /// Gives `decl` its [`StructId`]; its fields come later.
    fn declare(&mut self, decl: &'a ast::StructDecl, program: &ast::Program) -> SemanticResult<()> {
        let name = decl.name.as_str();
        // Anything but "unknown" means the name spells a built-in type, or
        // a malformed one such as an integer that is too wide.
        if !matches!(
            name.parse::<Type>(),
            Err(crate::error::TypeError::Unknown(_))
        ) {
            return Err(SemanticError::BuiltinTypeName {
                name: decl.name.clone(),
                span: decl.name_span,
            });
        }
        let id = self.table.push(StructDef {
            name: name.into(),
            fields: Vec::new(),
            span: decl.span,
        });
        self.members.push(Members::new());
        match self.ids.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(id);
            }
            Entry::Occupied(entry) => {
                let first = &program.structs[entry.get().as_u32() as usize];
                return Err(SemanticError::DuplicateStruct {
                    name: decl.name.clone(),
                    first_span: first.name_span,
                    span: decl.name_span,
                });
            }
        }
        Ok(())
    }

    /// Resolves the fields of `decl`, whose every type name now has an id.
    fn define_fields(&mut self, id: StructId, decl: &'a ast::StructDecl) -> SemanticResult<()> {
        let mut fields = Vec::with_capacity(decl.fields.len());
        for (index, field) in decl.fields.iter().enumerate() {
            let ty = self.resolve(&field.ty)?;
            // A reference is bound once, where it is declared. A field has
            // no such place: it is set whenever its struct is assigned.
            if ty.contains_reference() {
                return Err(SemanticError::ReferenceField {
                    name: field.name.clone(),
                    ty,
                    span: field.span,
                });
            }
            self.add_member(
                id,
                &field.name,
                field.span,
                Member::Field(index as u32),
                |name| {
                    decl.fields
                        .iter()
                        .find(|other| other.name == name)
                        .map(|other| other.span)
                },
            )?;
            fields.push(FieldDef {
                name: field.name.clone(),
                ty,
                span: field.span,
            });
        }
        let def = self.table.get_mut(id).expect("the struct was declared");
        def.fields = fields;
        Ok(())
    }

    /// Adds `member` to the namespace of struct `id` under `name`.
    /// `span_of` gives where a member already using a name is declared.
    pub(super) fn add_member(
        &mut self,
        id: StructId,
        name: &'a str,
        span: crate::source::Span,
        member: Member,
        span_of: impl FnOnce(&str) -> Option<crate::source::Span>,
    ) -> SemanticResult<()> {
        match self.members[id].entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(member);
                Ok(())
            }
            Entry::Occupied(_) => Err(SemanticError::DuplicateMember {
                name: name.to_owned(),
                ty: self.table.ty(id).to_string(),
                first_span: span_of(name).unwrap_or(span),
                span,
            }),
        }
    }

    /// Rejects a struct that contains itself by value, directly or through
    /// tuples and other structs: it would have no finite size. A pointer
    /// to itself is fine.
    fn check_not_recursive(&self, id: StructId, decl: &ast::StructDecl) -> SemanticResult<()> {
        let def = self.table.get(id).expect("the struct was declared");
        for (field, field_decl) in def.fields.iter().zip(&decl.fields) {
            if self.contains(&field.ty, id, &mut Vec::new()) {
                return Err(SemanticError::RecursiveStruct {
                    name: decl.name.clone(),
                    span: field_decl.span,
                });
            }
        }
        Ok(())
    }

    /// Whether a value of type `ty` stores a value of struct `target`.
    /// `visited` holds the structs already searched.
    fn contains(&self, ty: &Type, target: StructId, visited: &mut Vec<StructId>) -> bool {
        match ty {
            Type::Tuple(elems) => elems
                .iter()
                .any(|elem| self.contains(elem, target, visited)),
            Type::Struct(struct_ty) if struct_ty.id == target => true,
            Type::Struct(struct_ty) if !visited.contains(&struct_ty.id) => {
                visited.push(struct_ty.id);
                self.table.get(struct_ty.id).is_some_and(|def| {
                    def.fields
                        .iter()
                        .any(|field| self.contains(&field.ty, target, visited))
                })
            }
            _ => false,
        }
    }

    /// Resolves a type written in the source to the type it names.
    pub(super) fn resolve(&self, ty: &ast::TypeExpr) -> SemanticResult<Type> {
        use ast::TypeExprKind as Kind;
        Ok(match &ty.kind {
            Kind::Primitive(ty) => ty.clone(),
            Kind::Pointer(inner) => Type::Pointer(Box::new(self.resolve(inner)?)),
            Kind::Reference(inner) => Type::Reference(Box::new(self.resolve(inner)?)),
            Kind::Tuple(elems) => Type::Tuple(
                elems
                    .iter()
                    .map(|elem| self.resolve(elem))
                    .collect::<Result<_, _>>()?,
            ),
            Kind::Named(name) => match self.ids.get(name.as_str()) {
                Some(id) => self.table.ty(*id),
                None => {
                    return Err(SemanticError::UnknownType {
                        name: name.clone(),
                        span: ty.span,
                    });
                }
            },
        })
    }
}
