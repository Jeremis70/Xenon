use crate::ast::*;
use crate::tokens::{Span, Token, TokenKind};

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

pub type ParseResult<T> = Result<T, ParseError>;

impl ParseError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (span {}..{})",
            self.message, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for ParseError {}
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

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<&Token> {
        if self.position < self.tokens.len() {
            let idx = self.position;
            self.position += 1;
            self.tokens.get(idx)
        } else {
            None
        }
    }

    fn expect(&mut self, expected: TokenKind) -> ParseResult<()> {
        match self.advance() {
            Some(t) if t.kind == expected => Ok(()),
            Some(t) => Err(ParseError::new(
                format!("Expected {:?}, found {:?}", expected, t.kind),
                t.span,
            )),
            None => Err(self.error(format!("Expected {:?}, found end of input", expected))),
        }
    }

    fn expect_ident_string(&mut self) -> ParseResult<String> {
        match self.advance() {
            Some(Token {
                kind: TokenKind::Ident(s),
                ..
            }) => Ok(s.clone()),
            Some(t) => Err(ParseError::new(
                format!("Expected identifier, found {:?}", t.kind),
                t.span,
            )),
            None => Err(self.error("Expected identifier, found end of input")),
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
        let name = self.expect_ident_string()?;

        self.expect(TokenKind::LParen)?;
        self.expect(TokenKind::RParen)?;

        self.expect(TokenKind::Arrow)?;
        let return_type = self.expect_ident_string()?;

        self.expect(TokenKind::LBrace)?;
        let body = self.parse_body_mvp()?;
        self.expect(TokenKind::RBrace)?;

        Ok(Function {
            name,
            return_type,
            body,
        })
    }

    fn parse_body_mvp(&mut self) -> ParseResult<Vec<Expr>> {
        let mut body = Vec::new();

        self.expect(TokenKind::Return)?;

        let expr = match self.advance() {
            Some(Token {
                kind: TokenKind::Int(v),
                ..
            }) => Expr::Int(*v as i64),
            Some(Token {
                kind: TokenKind::Ident(s),
                ..
            }) => Expr::Ident(s.clone()),
            Some(t) => {
                return Err(ParseError::new(
                    format!("Expected integer or identifier, found {:?}", t.kind),
                    t.span,
                ));
            }
            None => return Err(self.error("Expected expression, found end of input")),
        };

        self.expect(TokenKind::Semicolon)?;
        body.push(Expr::Return(Box::new(expr)));

        Ok(body)
    }
}
