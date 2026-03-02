use crate::ast::*;
use crate::tokens::{Token, TokenKind};

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

    fn expect(&mut self, expected: &TokenKind) -> Result<(), String> {
        match self.advance() {
            Some(t) if &t.kind == expected => Ok(()),
            Some(t) => Err(format!("Expected {:?}, found {:?}", expected, t.kind)),
            None => Err(format!("Expected {:?}, found end of input", expected)),
        }
    }

    fn expect_ident_string(&mut self) -> Result<String, String> {
        match self.advance() {
            Some(Token {
                kind: TokenKind::Ident(s),
                ..
            }) => Ok(s.clone()),
            Some(t) => Err(format!("Expected Ident(_), found {:?}", t.kind)),
            None => Err("Expected Ident(_), found end of input".into()),
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, String> {
        let mut functions = Vec::new();
        while self.peek().is_some() {
            functions.push(self.parse_function()?);
        }
        Ok(Program { functions })
    }

    fn parse_function(&mut self) -> Result<Function, String> {
        self.expect(&TokenKind::Fn)?;
        let name = self.expect_ident_string()?;

        self.expect(&TokenKind::LParen)?;
        self.expect(&TokenKind::RParen)?;

        self.expect(&TokenKind::Arrow)?;
        let return_type = self.expect_ident_string()?; // "u32" for now

        self.expect(&TokenKind::LBrace)?;
        let body = self.parse_body_mvp()?;
        self.expect(&TokenKind::RBrace)?;

        Ok(Function {
            name,
            return_type,
            body,
        })
    }

    fn parse_body_mvp(&mut self) -> Result<Vec<Expr>, String> {
        let mut body = Vec::new();

        self.expect(&TokenKind::Return)?;

        let expr = match self.advance() {
            Some(Token {
                kind: TokenKind::Int(v),
                ..
            }) => Expr::Int(*v as i64),
            Some(Token {
                kind: TokenKind::Ident(s),
                ..
            }) => Expr::Ident(s.clone()),
            Some(t) => return Err(format!("Expected Int(_) or Ident(_), found {:?}", t.kind)),
            None => return Err("Expected expression, found end of input".into()),
        };

        self.expect(&TokenKind::Semicolon)?;
        body.push(Expr::Return(Box::new(expr)));

        Ok(body)
    }
}
