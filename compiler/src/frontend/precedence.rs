use crate::frontend::ast::BinOp;
use crate::frontend::tokens::TokenKind;

/// Binding powers and operator for a binary infix token.
pub(crate) struct OpInfo {
    pub(crate) left_bp: u8,
    pub(crate) right_bp: u8,
    pub(crate) op: BinOp,
}

/// Returns the [`OpInfo`] for binary infix operators.
/// Ternary `if` is handled separately in `led`.
pub(crate) fn infix_info(kind: &TokenKind) -> Option<OpInfo> {
    match kind {
        TokenKind::OrOr => Some(OpInfo {
            left_bp: 1,
            right_bp: 2,
            op: BinOp::LogicalOr,
        }),
        TokenKind::XorXor => Some(OpInfo {
            left_bp: 3,
            right_bp: 4,
            op: BinOp::LogicalXor,
        }),
        TokenKind::AndAnd => Some(OpInfo {
            left_bp: 5,
            right_bp: 6,
            op: BinOp::LogicalAnd,
        }),
        TokenKind::EqEq => Some(OpInfo {
            left_bp: 7,
            right_bp: 8,
            op: BinOp::Eq,
        }),
        TokenKind::NotEq => Some(OpInfo {
            left_bp: 7,
            right_bp: 8,
            op: BinOp::NotEq,
        }),
        TokenKind::Or => Some(OpInfo {
            left_bp: 9,
            right_bp: 10,
            op: BinOp::BitwiseOr,
        }),
        TokenKind::Xor => Some(OpInfo {
            left_bp: 11,
            right_bp: 12,
            op: BinOp::BitwiseXor,
        }),
        TokenKind::And => Some(OpInfo {
            left_bp: 13,
            right_bp: 14,
            op: BinOp::BitwiseAnd,
        }),
        TokenKind::Lt => Some(OpInfo {
            left_bp: 15,
            right_bp: 16,
            op: BinOp::Lt,
        }),
        TokenKind::Gt => Some(OpInfo {
            left_bp: 15,
            right_bp: 16,
            op: BinOp::Gt,
        }),
        TokenKind::LtEq => Some(OpInfo {
            left_bp: 15,
            right_bp: 16,
            op: BinOp::LtEq,
        }),
        TokenKind::GtEq => Some(OpInfo {
            left_bp: 15,
            right_bp: 16,
            op: BinOp::GtEq,
        }),
        TokenKind::LShift => Some(OpInfo {
            left_bp: 17,
            right_bp: 18,
            op: BinOp::LShift,
        }),
        TokenKind::RShift => Some(OpInfo {
            left_bp: 17,
            right_bp: 18,
            op: BinOp::RShift,
        }),
        TokenKind::Plus => Some(OpInfo {
            left_bp: 19,
            right_bp: 20,
            op: BinOp::Add,
        }),
        TokenKind::Minus => Some(OpInfo {
            left_bp: 19,
            right_bp: 20,
            op: BinOp::Sub,
        }),
        TokenKind::Star => Some(OpInfo {
            left_bp: 21,
            right_bp: 22,
            op: BinOp::Mul,
        }),
        TokenKind::Slash => Some(OpInfo {
            left_bp: 21,
            right_bp: 22,
            op: BinOp::Div,
        }),
        TokenKind::Percent => Some(OpInfo {
            left_bp: 21,
            right_bp: 22,
            op: BinOp::Mod,
        }),
        _ => None,
    }
}
