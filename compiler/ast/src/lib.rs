//! # ailang-ast
//!
//! Abstract Syntax Tree definitions for AILang.
//!
//! ## Design notes for future AI-native extension (v0.5+)
//!
//! Every top-level enum in this crate (`Stmt`, `Expr`, `TypeExpr`) is marked
//! `#[non_exhaustive]` from the *outside* perspective: consumers in this
//! workspace match exhaustively today, but the enums are grouped so that new
//! variants (`Agent`, `Workflow`, `Tool`, `Memory`, `Model`, `Knowledge`,
//! `Prompt`, `Reasoning`, `Embedding`, `Dataset`, `Schema`, `Pipeline`,
//! `Parallel`, `Permission`, `Team`, `Deploy`) can be appended to `Stmt`
//! without touching existing variants or breaking pattern matches elsewhere
//! (each phase's `match` already ends in patterns organized by category, see
//! the `// EXTENSION POINT` comments below).
//!
//! The AST is immutable once built (no interior mutability) so it can be
//! safely shared across interpreter and future compiler backends.

use ailang_diagnostics::Span;
use serde::{Deserialize, Serialize};

/// A full parsed source file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub statements: Vec<Stmt>,
    pub span: Span,
}

/// Identifier with its source span (kept separate from raw `String` so
/// renaming / go-to-definition in the future LSP layer has a location to
/// anchor to).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl Ident {
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }
}

/// Primitive/declared type annotations, e.g. `int`, `string`, `[int]`,
/// `{string: int}`, `fn(int, int) -> int`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeExpr {
    Int,
    Float,
    Bool,
    String,
    Char,
    Null,
    Void,
    Array(Box<TypeExpr>),
    Map(Box<TypeExpr>, Box<TypeExpr>),
    Function {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
    /// A user/unknown named type. In v0.1 this is only reachable for
    /// forward-declared or as-yet-unchecked names; the type checker treats
    /// it as `Unknown` and skips strict checking, rather than erroring, so
    /// programs using future constructs still parse.
    Named(String),
    // EXTENSION POINT (v0.5+): Agent, Model, Tool, Memory, Schema types.
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: Ident,
    pub type_annotation: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Char(char),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Gt,
    Lt,
    GtEq,
    LtEq,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Literal {
        value: Literal,
        span: Span,
    },
    Identifier {
        name: String,
        span: Span,
    },
    ArrayLiteral {
        elements: Vec<Expr>,
        span: Span,
    },
    MapLiteral {
        entries: Vec<(Expr, Expr)>,
        span: Span,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Assign {
        target: Box<Expr>,
        op: AssignOp,
        value: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// `a.b` — currently only used for module-qualified access
    /// (`std.io.print`) and reserved for future struct/agent field access.
    Member {
        object: Box<Expr>,
        property: Ident,
        span: Span,
    },
    Grouping {
        expr: Box<Expr>,
        span: Span,
    },
    /// `generate SchemaName` — requests a (v0.5 foundation: deterministic
    /// placeholder-filled) value conforming to a declared `schema`. See
    /// `ailang-interpreter`'s module doc for exactly what "generate"
    /// produces today and how a real model-backed implementation slots in
    /// later without an AST change.
    Generate {
        schema_name: Ident,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal { span, .. }
            | Expr::Identifier { span, .. }
            | Expr::ArrayLiteral { span, .. }
            | Expr::MapLiteral { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Assign { span, .. }
            | Expr::Call { span, .. }
            | Expr::Index { span, .. }
            | Expr::Member { span, .. }
            | Expr::Grouping { span, .. }
            | Expr::Generate { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    Expression {
        expr: Expr,
        span: Span,
    },
    VarDecl {
        name: Ident,
        is_const: bool,
        type_annotation: Option<TypeExpr>,
        initializer: Option<Expr>,
        span: Span,
    },
    Function {
        name: Ident,
        params: Vec<Param>,
        return_type: Option<TypeExpr>,
        body: Block,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    If {
        condition: Expr,
        then_branch: Block,
        else_branch: Option<Box<ElseBranch>>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Block,
        span: Span,
    },
    For {
        variable: Ident,
        start: Expr,
        end: Expr,
        inclusive: bool,
        body: Block,
        span: Span,
    },
    Block(Block),
    Import {
        path: Vec<Ident>,
        span: Span,
    },
    Module {
        name: Ident,
        body: Vec<Stmt>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },

    // ---------------------------------------------------------------
    // v0.5 AI-native constructs. Each is its own variant (not a shared
    // "AiDecl" blob) specifically so semantic analysis, the type checker,
    // the interpreter, and the formatter can each pattern-match precisely
    // on what they need — the same "no wildcard `_ =>` arms" discipline
    // documented in CONTRIBUTING.md is what makes this addition safe: the
    // compiler itself listed every place that needed a new arm.
    // ---------------------------------------------------------------
    /// `agent Name { model M; ... }` or `agent Name on location { ... }`.
    /// `location` is v1.0: a free-form identifier (`cloud`, `edge`,
    /// `local`, or any name the program wants) recording *where an agent
    /// is declared to run*. See `ailang-interpreter`'s module doc for what
    /// v1.0 actually does with it (real data, simulated execution — true
    /// multi-machine distribution is future work, not faked here).
    Agent {
        name: Ident,
        location: Option<Ident>,
        config: AgentConfig,
        span: Span,
    },
    /// `model Name;` or `model Name { endpoint "http://..."; key "value"; }`
    Model {
        name: Ident,
        endpoint: Option<Expr>,
        config: Vec<(Ident, Expr)>,
        span: Span,
    },
    /// `tool Name;` — declares a tool name available to `invoke_tool`.
    /// Resolution to an implementation (built-in or a same-named AILang
    /// function acting as a custom tool) happens at call time; see
    /// `ailang-interpreter`'s `invoke_tool` builtin.
    Tool {
        name: Ident,
        span: Span,
    },
    /// `permission internet;` — declares a capability name that `agent`
    /// blocks may list in their `permissions [...]` field.
    Permission {
        name: Ident,
        span: Span,
    },
    /// `memory Name persistent;` / `memory Name temporary;` — a top-level
    /// named memory store, referenced by name from an `agent` block's
    /// `memory` field.
    Memory {
        name: Ident,
        kind: MemoryKind,
        span: Span,
    },
    /// `prompt Name = "...";` / `prompt Name = """...""";`
    Prompt {
        name: Ident,
        template: Expr,
        span: Span,
    },
    /// `schema Name { field: Type; field: Type; }`
    Schema {
        name: Ident,
        fields: Vec<(Ident, TypeExpr)>,
        span: Span,
    },
    /// `workflow Name { Stage -> Stage -> Stage; }`
    Workflow {
        name: Ident,
        stages: Vec<Ident>,
        span: Span,
    },
    /// `parallel { stmt; stmt; }` — statements are independent tasks. See
    /// `ailang-interpreter`'s module doc for v0.5's sequential-execution
    /// foundation (real OS-thread concurrency is tracked in the roadmap).
    Parallel {
        body: Block,
        span: Span,
    },
    /// `team Name { Member; Member; }`
    Team {
        name: Ident,
        members: Vec<Ident>,
        span: Span,
    },
    /// `knowledge Name { source "./docs"; }`
    Knowledge {
        name: Ident,
        source: Expr,
        span: Span,
    },
    /// Generic declaration for the remaining v0.5 reserved keywords whose
    /// runtime behavior is an explicitly documented placeholder:
    /// `reasoning`, `embedding`, `dataset`, `vector`, `pipeline`, `task`,
    /// `deploy`. Grammar: `keyword Name;` or `keyword Name { key value; ... }`.
    /// Kept as one variant (rather than seven near-identical ones) because,
    /// unlike the constructs above, none of these has runtime-visible
    /// behavior yet for the type checker or interpreter to distinguish —
    /// promoting any one of them to a fully-modeled construct (its own
    /// variant, its own semantic rules) is additive whenever it's ready.
    AiPlaceholder {
        keyword: AiPlaceholderKind,
        name: Ident,
        config: Vec<(Ident, Expr)>,
        span: Span,
    },

    // ---------------------------------------------------------------
    // v2.0 distributed-intelligence constructs. See
    // `ailang-interpreter`'s module doc for what's real (in-process
    // simulation of the described behavior) vs. explicitly out of scope
    // (true multi-machine networking, real Kubernetes/cloud provisioning)
    // for this release — the same "no wildcard `_ =>` arms" discipline
    // used since v0.5 is what makes it safe to add these here.
    // ---------------------------------------------------------------
    /// `network Name { agent A; agent B; }` — a named, introspectable
    /// grouping of already-declared agents. Membership is mutable at
    /// runtime (`network_join`/`network_leave`), unlike `team`'s
    /// fixed-at-declaration member list.
    Network {
        name: Ident,
        members: Vec<Ident>,
        span: Span,
    },
    /// `cluster Name { replicas 100; autoscaling enabled; }`
    Cluster {
        name: Ident,
        replicas: Option<Expr>,
        autoscaling: Option<bool>,
        span: Span,
    },
    /// `on document_uploaded { ... }` — registers `body` as a handler for
    /// the named event; `emit(name, payload)` (a stdlib builtin, not its
    /// own AST node — see `ailang-interpreter`) runs every handler
    /// registered for that name, with `payload` bound in the handler's
    /// scope.
    OnEvent {
        event_name: Ident,
        body: Block,
        span: Span,
    },

    // ---------------------------------------------------------------
    // v3.0 AI Operating System constructs.
    // These model agents as OS processes, goals as first-class objects,
    // supervisors as fault-tolerance machinery, resources as managed
    // allocations, and applications as isolated sandboxes.
    // ---------------------------------------------------------------

    /// `goal Name "objective" { constraint ...; body }`
    /// Declares a goal-oriented task with measurable constraints.
    Goal {
        name: Ident,
        objective: String,
        constraints: Vec<GoalConstraint>,
        body: Block,
        span: Span,
    },

    /// `supervisor Name strategy OneForOne { children [A, B, C]; }`
    /// Erlang-style fault-tolerance tree.
    Supervisor {
        name: Ident,
        strategy: SupervisionStrategy,
        children: Vec<Ident>,
        span: Span,
    },

    /// `resource Name intelligence { token_budget 100000; cost_budget 5.0; }`
    /// Declares a managed AI resource (tokens, cost, energy, compute).
    Resource {
        name: Ident,
        kind: ResourceKind,
        config: Vec<(String, Expr)>,
        span: Span,
    },

    /// `reason { max_iterations 10; budget "1.0"; body }`
    /// A Reason-Verify-Revise loop — the agent's main cognitive loop.
    Reason {
        max_iterations: Option<Expr>,
        budget: Option<Expr>,
        body: Block,
        span: Span,
    },

    /// `device CamA camera { permissions ["read"]; }`
    /// Declares a hardware device accessible to agents.
    Device {
        name: Ident,
        kind: DeviceKind,
        permissions: Vec<String>,
        span: Span,
    },

    /// `ui Dashboard { title "My App"; panel PanelA; }`
    /// Declares a user-interface surface.
    Ui {
        name: Ident,
        title: Option<String>,
        panels: Vec<UiPanel>,
        span: Span,
    },

    /// `application MyApp { agents [A, B]; policies [p]; }`
    /// Top-level application manifest — the unit of deployment.
    Application {
        name: Ident,
        agents: Vec<Ident>,
        models: Vec<Ident>,
        knowledge: Vec<Ident>,
        memory: Vec<Ident>,
        workflows: Vec<Ident>,
        policies: Vec<PolicyRule>,
        span: Span,
    },

    /// `policy MyPolicy { allow agent_name internet; deny * filesystem.write; }`
    Policy {
        name: Ident,
        rules: Vec<PolicyRule>,
        span: Span,
    },

    /// `emit "event_name" payload_expr;`
    Emit {
        event: String,
        payload: Option<Expr>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElseBranch {
    Else(Block),
    ElseIf(Box<Stmt>), // always an `If`
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Expression { span, .. }
            | Stmt::VarDecl { span, .. }
            | Stmt::Function { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Import { span, .. }
            | Stmt::Module { span, .. }
            | Stmt::Break { span }
            | Stmt::Continue { span }
            | Stmt::Agent { span, .. }
            | Stmt::Model { span, .. }
            | Stmt::Tool { span, .. }
            | Stmt::Permission { span, .. }
            | Stmt::Memory { span, .. }
            | Stmt::Prompt { span, .. }
            | Stmt::Schema { span, .. }
            | Stmt::Workflow { span, .. }
            | Stmt::Parallel { span, .. }
            | Stmt::Team { span, .. }
            | Stmt::Knowledge { span, .. }
            | Stmt::AiPlaceholder { span, .. }
            | Stmt::Network { span, .. }
            | Stmt::Cluster { span, .. }
            | Stmt::OnEvent { span, .. }
            // v3.0 constructs
            | Stmt::Goal { span, .. }
            | Stmt::Supervisor { span, .. }
            | Stmt::Resource { span, .. }
            | Stmt::Reason { span, .. }
            | Stmt::Device { span, .. }
            | Stmt::Ui { span, .. }
            | Stmt::Application { span, .. }
            | Stmt::Policy { span, .. }
            | Stmt::Emit { span, .. } => *span,
            Stmt::Block(b) => b.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub statements: Vec<Stmt>,
    pub span: Span,
}

/// Configuration payload for an `agent { ... }` block. All fields are
/// optional and order-independent in source; `None`/empty means the field
/// wasn't specified.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Name of a previously (or later — declarations are hoisted, see
    /// `ailang-semantic`) declared `model`.
    pub model: Option<Ident>,
    pub memory: Option<MemoryRef>,
    /// Names of previously/later-declared `tool`s this agent may invoke
    /// via `invoke_tool`.
    pub tools: Vec<Ident>,
    /// Names of previously/later-declared `permission`s this agent is
    /// granted. `invoke_tool` checks a tool's required capability against
    /// this list at call time.
    pub permissions: Vec<Ident>,
    /// v2.0: `models [GPT5, Claude, Gemini];` — an agent may list several
    /// models instead of (or in addition to) a single `model` field; the
    /// runtime round-robins across them. See `ailang-interpreter`'s
    /// module doc for the routing algorithm (simple round-robin, not the
    /// capability/latency/cost-aware routing a real multi-provider
    /// platform would need — that requires real provider telemetry this
    /// offline-by-default foundation doesn't have).
    pub models: Vec<Ident>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MemoryRef {
    Persistent,
    Temporary,
    /// A reference to a named top-level `memory Name persistent|temporary;`
    /// declaration.
    Named(Ident),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryKind {
    Persistent,
    Temporary,
}

/// The reserved v0.5 keywords whose runtime behavior is an explicitly
/// documented placeholder today (see `Stmt::AiPlaceholder`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiPlaceholderKind {
    Reasoning,
    Embedding,
    Dataset,
    Vector,
    Pipeline,
    Task,
    Deploy,
}

impl AiPlaceholderKind {
    pub fn keyword(self) -> &'static str {
        match self {
            AiPlaceholderKind::Reasoning => "reasoning",
            AiPlaceholderKind::Embedding => "embedding",
            AiPlaceholderKind::Dataset => "dataset",
            AiPlaceholderKind::Vector => "vector",
            AiPlaceholderKind::Pipeline => "pipeline",
            AiPlaceholderKind::Task => "task",
            AiPlaceholderKind::Deploy => "deploy",
        }
    }
}

/// Keywords reserved for constructs beyond v0.5. As of v3.0, all v0.5
/// keywords plus the new v3.0 AI OS keywords are real tokens with grammar.
/// This list exists so a *future* release can reserve new AI-native
/// keywords the same way v0.1 reserved the ones that became v0.5.
pub const FUTURE_RESERVED_KEYWORDS: &[&str] = &[];

// ---------------------------------------------------------------
// v3.0 supporting types for the new AST nodes
// ---------------------------------------------------------------

/// A goal constraint — bounds on accuracy, cost, latency, deadline, energy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalConstraint {
    pub kind: ConstraintKind,
    pub op: ConstraintOp,
    pub value: ConstraintValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstraintKind {
    Accuracy,
    Budget,
    Deadline,
    Latency,
    Energy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstraintOp {
    Gt,
    Lt,
    Eq,
    GtEq,
    LtEq,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConstraintValue {
    Float(f64),
    Int(i64),
    String(String),
}

/// Supervision strategy — mirrors Erlang/OTP supervisor behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupervisionStrategy {
    /// Restart only the failed child.
    OneForOne,
    /// Restart all children when any one fails.
    OneForAll,
    /// Restart the failed child and every child declared after it.
    RestForOne,
}

/// Resource kind for `resource` declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceKind {
    Intelligence,
    Memory,
    Compute,
    Model,
    Knowledge,
}

/// Device kind for `device` declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    Camera,
    Microphone,
    Speaker,
    Sensor,
    Gpu,
}

/// A panel inside a `ui` block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiPanel {
    pub name: Ident,
    pub kind: String,
    pub config: Vec<(String, String)>,
}

/// A single access-control rule inside `application` or `policy` blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Agent name or `"*"` for all agents.
    pub subject: String,
    pub action: PolicyAction,
    /// Resource path e.g. `"internet"`, `"filesystem.write"`, `"*"`.
    pub resource: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyAction {
    Allow,
    Deny,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn future_reserved_keywords_is_empty_after_v05_promotion() {
        // v0.1's 16 reserved names all became real keywords in v0.5 (see
        // ailang_lexer::Token and the Stmt variants above). This list is
        // now the *next* release's reservation slot.
        assert!(FUTURE_RESERVED_KEYWORDS.is_empty());
    }
}
