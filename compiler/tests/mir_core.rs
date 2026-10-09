//! Tests for MIR construction, traversal, visitors, and pretty-printing.

use xenonc::index::Idx;
use xenonc::middle::ids::DefId;
use xenonc::middle::mir::traversal::{postorder, preorder, reachable_set, reverse_postorder};
use xenonc::middle::mir::visit::{MutatingUseContext, NonMutatingUseContext, NonUseContext};
use xenonc::middle::mir::*;
use xenonc::middle::target::TargetSpec;
use xenonc::source::Span;
use xenonc::types::Type;

const SI: SourceInfo = SourceInfo::outermost(Span::ZERO);

fn i32_const(value: i64) -> Operand {
    Operand::constant(Constant::int(value, Type::Int(32)))
}

fn copy(local: Local) -> Operand {
    Operand::Copy(local.into())
}

/// `fn add_one(x: i32) -> i32 { x + 1 }` with an explicit overflow check.
fn build_add_one(program: &mut MirProgram) -> Result<(), Box<dyn std::error::Error>> {
    let def_id = program.declare(FnDecl {
        name: "add_one".to_owned(),
        sig: FnSig {
            inputs: vec![Type::Int(32)],
            output: Type::Int(32),
        },
        span: Span::ZERO,
    });
    let x_decl = LocalDecl::named(Type::Int(32), "x", SI);
    let mut b = BodyBuilder::new(def_id, Span::ZERO, Type::Int(32), [x_decl]);
    let x = Local::new(1);

    let overflowed = b.new_temp(Type::Bool, Span::ZERO);
    b.storage_live(START_BLOCK, SI, overflowed);
    let operands = Box::new((copy(x), i32_const(1)));
    b.push_assign(
        START_BLOCK,
        SI,
        overflowed.into(),
        Rvalue::Overflows(BinOp::Add, operands.clone()),
    );
    let bb1 = b.new_block();
    b.terminate(
        START_BLOCK,
        SI,
        TerminatorKind::Assert {
            cond: copy(overflowed),
            expected: false,
            kind: AssertKind::Overflow(BinOp::Add),
            target: bb1,
        },
    );
    b.push_assign(
        bb1,
        SI,
        RETURN_PLACE.into(),
        Rvalue::BinaryOp(BinOp::Add, operands),
    );
    b.storage_dead(bb1, SI, overflowed);
    b.terminate(bb1, SI, TerminatorKind::Return);

    program.set_body(b.finish()?)?;
    Ok(())
}

/// `fn main() -> i32 { if true { add_one(41) } else { 0 } }`, plus an
/// unreachable block.
fn build_main(program: &mut MirProgram) -> Result<(), Box<dyn std::error::Error>> {
    let add_one = program
        .decls()
        .iter_enumerated()
        .find_map(|(id, decl)| (decl.name == "add_one").then_some(id))
        .ok_or("add_one must be declared first")?;
    let def_id = program.declare(FnDecl {
        name: "main".to_owned(),
        sig: FnSig {
            inputs: vec![],
            output: Type::Int(32),
        },
        span: Span::ZERO,
    });
    let mut b = BodyBuilder::new(def_id, Span::ZERO, Type::Int(32), []);
    let cond = b.new_temp(Type::Bool, Span::ZERO);
    let then_bb = b.new_block();
    let else_bb = b.new_block();
    let join_bb = b.new_block();
    let dead_bb = b.new_block();

    b.push_assign(
        START_BLOCK,
        SI,
        cond.into(),
        Rvalue::Use(Operand::constant(Constant::bool(true))),
    );
    b.terminate(
        START_BLOCK,
        SI,
        TerminatorKind::SwitchInt {
            discr: copy(cond),
            targets: SwitchTargets::bool(then_bb, else_bb),
        },
    );
    b.terminate(
        then_bb,
        SI,
        TerminatorKind::Call {
            func: add_one,
            args: vec![i32_const(41)],
            destination: RETURN_PLACE.into(),
            target: Some(join_bb),
        },
    );
    b.push_assign(else_bb, SI, RETURN_PLACE.into(), Rvalue::Use(i32_const(0)));
    b.goto(else_bb, SI, join_bb);
    b.terminate(join_bb, SI, TerminatorKind::Return);
    b.terminate(dead_bb, SI, TerminatorKind::Unreachable);

    program.set_body(b.finish()?)?;
    program.set_entry(def_id);
    Ok(())
}

fn sample_program() -> Result<MirProgram, Box<dyn std::error::Error>> {
    let mut program = MirProgram::new();
    build_add_one(&mut program)?;
    build_main(&mut program)?;
    Ok(program)
}

fn main_body(program: &MirProgram) -> &Body {
    let main = program.entry().and_then(|id| program.body(id));
    main.unwrap_or_else(|| panic!("sample program has a main body"))
}

#[test]
fn sample_program_verifies() -> Result<(), Box<dyn std::error::Error>> {
    let program = sample_program()?;
    verify_program(&program, &TargetSpec::host())?;
    Ok(())
}

#[test]
fn pretty_prints_deterministic_mir() -> Result<(), Box<dyn std::error::Error>> {
    let program = sample_program()?;
    let expected = "\
// MIR for `add_one` (phase: built)
fn add_one(i32 _1) -> i32 {
    let i32 _0;
    let bool _2;
    debug x => _1;

    bb0: {
        StorageLive(_2);
        _2 = OverflowsAdd(copy _1, const 1_i32);
        assert(!copy _2, \"attempt to add with overflow\") -> bb1;
    }

    bb1: {
        _0 = Add(copy _1, const 1_i32);
        StorageDead(_2);
        return;
    }
}

// MIR for `main` (phase: built)
fn main() -> i32 {
    let i32 _0;
    let bool _1;

    bb0: {
        _1 = const true;
        switchInt(copy _1) -> [0: bb2, otherwise: bb1];
    }

    bb1: {
        _0 = add_one(const 41_i32) -> bb3;
    }

    bb2: {
        _0 = const 0_i32;
        goto -> bb3;
    }

    bb3: {
        return;
    }

    bb4: {
        unreachable;
    }
}
";
    assert_eq!(pretty::mir_program_to_string(&program), expected);
    Ok(())
}

#[test]
fn traversals_skip_unreachable_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let program = sample_program()?;
    let body = main_body(&program);
    let bbs = |ids: &[usize]| ids.iter().map(|&i| BasicBlock::new(i)).collect::<Vec<_>>();

    assert_eq!(preorder(body), bbs(&[0, 2, 3, 1]));
    assert_eq!(postorder(body), bbs(&[3, 2, 1, 0]));
    assert_eq!(reverse_postorder(body), bbs(&[0, 1, 2, 3]));
    let reachable = reachable_set(body);
    assert!(reachable[BasicBlock::new(3)]);
    assert!(!reachable[BasicBlock::new(4)]);
    Ok(())
}

#[test]
fn predecessors_are_cached_and_invalidated() -> Result<(), Box<dyn std::error::Error>> {
    let mut program = sample_program()?;
    let main = program.entry().ok_or("entry set")?;
    let body = program.body_mut(main).ok_or("main has a body")?;
    let join = BasicBlock::new(3);

    let preds = &body.basic_blocks().predecessors()[join];
    assert_eq!(preds, &vec![BasicBlock::new(1), BasicBlock::new(2)]);

    // Redirect `bb2: goto -> bb3` to the unreachable block.
    for succ in body.basic_blocks_mut()[BasicBlock::new(2)]
        .terminator
        .kind
        .successors_mut()
    {
        *succ = BasicBlock::new(4);
    }
    let preds = body.basic_blocks().predecessors();
    assert_eq!(preds[join], vec![BasicBlock::new(1)]);
    assert_eq!(preds[BasicBlock::new(4)], vec![BasicBlock::new(2)]);
    Ok(())
}

#[derive(Default)]
struct LocalUses(Vec<(Local, PlaceContext, Location)>);

impl Visitor for LocalUses {
    fn visit_local(&mut self, local: &Local, context: PlaceContext, location: Location) {
        self.0.push((*local, context, location));
    }
}

#[test]
fn visitor_reports_uses_in_evaluation_order() -> Result<(), Box<dyn std::error::Error>> {
    let program = sample_program()?;
    let add_one = program.body(first_def(&program)).ok_or("add_one body")?;
    let mut uses = LocalUses::default();
    uses.visit_body(add_one);

    let at = |block: usize, statement_index| Location {
        block: BasicBlock::new(block),
        statement_index,
    };
    let (x, tmp) = (Local::new(1), Local::new(2));
    use PlaceContext::*;
    assert_eq!(
        uses.0,
        vec![
            (tmp, NonUse(NonUseContext::StorageLive), at(0, 0)),
            (x, NonMutatingUse(NonMutatingUseContext::Copy), at(0, 1)),
            (tmp, MutatingUse(MutatingUseContext::Store), at(0, 1)),
            (tmp, NonMutatingUse(NonMutatingUseContext::Copy), at(0, 2)),
            (x, NonMutatingUse(NonMutatingUseContext::Copy), at(1, 0)),
            (
                RETURN_PLACE,
                MutatingUse(MutatingUseContext::Store),
                at(1, 0)
            ),
            (tmp, NonUse(NonUseContext::StorageDead), at(1, 1)),
            (
                RETURN_PLACE,
                NonMutatingUse(NonMutatingUseContext::Return),
                at(1, 2)
            ),
        ]
    );
    Ok(())
}

/// The first declared function (`add_one` in the sample program).
fn first_def(program: &MirProgram) -> DefId {
    program
        .decls()
        .indices()
        .next()
        .unwrap_or_else(|| panic!("program has declarations"))
}

#[test]
fn deref_reads_the_base_pointer() -> Result<(), Box<dyn std::error::Error>> {
    let mut program = MirProgram::new();
    let ptr_ty = Type::Pointer(Box::new(Type::Int(32)));
    let def_id = program.declare(FnDecl {
        name: "store".to_owned(),
        sig: FnSig {
            inputs: vec![ptr_ty.clone()],
            output: Type::Bool,
        },
        span: Span::ZERO,
    });
    let mut b = BodyBuilder::new(
        def_id,
        Span::ZERO,
        Type::Bool,
        [LocalDecl::temp(ptr_ty, Span::ZERO)],
    );
    let ptr = Local::new(1);
    b.push_assign(
        START_BLOCK,
        SI,
        Place::from(ptr).deref(),
        Rvalue::Use(i32_const(7)),
    );
    b.push_assign(
        START_BLOCK,
        SI,
        RETURN_PLACE.into(),
        Rvalue::Use(Operand::constant(Constant::bool(true))),
    );
    b.terminate(START_BLOCK, SI, TerminatorKind::Return);
    let body = b.finish()?;

    let mut uses = LocalUses::default();
    uses.visit_body(&body);
    assert_eq!(
        uses.0[0].1,
        PlaceContext::NonMutatingUse(NonMutatingUseContext::Projection)
    );
    assert_eq!(body_statement(&body, 0), "(*_1) = const 7_i32");
    program.set_body(body)?;
    verify_program(&program, &TargetSpec::host())?;
    Ok(())
}

fn body_statement(body: &Body, index: usize) -> String {
    body.basic_blocks()[START_BLOCK].statements[index]
        .kind
        .to_string()
}

struct BumpConstants;

impl MutVisitor for BumpConstants {
    fn visit_constant(&mut self, constant: &mut Constant, _location: Location) {
        if let ConstValue::Int(value) = &mut constant.value {
            *value += 1;
        }
    }
}

#[test]
fn mut_visitor_rewrites_in_place() -> Result<(), Box<dyn std::error::Error>> {
    let mut program = sample_program()?;
    let main = program.entry().ok_or("entry set")?;
    let body = program.body_mut(main).ok_or("main has a body")?;
    BumpConstants.visit_body(body);
    let text = pretty::mir_program_to_string(&program);
    assert!(text.contains("_0 = add_one(const 42_i32) -> bb3;"));
    assert!(text.contains("_0 = const 1_i32;"));
    Ok(())
}

#[test]
fn phases_only_move_forward() -> Result<(), Box<dyn std::error::Error>> {
    let mut program = sample_program()?;
    let main = program.entry().ok_or("entry set")?;
    let body = program.body_mut(main).ok_or("main has a body")?;
    assert_eq!(body.phase(), MirPhase::Built);
    body.advance_phase(MirPhase::Runtime)?;
    body.advance_phase(MirPhase::Runtime)?;
    assert_eq!(
        body.advance_phase(MirPhase::Checked),
        Err(PhaseError {
            def_id: main,
            from: MirPhase::Runtime,
            to: MirPhase::Checked
        })
    );
    Ok(())
}

#[test]
fn local_kinds_follow_layout() -> Result<(), Box<dyn std::error::Error>> {
    let program = sample_program()?;
    let add_one = program.body(first_def(&program)).ok_or("add_one body")?;
    assert_eq!(add_one.local_kind(Local::new(0)), LocalKind::ReturnPlace);
    assert_eq!(add_one.local_kind(Local::new(1)), LocalKind::Argument);
    assert_eq!(add_one.local_kind(Local::new(2)), LocalKind::Temporary);
    assert_eq!(add_one.args_iter().collect::<Vec<_>>(), vec![Local::new(1)]);
    Ok(())
}

#[test]
fn builder_reports_misuse() {
    let def_id = DefId::new(0);
    let new = || BodyBuilder::new(def_id, Span::ZERO, Type::Bool, []);

    let mut b = new();
    b.new_block();
    b.terminate(START_BLOCK, SI, TerminatorKind::Unreachable);
    assert_eq!(
        b.finish().err(),
        Some(BuildError::Unterminated(BasicBlock::new(1)))
    );

    let mut b = new();
    b.terminate(START_BLOCK, SI, TerminatorKind::Return);
    b.push_assign(
        START_BLOCK,
        SI,
        RETURN_PLACE.into(),
        Rvalue::Use(Operand::constant(Constant::bool(true))),
    );
    assert_eq!(
        b.finish().err(),
        Some(BuildError::AlreadyTerminated(START_BLOCK))
    );

    let mut b = new();
    b.goto(BasicBlock::new(5), SI, START_BLOCK);
    b.terminate(START_BLOCK, SI, TerminatorKind::Return);
    assert_eq!(
        b.finish().err(),
        Some(BuildError::UnknownBlock(BasicBlock::new(5)))
    );
}

#[test]
fn program_rejects_inconsistent_bodies() -> Result<(), Box<dyn std::error::Error>> {
    let mut program = MirProgram::new();
    let undeclared = DefId::new(3);
    let mut b = BodyBuilder::new(undeclared, Span::ZERO, Type::Bool, []);
    b.terminate(START_BLOCK, SI, TerminatorKind::Unreachable);
    assert_eq!(
        program.set_body(b.finish()?),
        Err(ProgramError::UndeclaredFunction(undeclared))
    );

    let mut program = sample_program()?;
    let main = program.entry().ok_or("entry set")?;
    let duplicate = program.body(main).ok_or("main has a body")?.clone();
    assert_eq!(
        program.set_body(duplicate),
        Err(ProgramError::DuplicateBody(main))
    );
    Ok(())
}
