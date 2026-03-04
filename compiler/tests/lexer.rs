use xenonc::lexer::lex;
use xenonc::tokens::{Span, TokenKind};

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

    assert_eq!(tokens[1].value().as_ident(), "x");
    assert_eq!(tokens[5].value().as_ident(), "u32");
    assert_eq!(tokens[8].value().as_int(), 42);

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
    assert_eq!(tokens[0].value().as_str(), "hi\\n");
}

#[test]
fn lex_invalid_input_returns_error() {
    let err = lex("fn @").expect_err("lexing should fail on invalid token");

    assert_eq!(err.span, Span { start: 3, end: 4 });
}
