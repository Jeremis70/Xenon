use num_bigint::BigInt;
use xenonc::middle::target::TargetSpec;
use xenonc::types::Type;

#[test]
fn pointer_sized_integers_follow_target_width() {
    let target = TargetSpec::new(32);
    assert_eq!(target.int_width(&Type::USize), Some(32));
    assert_eq!(
        target.int_bounds(&Type::ISize),
        Some((BigInt::from(i32::MIN), BigInt::from(i32::MAX)))
    );
    assert_eq!(
        target.int_bounds(&Type::USize),
        Some((BigInt::ZERO, BigInt::from(u32::MAX)))
    );
}

#[test]
fn fixed_width_integers_ignore_target() {
    let target = TargetSpec::new(16);
    assert_eq!(target.int_width(&Type::Int(64)), Some(64));
    assert_eq!(target.int_bounds(&Type::Bool), None);
}
