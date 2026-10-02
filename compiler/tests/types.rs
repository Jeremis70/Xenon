use num_bigint::BigInt;
use xenonc::types::Type;

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
    assert_eq!(Type::USize.bounds(), None);
}

#[test]
fn classification_predicates() {
    assert!(Type::ISize.is_signed_integer());
    assert!(Type::USize.is_unsigned_integer());
    assert!(Type::BFloat16.is_float());
    assert!(!Type::Bool.is_integer());
    assert!(Type::Reference(Box::new(Type::Bool)).is_indirect());
}
