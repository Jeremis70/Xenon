use crate::ast::{BinOp, Expr, Function, Program, Stmt, Type, UnaryOp};
use crate::error::{ParseError, ParseResult, TypeError};
use crate::tokens::{Span, Token, TokenKind};

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
            functions.push(self.parse_function()?);
        }
        Ok(Program { functions })
    }

    fn parse_function(&mut self) -> ParseResult<Function> {
        self.expect(TokenKind::Fn)?;
        let name = self.expect(TokenKind::Ident)?.ident_value()?.to_string();

        self.expect(TokenKind::LParen)?;
        self.expect(TokenKind::RParen)?;

        self.expect(TokenKind::Arrow)?;
        let return_type = self.expect(TokenKind::Ident)?.ident_value()?.to_string();

        self.expect(TokenKind::LBrace)?;
        let body = self.parse_body()?;
        self.expect(TokenKind::RBrace)?;

        Ok(Function {
            name,
            return_type,
            body,
        })
    }

    fn parse_type(&self, token: &Token) -> ParseResult<Type> {
        token
            .ident_value()?
            .parse::<Type>()
            .map_err(|e: TypeError| ParseError::new(e.to_string(), token.span))
    }

    fn parse_expression(&mut self) -> ParseResult<Expr> {
        self.parse_expression_with_precedence(0)
    }

    fn parse_statement(&mut self) -> ParseResult<Stmt> {
        let first_token = self.expect([TokenKind::Return, TokenKind::Ident])?;
        match first_token.kind {
            TokenKind::Return => {
                let expr = self.parse_expression()?;
                self.expect(TokenKind::Semicolon)?;
                Ok(Stmt::Return(Box::new(expr)))
            }
            TokenKind::Ident => {
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
                ])?;
                match second_token.kind {
                    TokenKind::Ident => self.parse_var_decl(first_token, second_token),
                    kind if kind.is_assign_op() => {
                        let name = first_token.ident_value()?.to_string();
                        self.parse_var_assign(name, &kind)
                    }
                    _ => unreachable!("expect guarantees valid second token"),
                }
            }
            _ => unreachable!("expect guarantees token is either Return or Ident"),
        }
    }

    fn parse_var_decl(&mut self, type_token: &Token, name_token: &Token) -> ParseResult<Stmt> {
        let ty = self.parse_type(type_token)?;
        let name = name_token.ident_value()?.to_string();
        self.expect(TokenKind::Eq)?;
        let value = Box::new(self.parse_expression()?);
        self.expect(TokenKind::Semicolon)?;
        Ok(Stmt::VarDecl { name, ty, value })
    }

    fn parse_var_assign(&mut self, name: String, kind: &TokenKind) -> ParseResult<Stmt> {
        let rhs = self.parse_expression()?;
        let value = match BinOp::from_assign_token(kind) {
            Some(op) => Expr::BinOp {
                lhs: Box::new(Expr::Ident(name.clone())),
                op,
                rhs: Box::new(rhs),
            },
            None => rhs,
        };
        self.expect(TokenKind::Semicolon)?;
        Ok(Stmt::Assign {
            name,
            value: Box::new(value),
        })
    }

    fn parse_expression_with_precedence(&mut self, min_precedence: u8) -> ParseResult<Expr> {
        let mut left = self.parse_primary()?;

        while let Some(op_token) = self.peek() {
            let Some(op) = BinOp::from_op_token(&op_token.kind) else {
                break;
            };

            let (left_precedence, right_precedence) = op.precedence();
            if left_precedence < min_precedence {
                break;
            }

            self.advance();
            let right = self.parse_expression_with_precedence(right_precedence)?;

            left = Expr::BinOp {
                lhs: Box::new(left),
                op,
                rhs: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_primary(&mut self) -> ParseResult<Expr> {
        let token = self.expect([
            TokenKind::Int,
            TokenKind::Ident,
            TokenKind::Minus,
            TokenKind::Bang,
            TokenKind::Tilde,
            TokenKind::LParen,
        ])?;
        match token.kind {
            TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde => {
                let op =
                    UnaryOp::from_token(&token.kind).expect("token kind guarantees unary operator");
                let precedence = op.precedence();
                Ok(Expr::UnaryOp {
                    op,
                    operand: Box::new(self.parse_expression_with_precedence(precedence)?),
                })
            }
            TokenKind::Int => Ok(Expr::Int(
                token
                    .int_value()
                    .expect("token kind guarantees Int variant"),
            )),
            TokenKind::Ident => Ok(Expr::Ident(
                token
                    .ident_value()
                    .expect("token kind guarantees Ident variant")
                    .to_string(),
            )),
            TokenKind::LParen => {
                let expr = self.parse_expression()?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            _ => Err(self.error(format!("Unexpected token: {:?}", token.kind))),
        }
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
