//! Tokenization, followed by the re-lexing of tuple indices.

use num_bigint::BigInt;

use crate::error::{LexError, LexResult};
use crate::frontend::tokens::{Span, Token, TokenKind, TokenValue, tokenize};

pub fn lex(input: &str) -> LexResult<Vec<Token>> {
    let tokens = tokenize(input).map_err(LexError::invalid_token)?;
    split_tuple_indices(input, tokens)
}

/// Re-lexes the numbers that follow a `.` as tuple indices.
///
/// The number after `.` is a field index, never a literal, so only plain
/// decimal digits without a leading zero are accepted: `t.0x1`, `t.01`,
/// `t.1e3`, and `t.1i32` are errors (the check reads the raw text, since
/// `0x1` and `1` have the same value). The float in `t.0.1` is split into
/// `0 . 1`, so nested indices lex as nested field accesses.
fn split_tuple_indices(input: &str, tokens: Vec<Token>) -> LexResult<Vec<Token>> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut tokens = tokens.into_iter().peekable();
    while let Some(token) = tokens.next() {
        let after_period = out.last().is_some_and(|t| t.kind == TokenKind::Period);
        if !after_period || !matches!(token.kind, TokenKind::Int | TokenKind::Float) {
            out.push(token);
            continue;
        }
        // A name or number glued to the index, as in `t.1i32`.
        if let Some(next) = tokens.peek()
            && next.span.start == token.span.end
            && matches!(
                next.kind,
                TokenKind::Ident | TokenKind::Int | TokenKind::Float | TokenKind::Underscore
            )
        {
            return Err(LexError::invalid_tuple_index(token.span.to(next.span)));
        }
        let text = &input[token.span.start..token.span.end];
        match token.kind {
            TokenKind::Int => out.push(index_token(text, token.span)?),
            _ => {
                // An exponent makes the whole float invalid, not one half.
                let Some((first, second)) =
                    text.split_once('.').filter(|_| !text.contains(['e', 'E']))
                else {
                    return Err(LexError::invalid_tuple_index(token.span));
                };
                let first_end = token.span.start + first.len();
                out.push(index_token(
                    first,
                    Span {
                        start: token.span.start,
                        end: first_end,
                    },
                )?);
                out.push(Token {
                    kind: TokenKind::Period,
                    value: None,
                    span: Span {
                        start: first_end,
                        end: first_end + 1,
                    },
                });
                out.push(index_token(
                    second,
                    Span {
                        start: first_end + 1,
                        end: token.span.end,
                    },
                )?);
            }
        }
    }
    Ok(out)
}

/// The `Int` token of the tuple index `text`, which must be `0` or a
/// decimal number without a leading zero.
fn index_token(text: &str, span: Span) -> LexResult<Token> {
    let valid = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    let value = valid
        .then(|| BigInt::parse_bytes(text.as_bytes(), 10))
        .flatten()
        .ok_or(LexError::invalid_tuple_index(span))?;
    Ok(Token {
        kind: TokenKind::Int,
        value: Some(TokenValue::Int(value)),
        span,
    })
}
