use crate::error::{ParseError, ParseResult, TypeError};
use crate::frontend::ast::{
    Attribute, BinOp, Binding, Expr, ExprKind, Function, IncDecOp, Program, Stmt, StmtKind, Type,
    UnaryOp,
};
use crate::frontend::precedence::{OpInfo, UNARY_BP, infix_info};
use crate::frontend::tokens::{Span, Token, TokenKind};

/// A simple statement's trailing `if c` or `unless c`.
struct TrailingCondition {
    inverted: bool,
    condition: Expr,
    /// End of the statement the condition guards.
    guarded_end: usize,
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

    /// Whether the next token is `kind`.
    fn at(&self, kind: TokenKind) -> bool {
        self.peek().is_some_and(|t| t.kind == kind)
    }

    /// Consumes the next token if it is `kind`.
    fn eat(&mut self, kind: TokenKind) -> bool {
        let found = self.at(kind);
        if found {
            self.position += 1;
        }
        found
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

    /// Creates a span from `start` to the end of the most recently consumed token.
    fn span_since(&self, start: usize) -> Span {
        Span {
            start,
            end: self.prev_span().end,
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
            if !attributes.is_empty() && !self.at(TokenKind::Fn) {
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
        while self.at(TokenKind::Hash) {
            let hash_token = self.expect(TokenKind::Hash)?;
            let start = hash_token.span.start;
            self.expect(TokenKind::LBracket)?;
            let name_token = self.expect(TokenKind::Ident)?;
            let name = name_token.ident_value()?.to_string();
            self.expect(TokenKind::RBracket)?;
            attrs.push(Attribute {
                name,
                span: self.span_since(start),
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

        let body = self.parse_block()?;

        Ok(Function {
            name,
            params,
            return_type,
            body,
            attributes,
            span: self.span_since(start),
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
            TokenKind::Ampersand => {
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
        match self.parse_guarded_expression()? {
            (expr, None) => Ok(expr),
            (_, Some(_)) => Err(self.error("expected `else` to complete the `if` expression")),
        }
    }

    /// Parses an expression that may end with a simple statement's trailing
    /// `if c` (an `if` without `else`), returned alongside it.
    ///
    /// An `if c` is first read as a trailing condition and becomes a ternary
    /// once `else` follows, so every condition is parsed exactly once.
    fn parse_guarded_expression(&mut self) -> ParseResult<(Expr, Option<TrailingCondition>)> {
        let then_branch = self.parse_expr(0)?;
        if !self.at(TokenKind::If) {
            return Ok((then_branch, None));
        }
        let trailing = self
            .parse_trailing_condition()?
            .expect("an `if` starts a trailing condition");
        if !self.eat(TokenKind::Else) {
            return Ok((then_branch, Some(trailing)));
        }
        // Right-associative: `a if b else c if d else e` nests in the else
        // branch, which also carries a trailing condition up to the statement.
        let (else_branch, trailing_after) = self.parse_guarded_expression()?;
        let ternary = Expr {
            span: Span {
                start: then_branch.span.start,
                end: else_branch.span.end,
            },
            kind: ExprKind::IfElse {
                condition: Box::new(trailing.condition),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
        };
        Ok((ternary, trailing_after))
    }

    /// Parses `if c` or `unless c` when next.
    fn parse_trailing_condition(&mut self) -> ParseResult<Option<TrailingCondition>> {
        let inverted = match self.peek() {
            Some(t) if t.kind == TokenKind::If => false,
            Some(t) if t.kind == TokenKind::Unless => true,
            _ => return Ok(None),
        };
        let guarded_end = self.prev_span().end;
        self.advance();
        Ok(Some(TrailingCondition {
            inverted,
            condition: self.parse_expression()?,
            guarded_end,
        }))
    }

    fn parse_statement(&mut self) -> ParseResult<Stmt> {
        let first = self
            .peek()
            .ok_or_else(|| self.error("expected statement"))?;
        let start = first.span.start;
        match first.kind {
            TokenKind::Let => {
                self.advance();
                self.parse_var_decl(start)
            }
            TokenKind::If | TokenKind::Unless => {
                self.advance();
                self.parse_if(first.kind == TokenKind::Unless, start)
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
            TokenKind::Return
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::Ident
            | TokenKind::Star => self.parse_simple_statement(start),
            _ => Err(self.error("expected statement")),
        }
    }

    /// Parses a `;`-terminated statement that may carry a trailing
    /// condition: `return x if c;` is `if c { return x; }` and
    /// `x++ unless done;` is `unless done { x++; }`.
    fn parse_simple_statement(&mut self, start: usize) -> ParseResult<Stmt> {
        let (mut kind, trailing) = self.parse_simple_statement_kind()?;
        let trailing = match trailing {
            Some(trailing) => Some(trailing),
            None => self.parse_trailing_condition()?,
        };
        if let Some(trailing) = trailing {
            kind = StmtKind::If {
                inverted: trailing.inverted,
                condition: Box::new(trailing.condition),
                then_branch: vec![Stmt {
                    kind,
                    span: Span {
                        start,
                        end: trailing.guarded_end,
                    },
                }],
                else_branch: None,
            };
        }
        self.expect(TokenKind::Semicolon)?;
        Ok(Stmt {
            kind,
            span: self.span_since(start),
        })
    }

    /// Parses a simple statement up to its `;`, along with a trailing `if c`
    /// that ended one of its expressions.
    fn parse_simple_statement_kind(
        &mut self,
    ) -> ParseResult<(StmtKind, Option<TrailingCondition>)> {
        if self.eat(TokenKind::Return) {
            let (value, trailing) = self.parse_guarded_expression()?;
            return Ok((StmtKind::Return(Box::new(value)), trailing));
        }
        if self.eat(TokenKind::Break) {
            if self.at_statement_end() {
                return Ok((StmtKind::Break(None), None));
            }
            let (value, trailing) = self.parse_guarded_expression()?;
            return Ok((StmtKind::Break(Some(Box::new(value))), trailing));
        }
        if self.eat(TokenKind::Continue) {
            return Ok((StmtKind::Continue, None));
        }

        let (expr, trailing) = self.parse_guarded_expression()?;
        match self.peek() {
            Some(t) if trailing.is_none() && t.kind.is_assign_op() => {
                if !expr.is_place() {
                    return Err(ParseError::new(
                        "invalid assignment target: expected a variable or a dereference",
                        expr.span,
                    ));
                }
                self.advance();
                self.parse_var_assign(expr, &t.kind)
            }
            // Expression statement (e.g. a function call).
            _ => Ok((StmtKind::Expr(Box::new(expr)), trailing)),
        }
    }

    /// Whether the simple statement being parsed has no more operands:
    /// next is its `;` or a trailing condition.
    fn at_statement_end(&self) -> bool {
        self.peek().is_none_or(|t| {
            matches!(
                t.kind,
                TokenKind::Semicolon | TokenKind::If | TokenKind::Unless
            )
        })
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
        let body = self.parse_block()?;
        Ok(Expr {
            kind: ExprKind::Loop { body },
            span: self.span_since(start),
        })
    }

    fn parse_cond_loop(&mut self, token: &Token, start: usize) -> ParseResult<Expr> {
        match token.kind {
            TokenKind::While | TokenKind::Until => {
                let inverted = token.kind == TokenKind::Until;
                let condition = Box::new(self.parse_expression()?);
                let body = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::CondLoop {
                        post: false,
                        inverted,
                        condition,
                        body,
                    },
                    span: self.span_since(start),
                })
            }
            TokenKind::Do => {
                let body = self.parse_block()?;
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
                    span: self.span_since(start),
                })
            }
            _ => unreachable!("expect guarantees token is While, Until, or Do"),
        }
    }

    /// Parses the rest of `if c { .. }` with an optional `else` / `else if`
    /// chain, or of `unless c { .. }` (`inverted`), which takes no `else`.
    fn parse_if(&mut self, inverted: bool, start: usize) -> ParseResult<Stmt> {
        let condition = Box::new(self.parse_expression()?);
        let then_branch = self.parse_block()?;
        let else_branch = match self.peek() {
            Some(t) if t.kind == TokenKind::Else => {
                if inverted {
                    return Err(ParseError::new(
                        "`unless` cannot have an `else` branch",
                        t.span,
                    ));
                }
                self.advance();
                Some(self.parse_else()?)
            }
            _ => None,
        };

        Ok(Stmt {
            kind: StmtKind::If {
                inverted,
                condition,
                then_branch,
                else_branch,
            },
            span: self.span_since(start),
        })
    }

    /// Parses what follows `else`: a block or a chained `if`.
    fn parse_else(&mut self) -> ParseResult<Vec<Stmt>> {
        match self.peek() {
            Some(t) if t.kind == TokenKind::If => {
                self.advance();
                Ok(vec![self.parse_if(false, t.span.start)?])
            }
            _ => self.parse_block(),
        }
    }

    fn parse_var_decl(&mut self, start: usize) -> ParseResult<Stmt> {
        let mut binding = self.parse_typed_binding()?;
        self.expect(TokenKind::Eq)?;
        binding.default = Some(Box::new(self.parse_expression()?));
        self.expect(TokenKind::Semicolon)?;
        Ok(Stmt {
            kind: StmtKind::VarDecl(binding),
            span: self.span_since(start),
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
                span: self.span_since(start),
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

    /// Parses the remainder of an assignment statement whose target has
    /// already been parsed as a place expression.
    ///
    /// No desugaring happens here: `x += e` and `x++` keep their own AST
    /// nodes so the tree mirrors the source.
    fn parse_var_assign(
        &mut self,
        target: Expr,
        kind: &TokenKind,
    ) -> ParseResult<(StmtKind, Option<TrailingCondition>)> {
        let target = Box::new(target);
        if let Some(op) = IncDecOp::from_token(kind) {
            return Ok((StmtKind::IncDec { target, op }, None));
        }
        let (value, trailing) = self.parse_guarded_expression()?;
        let value = Box::new(value);
        let kind = match BinOp::from_compound_assign_token(kind) {
            Some(op) => StmtKind::CompoundAssign { target, op, value },
            None => StmtKind::Assign { target, value },
        };
        Ok((kind, trailing))
    }

    fn parse_expr(&mut self, min_bp: u8) -> ParseResult<Expr> {
        let token = self
            .advance()
            .ok_or_else(|| self.error("expected expression, found end of input"))?;
        let mut left = self.nud(token)?;

        while let Some(t) = self.peek() {
            let Some(info) = infix_info(&t.kind) else {
                break;
            };
            if info.left_bp < min_bp {
                break;
            }
            self.advance();
            left = self.led(left, info)?;
        }

        Ok(left)
    }

    /// Null-denotation: handles tokens that start an expression
    /// (literals, identifiers, unary operators, grouped parentheses).
    fn nud(&mut self, token: &'a Token) -> ParseResult<Expr> {
        let start = token.span.start;
        match token.kind {
            TokenKind::Minus | TokenKind::Not | TokenKind::Tilde => {
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
            TokenKind::Star => {
                // Prefix `*` is dereference. Infix `*` (multiplication) is
                // handled in `led`, so the Pratt split disambiguates them.
                let operand = self.parse_expr(UNARY_BP)?;
                Ok(Expr {
                    span: Span {
                        start,
                        end: operand.span.end,
                    },
                    kind: ExprKind::Deref(Box::new(operand)),
                })
            }
            TokenKind::At => {
                // `@<int literal>` is an address literal; `@<place>` is
                // address-of. The expected type decides whether address-of
                // yields a pointer or a reference (resolved during validation).
                if let Some(t) = self.peek()
                    && t.kind == TokenKind::Int
                {
                    let literal = self.expect(TokenKind::Int)?;
                    return Ok(Expr {
                        kind: ExprKind::Address(literal.int_value()?.clone()),
                        span: Span {
                            start,
                            end: literal.span.end,
                        },
                    });
                }
                let operand = self.parse_expr(UNARY_BP)?;
                if !operand.is_place() {
                    return Err(ParseError::new(
                        "`@` expects an addressable location",
                        operand.span,
                    ));
                }
                Ok(Expr {
                    span: Span {
                        start,
                        end: operand.span.end,
                    },
                    kind: ExprKind::AddressOf(Box::new(operand)),
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
                if self.at(TokenKind::LParen) {
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
                    span: self.span_since(start),
                })
            }
            TokenKind::Loop => self.parse_loop(start),
            _ => Err(ParseError::new(
                format!("Expected expression, found {:?}", token.kind),
                token.span,
            )),
        }
    }

    /// Left-denotation: continues `left` with a binary infix operator.
    fn led(&mut self, left: Expr, info: OpInfo) -> ParseResult<Expr> {
        let right = self.parse_expr(info.right_bp)?;
        Ok(Expr {
            span: Span {
                start: left.span.start,
                end: right.span.end,
            },
            kind: ExprKind::BinOp {
                lhs: Box::new(left),
                op: info.op,
                rhs: Box::new(right),
            },
        })
    }

    fn parse_call(&mut self, name: String, start: usize) -> ParseResult<Expr> {
        self.expect(TokenKind::LParen)?;
        let mut args = Vec::new();
        loop {
            if self.at(TokenKind::RParen) {
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
            span: self.span_since(start),
        })
    }

    /// Parses `{ stmts }`.
    fn parse_block(&mut self) -> ParseResult<Vec<Stmt>> {
        self.expect(TokenKind::LBrace)?;
        let body = self.parse_body()?;
        self.expect(TokenKind::RBrace)?;
        Ok(body)
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
