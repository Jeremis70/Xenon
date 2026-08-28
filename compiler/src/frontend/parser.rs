use crate::error::{ParseError, ParseResult, TypeError};
use crate::frontend::ast::{
    Attribute, BinOp, Binding, Expr, ExprKind, Function, Program, Stmt, StmtKind, Type, UnaryOp,
};
use crate::frontend::tokens::{Span, Token, TokenKind};
use num_bigint::BigInt;

/// Binding powers and operator for a binary infix token.
struct OpInfo {
    left_bp: u8,
    right_bp: u8,
    op: BinOp,
}

/// Returns the [`OpInfo`] for binary infix operators.
/// Ternary `if` is handled separately in `led`.
fn infix_info(kind: &TokenKind) -> Option<OpInfo> {
    match kind {
        TokenKind::OrOr => Some(OpInfo { left_bp: 1, right_bp: 2, op: BinOp::LogicalOr }),
        TokenKind::XorXor => Some(OpInfo { left_bp: 3, right_bp: 4, op: BinOp::LogicalXor }),
        TokenKind::AndAnd => Some(OpInfo { left_bp: 5, right_bp: 6, op: BinOp::LogicalAnd }),
        TokenKind::EqEq => Some(OpInfo { left_bp: 7, right_bp: 8, op: BinOp::Eq }),
        TokenKind::NotEq => Some(OpInfo { left_bp: 7, right_bp: 8, op: BinOp::NotEq }),
        TokenKind::Or => Some(OpInfo { left_bp: 9, right_bp: 10, op: BinOp::BitwiseOr }),
        TokenKind::Xor => Some(OpInfo { left_bp: 11, right_bp: 12, op: BinOp::BitwiseXor }),
        TokenKind::And => Some(OpInfo { left_bp: 13, right_bp: 14, op: BinOp::BitwiseAnd }),
        TokenKind::Lt => Some(OpInfo { left_bp: 15, right_bp: 16, op: BinOp::Lt }),
        TokenKind::Gt => Some(OpInfo { left_bp: 15, right_bp: 16, op: BinOp::Gt }),
        TokenKind::LtEq => Some(OpInfo { left_bp: 15, right_bp: 16, op: BinOp::LtEq }),
        TokenKind::GtEq => Some(OpInfo { left_bp: 15, right_bp: 16, op: BinOp::GtEq }),
        TokenKind::LShift => Some(OpInfo { left_bp: 17, right_bp: 18, op: BinOp::LShift }),
        TokenKind::RShift => Some(OpInfo { left_bp: 17, right_bp: 18, op: BinOp::RShift }),
        TokenKind::Plus => Some(OpInfo { left_bp: 19, right_bp: 20, op: BinOp::Add }),
        TokenKind::Minus => Some(OpInfo { left_bp: 19, right_bp: 20, op: BinOp::Sub }),
        TokenKind::Star => Some(OpInfo { left_bp: 21, right_bp: 22, op: BinOp::Mul }),
        TokenKind::Slash => Some(OpInfo { left_bp: 21, right_bp: 22, op: BinOp::Div }),
        TokenKind::Percent => Some(OpInfo { left_bp: 21, right_bp: 22, op: BinOp::Mod }),
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

    fn parse_type(&mut self) -> ParseResult<Type> {
        let token = self.peek().ok_or_else(|| self.error("expected type"))?;
        match token.kind {
            TokenKind::Star => {
                self.advance();
                let inner = self.parse_type()?;
                Ok(Type::Pointer(Box::new(inner)))
            }
            TokenKind::And => {
                self.advance();
                let inner = self.parse_type()?;
                Ok(Type::Reference(Box::new(inner)))
            }
            TokenKind::Ident => {
                let token = self.expect(TokenKind::Ident)?;
                token
                    .ident_value()?
                    .parse::<Type>()
                    .map_err(|e: TypeError| ParseError::new(e.to_string(), token.span))
            }
            _ => Err(ParseError::new(
                format!("expected type, found {:?}", token.kind),
                token.span,
            )),
        }
    }

    fn parse_expression(&mut self) -> ParseResult<Expr> {
        self.parse_expr(0)
    }

    fn parse_statement(&mut self) -> ParseResult<Stmt> {
        let first = self.peek().ok_or_else(|| self.error("expected statement"))?;
        let start = first.span.start;
        match first.kind {
            TokenKind::Return => {
                self.advance();
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
                let next_kind = self.tokens.get(self.position + 1).map(|t| t.kind);
                if next_kind == Some(TokenKind::LParen) {
                    // Function call statement
                    let name_token = self.expect(TokenKind::Ident)?;
                    let call_expr =
                        self.parse_call(name_token.ident_value()?.to_string(), start)?;
                    self.expect(TokenKind::Semicolon)?;
                    Ok(Stmt {
                        kind: StmtKind::Expr(Box::new(call_expr)),
                        span: Span {
                            start,
                            end: self.prev_span().end,
                        },
                    })
                } else if next_kind == Some(TokenKind::Ident) {
                    // Variable declaration: type name = expr;
                    self.parse_var_decl(start)
                } else if next_kind.is_some_and(|k| k.is_assign_op()) {
                    // Assignment or compound assignment
                    let name_token = self.expect(TokenKind::Ident)?;
                    let name = name_token.ident_value()?.to_string();
                    let assign_token = self.expect([
                        TokenKind::Eq,
                        TokenKind::PlusEq,
                        TokenKind::MinusEq,
                        TokenKind::StarEq,
                        TokenKind::SlashEq,
                        TokenKind::PercentEq,
                        TokenKind::AndEq,
                        TokenKind::OrEq,
                        TokenKind::XorEq,
                        TokenKind::LShiftEq,
                        TokenKind::RShiftEq,
                        TokenKind::PlusPlus,
                        TokenKind::MinusMinus,
                    ])?;
                    self.parse_var_assign(name, &assign_token.kind, name_token.span)
                } else {
                    // Consume the ident so the error points at the unexpected token
                    self.advance();
                    Err(self.error(
                        "expected '(', identifier, or assignment operator after identifier",
                    ))
                }
            }
            TokenKind::Star | TokenKind::And => {
                // Pointer/reference type var decl
                self.parse_var_decl(start)
            }
            TokenKind::If => {
                self.advance();
                self.parse_if(start)
            }
            TokenKind::Loop => {
                self.advance();
                let loop_expr = self.parse_loop(start)?;
                Ok(Stmt {
                    span: loop_expr.span,
                    kind: StmtKind::Expr(Box::new(loop_expr)),
                })
            }
            TokenKind::While | TokenKind::Do | TokenKind::Until => {
                let first_token = self.advance().ok_or_else(|| self.error("expected token"))?;
                let loop_expr = self.parse_cond_loop(first_token, start)?;
                Ok(Stmt {
                    span: loop_expr.span,
                    kind: StmtKind::Expr(Box::new(loop_expr)),
                })
            }
            TokenKind::Break => {
                self.advance();
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
                self.advance();
                self.expect(TokenKind::Semicolon)?;
                Ok(Stmt {
                    kind: StmtKind::Continue,
                    span: Span {
                        start,
                        end: self.prev_span().end,
                    },
                })
            }
            _ => Err(self.error("expected statement")),
        }
    }

    fn parse_typed_binding(&mut self) -> ParseResult<Binding> {
        let start = self.peek().map(|t| t.span.start).unwrap_or(0);
        let ty = self.parse_type()?;
        let name_token = self.expect(TokenKind::Ident)?;
        let name = name_token.ident_value()?.to_string();
        Ok(Binding {
            name: Some(name),
            ty,
            default: None,
            span: Span {
                start,
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

    fn parse_var_decl(&mut self, start: usize) -> ParseResult<Stmt> {
        let mut binding = self.parse_typed_binding()?;
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
        let start = self.peek().map(|t| t.span.start).unwrap_or(0);
        let ty = self.parse_type()?;
        Ok(match self.peek() {
            Some(t) if t.kind == TokenKind::Ident => {
                let name_token = self.expect(TokenKind::Ident)?;
                let name = name_token.ident_value()?.to_string();
                Binding {
                    name: Some(name),
                    ty,
                    default: None,
                    span: Span {
                        start,
                        end: name_token.span.end,
                    },
                }
            }
            _ => Binding {
                name: None,
                ty,
                default: None,
                span: Span {
                    start,
                    end: self.prev_span().end,
                },
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
            params.push(self.parse_typed_binding()?);
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

        while let Some(t) = self.peek() {
            let l_bp = infix_info(&t.kind)
                .map(|info| info.left_bp)
                .or(if t.kind == TokenKind::If { Some(0) } else { None });
            let Some(l_bp) = l_bp else { break };
            if l_bp < min_bp {
                break;
            }

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
                let info = infix_info(&token.kind).ok_or_else(|| {
                    self.error(format!("not an infix operator: {:?}", token.kind))
                })?;
                let right = self.parse_expr(info.right_bp)?;
                Ok(Expr {
                    span: Span {
                        start,
                        end: right.span.end,
                    },
                    kind: ExprKind::BinOp {
                        lhs: Box::new(left),
                        op: info.op,
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
