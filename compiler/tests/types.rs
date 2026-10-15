use num_bigint::BigInt;
use xenonc::error::TypeError;
use xenonc::types::{MAX_INT_WIDTH, Type};

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
    assert!(Type::Int(0).is_zero_sized() && Type::UInt(0).is_zero_sized());
    assert!(!Type::UInt(1).is_zero_sized() && !Type::Bool.is_zero_sized());
}
