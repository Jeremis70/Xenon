//! Strongly typed indices and index-keyed vectors.
//!
//! Compiler IRs refer to their entities (locals, basic blocks, functions,
//! scopes, ...) by dense integer indices rather than by pointers or names.
//! Wrapping each kind of index in its own newtype makes it impossible to,
//! say, index the basic blocks with a local, while [`IndexVec`] keeps the
//! storage a plain `Vec`.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

/// A dense index type usable as the key of an [`IndexVec`].
pub trait Idx: Copy + Eq + Ord + std::hash::Hash + fmt::Debug + 'static {
    /// Creates an index from a raw `usize`.
    ///
    /// # Panics
    ///
    /// Panics if `index` does not fit the index representation (`u32`). An
    /// IR with more than four billion entities of one kind is a compiler bug.
    fn new(index: usize) -> Self;

    /// Returns the raw position of this index.
    fn index(self) -> usize;
}

/// Declares a `u32`-backed newtype implementing [`Idx`].
///
/// The format string controls both `Debug` and `Display`, so every IR dump
/// spells the index the same way (for example `"bb{}"` prints `bb3`).
macro_rules! newtype_index {
    ($(#[$attr:meta])* $vis:vis struct $name:ident = $fmt:literal;) => {
        $(#[$attr])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis struct $name(u32);

        // Generated helpers; not every index type needs all of them.
        #[allow(dead_code)]
        impl $name {
            /// Creates the index from its raw `u32` value.
            pub const fn from_u32(value: u32) -> Self {
                Self(value)
            }

            /// Returns the raw `u32` value of the index.
            pub const fn as_u32(self) -> u32 {
                self.0
            }
        }

        impl $crate::index::Idx for $name {
            fn new(index: usize) -> Self {
                match u32::try_from(index) {
                    Ok(value) => Self(value),
                    Err(_) => panic!(concat!(stringify!($name), " index overflowed u32")),
                }
            }

            fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl ::std::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, $fmt, self.0)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, $fmt, self.0)
            }
        }
    };
}
pub(crate) use newtype_index;

/// A `Vec<T>` that can only be indexed by the index type `I`.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct IndexVec<I: Idx, T> {
    raw: Vec<T>,
    // `fn(&I)` keeps the vector covariant in `T` and `Send`/`Sync` regardless of `I`.
    _marker: PhantomData<fn(&I)>,
}

impl<I: Idx, T> IndexVec<I, T> {
    /// Creates an empty vector.
    pub const fn new() -> Self {
        Self {
            raw: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Creates an empty vector with room for `capacity` elements.
    pub fn with_capacity(capacity: usize) -> Self {
        Self::from_raw(Vec::with_capacity(capacity))
    }

    /// Wraps an existing `Vec`; element `n` becomes index `I::new(n)`.
    pub fn from_raw(raw: Vec<T>) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    /// Creates a vector of `len` clones of `elem`.
    pub fn from_elem_n(elem: T, len: usize) -> Self
    where
        T: Clone,
    {
        Self::from_raw(vec![elem; len])
    }

    /// Appends `value` and returns its index.
    pub fn push(&mut self, value: T) -> I {
        let index = self.next_index();
        self.raw.push(value);
        index
    }

    /// Returns the index the next pushed element will receive.
    pub fn next_index(&self) -> I {
        I::new(self.raw.len())
    }

    /// Returns the number of elements.
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// Returns `true` when the vector holds no elements.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Returns `true` when `index` refers to an element of this vector.
    pub fn contains_index(&self, index: I) -> bool {
        index.index() < self.raw.len()
    }

    /// Returns the element at `index`, or `None` if out of bounds.
    pub fn get(&self, index: I) -> Option<&T> {
        self.raw.get(index.index())
    }

    /// Returns the element at `index` mutably, or `None` if out of bounds.
    pub fn get_mut(&mut self, index: I) -> Option<&mut T> {
        self.raw.get_mut(index.index())
    }

    /// Iterates over the elements in index order.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.raw.iter()
    }

    /// Iterates mutably over the elements in index order.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.raw.iter_mut()
    }

    /// Iterates over `(index, element)` pairs.
    pub fn iter_enumerated(&self) -> impl DoubleEndedIterator<Item = (I, &T)> + '_ {
        self.raw
            .iter()
            .enumerate()
            .map(|(index, value)| (I::new(index), value))
    }

    /// Iterates mutably over `(index, element)` pairs.
    pub fn iter_enumerated_mut(&mut self) -> impl DoubleEndedIterator<Item = (I, &mut T)> + '_ {
        self.raw
            .iter_mut()
            .enumerate()
            .map(|(index, value)| (I::new(index), value))
    }

    /// Consumes the vector, yielding `(index, element)` pairs.
    pub fn into_iter_enumerated(self) -> impl DoubleEndedIterator<Item = (I, T)> {
        self.raw
            .into_iter()
            .enumerate()
            .map(|(index, value)| (I::new(index), value))
    }

    /// Iterates over every valid index.
    pub fn indices(&self) -> impl DoubleEndedIterator<Item = I> + 'static {
        (0..self.raw.len()).map(I::new)
    }

    /// Returns the elements as a plain slice.
    pub fn as_slice(&self) -> &[T] {
        &self.raw
    }
}

impl<I: Idx, T> Default for IndexVec<I, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Idx, T: fmt::Debug> fmt::Debug for IndexVec<I, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter_enumerated()).finish()
    }
}

impl<I: Idx, T> Index<I> for IndexVec<I, T> {
    type Output = T;

    fn index(&self, index: I) -> &T {
        &self.raw[index.index()]
    }
}

impl<I: Idx, T> IndexMut<I> for IndexVec<I, T> {
    fn index_mut(&mut self, index: I) -> &mut T {
        &mut self.raw[index.index()]
    }
}

impl<I: Idx, T> FromIterator<T> for IndexVec<I, T> {
    fn from_iter<It: IntoIterator<Item = T>>(iter: It) -> Self {
        Self::from_raw(iter.into_iter().collect())
    }
}

impl<I: Idx, T> Extend<T> for IndexVec<I, T> {
    fn extend<It: IntoIterator<Item = T>>(&mut self, iter: It) {
        self.raw.extend(iter);
    }
}

impl<I: Idx, T> IntoIterator for IndexVec<I, T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.raw.into_iter()
    }
}

impl<'a, I: Idx, T> IntoIterator for &'a IndexVec<I, T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.raw.iter()
    }
}

impl<'a, I: Idx, T> IntoIterator for &'a mut IndexVec<I, T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.raw.iter_mut()
    }
}
