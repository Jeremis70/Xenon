use num_bigint::BigInt;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::tokens::{Span, TokenKind};

#[test]
fn lex_emits_kinds_and_spans_point_to_source() {
    let src = "fn x()->u32{return 42;}";
    let tokens = lex(src).expect("lexing should succeed");

    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::Fn,
            TokenKind::Ident,
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::Arrow,
            TokenKind::Ident,
            TokenKind::LBrace,
            TokenKind::Return,
            TokenKind::Int,
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]
    );

    assert_eq!(tokens[1].ident_value().unwrap(), "x");
    assert_eq!(tokens[5].ident_value().unwrap(), "u32");
    assert_eq!(*tokens[8].int_value().unwrap(), BigInt::from(42));

    // Verify that the spans point to the correct substrings in the source
    assert_eq!(&src[tokens[0].span.start..tokens[0].span.end], "fn");
    assert_eq!(&src[tokens[1].span.start..tokens[1].span.end], "x");
    assert_eq!(&src[tokens[4].span.start..tokens[4].span.end], "->");
    assert_eq!(&src[tokens[8].span.start..tokens[8].span.end], "42");
}

#[test]
fn lex_string_literal_strip_quotes_no_decode() {
    let src = "\"hi\\n\"";
    let tokens = lex(src).expect("lexing should succeed");

    assert_eq!(tokens.len(), 1);
    assert_eq!(
        tokens[0].span,
        Span {
            start: 0,
            end: src.len()
        }
    );

    assert_eq!(tokens[0].kind, TokenKind::Str);
    assert_eq!(tokens[0].str_value().unwrap(), "hi\\n");
}

#[test]
fn lex_invalid_input_returns_error() {
    let err = lex("fn ¸").expect_err("lexing should fail on invalid token");

    assert_eq!(err.span, Span { start: 3, end: 5 });
}

#[test]
fn lex_hash_token() {
    let src = "#[entry]";
    let tokens = lex(src).expect("lexing should succeed");

    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Hash,
            TokenKind::LBracket,
            TokenKind::Ident,
            TokenKind::RBracket,
        ]
    );
    assert_eq!(tokens[2].ident_value().unwrap(), "entry");
    assert_eq!(&src[tokens[0].span.start..tokens[0].span.end], "#");
}

#[test]
fn lex_logical_keywords() {
    let tokens = lex("and or xor not android order").expect("lexing should succeed");

    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::And,
            TokenKind::Or,
            TokenKind::Xor,
            TokenKind::Not,
            TokenKind::Ident,
            TokenKind::Ident,
        ]
    );
}

#[test]
fn lex_bitwise_operators() {
    let tokens = lex("& | ^ &= |= ^=").expect("lexing should succeed");

    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Ampersand,
            TokenKind::Pipe,
            TokenKind::Caret,
            TokenKind::AmpersandEq,
            TokenKind::PipeEq,
            TokenKind::CaretEq,
        ]
    );
}

fn kinds(src: &str) -> Vec<TokenKind> {
    lex(src)
        .expect("lexing should succeed")
        .iter()
        .map(|t| t.kind)
        .collect()
}

#[test]
fn tuple_index_lexes_as_period_int() {
    use TokenKind::*;
    assert_eq!(kinds("t.0"), vec![Ident, Period, Int]);
    assert_eq!(kinds("t.10"), vec![Ident, Period, Int]);
    let tokens = lex("t.10").expect("lexing should succeed");
    assert_eq!(*tokens[2].int_value().unwrap(), BigInt::from(10));
}

/// The lexer has no notion of tuple indices: `0.12` after a `.` is a
/// float like any other. The parser takes it apart.
#[test]
fn nested_tuple_index_lexes_as_a_float() {
    use TokenKind::*;
    let tokens = lex("t.0.12").expect("lexing should succeed");
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, vec![Ident, Period, Float]);
    assert_eq!(tokens[2].float_text().unwrap(), "0.12");
}

#[test]
fn floats_need_leading_digits() {
    use TokenKind::*;
    assert_eq!(kinds("1.5"), vec![Float]);
    assert_eq!(kinds("x = .5"), vec![Ident, Eq, Period, Int]);
}

#[test]
fn underscore_is_its_own_token() {
    use TokenKind::*;
    assert_eq!(kinds("_ _x x_ __"), vec![Underscore, Ident, Ident, Ident]);
}
