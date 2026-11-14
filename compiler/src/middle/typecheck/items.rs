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
use crate::middle::ids::DefId;
use crate::source::Span;
use crate::types::{FieldDef, StructDef, StructId, StructTable, Type};

/// What a name after `.` denotes on a struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Member {
    /// The stored field with this index, in declaration order.
    Field(u32),
    /// A function called on a value of the struct, which it receives as
    /// its first argument.
    Method { def: DefId, receiver: ReceiverKind },
    /// A function called on the struct itself, `Name.function(..)`.
    AssocFn(DefId),
}

/// How a method receives the value it is called on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReceiverKind {
    /// `self`: a copy of the value.
    Value,
    /// `&self`: a reference to the value.
    Reference,
}

/// The members of one struct by name, each with where it is declared.
/// Fields, methods, and associated functions share this one namespace.
pub(super) type Members<'a> = HashMap<&'a str, (Member, Span)>;

/// A function of the program: a free function, or a member of an `impl`.
#[derive(Debug)]
pub(super) struct FnItem<'a> {
    pub(super) function: &'a ast::Function,
    /// The struct whose `impl` declares the function, if any.
    pub(super) owner: Option<StructId>,
    /// The name of the function in the compiled program. A member is
    /// `Struct.name`, which no free function can be called: a name has no
    /// `.`.
    pub(super) symbol: String,
}

/// The struct name the backend needs for itself: it renames a function
/// called `main` to `_xe.main`, which would be a member of this struct.
const RESERVED_STRUCT_NAME: &str = "_xe";

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
        if name == RESERVED_STRUCT_NAME {
            return Err(SemanticError::ReservedName {
                name: decl.name.clone(),
                span: decl.name_span,
            });
        }
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
            self.add_member(id, &field.name, field.span, Member::Field(index as u32))?;
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

    /// Adds `member`, declared at `span`, to the namespace of struct `id`
    /// under `name`.
    pub(super) fn add_member(
        &mut self,
        id: StructId,
        name: &'a str,
        span: Span,
        member: Member,
    ) -> SemanticResult<()> {
        match self.members[id].entry(name) {
            Entry::Vacant(entry) => {
                entry.insert((member, span));
                Ok(())
            }
            Entry::Occupied(entry) => Err(SemanticError::DuplicateMember {
                name: name.to_owned(),
                ty: self.table.ty(id).to_string(),
                first_span: entry.get().1,
                span,
            }),
        }
    }

    /// Lists every function of `program`, free functions first and then
    /// the members of each `impl` in source order, and adds the members to
    /// the namespace of their struct. A function's position is its
    /// [`DefId`].
    pub(super) fn collect_functions(
        &mut self,
        program: &'a ast::Program,
    ) -> SemanticResult<IndexVec<DefId, FnItem<'a>>> {
        let mut items: IndexVec<DefId, FnItem<'a>> = program
            .functions
            .iter()
            .map(|function| FnItem {
                function,
                owner: None,
                symbol: function.name.clone(),
            })
            .collect();
        for block in &program.impls {
            let Some(&owner) = self.ids.get(block.target.as_str()) else {
                return Err(SemanticError::UnknownImplTarget {
                    name: block.target.clone(),
                    span: block.target_span,
                });
            };
            for member in &block.members {
                let ast::ImplMember::Function(function) = member;
                let def = items.push(FnItem {
                    function,
                    owner: Some(owner),
                    symbol: format!("{}.{}", block.target, function.name),
                });
                let member = match function.receiver {
                    Some(ast::Receiver::Value(_)) => Member::Method {
                        def,
                        receiver: ReceiverKind::Value,
                    },
                    Some(ast::Receiver::Reference(_)) => Member::Method {
                        def,
                        receiver: ReceiverKind::Reference,
                    },
                    None => Member::AssocFn(def),
                };
                self.add_member(owner, &function.name, function.span, member)?;
            }
        }
        Ok(items)
    }

    /// The type a method of struct `owner` gives its receiver.
    pub(super) fn receiver_ty(&self, owner: StructId, receiver: ast::Receiver) -> Type {
        let ty = self.table.ty(owner);
        match receiver {
            ast::Receiver::Value(_) => ty,
            ast::Receiver::Reference(_) => Type::Reference(Box::new(ty)),
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
