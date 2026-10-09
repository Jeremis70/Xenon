//! Backward liveness of MIR locals.
//!
//! Liveness is intentionally a query result, not cached on `Body`: any MIR
//! mutation invalidates it, and analysis clients can scope its lifetime to a
//! pass invocation.

use std::collections::BTreeSet;

use crate::index::IndexVec;
use crate::middle::mir::body::{BasicBlock, Body, Local, Location};
use crate::middle::mir::visit::{MutatingUseContext, PlaceContext, Visitor};

use super::edge_definition;

/// The locals live on entry to and exit from each basic block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Liveness {
    /// Locals whose current value may be read in a block.
    pub live_in: IndexVec<BasicBlock, BTreeSet<Local>>,
    /// Locals whose current value may be needed after a block.
    pub live_out: IndexVec<BasicBlock, BTreeSet<Local>>,
}

impl Liveness {
    /// Returns whether `local` is live on entry to `block`.
    pub fn is_live_in(&self, block: BasicBlock, local: Local) -> bool {
        self.live_in
            .get(block)
            .is_some_and(|locals| locals.contains(&local))
    }

    /// Returns whether `local` is live on exit from `block`.
    pub fn is_live_out(&self, block: BasicBlock, local: Local) -> bool {
        self.live_out
            .get(block)
            .is_some_and(|locals| locals.contains(&local))
    }
}

/// Computes backward liveness to a fixed point over all blocks.
///
/// A call destination is a definition on its normal-return edge only. A
/// projected place uses its base pointer, but does not use the value of the
/// pointee. Taking the address of a direct local counts as a use of its
/// storage.
pub fn analyze_liveness(body: &Body) -> Liveness {
    let blocks = body.basic_blocks();
    let transfer: IndexVec<BasicBlock, UseDef> = blocks
        .iter_enumerated()
        .map(|(block, data)| {
            let mut use_def = UseDef::default();
            use_def.visit_basic_block_data(block, data);
            use_def
        })
        .collect();

    let mut live_in = IndexVec::from_elem_n(BTreeSet::new(), blocks.len());
    let mut live_out = IndexVec::from_elem_n(BTreeSet::new(), blocks.len());
    let mut changed = true;
    while changed {
        changed = false;
        for (block, data) in blocks.iter_enumerated().rev() {
            let terminator = &data.terminator.kind;
            let mut new_out = BTreeSet::new();
            for successor in terminator.successors() {
                let defined = edge_definition(terminator, successor);
                // Edges to nonexistent blocks are left to the verifier.
                new_out.extend(
                    live_in
                        .get(successor)
                        .into_iter()
                        .flatten()
                        .filter(|local| Some(**local) != defined),
                );
            }
            let UseDef { uses, defs } = &transfer[block];
            let mut new_in = uses.clone();
            new_in.extend(new_out.difference(defs));

            changed |= live_out[block] != new_out || live_in[block] != new_in;
            live_out[block] = new_out;
            live_in[block] = new_in;
        }
    }

    Liveness { live_in, live_out }
}

/// The upward-exposed uses and the definitions of one block.
#[derive(Debug, Default)]
struct UseDef {
    uses: BTreeSet<Local>,
    defs: BTreeSet<Local>,
}

impl Visitor for UseDef {
    fn visit_local(&mut self, local: &Local, context: PlaceContext, _location: Location) {
        match context {
            PlaceContext::MutatingUse(MutatingUseContext::Store) => {
                self.defs.insert(*local);
            }
            PlaceContext::NonMutatingUse(_)
            | PlaceContext::MutatingUse(MutatingUseContext::AddressOf) => {
                if !self.defs.contains(local) {
                    self.uses.insert(*local);
                }
            }
            // Call destinations are defined on the return edge, see
            // `edge_definition`; storage markers neither use nor define.
            PlaceContext::MutatingUse(MutatingUseContext::Call) | PlaceContext::NonUse(_) => {}
        }
    }
}
