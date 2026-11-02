//! # ailang-lexer
//!
//! Converts AILang source text into a stream of [`Token`]s using `logos`.
//!
//! The lexer is deliberately kept as a *pure function* (`&str -> Vec<Spanned
//! Token>` plus diagnostics) with no parser knowledge, so future phases
//! (formatter, LSP semantic highlighting, incremental re-lexing) can reuse it
//! directly.

use ailang_diagnostics::{Diagnostic, DiagnosticBag, Span};
use logos::Logos;
use serde::{Deserialize, Serialize};

#[derive(Logos, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
#[logos(skip r"/\*([^*]|\*[^/])*\*/")]
pub enum Token {
    // ---- Keywords ----
    #[token("let")]
    Let,
    #[token("const")]
    Const,
    #[token("fn")]
    Fn,
    #[token("return")]
    Return,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("while")]
    While,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("import")]
    Import,
    #[token("module")]
    Module,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("true")]
    True,
    #[token("false")]
    False,

    // ---- v0.5 AI-native keywords ----
    // Promoted from `ailang_ast::FUTURE_RESERVED_KEYWORDS` (v0.1) to real
    // reserved words. Each has grammar in `ailang-parser` and a `Stmt`/
    // `Expr` variant in `ailang-ast` — see that crate's module doc.
    #[token("agent")]
    Agent,
    #[token("model")]
    Model,
    #[token("workflow")]
    Workflow,
    #[token("tool")]
    Tool,
    #[token("memory")]
    Memory,
    #[token("knowledge")]
    Knowledge,
    #[token("prompt")]
    Prompt,
    #[token("reasoning")]
    Reasoning,
    #[token("schema")]
    Schema,
    #[token("pipeline")]
    Pipeline,
    #[token("parallel")]
    Parallel,
    #[token("permission")]
    Permission,
    #[token("deploy")]
    Deploy,
    #[token("team")]
    Team,
    #[token("embedding")]
    Embedding,
    #[token("dataset")]
    Dataset,
    #[token("vector")]
    Vector,
    #[token("task")]
    Task,
    #[token("generate")]
    Generate,

    // ---- v2.0 distributed-intelligence keywords ----
    #[token("network")]
    Network,
    #[token("cluster")]
    Cluster,
    #[token("on")]
    On,

    // ---- Primitive type keywords ----
    #[token("int")]
    IntType,
    #[token("float")]
    FloatType,
    #[token("bool")]
    BoolType,
    #[token("string")]
    StringType,
    #[token("char")]
    CharType,
    #[token("null")]
    Null,
    #[token("void")]
    Void,

    // ---- Literals ----
    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    FloatLiteral(f64),
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    IntLiteral(i64),
    // Triple-quoted strings (`"""..."""`) support multi-line text, used
    // primarily for `prompt` templates. They're matched with an explicit
    // higher priority than `StringLiteral` below so `""""""` (an empty
    // triple-quoted string) isn't mis-tokenized as an empty `StringLiteral`
    // followed by a stray `"`. The content is scanned manually in
    // `scan_triple_quoted` rather than via a single regex, because logos's
    // regex subset doesn't support the non-greedy multi-line matching a
    // "find the next `\"\"\"`" scan needs.
    #[token("\"\"\"", scan_triple_quoted, priority = 10)]
    TripleQuotedString(String),
    #[regex(r#""([^"\\]|\\.)*""#, |lex| unescape_string(lex.slice()))]
    StringLiteral(String),
    #[regex(r"'([^'\\]|\\.)'", |lex| unescape_char(lex.slice()))]
    CharLiteral(char),
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice().to_string())]
    Identifier(String),

    // ---- Operators ----
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token(">=")]
    GtEq,
    #[token("<=")]
    LtEq,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("->")]
    Arrow,
    #[token("..=")]
    DotDotEq,
    #[token("..")]
    DotDot,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("=")]
    Eq,
    #[token(">")]
    Gt,
    #[token("<")]
    Lt,
    #[token("!")]
    Bang,
    #[token(".")]
    Dot,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,

    Eof,
}

fn unescape_string(raw: &str) -> Option<String> {
    let inner = &raw[1..raw.len() - 1];
    Some(unescape_common(inner))
}

/// Callback for `Token::TripleQuotedString`. Called with the lexer
/// positioned right after the opening `"""`; scans `lex.remainder()` for
/// the closing `"""`, consumes through it via `lex.bump`, and returns the
/// content in between verbatim (no escape processing — triple-quoted
/// strings are meant for literal multi-line prompt text, so `\n` etc. are
/// not special inside them; only the real newlines the author typed are).
fn scan_triple_quoted(lex: &mut logos::Lexer<Token>) -> Option<String> {
    let rest = lex.remainder();
    let end = rest.find("\"\"\"")?; // None => unterminated triple-quoted string
    let content = rest[..end].to_string();
    lex.bump(end + 3); // consume the content plus the closing `"""`
    Some(content)
}

fn unescape_char(raw: &str) -> Option<char> {
    let inner = &raw[1..raw.len() - 1];
    let unescaped = unescape_common(inner);
    unescaped.chars().next()
}

fn unescape_common(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A token paired with the byte-span it occupies in the source file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

/// Tokenize `source`, returning every token (including a trailing `Eof`)
/// plus any lexical diagnostics (unterminated strings, unknown characters).
pub fn tokenize(source: &str) -> (Vec<SpannedToken>, DiagnosticBag) {
    let mut lexer = Token::lexer(source);
    let mut tokens = Vec::new();
    let mut diagnostics = DiagnosticBag::new();

    while let Some(result) = lexer.next() {
        let span: Span = lexer.span().into();
        match result {
            Ok(token) => tokens.push(SpannedToken { token, span }),
            Err(_) => {
                let bad = &source[span.start..span.end.max(span.start + 1).min(source.len())];
                diagnostics.push(
                    Diagnostic::error("L001", format!("unexpected character '{bad}'"), span)
                        .with_label("unrecognized token")
                        .with_help("remove or replace this character"),
                );
            }
        }
    }

    let end = source.len();
    tokens.push(SpannedToken {
        token: Token::Eof,
        span: Span::new(end, end),
    });

    (tokens, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Token> {
        tokenize(src).0.into_iter().map(|t| t.token).collect()
    }

    #[test]
    fn lexes_let_statement() {
        let toks = kinds("let x = 10;");
        assert_eq!(
            toks,
            vec![
                Token::Let,
                Token::Identifier("x".into()),
                Token::Eq,
                Token::IntLiteral(10),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_function_signature() {
        let toks = kinds("fn add(a: int, b: int) -> int {");
        assert_eq!(
            toks,
            vec![
                Token::Fn,
                Token::Identifier("add".into()),
                Token::LParen,
                Token::Identifier("a".into()),
                Token::Colon,
                Token::IntType,
                Token::Comma,
                Token::Identifier("b".into()),
                Token::Colon,
                Token::IntType,
                Token::RParen,
                Token::Arrow,
                Token::IntType,
                Token::LBrace,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn skips_comments() {
        let toks = kinds("// hi\nlet x = 1; /* block \n comment */ let y = 2;");
        assert!(matches!(toks[0], Token::Let));
    }

    #[test]
    fn lexes_string_with_escapes() {
        let toks = kinds(r#""hello\nworld""#);
        assert_eq!(toks[0], Token::StringLiteral("hello\nworld".to_string()));
    }

    #[test]
    fn lexes_range_operators() {
        assert_eq!(
            kinds("0..10"),
            vec![
                Token::IntLiteral(0),
                Token::DotDot,
                Token::IntLiteral(10),
                Token::Eof
            ]
        );
        assert_eq!(
            kinds("0..=10"),
            vec![
                Token::IntLiteral(0),
                Token::DotDotEq,
                Token::IntLiteral(10),
                Token::Eof
            ]
        );
    }

    #[test]
    fn reports_unknown_character() {
        let (_, diags) = tokenize("let x = 1 @ 2;");
        assert!(diags.has_errors());
    }

    #[test]
    fn lexes_v05_ai_native_keywords() {
        let toks = kinds("agent model workflow tool memory knowledge prompt reasoning schema pipeline parallel permission deploy team embedding dataset vector task generate");
        assert_eq!(
            toks,
            vec![
                Token::Agent,
                Token::Model,
                Token::Workflow,
                Token::Tool,
                Token::Memory,
                Token::Knowledge,
                Token::Prompt,
                Token::Reasoning,
                Token::Schema,
                Token::Pipeline,
                Token::Parallel,
                Token::Permission,
                Token::Deploy,
                Token::Team,
                Token::Embedding,
                Token::Dataset,
                Token::Vector,
                Token::Task,
                Token::Generate,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_v20_distributed_intelligence_keywords() {
        let toks = kinds("network cluster on");
        assert_eq!(
            toks,
            vec![Token::Network, Token::Cluster, Token::On, Token::Eof]
        );
    }

    #[test]
    fn lexes_triple_quoted_string() {
        let toks = kinds("\"\"\"line one\nline two\"\"\"");
        assert_eq!(
            toks,
            vec![
                Token::TripleQuotedString("line one\nline two".to_string()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_empty_triple_quoted_string() {
        let toks = kinds("\"\"\"\"\"\"");
        assert_eq!(
            toks,
            vec![Token::TripleQuotedString(String::new()), Token::Eof]
        );
    }

    #[test]
    fn triple_quoted_string_does_not_process_escapes() {
        // Unlike a regular string literal, `\n` inside a triple-quoted
        // string is literal backslash-n, not a newline — real newlines are
        // typed directly, which is the whole point of the syntax.
        let toks = kinds("\"\"\"a\\nb\"\"\"");
        assert_eq!(
            toks,
            vec![Token::TripleQuotedString("a\\nb".to_string()), Token::Eof]
        );
    }

    #[test]
    fn regular_string_still_lexes_next_to_triple_quote_support() {
        let toks = kinds(r#""hello""#);
        assert_eq!(
            toks,
            vec![Token::StringLiteral("hello".to_string()), Token::Eof]
        );
    }
}
