//! # ailang-repl
//!
//! An interactive read-eval-print loop for AILang.
//!
//! ## Design notes
//!
//! * **Plain stdin/stdout, no line-editing library.** v0.1 reads lines with
//!   `std::io::stdin().read_line`, which means no arrow-key history
//!   recall/in-line editing yet. This is a deliberate v0.1 tradeoff (see
//!   `docs/roadmap.md`): pulling in a full line-editing crate brought a deep
//!   transitive dependency tree that isn't worth it for the REPL's current
//!   scope. What *is* implemented for real: an in-memory `history: Vec<String>`
//!   (inspectable via `:history`), multi-line input (continues reading while
//!   braces are unbalanced), and an `AUTOCOMPLETE_KEYWORDS` list that a future
//!   line editor's completer can consume directly — the "foundation" the spec
//!   asks for is the data/structure, which is in place; the terminal-level
//!   interactivity is the tracked follow-up.
//! * **State persists across lines.** A single [`ailang_interpreter::Interpreter`]
//!   and its global environment live for the whole session, so `let x = 5;`
//!   on one line and `x + 2` on the next behave as the spec's example
//!   transcript shows.
//! * **Bare expressions print their value.** matching `python`/`node` REPL
//!   ergonomics.

use std::io::{self, BufRead, Write};

use ailang_diagnostics::DiagnosticBag;
use ailang_interpreter::Interpreter;

/// Reserved for a future line-editor's tab-completion. Centralized here so
/// the REPL and a future `rustyline`/`reedline` integration share one list
/// instead of duplicating it.
pub const AUTOCOMPLETE_KEYWORDS: &[&str] = &[
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
    "print",
    "println",
    "len",
    "type",
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
    "remember",
    "recall",
    "forget",
    "invoke_tool",
    "run_workflow",
    "run_team",
    "summarize",
    "translate",
    "classify",
    "embed",
    "render",
    "memory_history",
    "remember_in",
    "recall_in",
    "forget_in",
    "retrieve",
    "secret",
    "audit_log",
    "send_message",
    "receive_message",
    "broadcast",
    "agent_location",
    "get_trace",
    "push",
    "pop",
    "reverse",
    "sort",
    "json_encode",
    "json_decode",
    "read_file",
    "write_file",
    "http_get",
    "http_post",
    "hash",
    "sleep",
    "network_members",
    "network_leader",
    "network_health",
    "network_join",
    "network_leave",
    "cluster_status",
    "cluster_dispatch",
    "on_request",
    "request",
    "subscribe",
    "unsubscribe",
    "publish",
    "emit",
    "vector_index_add",
    "vector_index_search",
    "compress_context",
    "dedupe_chunks",
    "schedule",
    "run_scheduled",
    "agent_models",
];

/// Count `{`/`}` (outside of string literals) to detect whether `source`
/// needs a continuation line before it can be parsed.
fn braces_balanced(source: &str) -> bool {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth <= 0
}

pub fn run() -> anyhow::Result<()> {
    println!("AILang v0.1 REPL — type an expression, a statement, or :quit to exit.");
    println!(
        "(:history shows this session's input; multi-line `fn`/`if`/`while` blocks are supported.)"
    );

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut interp = Interpreter::new(stdout.lock());
    let mut history: Vec<String> = Vec::new();

    let mut buffer = String::new();
    loop {
        print_prompt(&buffer);
        let mut line = String::new();
        let bytes_read = stdin.lock().read_line(&mut line)?;
        if bytes_read == 0 {
            break; // EOF (e.g. piped input finished, or Ctrl-D)
        }

        if buffer.is_empty() {
            match line.trim() {
                ":quit" | ":q" | "exit" => break,
                ":history" => {
                    for (i, h) in history.iter().enumerate() {
                        println!("{:>3}  {h}", i + 1);
                    }
                    continue;
                }
                _ => {}
            }
        }

        history.push(line.trim_end().to_string());
        buffer.push_str(&line);

        if !braces_balanced(&buffer) {
            continue; // keep collecting continuation lines
        }

        eval_and_print(&buffer, &mut interp);
        buffer.clear();
    }
    println!("bye!");
    Ok(())
}

fn print_prompt(buffer: &str) {
    let prompt = if buffer.is_empty() { "> " } else { "... " };
    print!("{prompt}");
    io::stdout().flush().ok();
}

fn eval_and_print<W: std::io::Write>(input: &str, interp: &mut Interpreter<W>) {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return;
    }

    // Bare-expression convenience: a single expression with no trailing
    // `;` gets wrapped in `println(...)` so its value is visible, matching
    // the ergonomics in the language spec's REPL transcript (`> x + 2` =>
    // `7`).
    let wants_value = !trimmed.ends_with(';') && !trimmed.ends_with('}');
    let source = if wants_value {
        let (prog, diags) = ailang_parser::parse(&format!("{trimmed};"));
        match (&prog, diags.has_errors()) {
            (Some(p), false) if p.statements.len() == 1 => {
                if let ailang_ast::Stmt::Expression { .. } = &p.statements[0] {
                    format!("println({trimmed});")
                } else {
                    trimmed.to_string()
                }
            }
            _ => trimmed.to_string(),
        }
    } else {
        trimmed.to_string()
    };

    let (program, diags) = ailang_parser::parse(&source);
    if diags.has_errors() {
        report(&diags, &source);
        return;
    }
    let program = match program {
        Some(p) => p,
        None => return,
    };

    // Note: unlike the `ail check`/`run` file pipeline, the REPL does not
    // run `ailang-semantic` here. The analyzer starts each call with an
    // empty scope, so it can't see names defined by *earlier* REPL lines
    // and would wrongly flag them as undefined. The interpreter's
    // persistent global `Environment` already reports undefined variables
    // at runtime, so behavior is still correct — just without the extra
    // static pass. A REPL-aware semantic analyzer that can be seeded with
    // previously-declared names is tracked in `docs/roadmap.md`.

    if let Err(e) = interp.run(&program) {
        eprintln!("runtime error: {e}");
    }
    interp.out.flush().ok();
}

fn report(diags: &DiagnosticBag, source: &str) {
    for d in &diags.diagnostics {
        eprintln!("{}", d.render_plain("<repl>", source));
    }
}
