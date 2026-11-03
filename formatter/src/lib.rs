//! # ailang-fmt
//!
//! A `rustfmt`-style formatter for AILang source.
//!
//! ## Design
//!
//! v0.1's formatter re-serializes the **parsed AST** rather than doing a
//! token-stream rewrite. This guarantees canonical, self-consistent output
//! (every `.ail` file formats to a single fixed point) at the cost of not
//! yet preserving blank-line grouping or comments (comments are discarded
//! by the lexer today; see `docs/roadmap.md` for the plan to attach trivia
//! to tokens so a future version can preserve them). This is called out
//! explicitly rather than silently dropping comments so users aren't
//! surprised: `ail fmt` should currently be treated as safe for
//! comment-free files, and comment-preserving formatting is a tracked
//! roadmap item, not an oversight.
//!
//! Rules implemented: 4-space indentation, K&R (same-line) brace placement,
//! single space around binary/assignment operators, no space before `;` or
//! `,`, one space after `,`, trailing commas in multi-line array/map
//! literals.

use ailang_ast::*;

const INDENT: &str = "    ";

pub fn format_program(program: &Program) -> String {
    let mut out = String::new();
    for (i, stmt) in program.statements.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        write_stmt(&mut out, stmt, 0);
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str(INDENT);
    }
}

fn write_stmt(out: &mut String, stmt: &Stmt, level: usize) {
    indent(out, level);
    match stmt {
        Stmt::Expression { expr, .. } => {
            write_expr(out, expr);
            out.push(';');
            out.push('\n');
        }
        Stmt::VarDecl {
            name,
            is_const,
            type_annotation,
            initializer,
            ..
        } => {
            out.push_str(if *is_const { "const " } else { "let " });
            out.push_str(&name.name);
            if let Some(t) = type_annotation {
                out.push_str(": ");
                write_type(out, t);
            }
            if let Some(init) = initializer {
                out.push_str(" = ");
                write_expr(out, init);
            }
            out.push_str(";\n");
        }
        Stmt::Function {
            name,
            params,
            return_type,
            body,
            ..
        } => {
            out.push_str("fn ");
            out.push_str(&name.name);
            out.push('(');
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&p.name.name);
                if let Some(t) = &p.type_annotation {
                    out.push_str(": ");
                    write_type(out, t);
                }
            }
            out.push(')');
            if let Some(rt) = return_type {
                out.push_str(" -> ");
                write_type(out, rt);
            }
            out.push_str(" {\n");
            write_block_body(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Return { value, .. } => {
            out.push_str("return");
            if let Some(v) = value {
                out.push(' ');
                write_expr(out, v);
            }
            out.push_str(";\n");
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            out.push_str("if ");
            write_expr(out, condition);
            out.push_str(" {\n");
            write_block_body(out, then_branch, level + 1);
            indent(out, level);
            out.push('}');
            write_else(out, else_branch, level);
            out.push('\n');
        }
        Stmt::While {
            condition, body, ..
        } => {
            out.push_str("while ");
            write_expr(out, condition);
            out.push_str(" {\n");
            write_block_body(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::For {
            variable,
            start,
            end,
            inclusive,
            body,
            ..
        } => {
            out.push_str("for ");
            out.push_str(&variable.name);
            out.push_str(" in ");
            write_expr(out, start);
            out.push_str(if *inclusive { "..=" } else { ".." });
            write_expr(out, end);
            out.push_str(" {\n");
            write_block_body(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Block(b) => {
            out.push_str("{\n");
            write_block_body(out, b, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Import { path, .. } => {
            out.push_str("import ");
            out.push_str(
                &path
                    .iter()
                    .map(|i| i.name.clone())
                    .collect::<Vec<_>>()
                    .join("."),
            );
            out.push_str(";\n");
        }
        Stmt::Module { name, body, .. } => {
            out.push_str("module ");
            out.push_str(&name.name);
            out.push_str(" {\n");
            for (i, s) in body.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                write_stmt(out, s, level + 1);
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Break { .. } => out.push_str("break;\n"),
        Stmt::Continue { .. } => out.push_str("continue;\n"),

        // -------------------------------------------------------------
        // v0.5 AI-native constructs
        // -------------------------------------------------------------
        Stmt::Agent {
            name,
            location,
            config,
            ..
        } => {
            out.push_str("agent ");
            out.push_str(&name.name);
            if let Some(loc) = location {
                out.push_str(" on ");
                out.push_str(&loc.name);
            }
            out.push_str(" {\n");
            let inner = level + 1;
            if let Some(model) = &config.model {
                indent(out, inner);
                out.push_str("model ");
                out.push_str(&model.name);
                out.push_str(";\n");
            }
            if !config.models.is_empty() {
                indent(out, inner);
                out.push_str("models [");
                write_ident_list(out, &config.models);
                out.push_str("];\n");
            }
            if let Some(mem) = &config.memory {
                indent(out, inner);
                out.push_str("memory ");
                match mem {
                    MemoryRef::Persistent => out.push_str("persistent"),
                    MemoryRef::Temporary => out.push_str("temporary"),
                    MemoryRef::Named(id) => out.push_str(&id.name),
                }
                out.push_str(";\n");
            }
            if !config.tools.is_empty() {
                indent(out, inner);
                out.push_str("tools [");
                write_ident_list(out, &config.tools);
                out.push_str("];\n");
            }
            if !config.permissions.is_empty() {
                indent(out, inner);
                out.push_str("permissions [");
                write_ident_list(out, &config.permissions);
                out.push_str("];\n");
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Model {
            name,
            endpoint,
            config,
            ..
        } => {
            out.push_str("model ");
            out.push_str(&name.name);
            if endpoint.is_none() && config.is_empty() {
                out.push_str(";\n");
            } else {
                out.push_str(" {\n");
                if let Some(e) = endpoint {
                    indent(out, level + 1);
                    out.push_str("endpoint ");
                    write_expr(out, e);
                    out.push_str(";\n");
                }
                for (key, value) in config {
                    indent(out, level + 1);
                    out.push_str(&key.name);
                    out.push(' ');
                    write_expr(out, value);
                    out.push_str(";\n");
                }
                indent(out, level);
                out.push_str("}\n");
            }
        }
        Stmt::Tool { name, .. } => {
            out.push_str("tool ");
            out.push_str(&name.name);
            out.push_str(";\n");
        }
        Stmt::Permission { name, .. } => {
            out.push_str("permission ");
            out.push_str(&name.name);
            out.push_str(";\n");
        }
        Stmt::Memory { name, kind, .. } => {
            out.push_str("memory ");
            out.push_str(&name.name);
            out.push_str(match kind {
                MemoryKind::Persistent => " persistent;\n",
                MemoryKind::Temporary => " temporary;\n",
            });
        }
        Stmt::Prompt { name, template, .. } => {
            out.push_str("prompt ");
            out.push_str(&name.name);
            out.push_str(" = ");
            write_expr(out, template);
            out.push_str(";\n");
        }
        Stmt::Schema { name, fields, .. } => {
            out.push_str("schema ");
            out.push_str(&name.name);
            out.push_str(" {\n");
            for (field_name, field_type) in fields {
                indent(out, level + 1);
                out.push_str(&field_name.name);
                out.push_str(": ");
                write_type(out, field_type);
                out.push_str(";\n");
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Workflow { name, stages, .. } => {
            out.push_str("workflow ");
            out.push_str(&name.name);
            out.push_str(" {\n");
            indent(out, level + 1);
            for (i, stage) in stages.iter().enumerate() {
                if i > 0 {
                    out.push_str(" -> ");
                }
                out.push_str(&stage.name);
            }
            out.push_str(";\n");
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Parallel { body, .. } => {
            out.push_str("parallel {\n");
            write_block_body(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Team { name, members, .. } => {
            out.push_str("team ");
            out.push_str(&name.name);
            out.push_str(" {\n");
            for member in members {
                indent(out, level + 1);
                out.push_str(&member.name);
                out.push_str(";\n");
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Knowledge { name, source, .. } => {
            out.push_str("knowledge ");
            out.push_str(&name.name);
            out.push_str(" {\n");
            indent(out, level + 1);
            out.push_str("source ");
            write_expr(out, source);
            out.push_str(";\n");
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::AiPlaceholder {
            keyword,
            name,
            config,
            ..
        } => {
            out.push_str(keyword.keyword());
            out.push(' ');
            out.push_str(&name.name);
            if config.is_empty() {
                out.push_str(";\n");
            } else {
                out.push_str(" {\n");
                for (key, value) in config {
                    indent(out, level + 1);
                    out.push_str(&key.name);
                    out.push(' ');
                    write_expr(out, value);
                    out.push_str(";\n");
                }
                indent(out, level);
                out.push_str("}\n");
            }
        }
        Stmt::Network { name, members, .. } => {
            out.push_str("network ");
            out.push_str(&name.name);
            out.push_str(" {\n");
            for member in members {
                indent(out, level + 1);
                out.push_str("agent ");
                out.push_str(&member.name);
                out.push_str(";\n");
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Cluster {
            name,
            replicas,
            autoscaling,
            ..
        } => {
            out.push_str("cluster ");
            out.push_str(&name.name);
            out.push_str(" {\n");
            if let Some(r) = replicas {
                indent(out, level + 1);
                out.push_str("replicas ");
                write_expr(out, r);
                out.push_str(";\n");
            }
            if let Some(a) = autoscaling {
                indent(out, level + 1);
                out.push_str("autoscaling ");
                out.push_str(if *a { "enabled" } else { "disabled" });
                out.push_str(";\n");
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::OnEvent {
            event_name, body, ..
        } => {
            out.push_str("on ");
            out.push_str(&event_name.name);
            out.push_str(" {\n");
            write_block_body(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }

        // v3.0 constructs — formatted as declarative manifests
        Stmt::Goal { name, objective, constraints, body, .. } => {
            out.push_str(&format!("goal {} \"{}\" {{\n", name.name, objective));
            if !constraints.is_empty() {
                indent(out, level + 1);
                out.push_str("constraints {\n");
                for c in constraints {
                    indent(out, level + 2);
                    out.push_str(&format!("{:?} {:?} {:?};\n", c.kind, c.op, c.value));
                }
                indent(out, level + 1);
                out.push_str("}\n");
            }
            write_block_body(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }

        Stmt::Supervisor { name, strategy, children, .. } => {
            let s = match strategy {
                SupervisionStrategy::OneForOne => "one_for_one",
                SupervisionStrategy::OneForAll => "one_for_all",
                SupervisionStrategy::RestForOne => "rest_for_one",
            };
            out.push_str(&format!("supervisor {} {} {{\n", name.name, s));
            if !children.is_empty() {
                indent(out, level + 1);
                out.push_str("children [");
                write_ident_list(out, children);
                out.push_str("];\n");
            }
            indent(out, level);
            out.push_str("}\n");
        }

        Stmt::Resource { name, kind, config, .. } => {
            out.push_str(&format!("resource {} {:?} {{\n", name.name, kind));
            for (k, v) in config {
                indent(out, level + 1);
                out.push_str(&format!("{} ", k));
                write_expr(out, v);
                out.push_str(";\n");
            }
            indent(out, level);
            out.push_str("}\n");
        }

        Stmt::Reason { max_iterations, budget, body, .. } => {
            out.push_str("reason");
            if let Some(max) = max_iterations {
                out.push_str(" max ");
                write_expr(out, max);
                out.push_str(" iterations");
            }
            if let Some(b) = budget {
                out.push_str(" budget ");
                write_expr(out, b);
            }
            out.push_str(" {\n");
            write_block_body(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }

        Stmt::Device { name, kind, permissions, .. } => {
            out.push_str(&format!("device {} {:?} {{\n", name.name, kind));
            if !permissions.is_empty() {
                indent(out, level + 1);
                out.push_str(&format!("permissions [{}];\n", permissions.join(", ")));
            }
            indent(out, level);
            out.push_str("}\n");
        }

        Stmt::Ui { name, title, panels, .. } => {
            out.push_str(&format!("ui {} {{\n", name.name));
            if let Some(t) = title {
                indent(out, level + 1);
                out.push_str(&format!("title \"{}\";\n", t));
            }
            for panel in panels {
                indent(out, level + 1);
                out.push_str(&format!("panel {} {};\n", panel.name.name, panel.kind));
            }
            indent(out, level);
            out.push_str("}\n");
        }

        Stmt::Application { name, agents, models, knowledge, memory, workflows, policies, .. } => {
            out.push_str(&format!("application {} {{\n", name.name));
            if !agents.is_empty() {
                indent(out, level + 1);
                out.push_str("agents [");
                write_ident_list(out, agents);
                out.push_str("];\n");
            }
            if !models.is_empty() {
                indent(out, level + 1);
                out.push_str("models [");
                write_ident_list(out, models);
                out.push_str("];\n");
            }
            for km in knowledge.iter().chain(memory.iter()) {
                indent(out, level + 1);
                out.push_str(&format!("knowledge {};\n", km.name));
            }
            if !workflows.is_empty() {
                indent(out, level + 1);
                out.push_str("workflows [");
                write_ident_list(out, workflows);
                out.push_str("];\n");
            }
            for rule in policies {
                indent(out, level + 1);
                let action = match rule.action { PolicyAction::Allow => "allow", PolicyAction::Deny => "deny" };
                out.push_str(&format!("{} {} {};\n", action, rule.subject, rule.resource));
            }
            indent(out, level);
            out.push_str("}\n");
        }

        Stmt::Policy { name, rules, .. } => {
            out.push_str(&format!("policy {} {{\n", name.name));
            for rule in rules {
                indent(out, level + 1);
                let action = match rule.action { PolicyAction::Allow => "allow", PolicyAction::Deny => "deny" };
                out.push_str(&format!("{} {} {};\n", action, rule.subject, rule.resource));
            }
            indent(out, level);
            out.push_str("}\n");
        }

        Stmt::Emit { event, payload, .. } => {
            out.push_str(&format!("emit \"{}\"", event));
            if let Some(p) = payload {
                out.push(' ');
                write_expr(out, p);
            }
            out.push_str(";\n");
        }
    }
}

fn write_ident_list(out: &mut String, items: &[Ident]) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&item.name);
    }
}

fn write_else(out: &mut String, branch: &Option<Box<ElseBranch>>, level: usize) {
    if let Some(b) = branch {
        match b.as_ref() {
            ElseBranch::Else(block) => {
                out.push_str(" else {\n");
                write_block_body(out, block, level + 1);
                indent(out, level);
                out.push('}');
            }
            ElseBranch::ElseIf(stmt) => {
                out.push_str(" else ");
                if let Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } = stmt.as_ref()
                {
                    out.push_str("if ");
                    write_expr(out, condition);
                    out.push_str(" {\n");
                    write_block_body(out, then_branch, level + 1);
                    indent(out, level);
                    out.push('}');
                    write_else(out, else_branch, level);
                }
            }
        }
    }
}

fn write_block_body(out: &mut String, block: &Block, level: usize) {
    for stmt in &block.statements {
        write_stmt(out, stmt, level);
    }
}

fn write_type(out: &mut String, t: &TypeExpr) {
    match t {
        TypeExpr::Int => out.push_str("int"),
        TypeExpr::Float => out.push_str("float"),
        TypeExpr::Bool => out.push_str("bool"),
        TypeExpr::String => out.push_str("string"),
        TypeExpr::Char => out.push_str("char"),
        TypeExpr::Null => out.push_str("null"),
        TypeExpr::Void => out.push_str("void"),
        TypeExpr::Array(inner) => {
            out.push('[');
            write_type(out, inner);
            out.push(']');
        }
        TypeExpr::Map(k, v) => {
            out.push('{');
            write_type(out, k);
            out.push_str(": ");
            write_type(out, v);
            out.push('}');
        }
        TypeExpr::Function { params, ret } => {
            out.push_str("fn(");
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_type(out, p);
            }
            out.push_str(") -> ");
            write_type(out, ret);
        }
        TypeExpr::Named(n) => out.push_str(n),
    }
}

fn write_expr(out: &mut String, expr: &Expr) {
    match expr {
        Expr::Literal { value, .. } => write_literal(out, value),
        Expr::Identifier { name, .. } => out.push_str(name),
        Expr::ArrayLiteral { elements, .. } => {
            out.push('[');
            for (i, e) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr(out, e);
            }
            out.push(']');
        }
        Expr::MapLiteral { entries, .. } => {
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr(out, k);
                out.push_str(": ");
                write_expr(out, v);
            }
            out.push('}');
        }
        Expr::Binary {
            left, op, right, ..
        } => {
            write_expr(out, left);
            out.push(' ');
            out.push_str(binary_op_str(*op));
            out.push(' ');
            write_expr(out, right);
        }
        Expr::Unary { op, operand, .. } => {
            out.push_str(match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            });
            write_expr(out, operand);
        }
        Expr::Assign {
            target, op, value, ..
        } => {
            write_expr(out, target);
            out.push(' ');
            out.push_str(match op {
                AssignOp::Assign => "=",
                AssignOp::AddAssign => "+=",
                AssignOp::SubAssign => "-=",
                AssignOp::MulAssign => "*=",
                AssignOp::DivAssign => "/=",
            });
            out.push(' ');
            write_expr(out, value);
        }
        Expr::Call { callee, args, .. } => {
            write_expr(out, callee);
            out.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr(out, a);
            }
            out.push(')');
        }
        Expr::Index { object, index, .. } => {
            write_expr(out, object);
            out.push('[');
            write_expr(out, index);
            out.push(']');
        }
        Expr::Member {
            object, property, ..
        } => {
            write_expr(out, object);
            out.push('.');
            out.push_str(&property.name);
        }
        Expr::Grouping { expr, .. } => {
            out.push('(');
            write_expr(out, expr);
            out.push(')');
        }
        Expr::Generate { schema_name, .. } => {
            out.push_str("generate ");
            out.push_str(&schema_name.name);
        }
    }
}

fn write_literal(out: &mut String, lit: &Literal) {
    match lit {
        Literal::Int(n) => out.push_str(&n.to_string()),
        Literal::Float(n) => out.push_str(&n.to_string()),
        Literal::Bool(b) => out.push_str(&b.to_string()),
        Literal::String(s) => {
            out.push('"');
            out.push_str(&s.replace('\\', "\\\\").replace('"', "\\\""));
            out.push('"');
        }
        Literal::Char(c) => {
            out.push('\'');
            out.push(*c);
            out.push('\'');
        }
        Literal::Null => out.push_str("null"),
    }
}

fn binary_op_str(op: BinaryOp) -> &'static str {
    use BinaryOp::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Mod => "%",
        Eq => "==",
        NotEq => "!=",
        Gt => ">",
        Lt => "<",
        GtEq => ">=",
        LtEq => "<=",
        And => "&&",
        Or => "||",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailang_parser::parse;

    fn fmt(src: &str) -> String {
        let (program, diags) = parse(src);
        assert!(!diags.has_errors());
        format_program(&program.unwrap())
    }

    #[test]
    fn formats_let_statement() {
        assert_eq!(fmt("let   x=10;"), "let x = 10;\n");
    }

    #[test]
    fn formats_function() {
        let out = fmt("fn add(a:int,b:int)->int{return a+b;}");
        assert_eq!(
            out,
            "fn add(a: int, b: int) -> int {\n    return a + b;\n}\n"
        );
    }

    #[test]
    fn formatting_is_idempotent() {
        let once = fmt("fn add(a:int,b:int)->int{return a+b;}");
        let twice = fmt(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn formats_if_else() {
        let out = fmt("if x>0{print(1);}else{print(2);}");
        assert_eq!(
            out,
            "if x > 0 {\n    print(1);\n} else {\n    print(2);\n}\n"
        );
    }

    // -----------------------------------------------------------------
    // v0.5 AI-native construct formatting
    // -----------------------------------------------------------------

    fn assert_idempotent(src: &str) {
        let once = fmt(src);
        let twice = fmt(&once);
        assert_eq!(
            once, twice,
            "formatting is not stable (idempotent) for:\n{src}"
        );
    }

    #[test]
    fn formats_agent_with_full_config() {
        let out = fmt(
            "agent Researcher{model GPT5;memory persistent;tools[Search,Browser];permissions[internet];}",
        );
        assert_eq!(
            out,
            "agent Researcher {\n    model GPT5;\n    memory persistent;\n    tools [Search, Browser];\n    permissions [internet];\n}\n"
        );
    }

    #[test]
    fn agent_formatting_is_idempotent() {
        assert_idempotent(
            "agent Researcher { model GPT5; memory persistent; tools [Search]; permissions [internet]; }",
        );
    }

    #[test]
    fn formats_agent_with_location_clause() {
        let out = fmt("agent Planner on cloud{model GPT5;}");
        assert_eq!(out, "agent Planner on cloud {\n    model GPT5;\n}\n");
        assert_idempotent("agent Planner on cloud { model GPT5; }");
    }

    #[test]
    fn formats_bare_and_block_model_declarations() {
        assert_eq!(fmt("model GPT5;"), "model GPT5;\n");
        let out = fmt(r#"model LocalModel{endpoint "http://localhost:11434";}"#);
        assert_eq!(
            out,
            "model LocalModel {\n    endpoint \"http://localhost:11434\";\n}\n"
        );
    }

    #[test]
    fn formats_tool_permission_and_memory_declarations() {
        assert_eq!(fmt("tool Search;"), "tool Search;\n");
        assert_eq!(fmt("permission internet;"), "permission internet;\n");
        assert_eq!(
            fmt("memory UserMemory persistent;"),
            "memory UserMemory persistent;\n"
        );
    }

    #[test]
    fn formats_schema_declaration() {
        let out = fmt("schema Person{name:string;age:int;}");
        assert_eq!(
            out,
            "schema Person {\n    name: string;\n    age: int;\n}\n"
        );
        assert_idempotent("schema Person { name: string; age: int; }");
    }

    #[test]
    fn formats_workflow_as_a_single_line_chain() {
        let out = fmt("workflow Paper{Researcher->Writer->Reviewer;}");
        assert_eq!(
            out,
            "workflow Paper {\n    Researcher -> Writer -> Reviewer;\n}\n"
        );
        assert_idempotent("workflow Paper { Researcher -> Writer -> Reviewer; }");
    }

    #[test]
    fn formats_parallel_block() {
        let out = fmt("parallel{print(1);print(2);}");
        assert_eq!(out, "parallel {\n    print(1);\n    print(2);\n}\n");
    }

    #[test]
    fn formats_team_declaration() {
        let out = fmt("team Startup{CEO;CTO;}");
        assert_eq!(out, "team Startup {\n    CEO;\n    CTO;\n}\n");
    }

    #[test]
    fn formats_knowledge_declaration() {
        let out = fmt(r#"knowledge Docs{source "./docs";}"#);
        assert_eq!(out, "knowledge Docs {\n    source \"./docs\";\n}\n");
    }

    #[test]
    fn formats_generate_expression() {
        assert_eq!(
            fmt("let p = generate Person;"),
            "let p = generate Person;\n"
        );
    }

    #[test]
    fn formats_all_ai_placeholder_keywords() {
        let out = fmt("reasoning R; embedding E; dataset D;");
        // Top-level statements are blank-line-separated (consistent with
        // every other multi-statement program the formatter handles).
        assert_eq!(out, "reasoning R;\n\nembedding E;\n\ndataset D;\n");
    }

    #[test]
    fn formats_ai_placeholder_with_config_block() {
        let out = fmt(r#"dataset Reviews{source "./reviews.csv";}"#);
        assert_eq!(out, "dataset Reviews {\n    source \"./reviews.csv\";\n}\n");
    }

    #[test]
    fn full_v05_program_formatting_is_idempotent() {
        assert_idempotent(
            r#"
            model GPT5;
            tool Search;
            permission internet;
            agent Researcher { model GPT5; tools [Search]; permissions [internet]; }
            workflow Paper { Researcher -> Researcher; }
            schema Person { name: string; }
            knowledge Docs { source "./docs"; }
            team Squad { Researcher; }
            "#,
        );
    }

    // -----------------------------------------------------------------
    // v2.0 distributed-intelligence construct formatting
    // -----------------------------------------------------------------

    #[test]
    fn formats_network_declaration() {
        let out = fmt("agent A{} network N{agent A;}");
        assert_eq!(out, "agent A {\n}\n\nnetwork N {\n    agent A;\n}\n");
    }

    #[test]
    fn network_formatting_is_idempotent() {
        assert_idempotent("agent A {} agent B {} network N { agent A; agent B; }");
    }

    #[test]
    fn formats_cluster_declaration() {
        let out = fmt("cluster Workers{replicas 100;autoscaling enabled;}");
        assert_eq!(
            out,
            "cluster Workers {\n    replicas 100;\n    autoscaling enabled;\n}\n"
        );
        assert_idempotent("cluster Workers { replicas 100; autoscaling enabled; }");
    }

    #[test]
    fn formats_cluster_with_autoscaling_disabled() {
        let out = fmt("cluster Small{replicas 2;autoscaling disabled;}");
        assert!(out.contains("autoscaling disabled;"));
    }

    #[test]
    fn formats_on_event_handler() {
        let out = fmt("on task_completed{println(1);}");
        assert_eq!(out, "on task_completed {\n    println(1);\n}\n");
        assert_idempotent("on task_completed { println(1); }");
    }

    #[test]
    fn formats_agent_with_multiple_models() {
        let out = fmt("model GPT5;model Claude;agent A{models[GPT5,Claude];}");
        assert!(out.contains("models [GPT5, Claude];"));
        assert_idempotent("model GPT5; model Claude; agent A { models [GPT5, Claude]; }");
    }

    #[test]
    fn full_v20_program_formatting_is_idempotent() {
        assert_idempotent(
            r#"
            model GPT5;
            agent Planner on cloud { model GPT5; }
            agent Researcher on edge { models [GPT5]; }
            network Enterprise { agent Planner; agent Researcher; }
            cluster Workers { replicas 10; autoscaling enabled; }
            on task_completed { println("done"); }
            "#,
        );
    }
}
