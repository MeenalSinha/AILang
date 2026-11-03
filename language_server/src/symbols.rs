//! A flat, whole-program symbol index: every name's declaration site, and
//! every place that name is used as an identifier. This is what upgrades
//! `textDocument/definition` and `textDocument/references` from v0.5's
//! "always returns nothing" stubs into real (if intentionally simple)
//! implementations.
//!
//! ## Scope: flat, not scope-aware
//!
//! This index does **not** track lexical scoping the way
//! `ailang-semantic`'s analyzer does — it's a single `name -> span` map
//! for the whole program, so two different `x` variables in two different
//! functions collide (whichever declaration was walked last wins for
//! "definition", and "references" for `x` returns every use of any `x`
//! anywhere in the file, not just the one in the same scope). This is a
//! deliberate v1.0 scope cut, not an oversight: real go-to-definition
//! needs the same scope-chain resolution `ailang-semantic`'s
//! `SemanticAnalyzer` already does internally, and that analyzer is built
//! to *check* a program, not to answer "what does this specific
//! identifier at this specific position refer to" — exposing a proper
//! scope-aware query API from it is tracked in `docs/roadmap.md`. What
//! ships today is still genuinely useful for the common case (most names
//! in a small-to-medium AILang program are unique) and is real working
//! code, not a stub.

use std::collections::HashMap;

use ailang_ast::{Block, ElseBranch, Expr, MemoryRef, Program, Stmt};
use ailang_diagnostics::Span;

#[derive(Default)]
pub struct SymbolIndex {
    /// name -> the span of its (last-seen) declaration.
    pub declarations: HashMap<String, Span>,
    /// name -> every span where that name appears as an identifier
    /// (including, redundantly but harmlessly, its own declaration site
    /// for `let`/`fn`/param names, since those are also `Ident`s the
    /// walk passes through).
    pub usages: HashMap<String, Vec<Span>>,
}

impl SymbolIndex {
    fn declare(&mut self, name: &str, span: Span) {
        self.declarations.insert(name.to_string(), span);
    }

    fn use_name(&mut self, name: &str, span: Span) {
        self.usages.entry(name.to_string()).or_default().push(span);
    }
}

pub fn build_index(program: &Program) -> SymbolIndex {
    let mut index = SymbolIndex::default();
    for stmt in &program.statements {
        walk_stmt(stmt, &mut index);
    }
    index
}

fn walk_block(block: &Block, index: &mut SymbolIndex) {
    for stmt in &block.statements {
        walk_stmt(stmt, index);
    }
}

fn walk_stmt(stmt: &Stmt, index: &mut SymbolIndex) {
    match stmt {
        Stmt::Expression { expr, .. } => walk_expr(expr, index),
        Stmt::VarDecl {
            name, initializer, ..
        } => {
            index.declare(&name.name, name.span);
            if let Some(init) = initializer {
                walk_expr(init, index);
            }
        }
        Stmt::Function {
            name, params, body, ..
        } => {
            index.declare(&name.name, name.span);
            for p in params {
                index.declare(&p.name.name, p.name.span);
            }
            walk_block(body, index);
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                walk_expr(v, index);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            walk_expr(condition, index);
            walk_block(then_branch, index);
            if let Some(branch) = else_branch {
                match branch.as_ref() {
                    ElseBranch::Else(b) => walk_block(b, index),
                    ElseBranch::ElseIf(s) => walk_stmt(s, index),
                }
            }
        }
        Stmt::While {
            condition, body, ..
        } => {
            walk_expr(condition, index);
            walk_block(body, index);
        }
        Stmt::For {
            variable,
            start,
            end,
            body,
            ..
        } => {
            index.declare(&variable.name, variable.span);
            walk_expr(start, index);
            walk_expr(end, index);
            walk_block(body, index);
        }
        Stmt::Block(b) => walk_block(b, index),
        Stmt::Import { .. } | Stmt::Break { .. } | Stmt::Continue { .. } => {}
        Stmt::Module { name, body, .. } => {
            index.declare(&name.name, name.span);
            for s in body {
                walk_stmt(s, index);
            }
        }
        Stmt::Agent { name, config, .. } => {
            index.declare(&name.name, name.span);
            if let Some(m) = &config.model {
                index.use_name(&m.name, m.span);
            }
            if let Some(MemoryRef::Named(m)) = &config.memory {
                index.use_name(&m.name, m.span);
            }
            for t in &config.tools {
                index.use_name(&t.name, t.span);
            }
            for p in &config.permissions {
                index.use_name(&p.name, p.span);
            }
        }
        Stmt::Model {
            name,
            endpoint,
            config,
            ..
        } => {
            index.declare(&name.name, name.span);
            if let Some(e) = endpoint {
                walk_expr(e, index);
            }
            for (_, v) in config {
                walk_expr(v, index);
            }
        }
        Stmt::Tool { name, .. } | Stmt::Permission { name, .. } => {
            index.declare(&name.name, name.span)
        }
        Stmt::Memory { name, .. } => index.declare(&name.name, name.span),
        Stmt::Prompt { name, template, .. } => {
            index.declare(&name.name, name.span);
            walk_expr(template, index);
        }
        Stmt::Schema { name, .. } => index.declare(&name.name, name.span),
        Stmt::Workflow { name, stages, .. } => {
            index.declare(&name.name, name.span);
            for s in stages {
                index.use_name(&s.name, s.span);
            }
        }
        Stmt::Parallel { body, .. } => walk_block(body, index),
        Stmt::Team { name, members, .. } => {
            index.declare(&name.name, name.span);
            for m in members {
                index.use_name(&m.name, m.span);
            }
        }
        Stmt::Knowledge { name, source, .. } => {
            index.declare(&name.name, name.span);
            walk_expr(source, index);
        }
        Stmt::AiPlaceholder { name, config, .. } => {
            index.declare(&name.name, name.span);
            for (_, v) in config {
                walk_expr(v, index);
            }
        }
        Stmt::Network { name, members, .. } => {
            index.declare(&name.name, name.span);
            for m in members {
                index.use_name(&m.name, m.span);
            }
        }
        Stmt::Cluster { name, replicas, .. } => {
            index.declare(&name.name, name.span);
            if let Some(r) = replicas {
                walk_expr(r, index);
            }
        }
        Stmt::OnEvent {
            event_name, body, ..
        } => {
            // The event name isn't a declaration site to jump to (there's
            // no single canonical `on <event>` — see this crate's doc on
            // why `SymbolIndex` doesn't track it), but it's still a real
            // "use" of that name, and the handler body's own declarations/
            // uses matter for navigation inside it.
            index.use_name(&event_name.name, event_name.span);
            walk_block(body, index);
        }

        // v3.0 constructs — indexed so go-to-definition / hover / rename
        // all work for goals, supervisors, resources, devices, ui, etc.
        Stmt::Goal { name, body, .. } => {
            index.declare(&name.name, name.span);
            walk_block(body, index);
        }
        Stmt::Supervisor { name, children, .. } => {
            index.declare(&name.name, name.span);
            for child in children {
                index.use_name(&child.name, child.span);
            }
        }
        Stmt::Resource { name, config, .. } => {
            index.declare(&name.name, name.span);
            for (_, v) in config {
                walk_expr(v, index);
            }
        }
        Stmt::Reason { max_iterations, budget, body, .. } => {
            if let Some(e) = max_iterations { walk_expr(e, index); }
            if let Some(e) = budget { walk_expr(e, index); }
            walk_block(body, index);
        }
        Stmt::Device { name, .. } => {
            index.declare(&name.name, name.span);
        }
        Stmt::Ui { name, .. } => {
            index.declare(&name.name, name.span);
        }
        Stmt::Application { name, agents, models, workflows, .. } => {
            index.declare(&name.name, name.span);
            for a in agents { index.use_name(&a.name, a.span); }
            for m in models { index.use_name(&m.name, m.span); }
            for w in workflows { index.use_name(&w.name, w.span); }
        }
        Stmt::Policy { name, .. } => {
            index.declare(&name.name, name.span);
        }
        Stmt::Emit { payload, .. } => {
            if let Some(e) = payload { walk_expr(e, index); }
        }
    }
}

fn walk_expr(expr: &Expr, index: &mut SymbolIndex) {
    match expr {
        Expr::Literal { .. } => {}
        Expr::Identifier { name, span } => index.use_name(name, *span),
        Expr::ArrayLiteral { elements, .. } => {
            for e in elements {
                walk_expr(e, index);
            }
        }
        Expr::MapLiteral { entries, .. } => {
            for (k, v) in entries {
                walk_expr(k, index);
                walk_expr(v, index);
            }
        }
        Expr::Binary { left, right, .. } => {
            walk_expr(left, index);
            walk_expr(right, index);
        }
        Expr::Unary { operand, .. } => walk_expr(operand, index),
        Expr::Assign { target, value, .. } => {
            walk_expr(target, index);
            walk_expr(value, index);
        }
        Expr::Call { callee, args, .. } => {
            walk_expr(callee, index);
            for a in args {
                walk_expr(a, index);
            }
        }
        Expr::Index {
            object, index: idx, ..
        } => {
            walk_expr(object, index);
            walk_expr(idx, index);
        }
        Expr::Member { object, .. } => walk_expr(object, index),
        Expr::Grouping { expr, .. } => walk_expr(expr, index),
        Expr::Generate { schema_name, .. } => index.use_name(&schema_name.name, schema_name.span),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_for(src: &str) -> SymbolIndex {
        let (program, diags) = ailang_parser::parse(src);
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
        build_index(&program.unwrap())
    }

    #[test]
    fn finds_a_variable_declaration_and_its_uses() {
        let index = index_for("let x = 5; println(x); println(x + 1);");
        assert!(index.declarations.contains_key("x"));
        assert_eq!(index.usages.get("x").map(|v| v.len()), Some(2));
    }

    #[test]
    fn finds_a_function_declaration_and_its_call_sites() {
        let index =
            index_for("fn add(a: int, b: int) -> int { return a + b; } add(1, 2); add(3, 4);");
        assert!(index.declarations.contains_key("add"));
        assert_eq!(index.usages.get("add").map(|v| v.len()), Some(2));
        // Parameters are declared too.
        assert!(index.declarations.contains_key("a"));
        assert!(index.declarations.contains_key("b"));
    }

    #[test]
    fn finds_agent_field_references_to_model_and_tools() {
        let index = index_for("model GPT5; tool Search; agent A { model GPT5; tools [Search]; }");
        assert!(index.declarations.contains_key("GPT5"));
        assert!(index.declarations.contains_key("Search"));
        assert!(index.declarations.contains_key("A"));
        assert!(index.usages.contains_key("GPT5"));
        assert!(index.usages.contains_key("Search"));
    }

    #[test]
    fn finds_workflow_stage_references() {
        let index = index_for("agent A {} agent B {} workflow W { A -> B; }");
        assert_eq!(index.usages.get("A").map(|v| v.len()), Some(1));
        assert_eq!(index.usages.get("B").map(|v| v.len()), Some(1));
    }

    #[test]
    fn finds_network_member_references() {
        let index = index_for("agent A {} agent B {} network N { agent A; agent B; }");
        assert!(index.declarations.contains_key("N"));
        assert_eq!(index.usages.get("A").map(|v| v.len()), Some(1));
        assert_eq!(index.usages.get("B").map(|v| v.len()), Some(1));
    }
}
