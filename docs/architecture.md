# AILang v0.1 Architecture

This document describes how the pieces in `compiler/`, `cli/`, `repl/`,
`formatter/`, `package_manager/`, and `language_server/` fit together, and
*why* they're split the way they are — the split is what makes the v0.5 AI
primitives addable without a rewrite (see [`roadmap.md`](./roadmap.md)).

## Pipeline overview

```text
   source.ail
        |
        v
  ailang-lexer   ---->  Vec<SpannedToken>
        |
        v
  ailang-parser  ---->  ailang_ast::Program
        |
        +----------------------------+
        v                            v
  ailang-semantic              ailang-types
  (names, scope)               (static checking)
        |                            |
        +-------------+--------------+
                       v
              ailang-interpreter
              (tree-walk execution)
                       |
                       v
            stdout / runtime errors
```

Every arrow is a *crate boundary* in the Cargo workspace (see the root
`Cargo.toml`). Each phase depends only on `ailang-ast` and
`ailang-diagnostics` (plus `ailang-runtime` for the interpreter) — never on
each other's internals — so:

* The parser can be swapped for a `chumsky`/`lalrpop` implementation without
  touching the semantic analyzer, type checker, or interpreter (see
  `compiler/parser/src/lib.rs`'s module doc for why v0.1 uses hand-written
  recursive descent instead).
* A future bytecode compiler or JIT can consume the exact same
  `ailang_ast::Program` the interpreter does today, run alongside it as an
  alternative backend, and be selected with a CLI flag — nothing about the
  front end needs to change.
* Tooling (the LSP, the formatter) depends on `ailang-parser` +
  `ailang-ast` directly and never needs to shell out to the CLI.

## Crate map

| Crate | Path | Responsibility |
|---|---|---|
| `ailang-diagnostics` | `compiler/diagnostics` | `Span`, `Diagnostic`, `DiagnosticBag`; source-highlighted rendering via `ariadne`. Every other phase depends on this and only this for error reporting, so diagnostics look and behave consistently everywhere. |
| `ailang-ast` | `compiler/ast` | AST node definitions (`Stmt`, `Expr`, `TypeExpr`, ...). Pure data, no logic. Holds the `FUTURE_RESERVED_KEYWORDS` list and `EXTENSION POINT` markers for v0.5+ constructs. |
| `ailang-lexer` | `compiler/lexer` | `&str -> Vec<SpannedToken>` via `logos`. No parser knowledge. |
| `ailang-parser` | `compiler/parser` | Hand-written recursive-descent parser, `Vec<SpannedToken> -> ailang_ast::Program`, with panic-mode error recovery (`synchronize`) so one syntax error doesn't cascade. |
| `ailang-semantic` | `compiler/semantic` | Scope/symbol analysis: undefined variables, duplicate declarations, `const` reassignment, invalid `return`/`break`/`continue` placement, unused imports. No type information needed. |
| `ailang-types` | `compiler/types` | `Type` representation + a checker that infers `let` types, checks `if`/`while` conditions, function signatures, operator operands. Unresolvable names degrade to `Type::Unknown` rather than erroring, so v0.5+ constructs that aren't understood yet don't block compilation. |
| `ailang-runtime` | `compiler/runtime` | `Value`, `Environment` (`Rc<RefCell<..>>`-based scopes), `CallStack`. No AST-walking logic — just the data structures the interpreter mutates. |
| `ailang-interpreter` | `compiler/interpreter` | Tree-walk evaluator (`Interpreter::run`) plus the stdlib builtins (`stdlib` submodule: `print`, `len`, `assert`, etc). |
| `ailang-optimizer` | `compiler/optimizer` | **v1.0: wired into the pipeline** via `ail run --optimize` / `ail build --optimize`. A real, recursive, whole-program constant-folding and dead-code-elimination pass (still AST-to-AST, no separate IR — see `docs/roadmap.md`), verified to never change program output. |
| `ailang-ai-runtime` | `ai_runtime/` | The AI Runtime Foundation: `ModelProvider` (mock + real local-HTTP `HttpModelProvider`), `MemoryStore` (with versioning), `Tool`/`ToolRegistry` with permission enforcement, `PermissionSet`, sequential `run_workflow`/`run_team`, schema-default filling, prompt rendering, embedding, classification, real file-backed RAG retrieval (`knowledge.rs`), real in-process agent mailboxes + pub/sub (`messaging.rs`), secret access + audit logging (`security.rs`), execution tracing (`trace.rs`), real deterministic leader election + health checks (`network.rs`, v2.0), a real embedding-backed vector index with metadata filtering (`vector.rs`, v2.0), real context compression/deduplication (`context.rs`, v2.0), and a real priority scheduler (`scheduler.rs`, v2.0). Depends only on `ailang-runtime` (for `Value`) and `ailang-parser`/`ailang-ast` (for the calculator tool's arithmetic evaluator) — `ailang-interpreter` depends on it, not the reverse, keeping the crate graph a DAG. See its crate-level doc for exactly what's real vs. a documented scope cut, module by module. |
| `ail` (bin) | `cli/` | The `ail` command: `run`, `build`, `check`, `fmt`, `repl`, `test`, `init`, `new`, `version`, `deploy` (v1.0). Wires every phase above together and is the only crate that prints diagnostics to a terminal. `build` also resolves and locks local-path dependencies (writing `ai.lock`, with real semver-constraint checking as of v1.0) if an `ai.toml` is present. `run`/`build` support `--optimize` (runs `ailang-optimizer`), `--trace` (prints the interpreter's execution timeline), and `run` supports `--debug` (a minimal interactive step debugger) — all v1.0. |
| `ailang-repl` | `repl/` | Interactive REPL. Persists one `Interpreter`/`Environment` across lines. |
| `ailang-fmt` | `formatter/` | AST -> canonical formatted source. |
| `ailang-pm` | `package_manager/` | `ai.toml` manifest schema + `ail init`/`ail new` scaffolding + `ai.lock` lockfile. Local-path dependencies only; as of v1.0, version constraints on those dependencies are checked with a real, hand-rolled semver parser/matcher (`semver.rs`). `bundle_for_deploy` (v1.0) packages a project into `dist/<target>/` for `ail deploy`. |
| `ailang-lsp` (bin `ail-lsp`) | `language_server/` | Hand-rolled `Content-Length`-framed JSON-RPC server (not `tower-lsp`; see that crate's module doc for why) implementing `initialize`, diagnostics publishing (parse + semantic + type-check, as of the v1.0 audit fix), `hover`, real `definition`/`references` backed by a whole-program symbol index (`symbols.rs`, v1.0, flat/not scope-aware), and `formatting`. `rename` remains a stub. |
| `ailang-benches` (bin) | `benches/` | `std::time`-based benchmarks for lex/parse/semantic/interpret, standing in for `criterion` (see that crate's module doc). |

## Data flow through `ail run file.ail`

1. `cli::compile` reads the file and calls `ailang_parser::parse`.
2. If there are no lexer/parser errors, `ailang_semantic::SemanticAnalyzer::analyze` runs.
3. If there are no semantic errors, `ailang_types::TypeChecker::check_program` runs (its diagnostics are collected but, like semantic warnings, don't block execution unless they're errors).
4. All diagnostics collected across phases 1-3 are rendered via `ailang_diagnostics::Diagnostic::render` (which shells out to `ariadne` for the boxed, highlighted output you see in a terminal).
5. If there were no *errors* (warnings are fine), `ailang_interpreter::Interpreter::run` executes the `Program` directly against stdout.

## Why a tree-walk interpreter for v0.1

A bytecode VM or JIT is the obvious "real" long-term backend, and nothing
here prevents building one — `ailang-interpreter` only reads the AST, never
mutates it, so a second backend crate can consume the identical
`ailang_ast::Program`. For v0.1 specifically, a tree-walk interpreter was
chosen because:

* It's the fastest way to get every language construct in the spec
  executing *correctly*, which matters more than raw speed for a v0.1
  whose job is to prove the architecture, not to be fast.
* Diagnostics stay simple: a `RuntimeError` carries the exact `Span` of the
  AST node that failed, with no separate bytecode-to-source mapping to
  maintain.
* Memory management is `Rc<RefCell<..>>` (see `ailang-runtime`'s module
  doc), which is the standard "day one" choice for tree-walk interpreters
  (cf. `jlox` in *Crafting Interpreters*) and is fully abstracted behind
  `Environment`/`Value`, so swapping in a tracing GC later doesn't touch
  the interpreter's control flow.

## v0.5: where the AI runtime sits

`ailang-ai-runtime` sits beside `ailang-runtime` in the dependency graph,
not inside the lexer→parser→semantic→types→interpreter pipeline shown
above — it's consulted *by* the interpreter (for `agent`/`model`/`tool`/
`workflow`/`team`/`memory`/`prompt`/`schema` execution and the AI-oriented
stdlib functions) rather than being its own compilation phase. This is why
adding it didn't require a new phase in the diagram: v0.5's AST additions
flow through the exact same lexer→parser→semantic→types→interpreter
pipeline as every v0.1 construct; only the interpreter's *execution* of
those new `Stmt`/`Expr` variants reaches into `ailang-ai-runtime` for
behavior (provider dispatch, tool invocation, memory storage) that doesn't
belong in the interpreter crate itself. See `ailang-ai-runtime`'s
crate-level doc for its own internal structure.

## Error-handling philosophy

Every phase returns a `DiagnosticBag`, not a `Result<_, String>`. This means:

* Multiple errors can be reported per compilation (the parser's
  `synchronize` panic-mode recovery is what makes this useful — a missing
  semicolon doesn't hide every subsequent error).
* Severity (`Error` / `Warning` / `Note` / `Help`) is uniform, so `ail
  check` can distinguish "compilation failed" from "compiled with
  warnings" the same way `rustc` does.
* Rendering is centralized in one place (`Diagnostic::render`), so the CLI,
  the LSP, and (if built) an IDE plugin all get visually consistent output
  without duplicating `ariadne` boilerplate.
