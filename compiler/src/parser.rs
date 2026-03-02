use crate::ast::*;
use crate::lexer::Token;

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

    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        match self.advance() {
            Some(t) if t == expected => Ok(()),
            Some(t) => Err(format!("Expected {:?}, found {:?}", expected, t)),
            None => Err(format!("Expected {:?}, found end of input", expected)),
        }
    }

    fn expect_ident_string(&mut self) -> Result<String, String> {
        match self.advance() {
            Some(Token::Ident(s)) => Ok(s.clone()),
            Some(t) => Err(format!("Expected Ident(_), found {:?}", t)),
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
        self.expect(&Token::Fn)?;
        let name = self.expect_ident_string()?;

        self.expect(&Token::LParen)?;
        self.expect(&Token::RParen)?;

        self.expect(&Token::Arrow)?;
        let return_type = self.expect_ident_string()?; // "u32" for now

        self.expect(&Token::LBrace)?;
        let body = self.parse_body_mvp()?;
        self.expect(&Token::RBrace)?;

        Ok(Function {
            name,
            return_type,
            body,
        })
    }

    fn parse_body_mvp(&mut self) -> Result<Vec<Expr>, String> {
        let mut body = Vec::new();

        self.expect(&Token::Return)?;

        let expr = match self.advance() {
            Some(Token::Int(v)) => Expr::Int(*v as i64),
            Some(Token::Ident(s)) => Expr::Ident(s.clone()),
            Some(t) => return Err(format!("Expected Int(_) or Ident(_), found {:?}", t)),
            None => return Err("Expected expression, found end of input".into()),
        };

        self.expect(&Token::Semicolon)?;
        body.push(Expr::Return(Box::new(expr)));

        Ok(body)
    }
}
