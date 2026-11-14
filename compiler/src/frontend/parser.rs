use crate::error::{ParseError, ParseResult, TypeError};
use crate::frontend::ast::{
    Attribute, BinOp, Binding, Expr, ExprKind, Function, IncDecOp, Pattern, Program, Stmt,
    StmtKind, Type, UnaryOp,
};
use crate::frontend::precedence::{AS_BP, OpInfo, UNARY_BP, infix_info};
use crate::frontend::tokens::{Span, Token, TokenKind};

/// A simple statement's trailing `if c` or `unless c`.
struct TrailingCondition {
    inverted: bool,
    condition: Expr,
    /// End of the statement the condition guards.
    guarded_end: usize,
}

/// An expression that may end with a statement's trailing condition.
struct Guarded {
    expr: Expr,
    trailing: Option<TrailingCondition>,
    /// Whether `expr` is a conditional expression `a if c else b` written
    /// without parentheses.
    bare_ternary: bool,
}

/// A statement-level list of expressions, `a, b, ..`.
struct ExprList {
    /// The single expression, or a [`ExprKind::Tuple`] of several.
    expr: Expr,
    trailing: Option<TrailingCondition>,
    /// Whether the list had more than one element.
    is_list: bool,
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

        // Without `->`, the function returns `()`.
        let returns = if self.eat(TokenKind::Arrow) {
            self.parse_return_list()?
        } else {
            Vec::new()
        };

        let body = self.parse_block()?;

        Ok(Function {
            name,
            params,
            returns,
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
            TokenKind::LParen => {
                // `()` is unit, `(T)` is `T`, `(T,)` and `(T, U, ..)` are tuples.
                self.advance();
                if self.eat(TokenKind::RParen) {
                    return Ok(Type::unit());
                }
                let first = self.parse_type()?;
                if self.eat(TokenKind::RParen) {
                    return Ok(first);
                }
                self.expect([TokenKind::Comma, TokenKind::RParen])?;
                let mut elems = vec![first];
                while !self.at(TokenKind::RParen) {
                    elems.push(self.parse_type()?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RParen)?;
                Ok(Type::Tuple(elems))
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
        let guarded = self.parse_guarded()?;
        Ok((guarded.expr, guarded.trailing))
    }

    /// [`Self::parse_guarded_expression`], also reporting whether the
    /// expression is an unparenthesized conditional expression.
    fn parse_guarded(&mut self) -> ParseResult<Guarded> {
        let then_branch = self.parse_expr(0)?;
        if !self.at(TokenKind::If) {
            return Ok(Guarded {
                expr: then_branch,
                trailing: None,
                bare_ternary: false,
            });
        }
        let trailing = self
            .parse_trailing_condition()?
            .expect("an `if` starts a trailing condition");
        if !self.eat(TokenKind::Else) {
            return Ok(Guarded {
                expr: then_branch,
                trailing: Some(trailing),
                bare_ternary: false,
            });
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
        Ok(Guarded {
            expr: ternary,
            trailing: trailing_after,
            bare_ternary: true,
        })
    }

    /// Parses a statement-level list `a, b, ..` of guarded expressions.
    ///
    /// Several elements form a [`ExprKind::Tuple`]. A trailing `if c`
    /// without `else` guards the whole statement, so it may only follow the
    /// last element. A conditional expression inside a list must be
    /// parenthesized: `return a, b if c else d;` would read as returning
    /// the pair only if `c`. A one-element tuple needs parentheses, `(x,)`.
    fn parse_expr_list(&mut self) -> ParseResult<ExprList> {
        let mut items = Vec::new();
        let mut bare_ternary = None;
        let trailing = loop {
            let item = self.parse_guarded()?;
            if item.bare_ternary {
                bare_ternary.get_or_insert(item.expr.span);
            }
            items.push(item.expr);
            if let Some(trailing) = item.trailing {
                if self.at(TokenKind::Comma) {
                    return Err(self.error(
                        "a trailing `if` or `unless` must end the statement; \
                         parenthesize a conditional expression in a list",
                    ));
                }
                break Some(trailing);
            }
            if !self.eat(TokenKind::Comma) {
                break None;
            }
            if self.at_statement_end() || self.at(TokenKind::Eq) {
                return Err(ParseError::new(
                    "trailing comma: a one-element tuple needs parentheses, as in `(x,)`",
                    self.prev_span(),
                ));
            }
        };
        if items.len() == 1 {
            let expr = items.pop().expect("one item");
            return Ok(ExprList {
                expr,
                trailing,
                is_list: false,
            });
        }
        if let Some(span) = bare_ternary {
            return Err(ParseError::new(
                "parenthesize the conditional expression in this list, \
                 as in `a, (b if c else d)`",
                span,
            ));
        }
        let span = items[0].span.to(items[items.len() - 1].span);
        Ok(ExprList {
            expr: Expr {
                kind: ExprKind::Tuple(items),
                span,
            },
            trailing,
            is_list: true,
        })
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
            | TokenKind::Underscore
            | TokenKind::LParen
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
            if self.at_statement_end() {
                return Ok((StmtKind::Return(None), None));
            }
            let list = self.parse_expr_list()?;
            return Ok((StmtKind::Return(Some(Box::new(list.expr))), list.trailing));
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

        let ExprList {
            expr,
            trailing,
            is_list,
        } = self.parse_expr_list()?;
        match self.peek() {
            Some(t) if trailing.is_none() && t.kind == TokenKind::Eq => {
                if !expr.is_assign_target() {
                    return Err(ParseError::new(
                        "invalid assignment target: expected a variable, a dereference, \
                         a tuple element, `_`, or a list of those",
                        expr.span,
                    ));
                }
                self.advance();
                self.parse_var_assign(expr, &t.kind)
            }
            Some(t) if trailing.is_none() && t.kind.is_assign_op() => {
                if !expr.is_place() {
                    let message = if matches!(expr.kind, ExprKind::Tuple(_)) {
                        "compound assignment needs a single place, not a list"
                    } else {
                        "invalid assignment target: expected a variable or a dereference"
                    };
                    return Err(ParseError::new(message, expr.span));
                }
                self.advance();
                self.parse_var_assign(expr, &t.kind)
            }
            _ if is_list => Err(ParseError::new(
                "a list of values is only allowed in `return`, `let`, and assignments",
                expr.span,
            )),
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
        let pattern = self.parse_pattern_list()?;
        self.expect(TokenKind::Eq)?;
        let init = self.parse_expr_list()?;
        if init.trailing.is_some() {
            return Err(self.error("expected `else` to complete the `if` expression"));
        }
        self.expect(TokenKind::Semicolon)?;
        let init = Box::new(init.expr);
        let kind = match pattern {
            Pattern::Binding(mut binding) => {
                binding.default = Some(init);
                StmtKind::VarDecl(binding)
            }
            pattern => StmtKind::Destructure { pattern, init },
        };
        Ok(Stmt {
            kind,
            span: self.span_since(start),
        })
    }

    /// Parses the patterns of a `let`: one pattern, or a bare list
    /// `p1, p2, ..` that destructures a tuple.
    fn parse_pattern_list(&mut self) -> ParseResult<Pattern> {
        let first = self.parse_pattern()?;
        if !self.at(TokenKind::Comma) {
            return Ok(first);
        }
        let start = first.span().start;
        let mut items = vec![first];
        while self.eat(TokenKind::Comma) {
            if self.at(TokenKind::Eq) {
                return Err(ParseError::new(
                    "trailing comma: a one-element pattern needs parentheses, as in `(i32 x,)`",
                    self.prev_span(),
                ));
            }
            items.push(self.parse_pattern()?);
        }
        Ok(Pattern::Tuple(items, self.span_since(start)))
    }

    /// Parses one pattern: `_`, `<type> <name>`, or `( patterns )`.
    ///
    /// After `(`, the input is either a tuple type, as in
    /// `(i32, i32) pair`, or a nested pattern, as in `(i32 a, i32 b)`. A
    /// type is tried first; if it parses, the parser commits to a binding
    /// of that type. Otherwise the attempt is discarded without a trace
    /// and the input is parsed as a nested pattern, whose error, if any, is
    /// the one reported.
    fn parse_pattern(&mut self) -> ParseResult<Pattern> {
        let token = self
            .peek()
            .ok_or_else(|| self.error("expected a pattern"))?;
        match token.kind {
            TokenKind::Underscore => {
                self.advance();
                Ok(Pattern::Wildcard(token.span))
            }
            TokenKind::LParen => {
                let saved = self.position;
                match self.parse_type() {
                    Ok(ty) => {
                        let Some(name_token) = self.peek().filter(|t| t.kind == TokenKind::Ident)
                        else {
                            return Err(self
                                .error(format!("expected a variable name after the type `{ty}`")));
                        };
                        self.advance();
                        Ok(Pattern::Binding(Binding {
                            name: Some(name_token.ident_value()?.to_string()),
                            ty,
                            default: None,
                            span: token.span.to(name_token.span),
                        }))
                    }
                    Err(_) => {
                        self.position = saved;
                        self.parse_paren_pattern()
                    }
                }
            }
            _ => Ok(Pattern::Binding(self.parse_typed_binding()?)),
        }
    }

    /// Parses `( p )`, which is `p`, or `( p, .. )`, a tuple pattern.
    fn parse_paren_pattern(&mut self) -> ParseResult<Pattern> {
        let start = self.expect(TokenKind::LParen)?.span.start;
        if self.at(TokenKind::RParen) {
            return Err(self.error("expected a pattern: `()` has no values to bind"));
        }
        let first = self.parse_pattern()?;
        if self.eat(TokenKind::RParen) {
            return Ok(first);
        }
        self.expect([TokenKind::Comma, TokenKind::RParen])?;
        let mut items = vec![first];
        while !self.at(TokenKind::RParen) {
            items.push(self.parse_pattern()?);
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;
        Ok(Pattern::Tuple(items, self.span_since(start)))
    }

    /// Parses the results after `->`: `T`, `T name`, or a comma-separated
    /// list of either. Every result must be named, or none.
    fn parse_return_list(&mut self) -> ParseResult<Vec<Binding>> {
        let mut returns = vec![self.parse_return_type()?];
        while self.eat(TokenKind::Comma) {
            returns.push(self.parse_return_type()?);
        }
        let named = returns[0].name.is_some();
        if let Some(mixed) = returns.iter().find(|r| r.name.is_some() != named) {
            return Err(ParseError::new(
                "name every result or none of them",
                mixed.span,
            ));
        }
        Ok(returns)
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
        let ExprList {
            expr: value,
            trailing,
            is_list,
        } = self.parse_expr_list()?;
        let value = Box::new(value);
        let kind = match BinOp::from_compound_assign_token(kind) {
            Some(_) if is_list => {
                return Err(ParseError::new(
                    "compound assignment takes a single value, not a list",
                    value.span,
                ));
            }
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
            // `.N` binds tighter than every prefix and infix operator.
            if t.kind == TokenKind::Period {
                self.advance();
                left = self.parse_field(left)?;
                continue;
            }
            if t.kind == TokenKind::As {
                if AS_BP < min_bp {
                    break;
                }
                self.advance();
                let ty = self.parse_type()?;
                left = Expr {
                    span: self.span_since(left.span.start),
                    kind: ExprKind::Cast {
                        operand: Box::new(left),
                        ty,
                    },
                };
                continue;
            }
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
                // `()` is unit, `(e)` is `e`, `(e,)` and `(e, f, ..)` are tuples.
                if self.eat(TokenKind::RParen) {
                    return Ok(Expr {
                        kind: ExprKind::Tuple(Vec::new()),
                        span: self.span_since(start),
                    });
                }
                let first = self.parse_expression()?;
                if self.eat(TokenKind::RParen) {
                    return Ok(Expr {
                        kind: first.kind,
                        span: self.span_since(start),
                    });
                }
                self.expect([TokenKind::Comma, TokenKind::RParen])?;
                let mut elems = vec![first];
                while !self.at(TokenKind::RParen) {
                    elems.push(self.parse_expression()?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RParen)?;
                Ok(Expr {
                    kind: ExprKind::Tuple(elems),
                    span: self.span_since(start),
                })
            }
            TokenKind::Underscore => Ok(Expr {
                kind: ExprKind::Wildcard,
                span: token.span,
            }),
            TokenKind::Loop => self.parse_loop(start),
            _ => Err(ParseError::new(
                format!("Expected expression, found {:?}", token.kind),
                token.span,
            )),
        }
    }

    /// Parses the tuple index of `base.N`, after its `.`.
    ///
    /// The lexer knows nothing about tuple indices: like any number, it
    /// reads the `0.1` of `t.0.1` as one float. Only here, where an index
    /// is expected, is that float taken apart into the indices `0` and `1`.
    /// An index is plain decimal digits without a leading zero, so
    /// `t.0x1`, `t.01`, `t.1e3`, and `t.1i32` are rejected.
    fn parse_field(&mut self, base: Expr) -> ParseResult<Expr> {
        let token = match self.peek() {
            Some(t) if matches!(t.kind, TokenKind::Int | TokenKind::Float) => t,
            _ => return Err(self.error("expected a tuple index after `.`")),
        };
        self.advance();
        let expr = if token.kind == TokenKind::Int {
            // A plain decimal index is exactly the decimal rendering of its
            // value: `0x1` and `01` also mean 1, but their text is longer.
            let value = token.int_value()?;
            if value.to_string().len() != token.span.end - token.span.start {
                return Err(invalid_tuple_index(token.span));
            }
            field(
                base,
                tuple_index(&value.to_string(), token.span)?,
                token.span,
            )
        } else {
            let text = token.float_text()?;
            let Some((first, second)) = text.split_once('.').filter(|_| !text.contains(['e', 'E']))
            else {
                return Err(invalid_tuple_index(token.span));
            };
            let first_span = Span {
                start: token.span.start,
                end: token.span.start + first.len(),
            };
            let base = field(base, tuple_index(first, first_span)?, first_span);
            if second.is_empty() {
                // `t.0.` with the next index after a space or a comment.
                return self.parse_field(base);
            }
            let second_span = Span {
                start: first_span.end + 1,
                end: token.span.end,
            };
            field(base, tuple_index(second, second_span)?, second_span)
        };
        // A name or number glued to the index, as in `t.1i32` or `t.1e3`.
        if let Some(next) = self.peek()
            && next.span.start == token.span.end
            && matches!(
                next.kind,
                TokenKind::Ident | TokenKind::Int | TokenKind::Float | TokenKind::Underscore
            )
        {
            return Err(invalid_tuple_index(token.span.to(next.span)));
        }
        Ok(expr)
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

/// `base.index`, spanning from `base` to the index.
fn field(base: Expr, index: u32, index_span: Span) -> Expr {
    Expr {
        span: base.span.to(index_span),
        kind: ExprKind::Field {
            base: Box::new(base),
            index,
        },
    }
}

/// The tuple index written `text`: `0`, or decimal digits without a
/// leading zero.
fn tuple_index(text: &str, span: Span) -> ParseResult<u32> {
    let plain = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if !plain {
        return Err(invalid_tuple_index(span));
    }
    text.parse()
        .map_err(|_| ParseError::new("tuple index is too large", span))
}

fn invalid_tuple_index(span: Span) -> ParseError {
    ParseError::new(
        "invalid tuple index: expected plain decimal digits without a leading zero",
        span,
    )
}
