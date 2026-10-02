use xenonc::index::{Idx, IndexVec};
use xenonc::middle::mir::Local;

#[test]
fn push_returns_sequential_indices() {
    let mut vec: IndexVec<Local, &str> = IndexVec::new();
    let a = vec.push("a");
    let b = vec.push("b");
    assert_eq!((a.index(), b.index()), (0, 1));
    assert_eq!(vec[b], "b");
    assert_eq!(vec.next_index(), Local::new(2));
}

#[test]
fn enumerated_iteration_yields_typed_indices() {
    let vec: IndexVec<Local, u8> = [10, 20].into_iter().collect();
    let pairs: Vec<_> = vec.iter_enumerated().map(|(i, v)| (i, *v)).collect();
    assert_eq!(pairs, vec![(Local::new(0), 10), (Local::new(1), 20)]);
    assert!(vec.contains_index(Local::new(1)));
    assert!(!vec.contains_index(Local::new(2)));
}

#[test]
fn index_formatting_uses_declared_pattern() {
    assert_eq!(Local::new(7).to_string(), "_7");
    assert_eq!(format!("{:?}", Local::new(7)), "_7");
}
