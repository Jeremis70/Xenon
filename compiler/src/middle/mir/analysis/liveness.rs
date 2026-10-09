//! Backward liveness of MIR locals.
//!
//! Liveness is intentionally a query result, not cached on `Body`: any MIR
//! mutation invalidates it, and analysis clients can scope its lifetime to a
//! pass invocation.

use std::collections::BTreeSet;

use crate::index::{Idx, IndexVec};

use super::super::body::{BasicBlock, Body, Local};
use super::super::syntax::{Operand, Place, Rvalue, StatementKind, TerminatorKind};

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
    let block_count = body.basic_blocks().len();
    let mut uses = IndexVec::from_elem_n(BTreeSet::new(), block_count);
    let mut defs = IndexVec::from_elem_n(BTreeSet::new(), block_count);

    for (block, data) in body.basic_blocks().iter_enumerated() {
        let mut collector = UsageCollector::default();
        for statement in &data.statements {
            match &statement.kind {
                StatementKind::Assign(assign) => {
                    let (place, rvalue) = &**assign;
                    collect_rvalue(rvalue, &mut collector);
                    collect_store(place, &mut collector);
                }
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop => {}
            }
        }
        match &data.terminator.kind {
            TerminatorKind::SwitchInt { discr, .. }
            | TerminatorKind::Assert { cond: discr, .. } => {
                collect_operand(discr, &mut collector);
            }
            TerminatorKind::Call {
                args, destination, ..
            } => {
                for arg in args {
                    collect_operand(arg, &mut collector);
                }
                if !destination.projection.is_empty() {
                    collector.read(destination.local);
                }
            }
            TerminatorKind::Return => collector.read(Local::new(0)),
            TerminatorKind::Goto { .. }
            | TerminatorKind::Unreachable
            | TerminatorKind::EndOfBody => {}
        }
        uses[block] = collector.uses;
        defs[block] = collector.defs;
    }

    let mut live_in = IndexVec::from_elem_n(BTreeSet::new(), block_count);
    let mut live_out = IndexVec::from_elem_n(BTreeSet::new(), block_count);
    loop {
        let mut changed = false;
        for (block, data) in body.basic_blocks().iter_enumerated().rev() {
            let mut new_out = BTreeSet::new();
            for successor in data.terminator.kind.successors() {
                let Some(mut edge_live) = live_in.get(successor).cloned() else {
                    continue;
                };
                if let TerminatorKind::Call {
                    destination,
                    target: Some(target),
                    ..
                } = &data.terminator.kind
                    && *target == successor
                    && destination.projection.is_empty()
                {
                    edge_live.remove(&destination.local);
                }
                new_out.extend(edge_live);
            }
            let mut new_in = uses[block].clone();
            new_in.extend(new_out.difference(&defs[block]).copied());
            if live_out[block] != new_out {
                live_out[block] = new_out;
                changed = true;
            }
            if live_in[block] != new_in {
                live_in[block] = new_in;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    Liveness { live_in, live_out }
}

#[derive(Debug, Default)]
struct UsageCollector {
    uses: BTreeSet<Local>,
    defs: BTreeSet<Local>,
}

impl UsageCollector {
    fn read(&mut self, local: Local) {
        if !self.defs.contains(&local) {
            self.uses.insert(local);
        }
    }

    fn write(&mut self, local: Local) {
        self.defs.insert(local);
    }
}

fn collect_operand(operand: &Operand, usage: &mut UsageCollector) {
    if let Operand::Copy(place) = operand {
        collect_place_read(place, usage);
    }
}

fn collect_place_read(place: &Place, usage: &mut UsageCollector) {
    usage.read(place.local);
}

fn collect_store(place: &Place, usage: &mut UsageCollector) {
    if place.projection.is_empty() {
        usage.write(place.local);
    } else {
        usage.read(place.local);
    }
}

fn collect_rvalue(rvalue: &Rvalue, usage: &mut UsageCollector) {
    match rvalue {
        Rvalue::Use(operand) | Rvalue::UnaryOp(_, operand) | Rvalue::Cast(_, operand, _) => {
            collect_operand(operand, usage);
        }
        Rvalue::BinaryOp(_, operands) | Rvalue::Overflows(_, operands) => {
            collect_operand(&operands.0, usage);
            collect_operand(&operands.1, usage);
        }
        Rvalue::AddressOf(_, place) if !place.projection.is_empty() => {
            usage.read(place.local);
        }
        Rvalue::AddressOf(_, place) => usage.read(place.local),
    }
}
