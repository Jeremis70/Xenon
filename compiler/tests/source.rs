use xenonc::source::Span;

#[test]
fn to_covers_both_spans_in_any_order() {
    let a = Span::new(4, 8);
    let b = Span::new(1, 5);
    assert_eq!(a.to(b), Span::new(1, 8));
    assert_eq!(b.to(a), Span::new(1, 8));
}
