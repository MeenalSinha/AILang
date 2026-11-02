//! # ailang-semantic
//!
//! Name-resolution and scope analysis: undefined variables, duplicate
//! declarations, invalid assignment to `const`, unused imports, and invalid
//! `return` placement. This phase runs after parsing and before/alongside
//! the type checker; it does not need type information, only names and
//! scope structure.

use std::collections::{HashMap, HashSet};

use ailang_ast::{Block, ElseBranch, Expr, Ident, Literal, MemoryRef, Program, Stmt};
use ailang_diagnostics::{Diagnostic, DiagnosticBag, Span};

/// Names always available without an explicit declaration: the v0.1
/// standard library surface. Keeping this list here (rather than forcing
/// every program to `import std`) matches the spec's flat top-level
/// stdlib functions (`print`, `len`, etc).
pub const BUILTIN_NAMES: &[&str] = &[
    "print",
    "println",
    "input",
    "len",
    "type",
    "exit",
    "assert",
    "clock",
    "to_string",
    "parse_int",
    "parse_float",
    // v0.5 AI-oriented stdlib (see ailang-interpreter's stdlib module for
    // what's a real deterministic algorithm vs. a documented placeholder
    // pending real model-provider integration).
    "remember",
    "recall",
    "forget",
    "invoke_tool",
    "summarize",
    "translate",
    "classify",
    "embed",
    "run_workflow",
    "run_team",
    "render",
    // v1.0: agent/team-scoped memory + versioning.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymbolKind {
    Variable,
    Const,
    Function,
    Param,
    /// Shared bucket for every v0.5 AI-native declaration kind (`agent`,
    /// `model`, `tool`, `permission`, `memory`, `prompt`, `schema`,
    /// `workflow`, `team`, `knowledge`, and the generic AI placeholders).
    /// A single kind is enough here because all we need from `declare()`
    /// for these is duplicate-name detection in the same scope — cross-
    /// reference validation (does this agent's `model` field name an
    /// actually-declared model?) is handled separately by `DeclRegistry`,
    /// which tracks each kind in its own set because *that* validation
    /// does need to know which bucket a name belongs to.
    AiDecl,
}

/// Tracks which top-level names were declared as which kind of v0.5
/// AI-native construct, so cross-references (an `agent`'s `model`/`tools`/
/// `permissions`/`memory` fields, a `workflow`/`team`'s member list) can be
/// validated regardless of declaration order — every kind is hoisted in a
/// pre-pass the same way function declarations already were in v0.1, so
/// forward references work (e.g. a `workflow` can list agents declared
/// later in the file).
#[derive(Default)]
struct DeclRegistry {
    agents: HashSet<String>,
    models: HashSet<String>,
    tools: HashSet<String>,
    permissions: HashSet<String>,
    memories: HashSet<String>,
    schemas: HashSet<String>,
}

#[derive(Debug, Clone)]
struct Symbol {
    kind: SymbolKind,
    declared_at: Span,
}

#[derive(Default)]
struct Scope {
    symbols: HashMap<String, Symbol>,
}

pub struct SemanticAnalyzer {
    scopes: Vec<Scope>,
    diagnostics: DiagnosticBag,
    function_depth: usize,
    loop_depth: usize,
    imported: Vec<(String, Span)>,
    used_imports: HashSet<String>,
    registry: DeclRegistry,
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope::default()],
            diagnostics: DiagnosticBag::new(),
            function_depth: 0,
            loop_depth: 0,
            imported: Vec::new(),
            used_imports: HashSet::new(),
            registry: DeclRegistry::default(),
        }
    }

    pub fn analyze(mut self, program: &Program) -> DiagnosticBag {
        // Pre-pass: register all top-level function and AI-native
        // declaration names so forward references (mutual recursion, a
        // `workflow` naming an `agent` declared later, etc.) resolve
        // correctly regardless of source order.
        self.hoist_functions(&program.statements);
        self.hoist_ai_declarations(&program.statements);
        for stmt in &program.statements {
            self.check_stmt(stmt);
        }
        self.check_unused_imports();
        self.diagnostics
    }

    fn hoist_functions(&mut self, statements: &[Stmt]) {
        for stmt in statements {
            if let Stmt::Function { name, span, .. } = stmt {
                self.declare(&name.name, SymbolKind::Function, *span);
            }
        }
    }

    /// Pre-pass companion to `hoist_functions`: populates `self.registry`
    /// with every top-level `agent`/`model`/`tool`/`permission`/`memory`/
    /// `schema` name, recursing into `module` bodies the same way
    /// `hoist_functions` does. Does *not* validate cross-references yet —
    /// that happens in `check_stmt` once every name is known, so order in
    /// the source file never matters.
    fn hoist_ai_declarations(&mut self, statements: &[Stmt]) {
        for stmt in statements {
            match stmt {
                Stmt::Agent { name, .. } => {
                    self.registry.agents.insert(name.name.clone());
                }
                Stmt::Model { name, .. } => {
                    self.registry.models.insert(name.name.clone());
                }
                Stmt::Tool { name, .. } => {
                    self.registry.tools.insert(name.name.clone());
                }
                Stmt::Permission { name, .. } => {
                    self.registry.permissions.insert(name.name.clone());
                }
                Stmt::Memory { name, .. } => {
                    self.registry.memories.insert(name.name.clone());
                }
                Stmt::Schema { name, .. } => {
                    self.registry.schemas.insert(name.name.clone());
                }
                Stmt::Module { body, .. } => self.hoist_ai_declarations(body),
                _ => {}
            }
        }
    }

    fn check_unused_imports(&mut self) {
        for (name, span) in self.imported.clone() {
            if !self.used_imports.contains(&name) {
                self.diagnostics.push(
                    Diagnostic::warning("S001", format!("unused import `{name}`"), span)
                        .with_help("remove the unused import"),
                );
            }
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, kind: SymbolKind, span: Span) {
        let scope = self.scopes.last_mut().unwrap();
        if let Some(existing) = scope.symbols.get(name) {
            self.diagnostics.push(
                Diagnostic::error(
                    "S002",
                    format!("`{name}` is already declared in this scope"),
                    span,
                )
                .with_note(format!(
                    "previous declaration was at byte offset {}",
                    existing.declared_at.start
                )),
            );
            return;
        }
        scope.symbols.insert(
            name.to_string(),
            Symbol {
                kind,
                declared_at: span,
            },
        );
    }

    /// A `tool Name;` declaration is a deliberate exception to the usual
    /// duplicate-declaration rule: `ailang-interpreter`'s `invoke_tool`
    /// documents that a `tool` sharing its name with an ordinary `fn` is
    /// exactly how a *custom tool* is registered (see that builtin's doc
    /// comment). Since `hoist_functions` always pre-declares every
    /// function before this runs, a `tool` and a same-named `fn` colliding
    /// is the expected, supported shape — not a mistake — so it's
    /// silently allowed here rather than reported as `S002`. Any other
    /// collision (two `tool`s with the same name, or a `tool` colliding
    /// with a `let`/`const`/another `agent`/etc.) still errors normally.
    fn declare_tool(&mut self, name: &str, span: Span) {
        let already_a_function = matches!(
            self.scopes.last().unwrap().symbols.get(name),
            Some(Symbol {
                kind: SymbolKind::Function,
                ..
            })
        );
        if already_a_function {
            return;
        }
        self.declare(name, SymbolKind::AiDecl, span);
    }

    fn resolve(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(s) = scope.symbols.get(name) {
                return Some(s);
            }
        }
        None
    }

    fn check_identifier_use(&mut self, name: &str, span: Span) {
        if BUILTIN_NAMES.contains(&name) {
            return;
        }
        if self.resolve(name).is_none() {
            self.diagnostics.push(
                Diagnostic::error("S003", format!("undefined variable `{name}`"), span).with_help(
                    format!("declare it first with `let {name} = ...;` or check for a typo"),
                ),
            );
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl {
                name,
                is_const,
                initializer,
                span,
                ..
            } => {
                if let Some(init) = initializer {
                    self.check_expr(init);
                } else if *is_const {
                    self.diagnostics.push(Diagnostic::error(
                        "S004",
                        "`const` declarations must have an initializer",
                        *span,
                    ));
                }
                let kind = if *is_const {
                    SymbolKind::Const
                } else {
                    SymbolKind::Variable
                };
                self.declare(&name.name, kind, name.span);
            }
            Stmt::Function {
                params, body, span, ..
            } => {
                self.function_depth += 1;
                self.push_scope(); // scope for parameters
                for p in params {
                    self.declare(&p.name.name, SymbolKind::Param, p.span);
                }
                // The function body gets its own nested scope, distinct
                // from the parameter scope, so a `let` in the body can
                // shadow a parameter of the same name (e.g. `fn f(x: int)
                // { let x = x + 1; return x; }`) the same way any inner
                // block can shadow an outer variable. Without this, the
                // param and the body's first `let` would collide in one
                // shared scope and be flagged as a duplicate declaration
                // (S002) even though the interpreter executes this pattern
                // correctly (each `Environment` scope uses simple key
                // overwrite) — this was a real semantic/interpreter
                // mismatch found during the v0.1 audit, fixed here.
                self.push_scope();
                self.check_block(body);
                self.pop_scope();
                self.pop_scope();
                self.function_depth -= 1;
                let _ = span;
            }
            Stmt::Return { value, span } => {
                if self.function_depth == 0 {
                    self.diagnostics.push(Diagnostic::error(
                        "S005",
                        "`return` used outside of a function",
                        *span,
                    ));
                }
                if let Some(v) = value {
                    self.check_expr(v);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.check_expr(condition);
                self.push_scope();
                self.check_block(then_branch);
                self.pop_scope();
                if let Some(branch) = else_branch {
                    match branch.as_ref() {
                        ElseBranch::Else(b) => {
                            self.push_scope();
                            self.check_block(b);
                            self.pop_scope();
                        }
                        ElseBranch::ElseIf(s) => self.check_stmt(s),
                    }
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                self.check_expr(condition);
                self.loop_depth += 1;
                self.push_scope();
                self.check_block(body);
                self.pop_scope();
                self.loop_depth -= 1;
            }
            Stmt::For {
                variable,
                start,
                end,
                body,
                ..
            } => {
                self.check_expr(start);
                self.check_expr(end);
                self.loop_depth += 1;
                self.push_scope();
                self.declare(&variable.name, SymbolKind::Variable, variable.span);
                self.check_block(body);
                self.pop_scope();
                self.loop_depth -= 1;
            }
            Stmt::Block(b) => {
                self.push_scope();
                self.check_block(b);
                self.pop_scope();
            }
            Stmt::Expression { expr, .. } => self.check_expr(expr),
            Stmt::Import { path, span } => {
                let full: Vec<String> = path.iter().map(|i: &Ident| i.name.clone()).collect();
                self.imported.push((full.join("."), *span));
            }
            Stmt::Module { body, .. } => {
                // Module members are hoisted straight into the *enclosing*
                // scope, matching `ailang-interpreter`'s
                // `hoist_functions` behavior (see that crate's doc comment
                // on `Stmt::Module`). A child scope must NOT be used here:
                // popping it after this arm would make module functions
                // invisible to code that follows, even though they remain
                // callable at runtime — exactly the mismatch this fix
                // addresses.
                self.hoist_functions(body);
                for s in body {
                    self.check_stmt(s);
                }
            }
            Stmt::Break { span } | Stmt::Continue { span } => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(Diagnostic::error(
                        "S006",
                        "`break`/`continue` used outside of a loop",
                        *span,
                    ));
                }
            }

            // -----------------------------------------------------------
            // v0.5 AI-native constructs
            // -----------------------------------------------------------
            Stmt::Agent {
                name,
                location: _,
                config,
                span,
            } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                if let Some(model) = &config.model {
                    self.check_reference(
                        &self.registry.models.clone(),
                        &model.name,
                        model.span,
                        "model",
                    );
                }
                for model in &config.models {
                    self.check_reference(
                        &self.registry.models.clone(),
                        &model.name,
                        model.span,
                        "model",
                    );
                }
                if let Some(MemoryRef::Named(mem)) = &config.memory {
                    self.check_reference(
                        &self.registry.memories.clone(),
                        &mem.name,
                        mem.span,
                        "memory",
                    );
                }
                for tool in &config.tools {
                    self.check_reference(
                        &self.registry.tools.clone(),
                        &tool.name,
                        tool.span,
                        "tool",
                    );
                }
                for perm in &config.permissions {
                    self.check_reference(
                        &self.registry.permissions.clone(),
                        &perm.name,
                        perm.span,
                        "permission",
                    );
                }
                let _ = span;
            }
            Stmt::Model {
                name,
                endpoint,
                config,
                ..
            } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                if let Some(e) = endpoint {
                    self.check_expr(e);
                }
                for (_, value) in config {
                    self.check_expr(value);
                }
            }
            Stmt::Tool { name, .. } => {
                self.declare_tool(&name.name, name.span);
            }
            Stmt::Permission { name, .. } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
            }
            Stmt::Memory { name, .. } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
            }
            Stmt::Prompt { name, template, .. } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                self.check_expr(template);
                self.check_prompt_interpolation(template);
            }
            Stmt::Schema { name, .. } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                // Field types are checked structurally by `ailang-types`;
                // there's no expression here to walk.
            }
            Stmt::Workflow { name, stages, span } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                let mut seen = HashSet::new();
                for stage in stages {
                    self.check_reference(
                        &self.registry.agents.clone(),
                        &stage.name,
                        stage.span,
                        "agent",
                    );
                    if !seen.insert(stage.name.clone()) {
                        // A stage naming the same agent twice in one chain
                        // is the only "cycle" shape this v0.5 linear-chain
                        // grammar can express (see docs/roadmap.md for
                        // branching/joining DAG workflows as a v1.0 item).
                        self.diagnostics.push(
                            Diagnostic::error(
                                "S010",
                                format!("workflow `{}` visits agent `{}` more than once", name.name, stage.name),
                                stage.span,
                            )
                            .with_help("a v0.5 workflow is a simple linear chain; remove the repeated stage"),
                        );
                    }
                }
                let _ = span;
            }
            Stmt::Parallel { body, .. } => {
                // Independent tasks, but still ordinary lexical scoping:
                // reuse the same scope handling as a plain `{ }` block.
                // (Execution semantics — sequential vs. concurrent — are an
                // interpreter concern; see that crate's module doc.)
                self.push_scope();
                self.check_block(body);
                self.pop_scope();
            }
            Stmt::Team { name, members, .. } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                for member in members {
                    self.check_reference(
                        &self.registry.agents.clone(),
                        &member.name,
                        member.span,
                        "agent",
                    );
                }
            }
            Stmt::Knowledge { name, source, .. } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                self.check_expr(source);
            }
            Stmt::AiPlaceholder { name, config, .. } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                for (_, value) in config {
                    self.check_expr(value);
                }
            }

            // -----------------------------------------------------------
            // v2.0 distributed-intelligence constructs
            // -----------------------------------------------------------
            Stmt::Network { name, members, .. } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                for member in members {
                    self.check_reference(
                        &self.registry.agents.clone(),
                        &member.name,
                        member.span,
                        "agent",
                    );
                }
            }
            Stmt::Cluster { name, replicas, .. } => {
                self.declare(&name.name, SymbolKind::AiDecl, name.span);
                if let Some(r) = replicas {
                    self.check_expr(r);
                }
            }
            Stmt::OnEvent { body, .. } => {
                // Multiple `on <event>` handlers for the same event name
                // are expected and valid (see ailang-interpreter's module
                // doc), so `event_name` is deliberately NOT run through
                // `declare()` here — that would wrongly flag the second
                // handler for the same event as a duplicate declaration.
                //
                // `payload` is bound implicitly by the interpreter's
                // `emit()` at call time (see that method's doc) — it
                // never appears as a `let` in source, so it must be
                // declared here for the body to type-check/analyze
                // without a false "undefined variable" error, the same
                // way a `for` loop's variable is declared for its body.
                self.push_scope();
                self.declare("payload", SymbolKind::Variable, body.span);
                self.check_block(body);
                self.pop_scope();
            }
        }
    }

    /// Validates that `name` was declared as the expected AI-native kind
    /// (looked up in `registry_set`), emitting a clear diagnostic naming
    /// both the missing name and what kind of declaration was expected.
    fn check_reference(
        &mut self,
        registry_set: &HashSet<String>,
        name: &str,
        span: Span,
        kind: &str,
    ) {
        if !registry_set.contains(name) {
            self.diagnostics.push(
                Diagnostic::error(
                    "S011",
                    format!("undefined {kind} `{name}`"),
                    span,
                )
                .with_help(format!("declare it first with `{kind} {name};` (or the appropriate block form), or check for a typo")),
            );
        }
    }

    /// Prompt templates support `${identifier}` interpolation (see
    /// `ailang-interpreter`'s `render_prompt`). This performs the same
    /// undefined-name check `${name}` will hit at render time, but at
    /// compile time — so a typo'd interpolation is caught by `ail check`
    /// rather than only surfacing when the prompt is actually rendered.
    /// Non-string templates (which can't happen from today's grammar, but
    /// `template` is a general `Expr`) are silently skipped.
    fn check_prompt_interpolation(&mut self, template: &Expr) {
        let Expr::Literal {
            value: Literal::String(text),
            span,
        } = template
        else {
            return;
        };
        for name in extract_interpolation_names(text) {
            if !BUILTIN_NAMES.contains(&name.as_str()) && self.resolve(&name).is_none() {
                self.diagnostics.push(
                    Diagnostic::warning(
                        "S012",
                        format!("prompt interpolates undefined variable `${{{name}}}`"),
                        *span,
                    )
                    .with_help("this will render as an empty string unless the variable is defined before the prompt is rendered"),
                );
            }
        }
    }

    fn check_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal { .. } => {}
            Expr::Identifier { name, span } => {
                self.mark_import_used(name);
                self.check_identifier_use(name, *span);
            }
            Expr::ArrayLiteral { elements, .. } => {
                for e in elements {
                    self.check_expr(e);
                }
            }
            Expr::MapLiteral { entries, .. } => {
                for (k, v) in entries {
                    self.check_expr(k);
                    self.check_expr(v);
                }
            }
            Expr::Binary { left, right, .. } => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::Unary { operand, .. } => self.check_expr(operand),
            Expr::Assign {
                target,
                value,
                span,
                ..
            } => {
                if let Expr::Identifier { name, .. } = target.as_ref() {
                    if let Some(sym) = self.resolve(name) {
                        if sym.kind == SymbolKind::Const {
                            self.diagnostics.push(Diagnostic::error(
                                "S007",
                                format!("cannot assign to `{name}`: it is declared `const`"),
                                *span,
                            ));
                        }
                    }
                }
                self.check_expr(target);
                self.check_expr(value);
            }
            Expr::Call { callee, args, .. } => {
                self.check_expr(callee);
                for a in args {
                    self.check_expr(a);
                }
            }
            Expr::Index { object, index, .. } => {
                self.check_expr(object);
                self.check_expr(index);
            }
            Expr::Member { object, .. } => self.check_expr(object),
            Expr::Grouping { expr, .. } => self.check_expr(expr),
            Expr::Generate { schema_name, .. } => {
                self.check_reference(
                    &self.registry.schemas.clone(),
                    &schema_name.name,
                    schema_name.span,
                    "schema",
                );
            }
        }
    }

    fn mark_import_used(&mut self, name: &str) {
        for (imp, _) in &self.imported {
            if imp == name || imp.ends_with(&format!(".{name}")) {
                self.used_imports.insert(imp.clone());
            }
        }
    }
}

/// Extracts every `${identifier}` interpolation target from prompt
/// template text. Shared between `ailang-semantic` (compile-time undefined-
/// variable checking) and `ailang-interpreter` (actual substitution at
/// render time) would ideally live in one place; it's duplicated here in
/// miniature (semantic analysis only needs the *names*, not the
/// substitution) rather than adding a dependency edge from this crate to
/// the interpreter, which would invert the compiler pipeline's layering
/// (see docs/architecture.md).
fn extract_interpolation_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = text[i + 2..].find('}') {
                let name = &text[i + 2..i + 2 + end];
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    names.push(name.to_string());
                }
                i += 2 + end + 1;
                continue;
            }
        }
        i += 1;
    }
    names
}

impl Default for SemanticAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailang_parser::parse;

    fn analyze(src: &str) -> DiagnosticBag {
        let (program, parse_diags) = parse(src);
        assert!(!parse_diags.has_errors(), "{:?}", parse_diags.diagnostics);
        SemanticAnalyzer::new().analyze(&program.unwrap())
    }

    #[test]
    fn detects_undefined_variable() {
        let diags = analyze("print(x);");
        assert!(diags.has_errors());
    }

    #[test]
    fn allows_builtin_calls() {
        let diags = analyze(r#"print("hi"); println("there"); let n = len("abc");"#);
        assert!(!diags.has_errors());
    }

    #[test]
    fn detects_duplicate_declaration() {
        let diags = analyze("let x = 1; let x = 2;");
        assert!(diags.has_errors());
    }

    #[test]
    fn detects_const_reassignment() {
        let diags = analyze("const PI = 3.14; PI = 4;");
        assert!(diags.has_errors());
    }

    #[test]
    fn detects_return_outside_function() {
        let diags = analyze("return 5;");
        assert!(diags.has_errors());
    }

    #[test]
    fn detects_break_outside_loop() {
        let diags = analyze("break;");
        assert!(diags.has_errors());
    }

    #[test]
    fn allows_forward_recursive_calls() {
        let diags = analyze(
            "fn even(n: int) -> bool { if n == 0 { return true; } return odd(n - 1); } fn odd(n: int) -> bool { if n == 0 { return false; } return even(n - 1); }",
        );
        assert!(!diags.has_errors());
    }

    #[test]
    fn warns_on_unused_import() {
        let diags = analyze("import math;");
        assert!(!diags.has_errors());
        assert!(diags.warning_count() >= 1);
    }

    // -----------------------------------------------------------------
    // v0.5 AI-native construct semantic checks
    // -----------------------------------------------------------------

    #[test]
    fn accepts_agent_with_valid_references() {
        let diags = analyze(
            "model GPT5; tool Search; permission internet; \
             agent Researcher { model GPT5; tools [Search]; permissions [internet]; }",
        );
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn agent_can_forward_reference_a_model_declared_later() {
        // Declaration order must not matter (hoisting).
        let diags = analyze("agent A { model GPT5; } model GPT5;");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn rejects_agent_referencing_undeclared_model() {
        let diags = analyze("agent A { model DoesNotExist; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn rejects_agent_referencing_undeclared_tool() {
        let diags = analyze("agent A { tools [Ghost]; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn rejects_agent_referencing_undeclared_permission() {
        let diags = analyze("agent A { permissions [ghost]; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_agent_with_named_memory_reference() {
        let diags = analyze("memory UserMemory persistent; agent A { memory UserMemory; }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn rejects_agent_with_undeclared_named_memory() {
        let diags = analyze("agent A { memory Ghost; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_agent_memory_persistent_keyword_without_declaration() {
        // `persistent`/`temporary` are inline kinds, not named references,
        // so they never need a matching top-level `memory` declaration.
        let diags = analyze("agent A { memory persistent; }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn accepts_workflow_with_declared_agents() {
        let diags =
            analyze("agent Researcher {} agent Writer {} workflow Paper { Researcher -> Writer; }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn workflow_can_forward_reference_agents() {
        let diags =
            analyze("workflow Paper { Researcher -> Writer; } agent Researcher {} agent Writer {}");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn rejects_workflow_referencing_undeclared_agent() {
        let diags = analyze("agent Researcher {} workflow Paper { Researcher -> Ghost; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn rejects_workflow_visiting_the_same_agent_twice() {
        let diags = analyze("agent A {} workflow W { A -> A; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_team_with_declared_agent_members() {
        let diags = analyze("agent CEO {} agent CTO {} team Startup { CEO; CTO; }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn rejects_team_with_undeclared_member() {
        let diags = analyze("team Startup { Ghost; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_generate_with_declared_schema() {
        let diags = analyze("schema Person { name: string; } let p = generate Person;");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn rejects_generate_with_undeclared_schema() {
        let diags = analyze("let p = generate Ghost;");
        assert!(diags.has_errors());
    }

    #[test]
    fn rejects_duplicate_agent_names() {
        let diags = analyze("agent A {} agent A {}");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_all_ai_placeholder_declarations() {
        let diags = analyze(
            "reasoning R; embedding E; dataset D; vector V; pipeline P; task T; deploy Dep;",
        );
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn allows_tool_declaration_to_coexist_with_same_named_custom_tool_function() {
        // Regression test for a v0.5 audit finding: `tool Name;` sharing a
        // name with `fn Name() {}` is the documented custom-tool pattern
        // (see ailang-interpreter's invoke_tool), not a duplicate
        // declaration, regardless of which comes first in source order.
        let diags = analyze("tool Doubler; fn Doubler(x: int) -> int { return x * 2; }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);

        let diags_reversed = analyze("fn Doubler(x: int) -> int { return x * 2; } tool Doubler;");
        assert!(
            !diags_reversed.has_errors(),
            "{:?}",
            diags_reversed.diagnostics
        );
    }

    #[test]
    fn still_rejects_two_tool_declarations_with_the_same_name() {
        let diags = analyze("tool Search; tool Search;");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_prompt_with_valid_interpolation() {
        let diags = analyze(
            r#"let topic = "quantum computing"; prompt p = "Research ${topic} thoroughly.";"#,
        );
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn warns_on_prompt_interpolating_undefined_variable() {
        let diags = analyze(r#"prompt p = "Research ${undefined_topic}.";"#);
        assert!(!diags.has_errors());
        assert!(diags.warning_count() >= 1);
    }

    #[test]
    fn accepts_parallel_block_with_independent_statements() {
        let diags = analyze("parallel { print(1); print(2); }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn accepts_knowledge_and_prompt_declarations() {
        let diags = analyze(r#"knowledge Docs { source "./docs"; } prompt greeting = "hi";"#);
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    // -----------------------------------------------------------------
    // v2.0 distributed-intelligence construct semantic checks
    // -----------------------------------------------------------------

    #[test]
    fn accepts_network_with_declared_agent_members() {
        let diags = analyze(
            "agent Planner {} agent Researcher {} network Enterprise { agent Planner; agent Researcher; }",
        );
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn rejects_network_with_undeclared_member() {
        let diags = analyze("network N { agent Ghost; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_cluster_declaration() {
        let diags = analyze("cluster Workers { replicas 10; autoscaling enabled; }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn rejects_duplicate_cluster_names() {
        let diags = analyze("cluster W { replicas 1; } cluster W { replicas 2; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_on_event_handler() {
        let diags = analyze(r#"on task_completed { println("done"); }"#);
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn allows_multiple_on_event_handlers_for_the_same_event() {
        // Unlike other declarations, repeated handlers for one event name
        // are valid, not a duplicate-declaration error.
        let diags =
            analyze(r#"on task_completed { println(1); } on task_completed { println(2); }"#);
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn on_event_handler_body_checks_undefined_variables() {
        let diags = analyze("on task_completed { println(undefined_var); }");
        assert!(diags.has_errors());
    }

    #[test]
    fn on_event_handler_can_reference_the_implicit_payload_variable() {
        // Regression test: `payload` is bound by the interpreter's
        // `emit()` at call time, not by any `let` in source, so it must
        // not be flagged as undefined during static analysis.
        let diags = analyze(r#"on document_uploaded { println("got: " + payload); }"#);
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn accepts_agent_with_valid_multi_model_list() {
        let diags = analyze("model GPT5; model Claude; agent A { models [GPT5, Claude]; }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn rejects_agent_with_undeclared_model_in_models_list() {
        let diags = analyze("agent A { models [Ghost]; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn allows_block_shadowing_of_outer_variable() {
        // Nested-scope shadowing is normal and must NOT be a duplicate
        // declaration error.
        let diags = analyze("let x = 1; { let x = 2; println(x); } println(x);");
        assert!(!diags.has_errors());
    }

    #[test]
    fn allows_let_to_shadow_a_function_parameter() {
        // Regression test for a v0.1 audit finding: the function body must
        // get its own scope so a `let` can shadow a parameter, matching
        // both normal shadowing semantics and actual interpreter behavior.
        let diags = analyze("fn f(x: int) -> int { let x = x + 1; return x; }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn still_rejects_duplicate_declaration_in_the_same_scope() {
        // Shadowing across scopes is fine; redeclaring in the *same* scope
        // remains an error.
        let diags = analyze("fn f(x: int) -> int { let y = 1; let y = 2; return x + y; }");
        assert!(diags.has_errors());
    }
}
