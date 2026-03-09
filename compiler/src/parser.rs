use crate::ast::*;
use crate::error::{ParseError, ParseResult};
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
        let body = self.parse_body_mvp()?;
        self.expect(TokenKind::RBrace)?;

        Ok(Function {
            name,
            return_type,
            body,
        })
    }

    fn parse_expression(&mut self) -> ParseResult<Expr> {
        self.parse_expression_with_precedence(0)
    }

    fn parse_expression_with_precedence(&mut self, min_precedence: u8) -> ParseResult<Expr> {
        let mut left = self.parse_primary()?;

        while let Some(op_token) = self.peek() {
            let Some(op) = BinOp::from_token(&op_token.kind) else {
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
            TokenKind::LParen,
        ])?;
        match token.kind {
            TokenKind::Minus | TokenKind::Bang => Ok(Expr::UnaryOp {
                op: UnaryOp::from_token(&token.kind).unwrap(),
                operand: Box::new(self.parse_primary()?),
            }),
            TokenKind::Int => Ok(Expr::Int(token.int_value().unwrap())),
            TokenKind::Ident => Ok(Expr::Ident(token.ident_value().unwrap().to_string())),
            TokenKind::LParen => {
                let expr = self.parse_expression()?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            _ => Err(self.error(format!("Unexpected token: {:?}", token.kind))),
        }
    }

    fn parse_body_mvp(&mut self) -> ParseResult<Vec<Expr>> {
        self.expect(TokenKind::Return)?;
        let expr = self.parse_expression()?;
        println!("Parsed return expression: {expr:?}");
        self.expect(TokenKind::Semicolon)?;

        Ok(vec![Expr::Return(Box::new(expr))])
    }
}
