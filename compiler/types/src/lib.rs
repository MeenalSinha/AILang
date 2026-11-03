//! # ailang-types
//!
//! Static type representation and a best-effort type checker for AILang
//! v0.1.
//!
//! v0.1 deliberately supports only primitive types, arrays, maps, and
//! function types, with simple bidirectional inference for `let` without an
//! annotation. Generics are **not** implemented, but [`Type`] is structured
//! (a plain enum with boxed recursive members, no generic-arity assumptions
//! baked into call sites) so a `Type::Generic(String, Vec<Type>)` variant can
//! be added later without reshaping existing variants — see
//! `docs/roadmap.md`.

use std::collections::HashMap;
use std::fmt;

use ailang_ast::{
    BinaryOp, Block, ElseBranch, Expr, Literal, Param, Program, Stmt, TypeExpr, UnaryOp,
};
use ailang_diagnostics::{Diagnostic, DiagnosticBag, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Char,
    Null,
    Void,
    Array(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
    },
    /// Used for names the checker cannot yet resolve (forward references,
    /// future-reserved constructs, module-qualified names). Never causes a
    /// type-mismatch error against anything.
    Unknown,
    // ---- v0.5 AI-native value types ----
    // Deliberately minimal: none of these participate in arithmetic,
    // comparison, or arbitrary assignment coercion (they're `compatible`
    // only with themselves and `Unknown`, same as every other type here).
    // Richer structural typing is additive on top of this once there's
    // demand; see docs/roadmap.md.
    Agent,
    Model,
    Tool,
    MemoryHandle,
    Prompt,
    /// Carries the schema's name so two different schemas are distinct
    /// types, even though v0.5 doesn't yet check field-level structural
    /// compatibility.
    Schema(String),
    Workflow,
    Team,
    Knowledge,
    /// One of the generic placeholder constructs (`reasoning`, `embedding`,
    /// `dataset`, `vector`, `pipeline`, `task`, `deploy`).
    AiPlaceholder,
    // ---- v2.0 distributed-intelligence value types ----
    Network,
    Cluster,
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Float => write!(f, "float"),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Char => write!(f, "char"),
            Type::Null => write!(f, "null"),
            Type::Void => write!(f, "void"),
            Type::Array(t) => write!(f, "[{t}]"),
            Type::Map(k, v) => write!(f, "{{{k}: {v}}}"),
            Type::Function { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            Type::Unknown => write!(f, "unknown"),
            Type::Agent => write!(f, "agent"),
            Type::Model => write!(f, "model"),
            Type::Tool => write!(f, "tool"),
            Type::MemoryHandle => write!(f, "memory"),
            Type::Prompt => write!(f, "prompt"),
            Type::Schema(name) => write!(f, "schema<{name}>"),
            Type::Workflow => write!(f, "workflow"),
            Type::Team => write!(f, "team"),
            Type::Knowledge => write!(f, "knowledge"),
            Type::AiPlaceholder => write!(f, "ai_placeholder"),
            Type::Network => write!(f, "network"),
            Type::Cluster => write!(f, "cluster"),
        }
    }
}

impl Type {
    pub fn from_type_expr(te: &TypeExpr) -> Type {
        match te {
            TypeExpr::Int => Type::Int,
            TypeExpr::Float => Type::Float,
            TypeExpr::Bool => Type::Bool,
            TypeExpr::String => Type::String,
            TypeExpr::Char => Type::Char,
            TypeExpr::Null => Type::Null,
            TypeExpr::Void => Type::Void,
            TypeExpr::Array(inner) => Type::Array(Box::new(Type::from_type_expr(inner))),
            TypeExpr::Map(k, v) => Type::Map(
                Box::new(Type::from_type_expr(k)),
                Box::new(Type::from_type_expr(v)),
            ),
            TypeExpr::Function { params, ret } => Type::Function {
                params: params.iter().map(Type::from_type_expr).collect(),
                ret: Box::new(Type::from_type_expr(ret)),
            },
            TypeExpr::Named(_) => Type::Unknown,
        }
    }

    /// Numeric types can be compared/combined with `+ - * / %`.
    fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Float)
    }

    fn compatible(&self, other: &Type) -> bool {
        self == other || self == &Type::Unknown || other == &Type::Unknown
    }
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<Type>,
    ret: Type,
}

#[derive(Default)]
struct Scope {
    vars: HashMap<String, Type>,
}

pub struct TypeChecker {
    scopes: Vec<Scope>,
    functions: HashMap<String, FunctionSig>,
    diagnostics: DiagnosticBag,
    current_return: Vec<Type>,
    /// Field types for every declared `schema`, populated as schema
    /// declarations are checked, and consulted by `generate SchemaName`
    /// expressions (see the `Expr::Generate` arm of `check_expr`).
    schemas: HashMap<String, Vec<(String, Type)>>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope::default()],
            functions: HashMap::new(),
            diagnostics: DiagnosticBag::new(),
            current_return: Vec::new(),
            schemas: HashMap::new(),
        }
    }

    pub fn check_program(mut self, program: &Program) -> DiagnosticBag {
        self.collect_function_signatures(&program.statements);
        for stmt in &program.statements {
            self.check_stmt(stmt);
        }
        self.diagnostics
    }

    fn collect_function_signatures(&mut self, statements: &[Stmt]) {
        for stmt in statements {
            if let Stmt::Function {
                name,
                params,
                return_type,
                ..
            } = stmt
            {
                let sig = FunctionSig {
                    params: params
                        .iter()
                        .map(|p: &Param| {
                            p.type_annotation
                                .as_ref()
                                .map(Type::from_type_expr)
                                .unwrap_or(Type::Unknown)
                        })
                        .collect(),
                    ret: return_type
                        .as_ref()
                        .map(Type::from_type_expr)
                        .unwrap_or(Type::Void),
                };
                self.functions.insert(name.name.clone(), sig);
            } else if let Stmt::Module { body, .. } = stmt {
                self.collect_function_signatures(body);
            }
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: &str, ty: Type) {
        self.scopes
            .last_mut()
            .unwrap()
            .vars
            .insert(name.to_string(), ty);
    }

    fn lookup(&self, name: &str) -> Type {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.vars.get(name) {
                return t.clone();
            }
        }
        Type::Unknown
    }

    fn error(&mut self, code: &'static str, msg: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::error(code, msg, span));
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl {
                name,
                type_annotation,
                initializer,
                span,
                ..
            } => {
                let declared = type_annotation.as_ref().map(Type::from_type_expr);
                let inferred = initializer.as_ref().map(|e| self.check_expr(e));
                let final_ty = match (&declared, &inferred) {
                    (Some(d), Some(i)) => {
                        if !d.compatible(i) {
                            self.error(
                                "T001",
                                format!(
                                    "type mismatch in declaration of `{}`: expected `{}`, found `{}`",
                                    name.name, d, i
                                ),
                                *span,
                            );
                        }
                        d.clone()
                    }
                    (Some(d), None) => d.clone(),
                    (None, Some(i)) => i.clone(),
                    (None, None) => Type::Unknown,
                };
                self.define(&name.name, final_ty);
            }
            Stmt::Function {
                name,
                params,
                body,
                return_type,
                ..
            } => {
                self.push_scope();
                for p in params {
                    let t = p
                        .type_annotation
                        .as_ref()
                        .map(Type::from_type_expr)
                        .unwrap_or(Type::Unknown);
                    self.define(&p.name.name, t);
                }
                let ret_ty = return_type
                    .as_ref()
                    .map(Type::from_type_expr)
                    .unwrap_or(Type::Void);
                self.current_return.push(ret_ty);
                self.check_block(body);
                self.current_return.pop();
                self.pop_scope();
                let _ = name; // name already registered in signature pass
            }
            Stmt::Return { value, span } => {
                let actual = value
                    .as_ref()
                    .map(|e| self.check_expr(e))
                    .unwrap_or(Type::Void);
                if let Some(expected) = self.current_return.last().cloned() {
                    if !expected.compatible(&actual) {
                        self.error(
                            "T002",
                            format!(
                                "return type mismatch: expected `{expected}`, found `{actual}`"
                            ),
                            *span,
                        );
                    }
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                span,
            } => {
                let cond_ty = self.check_expr(condition);
                if !cond_ty.compatible(&Type::Bool) {
                    self.error(
                        "T003",
                        format!("`if` condition must be `bool`, found `{cond_ty}`"),
                        *span,
                    );
                }
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
                condition,
                body,
                span,
            } => {
                let cond_ty = self.check_expr(condition);
                if !cond_ty.compatible(&Type::Bool) {
                    self.error(
                        "T004",
                        format!("`while` condition must be `bool`, found `{cond_ty}`"),
                        *span,
                    );
                }
                self.push_scope();
                self.check_block(body);
                self.pop_scope();
            }
            Stmt::For {
                variable,
                start,
                end,
                body,
                ..
            } => {
                let s = self.check_expr(start);
                let e = self.check_expr(end);
                if !s.compatible(&Type::Int) || !e.compatible(&Type::Int) {
                    self.error("T005", "`for` range bounds must be `int`", variable.span);
                }
                self.push_scope();
                self.define(&variable.name, Type::Int);
                self.check_block(body);
                self.pop_scope();
            }
            Stmt::Block(b) => {
                self.push_scope();
                self.check_block(b);
                self.pop_scope();
            }
            Stmt::Expression { expr, .. } => {
                self.check_expr(expr);
            }
            Stmt::Module { body, .. } => {
                for s in body {
                    self.check_stmt(s);
                }
            }
            Stmt::Import { .. } | Stmt::Break { .. } | Stmt::Continue { .. } => {}

            // -----------------------------------------------------------
            // v0.5 AI-native constructs. Cross-reference validity (does
            // `model GPT5` actually exist?) is `ailang-semantic`'s job;
            // this checker only needs to type each declared name and walk
            // any embedded expressions for arithmetic/logical soundness.
            // -----------------------------------------------------------
            Stmt::Agent { name, .. } => self.define(&name.name, Type::Agent),
            Stmt::Model {
                name,
                endpoint,
                config,
                ..
            } => {
                self.define(&name.name, Type::Model);
                if let Some(e) = endpoint {
                    self.check_expr(e);
                }
                for (_, value) in config {
                    self.check_expr(value);
                }
            }
            Stmt::Tool { name, .. } => self.define(&name.name, Type::Tool),
            Stmt::Permission { name, .. } => self.define(&name.name, Type::Unknown),
            Stmt::Memory { name, .. } => self.define(&name.name, Type::MemoryHandle),
            Stmt::Prompt { name, template, .. } => {
                self.define(&name.name, Type::Prompt);
                let t = self.check_expr(template);
                if !t.compatible(&Type::String) {
                    self.error(
                        "T016",
                        format!("prompt template must be `string`, found `{t}`"),
                        template.span(),
                    );
                }
            }
            Stmt::Schema { name, fields, .. } => {
                let field_types = fields
                    .iter()
                    .map(|(fname, ty)| (fname.name.clone(), Type::from_type_expr(ty)))
                    .collect();
                self.schemas.insert(name.name.clone(), field_types);
                self.define(&name.name, Type::Schema(name.name.clone()));
            }
            Stmt::Workflow { name, .. } => self.define(&name.name, Type::Workflow),
            Stmt::Parallel { body, .. } => {
                self.push_scope();
                self.check_block(body);
                self.pop_scope();
            }
            Stmt::Team { name, .. } => self.define(&name.name, Type::Team),
            Stmt::Knowledge { name, source, .. } => {
                self.define(&name.name, Type::Knowledge);
                let t = self.check_expr(source);
                if !t.compatible(&Type::String) {
                    self.error(
                        "T017",
                        format!("knowledge `source` must be `string`, found `{t}`"),
                        source.span(),
                    );
                }
            }
            Stmt::AiPlaceholder { name, config, .. } => {
                self.define(&name.name, Type::AiPlaceholder);
                for (_, value) in config {
                    self.check_expr(value);
                }
            }
            Stmt::Network { name, .. } => self.define(&name.name, Type::Network),
            Stmt::Cluster { name, replicas, .. } => {
                self.define(&name.name, Type::Cluster);
                if let Some(r) = replicas {
                    let t = self.check_expr(r);
                    if !t.compatible(&Type::Int) {
                        self.error(
                            "T018",
                            format!("`replicas` must be `int`, found `{t}`"),
                            r.span(),
                        );
                    }
                }
            }
            Stmt::OnEvent { body, .. } => {
                // `payload` is bound implicitly at `emit()` time — see
                // ailang-semantic's matching fix for why it must be
                // declared here too, with type `Unknown` since its real
                // type depends entirely on whatever `emit()` is called
                // with at each call site.
                self.push_scope();
                self.define("payload", Type::Unknown);
                self.check_block(body);
                self.pop_scope();
            }

            // v3.0 constructs — typed as opaque AI OS resources.
            // Full structural typing of goals, supervisors, resources, etc.
            // is a planned v3.1 enhancement tracked in docs/roadmap.md.
            Stmt::Goal { name, body, .. } => {
                self.define(&name.name, Type::Unknown);
                self.push_scope();
                self.check_block(body);
                self.pop_scope();
            }

            Stmt::Supervisor { name, .. } => {
                self.define(&name.name, Type::Unknown);
            }

            Stmt::Resource { name, config, .. } => {
                self.define(&name.name, Type::Unknown);
                for (_, v) in config {
                    self.check_expr(v);
                }
            }

            Stmt::Reason { max_iterations, budget, body, .. } => {
                if let Some(e) = max_iterations {
                    self.check_expr(e);
                }
                if let Some(e) = budget {
                    self.check_expr(e);
                }
                self.push_scope();
                self.check_block(body);
                self.pop_scope();
            }

            Stmt::Device { name, .. } => {
                self.define(&name.name, Type::Unknown);
            }

            Stmt::Ui { name, .. } => {
                self.define(&name.name, Type::Unknown);
            }

            Stmt::Application { name, .. } => {
                self.define(&name.name, Type::Unknown);
            }

            Stmt::Policy { name, .. } => {
                self.define(&name.name, Type::Unknown);
            }

            Stmt::Emit { payload, .. } => {
                if let Some(e) = payload {
                    self.check_expr(e);
                }
            }
        }
    }

    fn check_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            Expr::Literal { value, .. } => match value {
                Literal::Int(_) => Type::Int,
                Literal::Float(_) => Type::Float,
                Literal::Bool(_) => Type::Bool,
                Literal::String(_) => Type::String,
                Literal::Char(_) => Type::Char,
                Literal::Null => Type::Null,
            },
            Expr::Identifier { name, .. } => self.lookup(name),
            Expr::ArrayLiteral { elements, .. } => {
                if let Some(first) = elements.first() {
                    let t = self.check_expr(first);
                    for el in &elements[1..] {
                        self.check_expr(el);
                    }
                    Type::Array(Box::new(t))
                } else {
                    Type::Array(Box::new(Type::Unknown))
                }
            }
            Expr::MapLiteral { entries, .. } => {
                if let Some((k, v)) = entries.first() {
                    let kt = self.check_expr(k);
                    let vt = self.check_expr(v);
                    for (k2, v2) in &entries[1..] {
                        self.check_expr(k2);
                        self.check_expr(v2);
                    }
                    Type::Map(Box::new(kt), Box::new(vt))
                } else {
                    Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown))
                }
            }
            Expr::Grouping { expr, .. } => self.check_expr(expr),
            Expr::Unary { op, operand, span } => {
                let t = self.check_expr(operand);
                match op {
                    UnaryOp::Neg => {
                        if !t.is_numeric() && t != Type::Unknown {
                            self.error("T006", format!("cannot negate `{t}`"), *span);
                        }
                        t
                    }
                    UnaryOp::Not => {
                        if !t.compatible(&Type::Bool) {
                            self.error("T007", format!("cannot apply `!` to `{t}`"), *span);
                        }
                        Type::Bool
                    }
                }
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                let lt = self.check_expr(left);
                let rt = self.check_expr(right);
                self.check_binary(*op, &lt, &rt, *span)
            }
            Expr::Assign {
                target,
                value,
                span,
                ..
            } => {
                let target_ty = self.check_expr(target);
                let value_ty = self.check_expr(value);
                if !target_ty.compatible(&value_ty) {
                    self.error(
                        "T008",
                        format!("cannot assign `{value_ty}` to variable of type `{target_ty}`"),
                        *span,
                    );
                }
                target_ty
            }
            Expr::Call { callee, args, span } => {
                let arg_types: Vec<Type> = args.iter().map(|a| self.check_expr(a)).collect();
                if let Expr::Identifier { name, .. } = callee.as_ref() {
                    if let Some(sig) = self.functions.get(name).cloned() {
                        if sig.params.len() != arg_types.len() {
                            self.error(
                                "T009",
                                format!(
                                    "function `{name}` expects {} argument(s), found {}",
                                    sig.params.len(),
                                    arg_types.len()
                                ),
                                *span,
                            );
                        } else {
                            for (expected, actual) in sig.params.iter().zip(arg_types.iter()) {
                                if !expected.compatible(actual) {
                                    self.error(
                                        "T010",
                                        format!(
                                            "argument type mismatch in call to `{name}`: expected `{expected}`, found `{actual}`"
                                        ),
                                        *span,
                                    );
                                }
                            }
                        }
                        return sig.ret;
                    }
                    // Unknown / builtin / stdlib function: skip strict
                    // checking rather than erroring (v0.1 stdlib functions
                    // aren't registered with signatures here yet).
                }
                Type::Unknown
            }
            Expr::Index {
                object,
                index,
                span,
            } => {
                let obj_ty = self.check_expr(object);
                let idx_ty = self.check_expr(index);
                match obj_ty {
                    Type::Array(inner) => {
                        if !idx_ty.compatible(&Type::Int) {
                            self.error("T011", "array index must be `int`", *span);
                        }
                        *inner
                    }
                    Type::Map(_, v) => *v,
                    Type::Unknown => Type::Unknown,
                    // A `generate SchemaName` expression is statically
                    // typed `Schema(name)` but is a `Value::Map` at
                    // runtime (see ailang-interpreter's `Expr::Generate`
                    // handling and ailang_ai_runtime::fill_schema_defaults).
                    // Field-level static typing of individual schema
                    // fields (`p["age"]` statically known as `int`) is a
                    // future enhancement — see docs/roadmap.md — so this
                    // degrades to `Unknown` rather than erroring.
                    Type::Schema(_) => Type::Unknown,
                    other => {
                        self.error("T012", format!("cannot index into `{other}`"), *span);
                        Type::Unknown
                    }
                }
            }
            Expr::Member { .. } => Type::Unknown,
            Expr::Generate { schema_name, .. } => self
                .schemas
                .get(&schema_name.name)
                .map(|_| Type::Schema(schema_name.name.clone()))
                // Undeclared schema is already reported by ailang-semantic
                // (diagnostic S011); the type checker degrades to Unknown
                // rather than duplicating that error.
                .unwrap_or(Type::Unknown),
        }
    }

    fn check_binary(&mut self, op: BinaryOp, lt: &Type, rt: &Type, span: Span) -> Type {
        use BinaryOp::*;
        match op {
            Add | Sub | Mul | Div | Mod => {
                if lt == &Type::String && rt == &Type::String && op == Add {
                    return Type::String;
                }
                if lt.is_numeric() && rt.is_numeric() {
                    if lt == &Type::Float || rt == &Type::Float {
                        return Type::Float;
                    }
                    return Type::Int;
                }
                if lt == &Type::Unknown || rt == &Type::Unknown {
                    return Type::Unknown;
                }
                self.error(
                    "T013",
                    format!("cannot apply operator to `{lt}` and `{rt}`"),
                    span,
                );
                Type::Unknown
            }
            Eq | NotEq => Type::Bool,
            Gt | Lt | GtEq | LtEq => {
                if (lt.is_numeric() && rt.is_numeric())
                    || lt == &Type::Unknown
                    || rt == &Type::Unknown
                {
                    Type::Bool
                } else {
                    self.error("T014", format!("cannot compare `{lt}` and `{rt}`"), span);
                    Type::Bool
                }
            }
            And | Or => {
                if !lt.compatible(&Type::Bool) || !rt.compatible(&Type::Bool) {
                    self.error("T015", "logical operators require `bool` operands", span);
                }
                Type::Bool
            }
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailang_parser::parse;

    fn check(src: &str) -> DiagnosticBag {
        let (program, parse_diags) = parse(src);
        assert!(!parse_diags.has_errors(), "{:?}", parse_diags.diagnostics);
        TypeChecker::new().check_program(&program.unwrap())
    }

    #[test]
    fn accepts_well_typed_function() {
        let diags = check("fn add(a: int, b: int) -> int { return a + b; }");
        assert!(!diags.has_errors());
    }

    #[test]
    fn rejects_bad_return_type() {
        let diags = check(r#"fn f() -> int { return "hi"; }"#);
        assert!(diags.has_errors());
    }

    #[test]
    fn rejects_non_bool_if_condition() {
        let diags = check("if 5 { print(1); }");
        assert!(diags.has_errors());
    }

    #[test]
    fn rejects_wrong_arg_count() {
        let diags = check("fn add(a: int, b: int) -> int { return a + b; } let x = add(1);");
        assert!(diags.has_errors());
    }

    #[test]
    fn infers_let_from_initializer() {
        let diags = check("let x = 5; let y: int = x;");
        assert!(!diags.has_errors());
    }

    // -----------------------------------------------------------------
    // v0.5 AI-native construct type checking
    // -----------------------------------------------------------------

    #[test]
    fn type_checks_full_agent_program_without_errors() {
        let diags = check(
            "model GPT5; tool Search; permission internet; \
             agent Researcher { model GPT5; tools [Search]; permissions [internet]; }",
        );
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn schema_and_generate_type_check_together() {
        let diags = check("schema Person { name: string; age: int; } let p = generate Person;");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn indexing_a_generated_schema_value_type_checks() {
        // Regression test: `generate` is statically `Schema(name)`, but is
        // a `Value::Map` at runtime, so field access must type-check.
        let diags =
            check("schema Person { name: string; } let p = generate Person; println(p[\"name\"]);");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn generate_on_undeclared_schema_degrades_to_unknown_without_double_reporting() {
        // ailang-semantic is responsible for the "undefined schema" error;
        // the type checker should not also flag it as a type mismatch.
        let diags = check("let p = generate Ghost;");
        assert!(!diags.has_errors());
    }

    #[test]
    fn prompt_template_must_be_a_string() {
        let diags = check("prompt p = 42;");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_valid_prompt_template() {
        let diags = check(r#"prompt p = "hello";"#);
        assert!(!diags.has_errors());
    }

    #[test]
    fn knowledge_source_must_be_a_string() {
        let diags = check("knowledge Docs { source 42; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn accepts_valid_knowledge_source() {
        let diags = check(r#"knowledge Docs { source "./docs"; }"#);
        assert!(!diags.has_errors());
    }

    #[test]
    fn workflow_and_team_and_placeholders_type_check_cleanly() {
        let diags = check(
            "agent A {} agent B {} \
             workflow W { A -> B; } \
             team T { A; B; } \
             reasoning R; embedding E; dataset D; vector V; pipeline P; task Tk; deploy Dep;",
        );
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn parallel_block_type_checks_its_statements() {
        // A type error inside a `parallel` block should still be caught.
        let diags = check(r#"parallel { let x: int = "not an int"; }"#);
        assert!(diags.has_errors());
    }

    // -----------------------------------------------------------------
    // v2.0 distributed-intelligence construct type checking
    // -----------------------------------------------------------------

    #[test]
    fn network_and_cluster_type_check_cleanly() {
        let diags = check(
            "agent A {} network N { agent A; } cluster C { replicas 10; autoscaling enabled; }",
        );
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn cluster_replicas_must_be_an_int() {
        let diags = check(r#"cluster C { replicas "many"; }"#);
        assert!(diags.has_errors());
    }

    #[test]
    fn on_event_body_type_checks() {
        let diags = check(r#"on task_completed { let x: int = "wrong"; }"#);
        assert!(diags.has_errors());
    }

    #[test]
    fn on_event_handler_can_reference_the_implicit_payload_variable() {
        let diags = check(r#"on document_uploaded { println(payload); }"#);
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }

    #[test]
    fn agent_with_multi_model_list_type_checks_cleanly() {
        let diags = check("model GPT5; model Claude; agent A { models [GPT5, Claude]; }");
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
    }
}
