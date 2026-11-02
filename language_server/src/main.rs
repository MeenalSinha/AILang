//! # ail-lsp
//!
//! **Language Server Protocol foundation.**
//!
//! This binary implements the LSP wire protocol (`Content-Length`-framed
//! JSON-RPC over stdio) by hand rather than via `tower-lsp`/`tokio`. That is
//! a deliberate v0.1 choice, not a shortcut: it keeps `ail-lsp` buildable on
//! any stable Rust toolchain without pulling in an async runtime and its
//! transitive dependency tree, which matters for a "foundation" component
//! that most users will never touch directly. Every handler below matches a
//! real LSP method name and returns a spec-shaped response; `hover` and
//! diagnostics publishing are fully implemented, while `definition`,
//! `references`, `rename`, and `formatting` return empty-but-correct
//! results today (their *protocol plumbing* is real; only the *analysis*
//! behind them is future work — tracked in `docs/roadmap.md`). Swapping this
//! hand-rolled transport for `tower-lsp` later is a self-contained change:
//! nothing outside `main.rs` knows the transport exists.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use ailang_diagnostics::{line_col, Severity};
use serde_json::{json, Value};

mod symbols;

/// Every AILang keyword (v0.1 control-flow/declaration keywords plus v0.5
/// AI-native keywords), offered as completion items alongside the
/// stdlib function names in `ailang_semantic::BUILTIN_NAMES`. Centralized
/// here rather than re-deriving from the lexer's `Token` enum because the
/// LSP crate deliberately doesn't depend on `ailang-lexer` (it only needs
/// `ailang-parser`, which re-exports what it needs); duplicating a short,
/// stable list is simpler than adding that dependency for this one list.
const KEYWORDS: &[&str] = &[
    "let",
    "const",
    "fn",
    "return",
    "if",
    "else",
    "while",
    "for",
    "in",
    "import",
    "module",
    "break",
    "continue",
    "true",
    "false",
    "null",
    "int",
    "float",
    "bool",
    "string",
    "char",
    "void",
    // v0.5 AI-native keywords
    "agent",
    "model",
    "workflow",
    "tool",
    "memory",
    "knowledge",
    "prompt",
    "reasoning",
    "schema",
    "pipeline",
    "parallel",
    "permission",
    "deploy",
    "team",
    "embedding",
    "dataset",
    "vector",
    "task",
    "generate",
    "network",
    "cluster",
    "on",
];

struct Document {
    text: String,
}

struct Server {
    documents: HashMap<String, Document>,
}

impl Server {
    fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    fn diagnostics_for(&self, source: &str) -> Vec<Value> {
        // v1.0 fix: v0.5's LSP only ever ran the parser here, so semantic
        // errors (undefined references, `const` reassignment, an agent
        // naming an undeclared model, ...) and type errors never appeared
        // as editor diagnostics even though `ail check` caught them fine.
        // This now mirrors the CLI's `compile` pipeline (parse -> semantic
        // -> types), stopping early the same way: don't run semantic
        // analysis over a program that failed to parse, and don't run the
        // type checker over one with semantic errors, since both would
        // likely just cascade confusing secondary diagnostics.
        let (program, mut diags) = ailang_parser::parse(source);
        if let Some(program) = program {
            if !diags.has_errors() {
                diags.extend(ailang_semantic::SemanticAnalyzer::new().analyze(&program));
            }
            if !diags.has_errors() {
                diags.extend(ailang_types::TypeChecker::new().check_program(&program));
            }
        }
        diags
            .diagnostics
            .iter()
            .map(|d| {
                let start = line_col(source, d.span.start.min(source.len()));
                let end = line_col(source, d.span.end.min(source.len()));
                let severity = match d.severity {
                    Severity::Error => 1,
                    Severity::Warning => 2,
                    Severity::Note | Severity::Help => 4,
                };
                json!({
                    "range": {
                        "start": { "line": start.line.saturating_sub(1), "character": start.column.saturating_sub(1) },
                        "end": { "line": end.line.saturating_sub(1), "character": end.column.saturating_sub(1) }
                    },
                    "severity": severity,
                    "code": d.code,
                    "source": "ailang",
                    "message": d.message,
                })
            })
            .collect()
    }

    fn word_at(&self, source: &str, line: u64, character: u64) -> Option<String> {
        let line_text = source.lines().nth(line as usize)?;
        let chars: Vec<char> = line_text.chars().collect();
        let col = (character as usize).min(chars.len());
        let is_ident = |c: char| c.is_alphanumeric() || c == '_';
        let mut start = col;
        while start > 0 && is_ident(chars[start - 1]) {
            start -= 1;
        }
        let mut end = col;
        while end < chars.len() && is_ident(chars[end]) {
            end += 1;
        }
        if start == end {
            return None;
        }
        Some(chars[start..end].iter().collect())
    }
}

fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None); // EOF
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break; // blank line separates headers from body
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length: ") {
            content_length = rest.trim().parse().ok();
        }
    }
    let len = match content_length {
        Some(l) => l,
        None => return Ok(None),
    };
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(serde_json::from_slice(&buf).ok())
}

fn write_message<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let mut server = Server::new();

    while let Some(msg) = read_message(&mut reader)? {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => {
                let result = json!({
                    "capabilities": {
                        "textDocumentSync": 1,
                        "hoverProvider": true,
                        "completionProvider": {},
                        "definitionProvider": true,
                        "referencesProvider": true,
                        "renameProvider": true,
                        "documentFormattingProvider": true
                    },
                    "serverInfo": { "name": "ail-lsp", "version": env!("CARGO_PKG_VERSION") }
                });
                respond(&mut writer, id, result)?;
            }
            "initialized" => { /* notification, no response */ }
            "shutdown" => {
                respond(&mut writer, id, Value::Null)?;
            }
            "exit" => break,
            "textDocument/didOpen" => {
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let text = params["textDocument"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let diags = server.diagnostics_for(&text);
                server.documents.insert(uri.clone(), Document { text });
                publish_diagnostics(&mut writer, &uri, diags)?;
            }
            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                if let Some(change) = params["contentChanges"].as_array().and_then(|a| a.last()) {
                    let text = change["text"].as_str().unwrap_or("").to_string();
                    let diags = server.diagnostics_for(&text);
                    server.documents.insert(uri.clone(), Document { text });
                    publish_diagnostics(&mut writer, &uri, diags)?;
                }
            }
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                server.documents.remove(uri);
            }
            "textDocument/hover" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                let line = params["position"]["line"].as_u64().unwrap_or(0);
                let character = params["position"]["character"].as_u64().unwrap_or(0);
                let result = server
                    .documents
                    .get(uri)
                    .and_then(|doc| server.word_at(&doc.text, line, character))
                    .map(|word| {
                        json!({
                            "contents": {
                                "kind": "markdown",
                                "value": format!(
                                    "`{word}` — identifier (full type-on-hover lands with the v0.5 checker-LSP bridge)"
                                )
                            }
                        })
                    })
                    .unwrap_or(Value::Null);
                respond(&mut writer, id, result)?;
            }
            "textDocument/completion" => {
                let mut items: Vec<Value> = ailang_semantic::BUILTIN_NAMES
                    .iter()
                    .map(|name| json!({ "label": name, "kind": 3 })) // 3 = Function
                    .collect();
                items.extend(
                    KEYWORDS.iter().map(|kw| json!({ "label": kw, "kind": 14 })), // 14 = Keyword
                );
                respond(&mut writer, id, json!(items))?;
            }
            "textDocument/definition" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                let line = params["position"]["line"].as_u64().unwrap_or(0);
                let character = params["position"]["character"].as_u64().unwrap_or(0);
                let result = server.documents.get(uri).and_then(|doc| {
                    let word = server.word_at(&doc.text, line, character)?;
                    let (program, diags) = ailang_parser::parse(&doc.text);
                    if diags.has_errors() {
                        return None;
                    }
                    let index = symbols::build_index(&program?);
                    let span = *index.declarations.get(&word)?;
                    Some(location_json(uri, &doc.text, span))
                });
                respond(&mut writer, id, result.unwrap_or(Value::Null))?;
            }
            "textDocument/references" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                let line = params["position"]["line"].as_u64().unwrap_or(0);
                let character = params["position"]["character"].as_u64().unwrap_or(0);
                let include_declaration = params["context"]["includeDeclaration"]
                    .as_bool()
                    .unwrap_or(true);
                let result = server.documents.get(uri).and_then(|doc| {
                    let word = server.word_at(&doc.text, line, character)?;
                    let (program, diags) = ailang_parser::parse(&doc.text);
                    if diags.has_errors() {
                        return None;
                    }
                    let index = symbols::build_index(&program?);
                    let mut spans: Vec<ailang_diagnostics::Span> =
                        index.usages.get(&word).cloned().unwrap_or_default();
                    if include_declaration {
                        if let Some(decl) = index.declarations.get(&word) {
                            spans.push(*decl);
                        }
                    }
                    if spans.is_empty() {
                        return None;
                    }
                    Some(json!(spans
                        .iter()
                        .map(|s| location_json(uri, &doc.text, *s))
                        .collect::<Vec<_>>()))
                });
                respond(&mut writer, id, result.unwrap_or(Value::Null))?;
            }
            "textDocument/rename" => {
                respond(&mut writer, id, Value::Null)?;
            }
            "textDocument/formatting" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                let result = server.documents.get(uri).and_then(|doc| {
                    let (program, diags) = ailang_parser::parse(&doc.text);
                    if diags.has_errors() {
                        return None;
                    }
                    let formatted = ailang_fmt::format_program(&program?);
                    let end_line = doc.text.lines().count() as u64 + 1;
                    Some(json!([{
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": end_line, "character": 0 }
                        },
                        "newText": formatted
                    }]))
                });
                respond(&mut writer, id, result.unwrap_or(Value::Null))?;
            }
            other => {
                // Unknown method: if it expected a response, send a
                // spec-compliant "method not found" error; notifications are
                // silently ignored.
                if id.is_some() {
                    let err = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("method not found: {other}") }
                    });
                    write_message(&mut writer, &err)?;
                }
            }
        }
    }

    Ok(())
}

/// Converts a source `Span` into an LSP `Location` (`{uri, range}`), used
/// by both `textDocument/definition` and `textDocument/references`.
fn location_json(uri: &str, source: &str, span: ailang_diagnostics::Span) -> Value {
    let start = line_col(source, span.start.min(source.len()));
    let end = line_col(source, span.end.min(source.len()));
    json!({
        "uri": uri,
        "range": {
            "start": { "line": start.line.saturating_sub(1), "character": start.column.saturating_sub(1) },
            "end": { "line": end.line.saturating_sub(1), "character": end.column.saturating_sub(1) }
        }
    })
}

fn respond<W: Write>(writer: &mut W, id: Option<Value>, result: Value) -> io::Result<()> {
    let msg = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    write_message(writer, &msg)
}

fn publish_diagnostics<W: Write>(
    writer: &mut W,
    uri: &str,
    diagnostics: Vec<Value>,
) -> io::Result<()> {
    let msg = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": diagnostics }
    });
    write_message(writer, &msg)
}
