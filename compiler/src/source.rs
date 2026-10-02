//! Source locations shared by every compiler stage.
//!
//! This module sits at the bottom of the dependency graph: the frontend,
//! the middle end (THIR/MIR), diagnostics, and backends may all depend on it,
//! but it depends on nothing else in the compiler.

/// A half-open byte range `start..end` into the compiled source text.
///
/// Spans are currently offsets into the combined source of a compilation.
/// They will gain a file identity once multi-file source maps are introduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// A zero-width span at position 0, used as a placeholder when no source
    /// location is available (e.g. synthetic nodes in tests).
    pub const ZERO: Span = Span { start: 0, end: 0 };

    /// Creates a span covering `start..end`.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Returns the smallest span covering both `self` and `other`.
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}
