use num_bigint::BigInt;
use xenonc::error::TypeError;
use xenonc::source::Span;
use xenonc::types::{FieldDef, MAX_INT_WIDTH, StructDef, StructTable, Type};

#[test]
fn integer_widths_are_limited_to_the_supported_range() {
    for prefix in ["i", "u"] {
        for width in [0, 1, MAX_INT_WIDTH] {
            let ty = format!("{prefix}{width}")
                .parse::<Type>()
                .expect("valid width");
            assert_eq!(
                ty,
                if prefix == "i" {
                    Type::Int(width)
                } else {
                    Type::UInt(width)
                }
            );
        }
        for width in [MAX_INT_WIDTH + 1, u32::MAX] {
            let raw = format!("{prefix}{width}");
            assert!(matches!(
                raw.parse::<Type>(),
                Err(TypeError::InvalidBitWidth { raw: found, .. }) if found == raw
            ));
        }
    }
}

#[test]
fn integer_bounds_match_twos_complement() {
    assert_eq!(
        Type::Int(8).bounds(),
        Some((BigInt::from(-128), BigInt::from(127)))
    );
    assert_eq!(
        Type::UInt(8).bounds(),
        Some((BigInt::ZERO, BigInt::from(255)))
    );
    assert_eq!(
        Type::Int(1).bounds(),
        Some((BigInt::from(-1), BigInt::ZERO))
    );
    assert_eq!(Type::Int(0).bounds(), Some((BigInt::ZERO, BigInt::ZERO)));
    assert_eq!(Type::UInt(0).bounds(), Some((BigInt::ZERO, BigInt::ZERO)));
    assert_eq!(Type::USize.bounds(), None);
}

#[test]
fn classification_predicates() {
    assert!(Type::ISize.is_signed_integer());
    assert!(Type::USize.is_unsigned_integer());
    assert!(Type::BFloat16.is_float());
    assert!(!Type::Bool.is_integer());
    assert!(Type::Reference(Box::new(Type::Bool)).is_indirect());
    let structs = StructTable::default();
    assert!(Type::Int(0).is_zero_sized(&structs) && Type::UInt(0).is_zero_sized(&structs));
    assert!(!Type::UInt(1).is_zero_sized(&structs) && !Type::Bool.is_zero_sized(&structs));
}

/// Defines a struct named `name` with fields of types `fields`.
fn define(structs: &mut StructTable, name: &str, fields: Vec<Type>) -> Type {
    let fields = fields
        .into_iter()
        .enumerate()
        .map(|(index, ty)| FieldDef {
            name: format!("f{index}"),
            ty,
            span: Span::ZERO,
        })
        .collect();
    let id = structs.push(StructDef {
        name: name.into(),
        fields,
        span: Span::ZERO,
    });
    structs.ty(id)
}

#[test]
fn empty_struct_is_zero_sized() {
    let mut structs = StructTable::default();
    let empty = define(&mut structs, "Empty", vec![]);
    let wrapper = define(&mut structs, "Wrapper", vec![empty.clone(), Type::unit()]);
    assert!(empty.is_zero_sized(&structs));
    assert!(wrapper.is_zero_sized(&structs));
}

#[test]
fn struct_with_sized_field_is_not_zero_sized() {
    let mut structs = StructTable::default();
    let rect = define(&mut structs, "Rect", vec![Type::UInt(0), Type::Float64]);
    assert!(!rect.is_zero_sized(&structs));
    assert_eq!(
        structs.field_tys(&rect),
        Some(vec![Type::UInt(0), Type::Float64])
    );
    assert_eq!(structs.field_ty(&rect, 1), Some(&Type::Float64));
    assert_eq!(structs.field_ty(&rect, 2), None);
}

#[test]
fn struct_displays_its_name_and_compares_by_identity() {
    let mut structs = StructTable::default();
    let a = define(&mut structs, "Rect", vec![]);
    let b = define(&mut structs, "Rect", vec![]);
    assert_eq!(a.to_string(), "Rect");
    assert_eq!(a, a.clone());
    assert_ne!(a, b);
    assert!(a.is_aggregate() && Type::unit().is_aggregate() && !Type::Bool.is_aggregate());
}
