use crate::error::{ParseError, ParseResult, TypeError};
use crate::frontend::ast::{
    Attribute, BinOp, Binding, Expr, ExprKind, Function, Program, Stmt, StmtKind, Type, UnaryOp,
};
use crate::frontend::tokens::{Span, Token, TokenKind};
use num_bigint::BigInt;

/// Returns the left binding power for an infix operator token.
fn lbp(kind: &TokenKind) -> Option<u8> {
    match kind {
        TokenKind::OrOr => Some(1),
        TokenKind::XorXor => Some(3),
        TokenKind::AndAnd => Some(5),
        TokenKind::EqEq | TokenKind::NotEq => Some(7),
        TokenKind::Or => Some(9),
        TokenKind::Xor => Some(11),
        TokenKind::And => Some(13),
        TokenKind::Lt | TokenKind::Gt | TokenKind::LtEq | TokenKind::GtEq => Some(15),
        TokenKind::LShift | TokenKind::RShift => Some(17),
        TokenKind::Plus | TokenKind::Minus => Some(19),
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Some(21),
        // right-associative: left_bp > right_bp
        TokenKind::Pow => Some(24),
        // ternary `x if c else y` — looser than all binary operators
        TokenKind::If => Some(0),
        _ => None,
    }
}

/// Returns `(right_bp, op)` for tokens that map directly to a [`BinOp`].
fn infix_op(kind: &TokenKind) -> Option<(u8, BinOp)> {
    match kind {
        TokenKind::OrOr => Some((2, BinOp::LogicalOr)),
        TokenKind::XorXor => Some((4, BinOp::LogicalXor)),
        TokenKind::AndAnd => Some((6, BinOp::LogicalAnd)),
        TokenKind::EqEq => Some((8, BinOp::Eq)),
        TokenKind::NotEq => Some((8, BinOp::NotEq)),
        TokenKind::Or => Some((10, BinOp::BitwiseOr)),
        TokenKind::Xor => Some((12, BinOp::BitwiseXor)),
        TokenKind::And => Some((14, BinOp::BitwiseAnd)),
        TokenKind::Lt => Some((16, BinOp::Lt)),
        TokenKind::Gt => Some((16, BinOp::Gt)),
        TokenKind::LtEq => Some((16, BinOp::LtEq)),
        TokenKind::GtEq => Some((16, BinOp::GtEq)),
        TokenKind::LShift => Some((18, BinOp::LShift)),
        TokenKind::RShift => Some((18, BinOp::RShift)),
        TokenKind::Plus => Some((20, BinOp::Add)),
        TokenKind::Minus => Some((20, BinOp::Sub)),
        TokenKind::Star => Some((22, BinOp::Mul)),
        TokenKind::Slash => Some((22, BinOp::Div)),
        TokenKind::Percent => Some((22, BinOp::Mod)),
        // Right associative operator: left_bp > right_bp
        TokenKind::Pow => Some((23, BinOp::Pow)),
        _ => None,
    }
}

pub struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn eof_span(&self) -> Span {
        self.tokens
            .last()
            .map(|t| Span {
                start: t.span.end,
                end: t.span.end,
            })
            .unwrap_or(Span { start: 0, end: 0 })
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        let span = self
            .peek()
            .map(|t| t.span)
            .unwrap_or_else(|| self.eof_span());
        ParseError::new(message, span)
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<&'a Token> {
        if self.position < self.tokens.len() {
            let idx = self.position;
            self.position += 1;
            Some(&self.tokens[idx])
        } else {
            None
        }
    }

    /// Returns the span of the most recently consumed token.
    fn prev_span(&self) -> Span {
        if self.position > 0 {
            self.tokens[self.position - 1].span
        } else {
            Span::ZERO
        }
    }

    fn expect(&mut self, kinds: impl AsRef<[TokenKind]>) -> ParseResult<&'a Token> {
        let kinds = kinds.as_ref();
        let description = if kinds.len() == 1 {
            format!("{:?}", kinds[0])
        } else {
            format!("one of {:?}", kinds)
        };
        match self.peek() {
            Some(t) if kinds.contains(&t.kind) => {
                self.advance();
                Ok(t)
            }
            Some(t) => Err(ParseError::new(
                format!("Expected {}, found {:?}", description, t.kind),
                t.span,
            )),
            None => Err(self.error(format!("Expected {}, found end of input", description))),
        }
    }

    pub fn parse_program(&mut self) -> ParseResult<Program> {
        let mut functions = Vec::new();
        while self.peek().is_some() {
            let attributes = self.parse_attributes()?;
            if !attributes.is_empty() && !self.peek().is_some_and(|t| t.kind == TokenKind::Fn) {
                let span = attributes.last().unwrap().span;
                return Err(ParseError::new(
                    "attributes must be followed by a function definition",
                    span,
                ));
            }
            functions.push(self.parse_function(attributes)?);
        }
        Ok(Program { functions })
    }

    fn parse_attributes(&mut self) -> ParseResult<Vec<Attribute>> {
        let mut attrs = Vec::new();
        while self.peek().is_some_and(|t| t.kind == TokenKind::Hash) {
            let hash_token = self.expect(TokenKind::Hash)?;
            let start = hash_token.span.start;
            self.expect(TokenKind::LBracket)?;
            let name_token = self.expect(TokenKind::Ident)?;
            let name = name_token.ident_value()?.to_string();
            self.expect(TokenKind::RBracket)?;
            attrs.push(Attribute {
                name,
                span: Span {
                    start,
                    end: self.prev_span().end,
                },
            });
        }
        Ok(attrs)
    }

    fn parse_function(&mut self, attributes: Vec<Attribute>) -> ParseResult<Function> {
        let fn_token = self.expect(TokenKind::Fn)?;
        let start = fn_token.span.start;
        let name = self.expect(TokenKind::Ident)?.ident_value()?.to_string();

        self.expect(TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(TokenKind::RParen)?;

        self.expect(TokenKind::Arrow)?;
        let return_type = self.parse_return_type()?;

        self.expect(TokenKind::LBrace)?;
        let body = self.parse_body()?;
        self.expect(TokenKind::RBrace)?;

        Ok(Function {
            name,
            params,
            return_type,
            body,
            attributes,
            span: Span {
                start,
                end: self.prev_span().end,
            },
        })
    }

    fn parse_type(&self, token: &Token) -> ParseResult<Type> {
        token
            .ident_value()?
            .parse::<Type>()
            .map_err(|e: TypeError| ParseError::new(e.to_string(), token.span))
    }

    fn parse_expression(&mut self) -> ParseResult<Expr> {
        self.parse_expr(0)
    }

    fn parse_statement(&mut self) -> ParseResult<Stmt> {
        let first_token = self.expect([
            TokenKind::Return,
            TokenKind::Ident,
            TokenKind::If,
            TokenKind::Loop,
            TokenKind::While,
            TokenKind::Do,
            TokenKind::Until,
            TokenKind::Break,
            TokenKind::Continue,
        ])?;
        let start = first_token.span.start;
        match first_token.kind {
            TokenKind::Return => {
                let expr = self.parse_expression()?;
                self.expect(TokenKind::Semicolon)?;
                Ok(Stmt {
                    kind: StmtKind::Return(Box::new(expr)),
                    span: Span {
                        start,
                        end: self.prev_span().end,
                    },
                })
            }
            TokenKind::Ident => {
                if self.peek().is_some_and(|t| t.kind == TokenKind::LParen) {
                    // Ident followed by `(` is a function call statement
                    let call_expr =
                        self.parse_call(first_token.ident_value()?.to_string(), start)?;
                    self.expect(TokenKind::Semicolon)?;
                    Ok(Stmt {
                        kind: StmtKind::Expr(Box::new(call_expr)),
                        span: Span {
                            start,
                            end: self.prev_span().end,
                        },
                    })
                } else {
                    let second_token = self.expect([
                        TokenKind::Ident,
                        TokenKind::Eq,
                        TokenKind::PlusEq,
                        TokenKind::MinusEq,
                        TokenKind::StarEq,
                        TokenKind::SlashEq,
                        TokenKind::PercentEq,
                        TokenKind::PowEq,
                        TokenKind::AndEq,
                        TokenKind::OrEq,
                        TokenKind::XorEq,
                        TokenKind::LShiftEq,
                        TokenKind::RShiftEq,
                        TokenKind::PlusPlus,
                        TokenKind::MinusMinus,
                    ])?;
                    match second_token.kind {
                        TokenKind::Ident => self.parse_var_decl(first_token, second_token, start),
                        kind if kind.is_assign_op() => {
                            let name = first_token.ident_value()?.to_string();
                            self.parse_var_assign(name, &kind, first_token.span)
                        }
                        _ => unreachable!("expect guarantees valid second token"),
                    }
                }
            }
            TokenKind::If => self.parse_if(start),
            TokenKind::Loop => {
                let loop_expr = self.parse_loop(start)?;
                Ok(Stmt {
                    span: loop_expr.span,
                    kind: StmtKind::Expr(Box::new(loop_expr)),
                })
            }
            TokenKind::While | TokenKind::Do | TokenKind::Until => {
                let loop_expr = self.parse_cond_loop(first_token, start)?;
                Ok(Stmt {
                    span: loop_expr.span,
                    kind: StmtKind::Expr(Box::new(loop_expr)),
                })
            }
            TokenKind::Break => {
                let value = if self.peek().is_some_and(|t| t.kind != TokenKind::Semicolon) {
                    Some(Box::new(self.parse_expression()?))
                } else {
                    None
                };
                self.expect(TokenKind::Semicolon)?;
                Ok(Stmt {
                    kind: StmtKind::Break(value),
                    span: Span {
                        start,
                        end: self.prev_span().end,
                    },
                })
            }
            TokenKind::Continue => {
                self.expect(TokenKind::Semicolon)?;
                Ok(Stmt {
                    kind: StmtKind::Continue,
                    span: Span {
                        start,
                        end: self.prev_span().end,
                    },
                })
            }
            _ => unreachable!(
                "expect guarantees token is Return, Ident, If, Loop, Break, or Continue"
            ),
        }
    }

    fn parse_typed_binding(&self, type_token: &Token, name_token: &Token) -> ParseResult<Binding> {
        let ty = self.parse_type(type_token)?;
        let name = name_token.ident_value()?.to_string();
        Ok(Binding {
            name: Some(name),
            ty,
            default: None,
            span: Span {
                start: type_token.span.start,
                end: name_token.span.end,
            },
        })
    }

    fn parse_loop(&mut self, start: usize) -> ParseResult<Expr> {
        self.expect(TokenKind::LBrace)?;
        let body = self.parse_body()?;
        self.expect(TokenKind::RBrace)?;
        Ok(Expr {
            kind: ExprKind::Loop { body },
            span: Span {
                start,
                end: self.prev_span().end,
            },
        })
    }

    fn parse_cond_loop(&mut self, token: &Token, start: usize) -> ParseResult<Expr> {
        match token.kind {
            TokenKind::While | TokenKind::Until => {
                let inverted = token.kind == TokenKind::Until;
                let condition = Box::new(self.parse_expression()?);
                self.expect(TokenKind::LBrace)?;
                let body = self.parse_body()?;
                self.expect(TokenKind::RBrace)?;
                Ok(Expr {
                    kind: ExprKind::CondLoop {
                        post: false,
                        inverted,
                        condition,
                        body,
                    },
                    span: Span {
                        start,
                        end: self.prev_span().end,
                    },
                })
            }
            TokenKind::Do => {
                self.expect(TokenKind::LBrace)?;
                let body = self.parse_body()?;
                self.expect(TokenKind::RBrace)?;
                let inverted =
                    self.expect([TokenKind::While, TokenKind::Until])?.kind == TokenKind::Until;
                let condition = Box::new(self.parse_expression()?);
                Ok(Expr {
                    kind: ExprKind::CondLoop {
                        post: true,
                        inverted,
                        condition,
                        body,
                    },
                    span: Span {
                        start,
                        end: self.prev_span().end,
                    },
                })
            }
            _ => unreachable!("expect guarantees token is While, Until, or Do"),
        }
    }

    fn parse_if(&mut self, start: usize) -> ParseResult<Stmt> {
        let condition = Box::new(self.parse_expression()?);
        self.expect(TokenKind::LBrace)?;
        let then_branch = self.parse_body()?;
        self.expect(TokenKind::RBrace)?;
        let else_branch = if self.peek().is_some_and(|t| t.kind == TokenKind::Else) {
            self.expect(TokenKind::Else)?;
            if self.peek().is_some_and(|t| t.kind == TokenKind::If) {
                let if_token = self.expect(TokenKind::If)?;
                Some(vec![self.parse_if(if_token.span.start)?])
            } else {
                self.expect(TokenKind::LBrace)?;
                let stmts = self.parse_body()?;
                self.expect(TokenKind::RBrace)?;
                Some(stmts)
            }
        } else {
            None
        };

        Ok(Stmt {
            kind: StmtKind::If {
                condition,
                then_branch,
                else_branch,
            },
            span: Span {
                start,
                end: self.prev_span().end,
            },
        })
    }

    fn parse_var_decl(
        &mut self,
        type_token: &Token,
        name_token: &Token,
        start: usize,
    ) -> ParseResult<Stmt> {
        let mut binding = self.parse_typed_binding(type_token, name_token)?;
        self.expect(TokenKind::Eq)?;
        binding.default = Some(Box::new(self.parse_expression()?));
        self.expect(TokenKind::Semicolon)?;
        Ok(Stmt {
            kind: StmtKind::VarDecl(binding),
            span: Span {
                start,
                end: self.prev_span().end,
            },
        })
    }

    fn parse_return_type(&mut self) -> ParseResult<Binding> {
        let type_token = self.expect(TokenKind::Ident)?;
        let ty = self.parse_type(type_token)?;
        Ok(match self.peek() {
            Some(t) if t.kind == TokenKind::Ident => {
                let name_token = self.expect(TokenKind::Ident)?;
                self.parse_typed_binding(type_token, name_token)?
            }
            _ => Binding {
                name: None,
                ty,
                default: None,
                span: type_token.span,
            },
        })
    }

    fn parse_params(&mut self) -> ParseResult<Vec<Binding>> {
        let mut params = Vec::new();
        loop {
            match self.peek() {
                Some(t) if t.kind == TokenKind::RParen => break,
                None => break,
                _ => {}
            }
            if !params.is_empty() {
                self.expect(TokenKind::Comma)?;
            }
            let type_token = self.expect(TokenKind::Ident)?;
            let name_token = self.expect(TokenKind::Ident)?;
            params.push(self.parse_typed_binding(type_token, name_token)?);
        }
        Ok(params)
    }

    fn parse_var_assign(
        &mut self,
        name: String,
        kind: &TokenKind,
        ident_span: Span,
    ) -> ParseResult<Stmt> {
        let rhs = match kind {
            TokenKind::PlusPlus | TokenKind::MinusMinus => Expr {
                kind: ExprKind::Int(BigInt::from(1)),
                span: self.prev_span(),
            },
            _ => self.parse_expression()?,
        };
        let value = match BinOp::from_assign_token(kind) {
            Some(op) => {
                let span = Span {
                    start: ident_span.start,
                    end: rhs.span.end,
                };
                Expr {
                    kind: ExprKind::BinOp {
                        lhs: Box::new(Expr {
                            kind: ExprKind::Ident(name.clone()),
                            span: ident_span,
                        }),
                        op,
                        rhs: Box::new(rhs),
                    },
                    span,
                }
            }
            None => rhs,
        };
        self.expect(TokenKind::Semicolon)?;
        Ok(Stmt {
            kind: StmtKind::Assign {
                name,
                value: Box::new(value),
            },
            span: Span {
                start: ident_span.start,
                end: self.prev_span().end,
            },
        })
    }

    fn parse_expr(&mut self, min_bp: u8) -> ParseResult<Expr> {
        let token = self.expect([
            TokenKind::Int,
            TokenKind::Float,
            TokenKind::True,
            TokenKind::False,
            TokenKind::Ident,
            TokenKind::Minus,
            TokenKind::Bang,
            TokenKind::Tilde,
            TokenKind::LParen,
            TokenKind::Loop,
        ])?;
        let mut left = self.nud(token)?;

        loop {
            let l_bp = {
                let Some(t) = self.peek() else { break };
                let Some(l) = lbp(&t.kind) else { break };
                if l < min_bp {
                    break;
                }
                l
            };
            let _ = l_bp;

            let op_token = match self.advance() {
                Some(t) => t,
                None => break,
            };
            left = self.led(left, op_token)?;
        }

        Ok(left)
    }

    /// Null-denotation: handles tokens that start an expression
    /// (literals, identifiers, unary operators, grouped parentheses).
    fn nud(&mut self, token: &'a Token) -> ParseResult<Expr> {
        let start = token.span.start;
        match token.kind {
            TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde => {
                let op = UnaryOp::from_token(&token.kind)
                    .ok_or_else(|| self.error(format!("not a unary operator: {:?}", token.kind)))?;
                let bp = op.precedence();
                let operand = self.parse_expr(bp)?;
                Ok(Expr {
                    span: Span {
                        start,
                        end: operand.span.end,
                    },
                    kind: ExprKind::UnaryOp {
                        op,
                        operand: Box::new(operand),
                    },
                })
            }
            TokenKind::Int => Ok(Expr {
                kind: ExprKind::Int(token.int_value()?.clone()),
                span: token.span,
            }),
            TokenKind::Float => Ok(Expr {
                kind: ExprKind::Float(token.float_value()?),
                span: token.span,
            }),
            TokenKind::True => Ok(Expr {
                kind: ExprKind::Bool(true),
                span: token.span,
            }),
            TokenKind::False => Ok(Expr {
                kind: ExprKind::Bool(false),
                span: token.span,
            }),
            TokenKind::Ident => {
                let name = token.ident_value()?.to_string();
                if self.peek().is_some_and(|t| t.kind == TokenKind::LParen) {
                    self.parse_call(name, start)
                } else {
                    Ok(Expr {
                        kind: ExprKind::Ident(name),
                        span: token.span,
                    })
                }
            }
            TokenKind::LParen => {
                let expr = self.parse_expression()?;
                self.expect(TokenKind::RParen)?;
                Ok(Expr {
                    kind: expr.kind,
                    span: Span {
                        start,
                        end: self.prev_span().end,
                    },
                })
            }
            TokenKind::Loop => self.parse_loop(start),
            _ => Err(self.error(format!("unexpected token in expression: {:?}", token.kind))),
        }
    }

    /// Left-denotation: handles tokens that continue an expression
    /// (binary infix operators and the `x if c else y` ternary).
    fn led(&mut self, left: Expr, token: &'a Token) -> ParseResult<Expr> {
        let start = left.span.start;
        match token.kind {
            TokenKind::If => {
                let condition = self.parse_expression()?;
                self.expect(TokenKind::Else)?;
                let else_branch = self.parse_expression()?;
                Ok(Expr {
                    span: Span {
                        start,
                        end: else_branch.span.end,
                    },
                    kind: ExprKind::IfElse {
                        condition: Box::new(condition),
                        then_branch: Box::new(left),
                        else_branch: Box::new(else_branch),
                    },
                })
            }
            _ => {
                let (r_bp, op) = infix_op(&token.kind).ok_or_else(|| {
                    self.error(format!("not an infix operator: {:?}", token.kind))
                })?;
                let right = self.parse_expr(r_bp)?;
                Ok(Expr {
                    span: Span {
                        start,
                        end: right.span.end,
                    },
                    kind: ExprKind::BinOp {
                        lhs: Box::new(left),
                        op,
                        rhs: Box::new(right),
                    },
                })
            }
        }
    }

    fn parse_call(&mut self, name: String, start: usize) -> ParseResult<Expr> {
        self.expect(TokenKind::LParen)?;
        let mut args = Vec::new();
        loop {
            if self.peek().is_some_and(|t| t.kind == TokenKind::RParen) {
                break;
            }
            if !args.is_empty() {
                self.expect(TokenKind::Comma)?;
            }
            args.push(self.parse_expression()?);
        }
        self.expect(TokenKind::RParen)?;
        Ok(Expr {
            kind: ExprKind::Call { name, args },
            span: Span {
                start,
                end: self.prev_span().end,
            },
        })
    }

    fn parse_body(&mut self) -> ParseResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        while let Some(token) = self.peek() {
            if token.kind == TokenKind::RBrace {
                break;
            }
            stmts.push(self.parse_statement()?);
        }
        Ok(stmts)
    }
}
