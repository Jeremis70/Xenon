//! Control-flow graph traversal orders.
//!
//! All traversals start at [`START_BLOCK`], visit successors in the order
//! given by [`TerminatorKind::successors`](super::TerminatorKind::successors),
//! and skip edges to blocks that do not exist (the verifier reports those).
//! Unreachable blocks are never yielded.

use crate::index::IndexVec;

use super::body::{BasicBlock, Body, START_BLOCK};

/// The set of blocks reachable from the start block.
pub fn reachable_set(body: &Body) -> IndexVec<BasicBlock, bool> {
    let mut reachable = IndexVec::from_elem_n(false, body.basic_blocks().len());
    for block in preorder(body) {
        reachable[block] = true;
    }
    reachable
}

/// Reachable blocks in depth-first preorder: each block before its
/// successors.
pub fn preorder(body: &Body) -> Vec<BasicBlock> {
    let blocks = body.basic_blocks();
    let mut visited = IndexVec::from_elem_n(false, blocks.len());
    let mut order = Vec::with_capacity(blocks.len());
    let mut stack = Vec::new();
    if blocks.contains_index(START_BLOCK) {
        stack.push(START_BLOCK);
    }
    while let Some(block) = stack.pop() {
        if std::mem::replace(&mut visited[block], true) {
            continue;
        }
        order.push(block);
        // Reverse so the first successor is popped, and thus visited, first.
        let successors = blocks[block].terminator.kind.successors().rev();
        stack.extend(successors.filter(|succ| blocks.contains_index(*succ)));
    }
    order
}

/// Reachable blocks in depth-first postorder: each block after all of its
/// successors, except along back edges.
pub fn postorder(body: &Body) -> Vec<BasicBlock> {
    let blocks = body.basic_blocks();
    let mut visited = IndexVec::from_elem_n(false, blocks.len());
    let mut order = Vec::with_capacity(blocks.len());
    // Each frame holds a block and the successors it has yet to explore.
    let mut stack: Vec<(BasicBlock, std::vec::IntoIter<BasicBlock>)> = Vec::new();

    let mut enter =
        |block: BasicBlock, stack: &mut Vec<(BasicBlock, std::vec::IntoIter<BasicBlock>)>| {
            if blocks.contains_index(block) && !std::mem::replace(&mut visited[block], true) {
                let successors: Vec<_> = blocks[block].terminator.kind.successors().collect();
                stack.push((block, successors.into_iter()));
            }
        };

    enter(START_BLOCK, &mut stack);
    while let Some((_, successors)) = stack.last_mut() {
        match successors.next() {
            Some(succ) => enter(succ, &mut stack),
            None => {
                if let Some((block, _)) = stack.pop() {
                    order.push(block);
                }
            }
        }
    }
    order
}

/// Reachable blocks in reverse postorder: each block before its successors,
/// except along back edges. This is the natural order for forward dataflow.
pub fn reverse_postorder(body: &Body) -> Vec<BasicBlock> {
    let mut order = postorder(body);
    order.reverse();
    order
}
