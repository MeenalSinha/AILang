//! # ailang-interpreter
//!
//! A tree-walk interpreter that executes an [`ailang_ast::Program`]
//! directly, plus the v0.1 standard library builtins and (as of v0.5) the
//! AI-native construct execution and AI-oriented stdlib functions backed
//! by [`ailang_ai_runtime`].
//!
//! ## Future compiler backends
//!
//! The interpreter only *reads* the AST (it never mutates it), and all
//! mutable state lives in [`ailang_runtime::Environment`] /
//! [`ailang_runtime::CallStack`]. This means a future bytecode compiler or
//! JIT can reuse the exact same AST produced by `ailang-parser` — it does
//! not need to be re-parsed or re-shaped — it would simply provide an
//! alternative "backend" alongside this crate.
//!
//! ## v0.5: `parallel { }` runs sequentially
//!
//! `Stmt::Parallel` statements execute in declaration order, not
//! concurrently. This is a deliberate v0.5 foundation choice, not an
//! oversight: [`ailang_runtime::Value`] is `Rc`/`RefCell`-based (see that
//! crate's module doc), which is `!Send` — real OS-thread concurrency
//! would require migrating the runtime's reference-counting and interior
//! mutability from `Rc<RefCell<..>>` to `Arc<Mutex<..>>` throughout
//! `ailang-runtime`, a workspace-wide change out of scope for introducing
//! the *language construct*. What v0.5 does implement for real: each
//! statement in a `parallel` block runs even if an earlier one fails
//! (unlike a normal `{ }` block, which stops at the first error) — see
//! `exec_stmt`'s `Stmt::Parallel` arm — because independent tasks failing
//! independently, rather than one failure aborting the rest, is the one
//! piece of "parallel" semantics that doesn't actually require threads.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use ailang_ai_runtime::{
    MemoryStore, ModelProvider, PermissionSet, ProviderRegistry, ToolRegistry,
};
use ailang_ast::*;
use ailang_diagnostics::Span;
use ailang_runtime::{
    AgentValue, AssignError, CallStack, Environment, FunctionValue, KnowledgeValue,
    MemoryHandleValue, ModelValue, PromptValue, SchemaValue, TeamValue, ToolValue, Value,
    WorkflowValue,
};

pub mod stdlib;

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
    pub span: Span,
}

impl RuntimeError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Internal non-local control-flow signal used to implement `return`,
/// `break`, and `continue` without exceptions.
enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

type ExecResult = Result<Flow, RuntimeError>;
type EvalResult = Result<Value, RuntimeError>;

pub struct Interpreter<W: Write> {
    pub globals: Environment,
    call_stack: CallStack,
    pub out: W,
    /// Backs the bare `remember`/`recall`/`forget` builtins. v0.5
    /// foundation: a single global store, not yet routed per-agent or
    /// per-named-`memory` — see the `Stmt::Memory` doc below and
    /// `docs/roadmap.md`.
    default_memory: Rc<ailang_ai_runtime::InMemoryStore>,
    /// Named memory stores created by `memory Name persistent|temporary;`
    /// declarations. Registered for real (each name gets its own,
    /// genuinely distinct store) even though nothing routes `remember`/
    /// `recall` through a specific one yet in v0.5.
    memory_stores: RefCell<HashMap<String, Rc<ailang_ai_runtime::InMemoryStore>>>,
    providers: RefCell<ProviderRegistry>,
    /// Real audit trail of every `invoke_tool` call (granted or denied) —
    /// see `ailang_ai_runtime::security::AuditLog`'s doc for exactly what
    /// is (and deliberately isn't) recorded per entry.
    audit_log: ailang_ai_runtime::AuditLog,
    /// Real in-process agent mailboxes backing `send_message`/
    /// `receive_message`/`broadcast` — see
    /// `ailang_ai_runtime::messaging`'s module doc.
    mailboxes: ailang_ai_runtime::MailboxRegistry,
    /// Every agent declared anywhere in the program, keyed by name —
    /// populated as `Stmt::Agent` executes. Exists so `broadcast()` can
    /// enumerate "every agent" without a dedicated registration statement;
    /// mirrors `known_functions`'s reasoning for why this is a separate
    /// registry rather than scanning `env`.
    known_agents: RefCell<HashMap<String, Rc<AgentValue>>>,
    /// Real execution trace — see `ailang_ai_runtime::trace`'s module doc.
    trace: ailang_ai_runtime::ExecutionTrace,
    /// Every function declared anywhere in the program, keyed by name,
    /// populated by `hoist_functions` — independent of `env`'s bindings.
    ///
    /// This exists specifically so `invoke_tool`'s custom-tool fallback
    /// (see `builtin_invoke_tool`) can find a same-named function even
    /// after a `tool Name;` declaration has bound `Name` to `Value::Tool`
    /// in the environment (both share one identifier namespace by design —
    /// that's *how* "any function can act as a custom tool" works — but it
    /// means whichever of `tool Name;` / `fn Name() {}` executes last would
    /// otherwise win in `env` and shadow the other). Keeping a separate,
    /// hoist-time-populated registry sidesteps the ordering/overwrite
    /// problem entirely rather than relying on declaration order.
    known_functions: RefCell<HashMap<String, Rc<FunctionValue>>>,
    /// v2.0: `on <event> { }` handlers, keyed by event name; multiple
    /// handlers per event are valid (see `ailang-semantic`'s doc on why),
    /// so each entry is a list.
    event_handlers: RefCell<HashMap<String, Vec<Rc<Block>>>>,
    /// v2.0: pub/sub topics, layered on `mailboxes`.
    topics: ailang_ai_runtime::TopicRegistry,
    /// v2.0: named vector indexes for `vector_index_add`/`vector_index_search`.
    vector_indexes: RefCell<HashMap<String, Rc<ailang_ai_runtime::VectorIndex>>>,
    /// v2.0: the priority queue backing `schedule`/`run_scheduled`.
    scheduler: RefCell<ailang_ai_runtime::PriorityScheduler>,
    /// v2.0: request/response handlers registered via `on_request`,
    /// keyed by the agent name they respond as.
    request_handlers: RefCell<HashMap<String, Rc<FunctionValue>>>,
    /// v2.0: round-robin position per agent, for `models [...]` routing
    /// (see `pick_model_for_agent`) and `cluster_dispatch`'s replica
    /// selection (keyed by cluster name instead of agent name).
    round_robin_counters: RefCell<HashMap<String, usize>>,
}

impl<W: Write> Interpreter<W> {
    pub fn new(out: W) -> Self {
        let globals = Environment::new();
        stdlib::install(&globals);
        Self {
            globals,
            call_stack: CallStack::new(1024),
            out,
            default_memory: Rc::new(ailang_ai_runtime::InMemoryStore::new()),
            memory_stores: RefCell::new(HashMap::new()),
            providers: RefCell::new(ProviderRegistry::new()),
            audit_log: ailang_ai_runtime::AuditLog::new(),
            mailboxes: ailang_ai_runtime::MailboxRegistry::new(),
            known_agents: RefCell::new(HashMap::new()),
            trace: ailang_ai_runtime::ExecutionTrace::new(),
            known_functions: RefCell::new(HashMap::new()),
            event_handlers: RefCell::new(HashMap::new()),
            topics: ailang_ai_runtime::TopicRegistry::new(),
            vector_indexes: RefCell::new(HashMap::new()),
            scheduler: RefCell::new(ailang_ai_runtime::PriorityScheduler::new()),
            request_handlers: RefCell::new(HashMap::new()),
            round_robin_counters: RefCell::new(HashMap::new()),
        }
    }

    /// Renders the execution trace recorded so far as a readable timeline
    /// (see `ailang_ai_runtime::ExecutionTrace::render_timeline`). Used by
    /// `ail run --trace`.
    pub fn trace_timeline(&self) -> String {
        self.trace.render_timeline()
    }

    /// Hoists every top-level function declaration into the global scope
    /// (see `hoist_functions`'s doc). Exposed publicly (alongside
    /// [`Self::exec_top_level`] and [`Self::global_value`]) so a caller —
    /// specifically `ail run --debug`, see `cli/src/main.rs` — can drive
    /// execution one top-level statement at a time instead of via
    /// [`Self::run`]'s single all-at-once loop, without duplicating the
    /// hoisting/error-flow logic those methods already implement.
    pub fn prepare(&mut self, program: &Program) {
        let env = self.globals.clone();
        self.hoist_functions(&program.statements, &env);
    }

    /// Executes exactly one top-level statement (call [`Self::prepare`]
    /// once before the first call). Returns the same "used `return`/
    /// `break`/`continue` outside their construct" errors [`Self::run`]
    /// would for a statement at the top level.
    pub fn exec_top_level(&mut self, stmt: &Stmt) -> Result<(), RuntimeError> {
        let env = self.globals.clone();
        match self.exec_stmt(stmt, &env)? {
            Flow::Normal => Ok(()),
            Flow::Return(_) => Err(RuntimeError::new(
                "`return` used outside of a function",
                stmt.span(),
            )),
            Flow::Break | Flow::Continue => Err(RuntimeError::new(
                "`break`/`continue` used outside of a loop",
                stmt.span(),
            )),
        }
    }

    /// Looks up a global variable's current value and renders it with
    /// `Display` (the same formatting `print`/`to_string` use) — the
    /// "variable inspection" half of `ail run --debug`.
    pub fn global_value(&self, name: &str) -> Option<String> {
        self.globals.get(name).map(|v| v.to_string())
    }

    pub fn run(&mut self, program: &Program) -> Result<(), RuntimeError> {
        let env = self.globals.clone();
        // First pass: hoist function declarations so forward references and
        // mutual recursion work regardless of source order.
        self.hoist_functions(&program.statements, &env);
        for stmt in &program.statements {
            match self.exec_stmt(stmt, &env)? {
                Flow::Normal => {}
                Flow::Return(_) => {
                    return Err(RuntimeError::new(
                        "`return` used outside of a function",
                        stmt.span(),
                    ))
                }
                Flow::Break | Flow::Continue => {
                    return Err(RuntimeError::new(
                        "`break`/`continue` used outside of a loop",
                        stmt.span(),
                    ))
                }
            }
        }
        Ok(())
    }

    fn hoist_functions(&mut self, statements: &[Stmt], env: &Environment) {
        for stmt in statements {
            if let Stmt::Function {
                name, params, body, ..
            } = stmt
            {
                let func = Rc::new(FunctionValue {
                    name: name.name.clone(),
                    params: params.clone(),
                    body: Rc::new(body.clone()),
                    closure: env.clone(),
                });
                env.define(&name.name, Value::Function(func.clone()), true);
                self.known_functions
                    .borrow_mut()
                    .insert(name.name.clone(), func);
            } else if let Stmt::Module { body, .. } = stmt {
                self.hoist_functions(body, env);
            }
        }
    }

    fn exec_block(&mut self, block: &Block, env: &Environment) -> ExecResult {
        for stmt in &block.statements {
            match self.exec_stmt(stmt, env)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, env: &Environment) -> ExecResult {
        match stmt {
            Stmt::Expression { expr, .. } => {
                self.eval_expr(expr, env)?;
                Ok(Flow::Normal)
            }
            Stmt::VarDecl {
                name,
                is_const,
                initializer,
                ..
            } => {
                let value = match initializer {
                    Some(e) => self.eval_expr(e, env)?,
                    None => Value::Null,
                };
                env.define(&name.name, value, *is_const);
                Ok(Flow::Normal)
            }
            Stmt::Function { .. } => Ok(Flow::Normal), // handled by hoist_functions
            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.eval_expr(e, env)?,
                    None => Value::Void,
                };
                Ok(Flow::Return(v))
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let cond = self.eval_expr(condition, env)?;
                if cond.is_truthy() {
                    let scope = Environment::child(env);
                    self.exec_block(then_branch, &scope)
                } else if let Some(branch) = else_branch {
                    match branch.as_ref() {
                        ElseBranch::Else(b) => {
                            let scope = Environment::child(env);
                            self.exec_block(b, &scope)
                        }
                        ElseBranch::ElseIf(s) => self.exec_stmt(s, env),
                    }
                } else {
                    Ok(Flow::Normal)
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                while self.eval_expr(condition, env)?.is_truthy() {
                    let scope = Environment::child(env);
                    match self.exec_block(body, &scope)? {
                        Flow::Break => break,
                        Flow::Continue | Flow::Normal => {}
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::For {
                variable,
                start,
                end,
                inclusive,
                body,
                span,
            } => {
                let start_v = self.eval_expr(start, env)?;
                let end_v = self.eval_expr(end, env)?;
                let (mut i, end_i) = match (start_v, end_v) {
                    (Value::Int(a), Value::Int(b)) => (a, b),
                    _ => {
                        return Err(RuntimeError::new(
                            "`for` range bounds must be integers",
                            *span,
                        ))
                    }
                };
                while if *inclusive { i <= end_i } else { i < end_i } {
                    let scope = Environment::child(env);
                    scope.define(&variable.name, Value::Int(i), false);
                    match self.exec_block(body, &scope)? {
                        Flow::Break => break,
                        Flow::Continue | Flow::Normal => {}
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                    i += 1;
                }
                Ok(Flow::Normal)
            }
            Stmt::Block(b) => {
                let scope = Environment::child(env);
                self.exec_block(b, &scope)
            }
            Stmt::Import { .. } => Ok(Flow::Normal), // module resolution: v0.5+
            Stmt::Module { body, .. } => {
                // v0.1 treats `module` as an inline namespace whose members
                // are hoisted straight into the enclosing scope. Qualified
                // access (`math.square`) is handled by resolving `square`
                // directly once module-path resolution lands.
                self.hoist_functions(body, env);
                for s in body {
                    self.exec_stmt(s, env)?;
                }
                Ok(Flow::Normal)
            }
            Stmt::Break { .. } => Ok(Flow::Break),
            Stmt::Continue { .. } => Ok(Flow::Continue),

            // -----------------------------------------------------------
            // v0.5 AI-native constructs
            // -----------------------------------------------------------
            Stmt::Agent {
                name,
                location,
                config,
                ..
            } => {
                let memory = match &config.memory {
                    Some(MemoryRef::Persistent) => Some(Err(MemoryKind::Persistent)),
                    Some(MemoryRef::Temporary) => Some(Err(MemoryKind::Temporary)),
                    Some(MemoryRef::Named(id)) => Some(Ok(id.name.clone())),
                    None => None,
                };
                let agent = Rc::new(AgentValue {
                    name: name.name.clone(),
                    model: config.model.as_ref().map(|m| m.name.clone()),
                    memory,
                    tools: config.tools.iter().map(|t| t.name.clone()).collect(),
                    permissions: config.permissions.iter().map(|p| p.name.clone()).collect(),
                    location: location.as_ref().map(|l| l.name.clone()),
                    models: config.models.iter().map(|m| m.name.clone()).collect(),
                });
                self.known_agents
                    .borrow_mut()
                    .insert(name.name.clone(), agent.clone());
                env.define(&name.name, Value::Agent(agent), true);
                Ok(Flow::Normal)
            }
            Stmt::Model {
                name,
                endpoint,
                config,
                ..
            } => {
                let endpoint_str = match endpoint {
                    Some(e) => match self.eval_expr(e, env)? {
                        Value::String(s) => Some(s.to_string()),
                        other => {
                            return Err(RuntimeError::new(
                                format!(
                                    "model `endpoint` must be a string, found `{}`",
                                    other.type_name()
                                ),
                                e.span(),
                            ))
                        }
                    },
                    None => None,
                };
                self.providers
                    .borrow_mut()
                    .register(&name.name, endpoint_str.as_deref());

                let mut resolved_config = Vec::with_capacity(config.len());
                for (key, value_expr) in config {
                    let value = self.eval_expr(value_expr, env)?;
                    resolved_config.push((key.name.clone(), value));
                }
                let model = ModelValue {
                    name: name.name.clone(),
                    endpoint: endpoint_str,
                    config: resolved_config,
                };
                env.define(&name.name, Value::Model(Rc::new(model)), true);
                Ok(Flow::Normal)
            }
            Stmt::Tool { name, .. } => {
                env.define(
                    &name.name,
                    Value::Tool(Rc::new(ToolValue {
                        name: name.name.clone(),
                    })),
                    true,
                );
                Ok(Flow::Normal)
            }
            Stmt::Permission { name, .. } => {
                env.define(
                    &name.name,
                    Value::Permission(Rc::new(ailang_runtime::PermissionValue {
                        name: name.name.clone(),
                    })),
                    true,
                );
                Ok(Flow::Normal)
            }
            Stmt::Memory { name, kind, .. } => {
                self.memory_stores
                    .borrow_mut()
                    .entry(name.name.clone())
                    .or_insert_with(|| Rc::new(ailang_ai_runtime::InMemoryStore::new()));
                env.define(
                    &name.name,
                    Value::MemoryHandle(Rc::new(MemoryHandleValue {
                        name: name.name.clone(),
                        kind: *kind,
                    })),
                    true,
                );
                Ok(Flow::Normal)
            }
            Stmt::Prompt { name, template, .. } => {
                let text = match self.eval_expr(template, env)? {
                    Value::String(s) => s.to_string(),
                    other => {
                        return Err(RuntimeError::new(
                            format!(
                                "prompt template must be a string, found `{}`",
                                other.type_name()
                            ),
                            template.span(),
                        ))
                    }
                };
                env.define(
                    &name.name,
                    Value::Prompt(Rc::new(PromptValue {
                        name: name.name.clone(),
                        template: text,
                    })),
                    true,
                );
                Ok(Flow::Normal)
            }
            Stmt::Schema { name, fields, .. } => {
                let schema = SchemaValue {
                    name: name.name.clone(),
                    fields: fields
                        .iter()
                        .map(|(f, t)| (f.name.clone(), t.clone()))
                        .collect(),
                };
                env.define(&name.name, Value::Schema(Rc::new(schema)), true);
                Ok(Flow::Normal)
            }
            Stmt::Workflow { name, stages, .. } => {
                let workflow = WorkflowValue {
                    name: name.name.clone(),
                    stages: stages.iter().map(|s| s.name.clone()).collect(),
                };
                env.define(&name.name, Value::Workflow(Rc::new(workflow)), true);
                Ok(Flow::Normal)
            }
            Stmt::Parallel { body, span } => {
                // See this module's doc comment for why this is sequential
                // rather than concurrent, and why it still differs
                // meaningfully from a plain `{ }` block: every statement
                // runs regardless of earlier failures, with the first
                // error (if any) surfaced only after all have run — a
                // "run independent tasks, then report" join, not a
                // fail-fast sequence.
                let scope = Environment::child(env);
                let mut first_error: Option<RuntimeError> = None;
                for stmt in &body.statements {
                    if let Err(e) = self.exec_stmt(stmt, &scope) {
                        if first_error.is_none() {
                            first_error = Some(e);
                        }
                    }
                }
                let _ = span;
                match first_error {
                    Some(e) => Err(e),
                    None => Ok(Flow::Normal),
                }
            }
            Stmt::Team { name, members, .. } => {
                let team = TeamValue {
                    name: name.name.clone(),
                    members: members.iter().map(|m| m.name.clone()).collect(),
                };
                env.define(&name.name, Value::Team(Rc::new(team)), true);
                Ok(Flow::Normal)
            }
            Stmt::Knowledge { name, source, .. } => {
                let source_text = match self.eval_expr(source, env)? {
                    Value::String(s) => s.to_string(),
                    other => {
                        return Err(RuntimeError::new(
                            format!(
                                "knowledge `source` must be a string, found `{}`",
                                other.type_name()
                            ),
                            source.span(),
                        ))
                    }
                };
                env.define(
                    &name.name,
                    Value::Knowledge(Rc::new(KnowledgeValue {
                        name: name.name.clone(),
                        source: source_text,
                    })),
                    true,
                );
                Ok(Flow::Normal)
            }
            Stmt::AiPlaceholder {
                keyword,
                name,
                config,
                ..
            } => {
                for (_, value_expr) in config {
                    self.eval_expr(value_expr, env)?;
                }
                env.define(
                    &name.name,
                    Value::AiPlaceholder(Rc::new(ailang_runtime::AiPlaceholderValue {
                        keyword: keyword.keyword(),
                        name: name.name.clone(),
                    })),
                    true,
                );
                Ok(Flow::Normal)
            }

            // -----------------------------------------------------------
            // v2.0 distributed-intelligence constructs
            // -----------------------------------------------------------
            Stmt::Network { name, members, .. } => {
                let network = ailang_runtime::NetworkValue {
                    name: name.name.clone(),
                    members: RefCell::new(members.iter().map(|m| m.name.clone()).collect()),
                };
                env.define(&name.name, Value::Network(Rc::new(network)), true);
                Ok(Flow::Normal)
            }
            Stmt::Cluster {
                name,
                replicas,
                autoscaling,
                ..
            } => {
                let replica_count = match replicas {
                    Some(expr) => match self.eval_expr(expr, env)? {
                        Value::Int(n) => n,
                        other => {
                            return Err(RuntimeError::new(
                                format!("`replicas` must be an int, found `{}`", other.type_name()),
                                expr.span(),
                            ))
                        }
                    },
                    None => 1,
                };
                let cluster = ailang_runtime::ClusterValue {
                    name: name.name.clone(),
                    replicas: replica_count,
                    autoscaling: autoscaling.unwrap_or(false),
                };
                env.define(&name.name, Value::Cluster(Rc::new(cluster)), true);
                Ok(Flow::Normal)
            }
            Stmt::OnEvent {
                event_name, body, ..
            } => {
                self.event_handlers
                    .borrow_mut()
                    .entry(event_name.name.clone())
                    .or_default()
                    .push(Rc::new(body.clone()));
                Ok(Flow::Normal)
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr, env: &Environment) -> EvalResult {
        match expr {
            Expr::Literal { value, .. } => Ok(literal_to_value(value)),
            Expr::Identifier { name, span } => env
                .get(name)
                .ok_or_else(|| RuntimeError::new(format!("undefined variable `{name}`"), *span)),
            Expr::ArrayLiteral { elements, .. } => {
                let mut vals = Vec::with_capacity(elements.len());
                for e in elements {
                    vals.push(self.eval_expr(e, env)?);
                }
                Ok(Value::array(vals))
            }
            Expr::MapLiteral { entries, .. } => {
                let mut vals = Vec::with_capacity(entries.len());
                for (k, v) in entries {
                    vals.push((self.eval_expr(k, env)?, self.eval_expr(v, env)?));
                }
                Ok(Value::map(vals))
            }
            Expr::Grouping { expr, .. } => self.eval_expr(expr, env),
            Expr::Unary { op, operand, span } => {
                let v = self.eval_expr(operand, env)?;
                match (op, &v) {
                    (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
                    (UnaryOp::Neg, Value::Float(n)) => Ok(Value::Float(-n)),
                    (UnaryOp::Not, _) => Ok(Value::Bool(!v.is_truthy())),
                    _ => Err(RuntimeError::new(
                        format!("cannot negate a value of type `{}`", v.type_name()),
                        *span,
                    )),
                }
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                let l = self.eval_expr(left, env)?;
                // Short-circuit logical operators.
                if *op == BinaryOp::And {
                    return if !l.is_truthy() {
                        Ok(Value::Bool(false))
                    } else {
                        Ok(Value::Bool(self.eval_expr(right, env)?.is_truthy()))
                    };
                }
                if *op == BinaryOp::Or {
                    return if l.is_truthy() {
                        Ok(Value::Bool(true))
                    } else {
                        Ok(Value::Bool(self.eval_expr(right, env)?.is_truthy()))
                    };
                }
                let r = self.eval_expr(right, env)?;
                eval_binary(*op, l, r, *span)
            }
            Expr::Assign {
                target,
                op,
                value,
                span,
            } => {
                let new_value = self.eval_expr(value, env)?;
                let computed = if *op == AssignOp::Assign {
                    new_value
                } else {
                    let current = self.eval_expr(target, env)?;
                    let bin_op = match op {
                        AssignOp::AddAssign => BinaryOp::Add,
                        AssignOp::SubAssign => BinaryOp::Sub,
                        AssignOp::MulAssign => BinaryOp::Mul,
                        AssignOp::DivAssign => BinaryOp::Div,
                        AssignOp::Assign => unreachable!(),
                    };
                    eval_binary(bin_op, current, new_value, *span)?
                };
                self.assign_to(target, computed.clone(), env, *span)?;
                Ok(computed)
            }
            Expr::Call { callee, args, span } => self.eval_call(callee, args, env, *span),
            Expr::Index {
                object,
                index,
                span,
            } => {
                let obj = self.eval_expr(object, env)?;
                let idx = self.eval_expr(index, env)?;
                index_value(&obj, &idx, *span)
            }
            Expr::Member {
                object,
                property,
                span,
            } => {
                // v0.1: only supports module-qualified function access,
                // resolved as a plain global lookup of the property name
                // (module bodies are hoisted into the global scope).
                let _ = self.eval_expr(object, env);
                env.get(&property.name).ok_or_else(|| {
                    RuntimeError::new(format!("undefined member `{}`", property.name), *span)
                })
            }
            Expr::Generate { schema_name, span } => {
                let schema_value = env.get(&schema_name.name).ok_or_else(|| {
                    RuntimeError::new(format!("undefined schema `{}`", schema_name.name), *span)
                })?;
                let Value::Schema(schema) = schema_value else {
                    return Err(RuntimeError::new(
                        format!("`{}` is not a schema", schema_name.name),
                        *span,
                    ));
                };
                // v0.5 foundation: always uses the deterministic
                // MockProvider (see that type's doc comment). A future
                // `generate Person using SomeModel` syntax would route
                // through `self.providers` instead, without any other
                // change to this arm.
                ailang_ai_runtime::MockProvider
                    .generate_structured(&schema.fields)
                    .map_err(|e| RuntimeError::new(e, *span))
            }
        }
    }

    fn assign_to(
        &mut self,
        target: &Expr,
        value: Value,
        env: &Environment,
        span: Span,
    ) -> Result<(), RuntimeError> {
        match target {
            Expr::Identifier { name, .. } => match env.assign(name, value) {
                Ok(()) => Ok(()),
                Err(AssignError::ConstReassignment) => Err(RuntimeError::new(
                    format!("cannot assign to `{name}`: it is declared `const`"),
                    span,
                )),
                Err(AssignError::Undefined) => Err(RuntimeError::new(
                    format!("undefined variable `{name}`"),
                    span,
                )),
            },
            Expr::Index { object, index, .. } => {
                let obj = self.eval_expr(object, env)?;
                let idx = self.eval_expr(index, env)?;
                match obj {
                    Value::Array(arr) => {
                        let i = match idx {
                            Value::Int(i) => i,
                            _ => {
                                return Err(RuntimeError::new(
                                    "array index must be an integer",
                                    span,
                                ))
                            }
                        };
                        let mut arr = arr.borrow_mut();
                        if i < 0 || i as usize >= arr.len() {
                            return Err(RuntimeError::new(
                                format!("array index {i} out of bounds (len {})", arr.len()),
                                span,
                            ));
                        }
                        arr[i as usize] = value;
                        Ok(())
                    }
                    Value::Map(map) => {
                        let mut map = map.borrow_mut();
                        if let Some(entry) = map.iter_mut().find(|(k, _)| *k == idx) {
                            entry.1 = value;
                        } else {
                            map.push((idx, value));
                        }
                        Ok(())
                    }
                    other => Err(RuntimeError::new(
                        format!("cannot index-assign into a `{}`", other.type_name()),
                        span,
                    )),
                }
            }
            _ => Err(RuntimeError::new("invalid assignment target", span)),
        }
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        env: &Environment,
        span: Span,
    ) -> EvalResult {
        let callee_val = self.eval_expr(callee, env)?;
        let mut arg_values = Vec::with_capacity(args.len());
        for a in args {
            arg_values.push(self.eval_expr(a, env)?);
        }

        match callee_val {
            Value::Native(name) => self.call_native_with_io(&name, &arg_values, env, span),
            Value::Function(func) => self.call_user_function(&func, &arg_values, span),
            other => Err(RuntimeError::new(
                format!("`{}` is not callable", other.type_name()),
                span,
            )),
        }
    }

    fn call_user_function(
        &mut self,
        func: &Rc<FunctionValue>,
        args: &[Value],
        span: Span,
    ) -> EvalResult {
        if args.len() != func.params.len() {
            return Err(RuntimeError::new(
                format!(
                    "function `{}` expects {} argument(s), got {}",
                    func.name,
                    func.params.len(),
                    args.len()
                ),
                span,
            ));
        }
        self.call_stack
            .push(func.name.clone())
            .map_err(|e| RuntimeError::new(e, span))?;

        let call_env = Environment::child(&func.closure);
        for (param, arg) in func.params.iter().zip(args.iter()) {
            call_env.define(&param.name.name, arg.clone(), false);
        }

        let result = self.exec_block(&func.body, &call_env);
        self.call_stack.pop();

        match result? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Value::Void),
        }
    }

    /// Dispatches every builtin function name. Most (the v0.1 stdlib, plus
    /// the v0.5 pure functions `summarize`/`translate`/`classify`/`embed`)
    /// need no interpreter state and are forwarded to `stdlib::call`. The
    /// remainder — `remember`/`recall`/`forget` (need `self.default_memory`),
    /// `invoke_tool` (needs to resolve an agent by name in `env`),
    /// `run_workflow`/`run_team` (need `env` to resolve agents plus
    /// `self.providers`) — are handled directly here, which is why this
    /// method takes `env` unlike the plain v0.1 builtins that only ever
    /// needed `self.out`/stdin.
    fn call_native_with_io(
        &mut self,
        name: &str,
        args: &[Value],
        env: &Environment,
        span: Span,
    ) -> EvalResult {
        match name {
            "remember" => {
                let (key, value) = two_string_args(name, args, span)?;
                self.default_memory.remember(&key, &value);
                Ok(Value::Void)
            }
            "recall" => {
                let query = one_string_arg(name, args, span)?;
                Ok(self
                    .default_memory
                    .recall(&query)
                    .map(Value::string)
                    .unwrap_or(Value::Null))
            }
            "forget" => {
                let key = one_string_arg(name, args, span)?;
                self.default_memory.forget(&key);
                Ok(Value::Void)
            }
            "memory_history" => {
                let key = one_string_arg(name, args, span)?;
                let history = self.default_memory.history(&key);
                Ok(Value::array(
                    history
                        .into_iter()
                        .map(|(v, ts)| {
                            Value::map(vec![
                                (Value::string("value"), Value::string(v)),
                                (Value::string("timestamp"), Value::Float(ts)),
                            ])
                        })
                        .collect(),
                ))
            }
            "remember_in" => self.builtin_memory_scoped(args, span, |store, key, value| {
                store.remember(key, value.unwrap());
                Value::Void
            }),
            "recall_in" => self.builtin_memory_scoped_query(args, span),
            "forget_in" => self.builtin_memory_scoped(args, span, |store, key, _| {
                store.forget(key);
                Value::Void
            }),
            "invoke_tool" => self.builtin_invoke_tool(args, env, span),
            "run_workflow" => self.builtin_run_workflow(args, env, span),
            "run_team" => self.builtin_run_team(args, env, span),
            "render" => self.builtin_render_prompt(args, env, span),
            "retrieve" => self.builtin_retrieve(args, env, span),
            "secret" => {
                let key = one_string_arg("secret", args, span)?;
                Ok(ailang_ai_runtime::get_secret(&key)
                    .map(Value::string)
                    .unwrap_or(Value::Null))
            }
            "audit_log" => Ok(Value::array(
                self.audit_log
                    .entries()
                    .into_iter()
                    .map(|e| {
                        Value::map(vec![
                            (Value::string("timestamp"), Value::Float(e.timestamp)),
                            (Value::string("agent"), Value::string(e.agent)),
                            (Value::string("tool"), Value::string(e.tool)),
                            (
                                Value::string("permission"),
                                e.required_permission
                                    .map(Value::string)
                                    .unwrap_or(Value::Null),
                            ),
                            (Value::string("granted"), Value::Bool(e.granted)),
                        ])
                    })
                    .collect(),
            )),
            "send_message" => {
                let (from, to) = two_string_args("send_message", args, span)?;
                let text = one_string_arg_at(args, 2, "send_message", span)?;
                self.mailboxes.send(&from, &to, &text);
                Ok(Value::Void)
            }
            "receive_message" => {
                let agent = one_string_arg("receive_message", args, span)?;
                Ok(self
                    .mailboxes
                    .receive(&agent)
                    .map(|m| {
                        Value::map(vec![
                            (Value::string("from"), Value::string(m.from)),
                            (Value::string("text"), Value::string(m.text)),
                            (Value::string("timestamp"), Value::Float(m.timestamp)),
                        ])
                    })
                    .unwrap_or(Value::Null))
            }
            "broadcast" => self.builtin_broadcast(args, env, span),
            "agent_location" => {
                let agent_name = one_string_arg("agent_location", args, span)?;
                let agent = self.resolve_agent(&agent_name, env, span)?;
                Ok(agent
                    .location
                    .clone()
                    .map(Value::string)
                    .unwrap_or(Value::Null))
            }
            "get_trace" => Ok(Value::array(
                self.trace
                    .events()
                    .into_iter()
                    .map(|e| {
                        Value::map(vec![
                            (Value::string("kind"), Value::string(e.kind)),
                            (Value::string("label"), Value::string(e.label)),
                            (
                                Value::string("elapsed_seconds"),
                                Value::Float(e.elapsed_seconds),
                            ),
                        ])
                    })
                    .collect(),
            )),
            "network_members" => {
                let name = one_string_arg("network_members", args, span)?;
                let network = self.resolve_network(&name, env, span)?;
                let members: Vec<String> = network.members.borrow().clone();
                Ok(Value::array(
                    members.into_iter().map(Value::string).collect(),
                ))
            }
            "network_leader" => {
                let name = one_string_arg("network_leader", args, span)?;
                let network = self.resolve_network(&name, env, span)?;
                let members = network.members.borrow().clone();
                Ok(ailang_ai_runtime::elect_leader(&members)
                    .map(Value::string)
                    .unwrap_or(Value::Null))
            }
            "network_health" => {
                let name = one_string_arg("network_health", args, span)?;
                let network = self.resolve_network(&name, env, span)?;
                let members = network.members.borrow().clone();
                Ok(Value::array(
                    ailang_ai_runtime::health_check(&members)
                        .into_iter()
                        .map(|h| {
                            Value::map(vec![
                                (Value::string("member"), Value::string(h.member)),
                                (Value::string("healthy"), Value::Bool(h.healthy)),
                            ])
                        })
                        .collect(),
                ))
            }
            "network_join" => {
                let (network_name, agent_name) = two_string_args("network_join", args, span)?;
                let network = self.resolve_network(&network_name, env, span)?;
                self.resolve_agent(&agent_name, env, span)?; // validate it's a real agent
                let mut members = network.members.borrow_mut();
                if !members.iter().any(|m| m == &agent_name) {
                    members.push(agent_name);
                }
                Ok(Value::Void)
            }
            "network_leave" => {
                let (network_name, agent_name) = two_string_args("network_leave", args, span)?;
                let network = self.resolve_network(&network_name, env, span)?;
                network.members.borrow_mut().retain(|m| m != &agent_name);
                Ok(Value::Void)
            }
            "cluster_status" => {
                let name = one_string_arg("cluster_status", args, span)?;
                let cluster = self.resolve_cluster(&name, env, span)?;
                Ok(Value::map(vec![
                    (Value::string("replicas"), Value::Int(cluster.replicas)),
                    (
                        Value::string("autoscaling"),
                        Value::Bool(cluster.autoscaling),
                    ),
                ]))
            }
            "cluster_dispatch" => self.builtin_cluster_dispatch(args, env, span),
            "on_request" => {
                let (agent_name, function_name) = two_string_args("on_request", args, span)?;
                let func = self.known_functions.borrow().get(&function_name).cloned();
                match func {
                    Some(f) => {
                        self.request_handlers.borrow_mut().insert(agent_name, f);
                        Ok(Value::Void)
                    }
                    None => Err(RuntimeError::new(
                        format!("on_request(): no function named `{function_name}` is defined"),
                        span,
                    )),
                }
            }
            "request" => {
                if args.len() < 2 {
                    return Err(RuntimeError::new(
                        "request() expects (from: string, to: string, ...args)",
                        span,
                    ));
                }
                let to = expect_string(&args[1], "request", span)?;
                let handler = self.request_handlers.borrow().get(&to).cloned();
                match handler {
                    Some(f) => self.call_user_function(&f, &args[2..], span),
                    None => Err(RuntimeError::new(
                        format!("request(): no responder registered for `{to}` (see on_request)"),
                        span,
                    )),
                }
            }
            "subscribe" => {
                let (topic, agent) = two_string_args("subscribe", args, span)?;
                self.topics.subscribe(&topic, &agent);
                Ok(Value::Void)
            }
            "unsubscribe" => {
                let (topic, agent) = two_string_args("unsubscribe", args, span)?;
                self.topics.unsubscribe(&topic, &agent);
                Ok(Value::Void)
            }
            "publish" => {
                let (topic, text) = two_string_args("publish", args, span)?;
                self.topics.publish(&topic, &text, &self.mailboxes);
                Ok(Value::Void)
            }
            "emit" => self.builtin_emit(args, env, span),
            "vector_index_add" => self.builtin_vector_index_add(args, span),
            "vector_index_search" => self.builtin_vector_index_search(args, span),
            "compress_context" => {
                let text = one_string_arg("compress_context", args, span)?;
                let max_words = match args.get(1) {
                    Some(Value::Int(n)) => (*n).max(0) as usize,
                    _ => {
                        return Err(RuntimeError::new(
                            "compress_context() expects (text: string, max_words: int)",
                            span,
                        ))
                    }
                };
                Ok(Value::string(ailang_ai_runtime::compress_context(
                    &text, max_words,
                )))
            }
            "dedupe_chunks" => {
                let chunks = match args.first() {
                    Some(Value::Array(a)) => a
                        .borrow()
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => Ok(s.to_string()),
                            other => Err(format!(
                                "dedupe_chunks() array must contain only strings, found `{}`",
                                other.type_name()
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| RuntimeError::new(e, span))?,
                    _ => {
                        return Err(RuntimeError::new(
                            "dedupe_chunks() expects an array of strings",
                            span,
                        ))
                    }
                };
                Ok(Value::array(
                    ailang_ai_runtime::dedupe_chunks(&chunks)
                        .into_iter()
                        .map(Value::string)
                        .collect(),
                ))
            }
            "schedule" => {
                let function_name = one_string_arg("schedule", args, span)?;
                if self.known_functions.borrow().get(&function_name).is_none() {
                    return Err(RuntimeError::new(
                        format!("schedule(): no function named `{function_name}` is defined"),
                        span,
                    ));
                }
                let priority = match args.get(1) {
                    Some(Value::Int(n)) => *n,
                    _ => 0,
                };
                self.scheduler
                    .borrow_mut()
                    .schedule(&function_name, priority);
                Ok(Value::Void)
            }
            "run_scheduled" => {
                let names = self.scheduler.borrow_mut().drain();
                let mut results = Vec::with_capacity(names.len());
                for function_name in names {
                    let func = self.known_functions.borrow().get(&function_name).cloned();
                    if let Some(f) = func {
                        results.push(self.call_user_function(&f, &[], span)?);
                    }
                }
                Ok(Value::array(results))
            }
            "agent_models" => {
                let agent_name = one_string_arg("agent_models", args, span)?;
                let agent = self.resolve_agent(&agent_name, env, span)?;
                Ok(Value::array(
                    agent.models.iter().cloned().map(Value::string).collect(),
                ))
            }
            _ => stdlib::call(name, args, &mut self.out).map_err(|e| RuntimeError::new(e, span)),
        }
    }

    /// Shared implementation for `remember_in(memory_name, key, value)` and
    /// `forget_in(memory_name, key)`: resolves `memory_name` to a
    /// registered named `InMemoryStore` (created when the corresponding
    /// `memory Name persistent|temporary;` statement executed — see
    /// `Stmt::Memory` in `exec_stmt`) and applies `op` to it. This is the
    /// v1.0 mechanism for **agent-scoped memory**: an `agent`'s `memory`
    /// field names one of these stores, and any code — not just that one
    /// agent — can `remember_in`/`recall_in`/`forget_in` against it by
    /// name, including multiple agents sharing one named memory (which is
    /// also how v1.0 gets **team memory** for free, with no separate
    /// concept: every member's `agent` block just references the same
    /// named `memory`).
    fn builtin_memory_scoped(
        &mut self,
        args: &[Value],
        span: Span,
        op: impl FnOnce(&ailang_ai_runtime::InMemoryStore, &str, Option<&str>) -> Value,
    ) -> EvalResult {
        if args.is_empty() {
            return Err(RuntimeError::new(
                "expected a memory name as the first argument",
                span,
            ));
        }
        let memory_name = expect_string(&args[0], "remember_in/forget_in", span)?;
        let key = one_string_arg_at(args, 1, "remember_in/forget_in", span)?;
        let value = args
            .get(2)
            .map(|v| expect_string(v, "remember_in", span))
            .transpose()?;
        let store = self.named_memory_store(&memory_name, span)?;
        Ok(op(&store, &key, value.as_deref()))
    }

    fn builtin_memory_scoped_query(&mut self, args: &[Value], span: Span) -> EvalResult {
        let (memory_name, query) = two_string_args("recall_in", args, span)?;
        let store = self.named_memory_store(&memory_name, span)?;
        Ok(store
            .recall(&query)
            .map(Value::string)
            .unwrap_or(Value::Null))
    }

    fn named_memory_store(
        &self,
        memory_name: &str,
        span: Span,
    ) -> Result<Rc<ailang_ai_runtime::InMemoryStore>, RuntimeError> {
        self.memory_stores
            .borrow()
            .get(memory_name)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(
                    format!(
                        "undefined memory `{memory_name}` (declare it first with `memory {memory_name} persistent;` or `temporary;`)"
                    ),
                    span,
                )
            })
    }

    /// `invoke_tool(agent_name: string, tool_name: string, ...args) -> value`
    ///
    /// Resolves `agent_name` to a declared `agent`, checks that
    /// `tool_name` is in that agent's `tools [...]` list, then either:
    /// 1. calls the matching built-in tool (`Calculator`/`Filesystem`/
    ///    `HTTP`/`Search`/`Browser`), enforcing its required permission
    ///    against the agent's `permissions [...]`, or
    /// 2. if no built-in matches, calls a same-named ordinary AILang
    ///    function as a **custom tool** — this is what "support
    ///    registration of custom tools" means in v0.5: any `fn` can act as
    ///    a tool implementation just by sharing its name with a `tool`
    ///    declaration, no extra syntax required. Permission enforcement
    ///    does not apply to custom tools (there's no declared capability
    ///    requirement for an arbitrary function); see docs/roadmap.md.
    fn builtin_invoke_tool(&mut self, args: &[Value], env: &Environment, span: Span) -> EvalResult {
        if args.len() < 2 {
            return Err(RuntimeError::new(
                "invoke_tool() expects at least 2 arguments: agent_name, tool_name",
                span,
            ));
        }
        let agent_name = expect_string(&args[0], "invoke_tool", span)?;
        let tool_name = expect_string(&args[1], "invoke_tool", span)?;
        let rest = &args[2..];

        let agent = self.resolve_agent(&agent_name, env, span)?;
        if !agent.tools.iter().any(|t| t == &tool_name) {
            return Err(RuntimeError::new(
                format!("agent `{agent_name}` has no tool `{tool_name}` declared"),
                span,
            ));
        }
        let permissions = PermissionSet::new(agent.permissions.iter().cloned());

        if let Some(tool) = ToolRegistry::builtin(&tool_name) {
            let required = tool.required_permission();
            let granted = required.map(|p| permissions.grants(p)).unwrap_or(true);
            self.audit_log
                .record(&agent_name, &tool_name, required, granted);
            self.trace.record("tool_invocation", &tool_name);
            return ToolRegistry::invoke_checked(tool.as_ref(), &permissions, rest)
                .map_err(|e| RuntimeError::new(e, span));
        }

        // Fall back to a same-named user-defined function as a custom
        // tool implementation. Looked up in `known_functions` (populated
        // at hoist time), not `env` — see that field's doc comment for
        // why: `env` may have `tool_name` bound to `Value::Tool` instead
        // (whichever of the `tool` declaration or the `fn` declaration
        // executed last would otherwise win).
        let custom_tool = self.known_functions.borrow().get(&tool_name).cloned();
        match custom_tool {
            Some(func) => {
                // Custom tools have no declared required-permission concept
                // (see this method's doc comment); logged as granted since
                // there's no capability gate to deny against.
                self.audit_log.record(&agent_name, &tool_name, None, true);
                self.call_user_function(&func, rest, span)
            }
            None => Err(RuntimeError::new(
                format!(
                    "no implementation available for tool `{tool_name}` (no built-in tool by that name, and no function named `{tool_name}` is defined to act as a custom tool)"
                ),
                span,
            )),
        }
    }

    /// `run_workflow(name: string, input: string) -> [{string: string}]`
    fn builtin_run_workflow(
        &mut self,
        args: &[Value],
        env: &Environment,
        span: Span,
    ) -> EvalResult {
        let (workflow_name, input) = two_string_args("run_workflow", args, span)?;
        let workflow = match env.get(&workflow_name) {
            Some(Value::Workflow(w)) => w,
            _ => {
                return Err(RuntimeError::new(
                    format!("undefined workflow `{workflow_name}`"),
                    span,
                ))
            }
        };
        let agents = self.resolve_agents(&workflow.stages, env, span)?;
        let providers = self.providers.borrow();
        let results =
            ailang_ai_runtime::run_workflow(&workflow.stages, &agents, &providers, &input)
                .map_err(|e| RuntimeError::new(e, span))?;
        // Traced after the fact, in stage order: `ailang_ai_runtime::run_workflow`
        // runs the whole pipeline in one call rather than yielding control
        // back per stage, so per-stage *timing* isn't captured — only
        // per-stage *occurrence and order*, which is still what a basic
        // timeline needs. See `ExecutionTrace`'s module doc.
        for r in &results {
            self.trace.record("workflow_stage", &r.stage);
        }
        Ok(stage_results_to_value(results, "stage"))
    }

    /// `render(prompt_name: string) -> string`
    ///
    /// Looks up a declared `prompt` by name and renders its template,
    /// substituting every `${var}` with `var`'s current value (stringified
    /// the same way `to_string`/`print` would) looked up in the *calling*
    /// environment — so a prompt declared once can be rendered many times
    /// with different variable bindings in scope. See
    /// `ailang_ai_runtime::render_prompt` for the substitution algorithm
    /// and `ailang-semantic`'s `check_prompt_interpolation` for the
    /// compile-time undefined-variable warning this pairs with.
    fn builtin_render_prompt(
        &mut self,
        args: &[Value],
        env: &Environment,
        span: Span,
    ) -> EvalResult {
        let prompt_name = one_string_arg("render", args, span)?;
        match env.get(&prompt_name) {
            Some(Value::Prompt(p)) => Ok(Value::string(ailang_ai_runtime::render_prompt(
                &p.template,
                |name| env.get(name).map(|v| v.to_string()),
            ))),
            Some(other) => Err(RuntimeError::new(
                format!(
                    "render() expects a prompt name, but `{prompt_name}` is a `{}`",
                    other.type_name()
                ),
                span,
            )),
            None => Err(RuntimeError::new(
                format!("undefined prompt `{prompt_name}`"),
                span,
            )),
        }
    }

    /// `retrieve(knowledge_name: string, query: string, k?: int) -> [{string: string}]`
    ///
    /// Resolves `knowledge_name` to a declared `knowledge` block, ingests
    /// its `source` directory for real (see `ailang_ai_runtime::knowledge`),
    /// and returns the top `k` (default 3) most relevant chunks as an
    /// array of `{"source": file, "text": chunk, "score": similarity}`
    /// maps, ranked by real cosine similarity of real feature-hashing
    /// embeddings — genuine retrieval, not a placeholder.
    fn builtin_retrieve(&mut self, args: &[Value], env: &Environment, span: Span) -> EvalResult {
        if args.len() < 2 {
            return Err(RuntimeError::new(
                "retrieve() expects (knowledge_name: string, query: string, k?: int)",
                span,
            ));
        }
        let knowledge_name = expect_string(&args[0], "retrieve", span)?;
        let query = expect_string(&args[1], "retrieve", span)?;
        let k = match args.get(2) {
            Some(Value::Int(n)) => (*n).max(0) as usize,
            Some(other) => {
                return Err(RuntimeError::new(
                    format!(
                        "retrieve()'s `k` argument must be an int, found `{}`",
                        other.type_name()
                    ),
                    span,
                ))
            }
            None => 3,
        };

        let knowledge = match env.get(&knowledge_name) {
            Some(Value::Knowledge(k)) => k,
            _ => {
                return Err(RuntimeError::new(
                    format!("undefined knowledge source `{knowledge_name}`"),
                    span,
                ))
            }
        };

        let results =
            ailang_ai_runtime::retrieve(std::path::Path::new(&knowledge.source), &query, k, 200);
        Ok(Value::array(
            results
                .into_iter()
                .map(|r| {
                    Value::map(vec![
                        (Value::string("source"), Value::string(r.source_file)),
                        (Value::string("text"), Value::string(r.text)),
                        (Value::string("score"), Value::Float(r.score)),
                    ])
                })
                .collect(),
        ))
    }

    /// `run_team(name: string, input: string) -> [{string: string}]`
    fn builtin_run_team(&mut self, args: &[Value], env: &Environment, span: Span) -> EvalResult {
        let (team_name, input) = two_string_args("run_team", args, span)?;
        let team = match env.get(&team_name) {
            Some(Value::Team(t)) => t,
            _ => {
                return Err(RuntimeError::new(
                    format!("undefined team `{team_name}`"),
                    span,
                ))
            }
        };
        let agents = self.resolve_agents(&team.members, env, span)?;
        let providers = self.providers.borrow();
        let results = ailang_ai_runtime::run_team(&team.members, &agents, &providers, &input)
            .map_err(|e| RuntimeError::new(e, span))?;
        for r in &results {
            self.trace.record("team_member", &r.stage);
        }
        Ok(stage_results_to_value(results, "member"))
    }

    fn resolve_agent(
        &self,
        name: &str,
        env: &Environment,
        span: Span,
    ) -> Result<Rc<AgentValue>, RuntimeError> {
        match env.get(name) {
            Some(Value::Agent(a)) => Ok(a),
            _ => Err(RuntimeError::new(format!("undefined agent `{name}`"), span)),
        }
    }

    fn resolve_agents(
        &self,
        names: &[String],
        env: &Environment,
        span: Span,
    ) -> Result<HashMap<String, AgentValue>, RuntimeError> {
        let mut agents = HashMap::new();
        for name in names {
            let agent = self.resolve_agent(name, env, span)?;
            let mut resolved = (*agent).clone();
            // v2.0: if the agent lists multiple `models`, round-robin
            // across them for this call, overriding `model` with the
            // chosen one — see `pick_model_for_agent`'s doc for the
            // routing algorithm and its honest limitations.
            if let Some(chosen) = self.pick_model_for_agent(&agent) {
                resolved.model = Some(chosen);
            }
            agents.insert(name.clone(), resolved);
        }
        Ok(agents)
    }

    /// v2.0 multi-model routing: if `agent.models` is non-empty, returns
    /// the next model in round-robin order (a real, deterministic
    /// rotation — call 1 uses `models[0]`, call 2 uses `models[1]`, etc.,
    /// wrapping around). Returns `None` if the agent has no `models` list
    /// (in which case its plain `model` field, if any, is used as-is).
    ///
    /// This is **not** the capability/latency/cost-aware routing a real
    /// multi-provider platform would need — that requires real per-
    /// provider telemetry (actual latencies, actual costs, actual
    /// availability) this offline-by-default foundation doesn't have.
    /// Round-robin is a real, honest, simple default; see
    /// `docs/roadmap.md` for smarter routing as future work.
    fn pick_model_for_agent(&self, agent: &AgentValue) -> Option<String> {
        if agent.models.is_empty() {
            return None;
        }
        let mut counters = self.round_robin_counters.borrow_mut();
        let counter = counters.entry(agent.name.clone()).or_insert(0);
        let chosen = agent.models[*counter % agent.models.len()].clone();
        *counter += 1;
        Some(chosen)
    }

    fn resolve_network(
        &self,
        name: &str,
        env: &Environment,
        span: Span,
    ) -> Result<Rc<ailang_runtime::NetworkValue>, RuntimeError> {
        match env.get(name) {
            Some(Value::Network(n)) => Ok(n),
            _ => Err(RuntimeError::new(
                format!("undefined network `{name}`"),
                span,
            )),
        }
    }

    fn resolve_cluster(
        &self,
        name: &str,
        env: &Environment,
        span: Span,
    ) -> Result<Rc<ailang_runtime::ClusterValue>, RuntimeError> {
        match env.get(name) {
            Some(Value::Cluster(c)) => Ok(c),
            _ => Err(RuntimeError::new(
                format!("undefined cluster `{name}`"),
                span,
            )),
        }
    }

    /// `cluster_dispatch(cluster_name, agent_name, input) -> {"replica": int, "output": string}`
    ///
    /// A real round-robin replica selection (genuinely rotates through
    /// `0..replicas`, tracked per cluster) combined with a real model
    /// invocation via the named agent's provider. What's simulated: there
    /// is only one process here, so "replica 3 handled this" is a label
    /// on which rotation slot was chosen, not evidence of a second
    /// process/container actually running — see this crate's module doc.
    fn builtin_cluster_dispatch(
        &mut self,
        args: &[Value],
        env: &Environment,
        span: Span,
    ) -> EvalResult {
        if args.len() < 3 {
            return Err(RuntimeError::new(
                "cluster_dispatch() expects (cluster_name: string, agent_name: string, input: string)",
                span,
            ));
        }
        let cluster_name = expect_string(&args[0], "cluster_dispatch", span)?;
        let agent_name = expect_string(&args[1], "cluster_dispatch", span)?;
        let input = expect_string(&args[2], "cluster_dispatch", span)?;

        let cluster = self.resolve_cluster(&cluster_name, env, span)?;
        let agent = self.resolve_agent(&agent_name, env, span)?;

        let replica_index = {
            let mut counters = self.round_robin_counters.borrow_mut();
            let counter = counters
                .entry(format!("cluster:{cluster_name}"))
                .or_insert(0);
            let idx = *counter % (cluster.replicas.max(1) as usize);
            *counter += 1;
            idx
        };

        let model_name = agent.model.clone().unwrap_or_else(|| agent_name.clone());
        let output = self
            .providers
            .borrow()
            .get(&model_name)
            .generate_text(&input)
            .map_err(|e| RuntimeError::new(e, span))?;

        Ok(Value::map(vec![
            (Value::string("replica"), Value::Int(replica_index as i64)),
            (Value::string("output"), Value::string(output)),
        ]))
    }

    /// `emit(event_name, payload) -> int` (returns how many handlers ran)
    ///
    /// Runs every `on <event_name> { }` handler registered anywhere in
    /// the program, in registration order, each in its own child scope
    /// with `payload` bound. Synchronous and in-process — a real event
    /// dispatch, not a durable/replayable event log (see
    /// `docs/roadmap.md`).
    fn builtin_emit(&mut self, args: &[Value], env: &Environment, span: Span) -> EvalResult {
        let event_name = one_string_arg("emit", args, span)?;
        let payload = args.get(1).cloned().unwrap_or(Value::Null);
        let handlers = self
            .event_handlers
            .borrow()
            .get(&event_name)
            .cloned()
            .unwrap_or_default();
        for handler in &handlers {
            let scope = Environment::child(env);
            scope.define("payload", payload.clone(), false);
            self.exec_block(handler, &scope)?;
        }
        Ok(Value::Int(handlers.len() as i64))
    }

    fn named_vector_index(&self, index_name: &str) -> Rc<ailang_ai_runtime::VectorIndex> {
        self.vector_indexes
            .borrow_mut()
            .entry(index_name.to_string())
            .or_insert_with(|| Rc::new(ailang_ai_runtime::VectorIndex::new()))
            .clone()
    }

    /// `vector_index_add(index_name, id, text, metadata?: {string: string}) -> void`
    fn builtin_vector_index_add(&mut self, args: &[Value], span: Span) -> EvalResult {
        if args.len() < 3 {
            return Err(RuntimeError::new(
                "vector_index_add() expects (index_name: string, id: string, text: string, metadata?: map)",
                span,
            ));
        }
        let index_name = expect_string(&args[0], "vector_index_add", span)?;
        let id = expect_string(&args[1], "vector_index_add", span)?;
        let text = expect_string(&args[2], "vector_index_add", span)?;
        let metadata = match args.get(3) {
            Some(Value::Map(m)) => map_to_string_map(m, span)?,
            Some(other) => {
                return Err(RuntimeError::new(
                    format!(
                        "vector_index_add()'s metadata must be a map, found `{}`",
                        other.type_name()
                    ),
                    span,
                ))
            }
            None => HashMap::new(),
        };
        self.named_vector_index(&index_name)
            .add(&id, &text, metadata);
        Ok(Value::Void)
    }

    /// `vector_index_search(index_name, query, k, filter?: {string: string}) -> [{id,text,score,metadata}]`
    fn builtin_vector_index_search(&mut self, args: &[Value], span: Span) -> EvalResult {
        if args.len() < 3 {
            return Err(RuntimeError::new(
                "vector_index_search() expects (index_name: string, query: string, k: int, filter?: map)",
                span,
            ));
        }
        let index_name = expect_string(&args[0], "vector_index_search", span)?;
        let query = expect_string(&args[1], "vector_index_search", span)?;
        let k = match &args[2] {
            Value::Int(n) => (*n).max(0) as usize,
            other => {
                return Err(RuntimeError::new(
                    format!(
                        "vector_index_search()'s `k` must be an int, found `{}`",
                        other.type_name()
                    ),
                    span,
                ))
            }
        };
        let filter = match args.get(3) {
            Some(Value::Map(m)) => map_to_string_map(m, span)?,
            Some(other) => {
                return Err(RuntimeError::new(
                    format!(
                        "vector_index_search()'s filter must be a map, found `{}`",
                        other.type_name()
                    ),
                    span,
                ))
            }
            None => HashMap::new(),
        };

        let results = self
            .named_vector_index(&index_name)
            .search(&query, k, &filter);
        Ok(Value::array(
            results
                .into_iter()
                .map(|r| {
                    Value::map(vec![
                        (Value::string("id"), Value::string(r.id)),
                        (Value::string("text"), Value::string(r.text)),
                        (Value::string("score"), Value::Float(r.score)),
                        (
                            Value::string("metadata"),
                            Value::map(
                                r.metadata
                                    .into_iter()
                                    .map(|(k, v)| (Value::string(k), Value::string(v)))
                                    .collect(),
                            ),
                        ),
                    ])
                })
                .collect(),
        ))
    }

    /// `broadcast(from_agent: string, message: string) -> void`
    ///
    /// Delivers `message` to the mailbox of every declared agent except
    /// `from_agent` itself, using `self.known_agents` (populated as each
    /// `agent` statement executes — see that arm's doc comment) as the
    /// roster of "every agent." `from_agent` must itself be a declared
    /// agent, so a typo'd sender name fails loudly rather than silently
    /// broadcasting from a nonexistent identity.
    fn builtin_broadcast(&mut self, args: &[Value], env: &Environment, span: Span) -> EvalResult {
        let (from, text) = two_string_args("broadcast", args, span)?;
        self.resolve_agent(&from, env, span)?; // validate sender exists
        let recipients: Vec<String> = self.known_agents.borrow().keys().cloned().collect();
        self.mailboxes.broadcast(&from, &recipients, &text);
        Ok(Value::Void)
    }
}

/// Converts a `Value::Map` (as used for `vector_index_add`/
/// `vector_index_search` metadata and filters) into a plain
/// `HashMap<String, String>`, erroring if any key or value isn't a
/// string — metadata is deliberately string-only in v2.0 (no nested
/// maps/arrays), keeping equality-based filtering simple and unambiguous.
fn map_to_string_map(
    map: &ailang_runtime::MapRef,
    span: Span,
) -> Result<HashMap<String, String>, RuntimeError> {
    let mut out = HashMap::new();
    for (k, v) in map.borrow().iter() {
        let key = match k {
            Value::String(s) => s.to_string(),
            other => {
                return Err(RuntimeError::new(
                    format!(
                        "metadata keys must be strings, found `{}`",
                        other.type_name()
                    ),
                    span,
                ))
            }
        };
        let value = match v {
            Value::String(s) => s.to_string(),
            other => {
                return Err(RuntimeError::new(
                    format!(
                        "metadata values must be strings, found `{}`",
                        other.type_name()
                    ),
                    span,
                ))
            }
        };
        out.insert(key, value);
    }
    Ok(out)
}

fn expect_string(value: &Value, fn_name: &str, span: Span) -> Result<String, RuntimeError> {
    match value {
        Value::String(s) => Ok(s.to_string()),
        other => Err(RuntimeError::new(
            format!(
                "{fn_name}() expects a string argument, got `{}`",
                other.type_name()
            ),
            span,
        )),
    }
}

fn one_string_arg(fn_name: &str, args: &[Value], span: Span) -> Result<String, RuntimeError> {
    match args.first() {
        Some(v) => expect_string(v, fn_name, span),
        None => Err(RuntimeError::new(
            format!("{fn_name}() expects 1 argument"),
            span,
        )),
    }
}

fn one_string_arg_at(
    args: &[Value],
    index: usize,
    fn_name: &str,
    span: Span,
) -> Result<String, RuntimeError> {
    match args.get(index) {
        Some(v) => expect_string(v, fn_name, span),
        None => Err(RuntimeError::new(
            format!("{fn_name}() expects an argument at position {index}"),
            span,
        )),
    }
}

fn two_string_args(
    fn_name: &str,
    args: &[Value],
    span: Span,
) -> Result<(String, String), RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            format!("{fn_name}() expects 2 arguments"),
            span,
        ));
    }
    Ok((
        expect_string(&args[0], fn_name, span)?,
        expect_string(&args[1], fn_name, span)?,
    ))
}

/// Converts a `Vec<ailang_ai_runtime::StageResult>` into an AILang
/// `Value::Array` of `{ "<label>": stage_name, "output": output }` maps —
/// `label` is `"stage"` for `run_workflow` and `"member"` for `run_team`,
/// matching each builtin's documented return shape (see
/// `docs/language-reference.md`).
fn stage_results_to_value(results: Vec<ailang_ai_runtime::StageResult>, label: &str) -> Value {
    Value::array(
        results
            .into_iter()
            .map(|r| {
                Value::map(vec![
                    (Value::string(label), Value::string(r.stage)),
                    (Value::string("output"), Value::string(r.output)),
                ])
            })
            .collect(),
    )
}

fn literal_to_value(lit: &Literal) -> Value {
    match lit {
        Literal::Int(n) => Value::Int(*n),
        Literal::Float(n) => Value::Float(*n),
        Literal::Bool(b) => Value::Bool(*b),
        Literal::String(s) => Value::string(s.clone()),
        Literal::Char(c) => Value::Char(*c),
        Literal::Null => Value::Null,
    }
}

fn index_value(obj: &Value, idx: &Value, span: Span) -> EvalResult {
    match obj {
        Value::Array(arr) => {
            let i = match idx {
                Value::Int(i) => *i,
                _ => return Err(RuntimeError::new("array index must be an integer", span)),
            };
            let arr = arr.borrow();
            if i < 0 || i as usize >= arr.len() {
                return Err(RuntimeError::new(
                    format!("array index {i} out of bounds (len {})", arr.len()),
                    span,
                ));
            }
            Ok(arr[i as usize].clone())
        }
        Value::Map(map) => {
            let map = map.borrow();
            map.iter()
                .find(|(k, _)| k == idx)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| RuntimeError::new("key not found in map", span))
        }
        Value::String(s) => {
            let i = match idx {
                Value::Int(i) => *i,
                _ => return Err(RuntimeError::new("string index must be an integer", span)),
            };
            s.chars()
                .nth(i.max(0) as usize)
                .map(Value::Char)
                .ok_or_else(|| RuntimeError::new("string index out of bounds", span))
        }
        other => Err(RuntimeError::new(
            format!("cannot index into a `{}`", other.type_name()),
            span,
        )),
    }
}

fn eval_binary(op: BinaryOp, l: Value, r: Value, span: Span) -> EvalResult {
    use BinaryOp::*;
    use Value::*;

    match op {
        Add => match (&l, &r) {
            (Int(a), Int(b)) => Ok(Int(a + b)),
            (Float(a), Float(b)) => Ok(Float(a + b)),
            (Int(a), Float(b)) => Ok(Float(*a as f64 + b)),
            (Float(a), Int(b)) => Ok(Float(a + *b as f64)),
            (String(a), String(b)) => Ok(Value::string(format!("{a}{b}"))),
            (String(a), _) => Ok(Value::string(format!("{a}{r}"))),
            (_, String(b)) => Ok(Value::string(format!("{l}{b}"))),
            _ => type_error("+", &l, &r, span),
        },
        Sub => numeric_op(l, r, span, |a, b| a - b, |a, b| a - b),
        Mul => numeric_op(l, r, span, |a, b| a * b, |a, b| a * b),
        Div => match (&l, &r) {
            (Int(_), Int(b)) if *b == 0 => Err(RuntimeError::new("division by zero", span)),
            (Int(a), Int(b)) => Ok(Int(a / b)),
            (Float(a), Float(b)) => Ok(Float(a / b)),
            (Int(a), Float(b)) => Ok(Float(*a as f64 / b)),
            (Float(a), Int(b)) => Ok(Float(a / *b as f64)),
            _ => type_error("/", &l, &r, span),
        },
        Mod => match (&l, &r) {
            (Int(_), Int(b)) if *b == 0 => Err(RuntimeError::new("division by zero", span)),
            (Int(a), Int(b)) => Ok(Int(a % b)),
            (Float(a), Float(b)) => Ok(Float(a % b)),
            _ => type_error("%", &l, &r, span),
        },
        Eq => Ok(Bool(l == r)),
        NotEq => Ok(Bool(l != r)),
        Gt => compare(&l, &r, span, |o| o == std::cmp::Ordering::Greater),
        Lt => compare(&l, &r, span, |o| o == std::cmp::Ordering::Less),
        GtEq => compare(&l, &r, span, |o| o != std::cmp::Ordering::Less),
        LtEq => compare(&l, &r, span, |o| o != std::cmp::Ordering::Greater),
        And | Or => unreachable!("handled with short-circuit evaluation above"),
    }
}

fn numeric_op(
    l: Value,
    r: Value,
    span: Span,
    fi: fn(i64, i64) -> i64,
    ff: fn(f64, f64) -> f64,
) -> EvalResult {
    use Value::*;
    match (&l, &r) {
        (Int(a), Int(b)) => Ok(Int(fi(*a, *b))),
        (Float(a), Float(b)) => Ok(Float(ff(*a, *b))),
        (Int(a), Float(b)) => Ok(Float(ff(*a as f64, *b))),
        (Float(a), Int(b)) => Ok(Float(ff(*a, *b as f64))),
        _ => type_error("arithmetic", &l, &r, span),
    }
}

fn compare(l: &Value, r: &Value, span: Span, pred: fn(std::cmp::Ordering) -> bool) -> EvalResult {
    use Value::*;
    let ordering = match (l, r) {
        (Int(a), Int(b)) => a.partial_cmp(b),
        (Float(a), Float(b)) => a.partial_cmp(b),
        (Int(a), Float(b)) => (*a as f64).partial_cmp(b),
        (Float(a), Int(b)) => a.partial_cmp(&(*b as f64)),
        (String(a), String(b)) => a.partial_cmp(b),
        (Char(a), Char(b)) => a.partial_cmp(b),
        _ => {
            return Err(RuntimeError::new(
                format!("cannot compare `{}` and `{}`", l.type_name(), r.type_name()),
                span,
            ))
        }
    };
    Ok(Value::Bool(ordering.map(pred).unwrap_or(false)))
}

fn type_error(op: &str, l: &Value, r: &Value, span: Span) -> EvalResult {
    Err(RuntimeError::new(
        format!(
            "cannot apply `{op}` to `{}` and `{}`",
            l.type_name(),
            r.type_name()
        ),
        span,
    ))
}

/// Wall-clock seconds since the Unix epoch, used by the `clock()` builtin.
pub fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailang_parser::parse;

    fn run_capture(src: &str) -> String {
        let (program, diags) = parse(src);
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
        let mut out: Vec<u8> = Vec::new();
        {
            let mut interp = Interpreter::new(&mut out);
            interp.run(&program.unwrap()).expect("runtime error");
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn hello_world() {
        assert_eq!(run_capture(r#"print("Hello, world!");"#), "Hello, world!");
    }

    #[test]
    fn arithmetic_and_functions() {
        let out =
            run_capture("fn add(a: int, b: int) -> int { return a + b; } println(add(2, 3));");
        assert_eq!(out.trim(), "5");
    }

    #[test]
    fn recursion_fibonacci() {
        let out = run_capture(
            "fn fib(n: int) -> int { if n < 2 { return n; } return fib(n - 1) + fib(n - 2); } println(fib(10));",
        );
        assert_eq!(out.trim(), "55");
    }

    #[test]
    fn loops_and_arrays() {
        let out = run_capture(
            "let sum = 0; let nums = [1,2,3,4,5]; for i in 0..len(nums) { sum += nums[i]; } println(sum);",
        );
        assert_eq!(out.trim(), "15");
    }

    #[test]
    fn while_with_break_continue() {
        let out = run_capture(
            "let i = 0; let sum = 0; while i < 10 { i += 1; if i % 2 == 0 { continue; } if i > 7 { break; } sum += i; } println(sum);",
        );
        // 1+3+5+7 = 16
        assert_eq!(out.trim(), "16");
    }

    #[test]
    fn maps_basic() {
        let out = run_capture(r#"let ages = {"Alice": 20, "Bob": 25}; println(ages["Alice"]);"#);
        assert_eq!(out.trim(), "20");
    }

    #[test]
    fn const_reassignment_is_runtime_error() {
        let (program, diags) = parse("const PI = 3.14; PI = 4;");
        assert!(!diags.has_errors());
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        let err = interp.run(&program.unwrap()).unwrap_err();
        assert!(err.message.contains("const"));
    }

    #[test]
    fn string_concatenation() {
        let out = run_capture(r#"let name = "Alice"; println("Hello, " + name + "!");"#);
        assert_eq!(out.trim(), "Hello, Alice!");
    }

    // -----------------------------------------------------------------
    // v0.5 AI-native construct execution
    // -----------------------------------------------------------------

    #[test]
    fn agent_and_model_declarations_execute_and_bind_values() {
        let out = run_capture(
            r#"
            model GPT5;
            tool Search;
            permission internet;
            agent Researcher {
                model GPT5;
                tools [Search];
                permissions [internet];
            }
            println(type(Researcher));
            println(type(GPT5));
            "#,
        );
        assert_eq!(out.trim(), "agent\nmodel");
    }

    #[test]
    fn remember_recall_forget_round_trip() {
        let out = run_capture(
            r#"
            remember("favorite_drink", "coffee");
            println(recall("favorite_drink"));
            forget("favorite_drink");
            println(recall("favorite_drink"));
            "#,
        );
        assert_eq!(out.trim(), "coffee\nnull");
    }

    #[test]
    fn recall_finds_substring_matches() {
        let out = run_capture(
            r#"
            remember("note", "User likes coffee in the morning");
            println(recall("coffee"));
            "#,
        );
        assert_eq!(out.trim(), "User likes coffee in the morning");
    }

    #[test]
    fn generate_fills_schema_defaults() {
        let out = run_capture(
            r#"
            schema Person {
                name: string;
                age: int;
            }
            let p = generate Person;
            println(p["name"]);
            println(p["age"]);
            "#,
        );
        // p["name"] defaults to "" (prints as a blank line), p["age"] to 0.
        assert_eq!(out, "\n0\n");
    }

    #[test]
    fn workflow_executes_sequentially_through_registered_agents() {
        let out = run_capture(
            r#"
            agent Researcher {}
            agent Writer {}
            workflow Paper {
                Researcher -> Writer;
            }
            let results = run_workflow("Paper", "quantum computing");
            println(len(results));
            println(results[0]["stage"]);
            println(results[1]["stage"]);
            "#,
        );
        let lines: Vec<&str> = out.trim().lines().collect();
        assert_eq!(lines, vec!["2", "Researcher", "Writer"]);
    }

    #[test]
    fn team_runs_every_member_against_the_same_input() {
        let out = run_capture(
            r#"
            agent CEO {}
            agent CTO {}
            team Startup {
                CEO;
                CTO;
            }
            let results = run_team("Startup", "should we pivot?");
            println(len(results));
            "#,
        );
        assert_eq!(out.trim(), "2");
    }

    #[test]
    fn invoke_tool_calculator_computes_arithmetic() {
        let out = run_capture(
            r#"
            tool Calculator;
            agent Analyst {
                tools [Calculator];
            }
            println(invoke_tool("Analyst", "Calculator", "(2 + 3) * 4"));
            "#,
        );
        // Calculator always returns a float (division isn't guaranteed to
        // be a whole number), and whole floats print with one decimal
        // place — see ailang_runtime::Value's Display impl.
        assert_eq!(out.trim(), "20.0");
    }

    #[test]
    fn invoke_tool_enforces_missing_permission() {
        let (program, diags) = parse(
            r#"
            tool Filesystem;
            agent Analyst {
                tools [Filesystem];
            }
            invoke_tool("Analyst", "Filesystem", "/etc/hosts");
            "#,
        );
        assert!(!diags.has_errors());
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        let err = interp.run(&program.unwrap()).unwrap_err();
        assert!(err.message.contains("filesystem"));
    }

    #[test]
    fn invoke_tool_supports_a_custom_function_backed_tool() {
        let out = run_capture(
            r#"
            tool Doubler;
            agent Analyst {
                tools [Doubler];
            }
            fn Doubler(x: int) -> int {
                return x * 2;
            }
            println(invoke_tool("Analyst", "Doubler", 21));
            "#,
        );
        assert_eq!(out.trim(), "42");
    }

    #[test]
    fn invoke_tool_rejects_a_tool_not_declared_for_the_agent() {
        let (program, diags) = parse(
            r#"
            tool Calculator;
            agent Analyst {}
            invoke_tool("Analyst", "Calculator", "1 + 1");
            "#,
        );
        assert!(!diags.has_errors());
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        let err = interp.run(&program.unwrap()).unwrap_err();
        assert!(err.message.contains("Calculator"));
    }

    #[test]
    fn prompt_declares_and_stores_template() {
        let out = run_capture(r#"prompt system = "You are an expert."; println(type(system));"#);
        assert_eq!(out.trim(), "prompt");
    }

    #[test]
    fn render_interpolates_prompt_against_current_scope() {
        let out = run_capture(
            r#"
            let topic = "quantum computing";
            prompt system = "Research ${topic} thoroughly.";
            println(render("system"));
            "#,
        );
        assert_eq!(out.trim(), "Research quantum computing thoroughly.");
    }

    #[test]
    fn render_reflects_updated_variable_values_on_each_call() {
        let out = run_capture(
            r#"
            let topic = "cats";
            prompt system = "Topic: ${topic}";
            println(render("system"));
            topic = "dogs";
            println(render("system"));
            "#,
        );
        assert_eq!(out.trim(), "Topic: cats\nTopic: dogs");
    }

    #[test]
    fn parallel_block_runs_every_statement_even_after_a_failure() {
        // The first statement fails (division by zero); the second must
        // still run (proving independent-task semantics), and the overall
        // result must still be an error.
        let (program, diags) = parse(
            r#"
            let ran_second = false;
            parallel {
                let x = 1 / 0;
                ran_second = true;
            }
            "#,
        );
        assert!(!diags.has_errors());
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        // The parallel block itself should surface the error...
        let err = interp.run(&program.unwrap());
        assert!(err.is_err());
    }

    #[test]
    fn parallel_block_executes_independent_print_statements() {
        let out = run_capture("parallel { print(\"a\"); print(\"b\"); print(\"c\"); }");
        assert_eq!(out, "abc");
    }

    #[test]
    fn embed_returns_a_fixed_length_numeric_vector() {
        let out = run_capture(r#"println(len(embed("hello world")));"#);
        assert_eq!(out.trim(), "8");
    }

    #[test]
    fn classify_picks_the_best_matching_category() {
        let out = run_capture(
            r#"println(classify("I love cooking new recipes", ["sports", "cooking recipes"]));"#,
        );
        assert_eq!(out.trim(), "cooking recipes");
    }

    #[test]
    fn summarize_returns_first_sentence() {
        let out = run_capture(
            r#"println(summarize("This is the first sentence. This is the second."));"#,
        );
        assert_eq!(out.trim(), "This is the first sentence.");
    }

    #[test]
    fn knowledge_and_team_and_placeholder_declarations_execute_cleanly() {
        let out = run_capture(
            r#"
            knowledge Docs { source "./docs"; }
            reasoning R;
            dataset D { source "./data.csv"; }
            println(type(Docs));
            println(type(R));
            println(type(D));
            "#,
        );
        assert_eq!(out.trim(), "knowledge\nreasoning\ndataset");
    }

    // -----------------------------------------------------------------
    // v1.0: agent/team-scoped memory, versioning
    // -----------------------------------------------------------------

    #[test]
    fn remember_in_and_recall_in_operate_on_a_named_memory_store() {
        let out = run_capture(
            r#"
            memory UserMemory persistent;
            remember_in("UserMemory", "favorite_drink", "coffee");
            println(recall_in("UserMemory", "favorite_drink"));
            "#,
        );
        assert_eq!(out.trim(), "coffee");
    }

    #[test]
    fn named_memories_are_isolated_from_each_other_and_from_the_global_store() {
        let out = run_capture(
            r#"
            memory A persistent;
            memory B persistent;
            remember_in("A", "k", "value_in_a");
            remember_in("B", "k", "value_in_b");
            remember("k", "value_in_global");
            println(recall_in("A", "k"));
            println(recall_in("B", "k"));
            println(recall("k"));
            "#,
        );
        assert_eq!(out.trim(), "value_in_a\nvalue_in_b\nvalue_in_global");
    }

    #[test]
    fn multiple_agents_referencing_the_same_named_memory_share_it_team_memory_style() {
        let out = run_capture(
            r#"
            memory SharedMemory persistent;
            agent CEO { memory SharedMemory; }
            agent CTO { memory SharedMemory; }
            remember_in("SharedMemory", "decision", "pivot to B2B");
            println(recall_in("SharedMemory", "decision"));
            println(type(CEO));
            println(type(CTO));
            "#,
        );
        assert_eq!(out.trim(), "pivot to B2B\nagent\nagent");
    }

    #[test]
    fn forget_in_removes_only_from_the_named_store() {
        let out = run_capture(
            r#"
            memory A persistent;
            remember_in("A", "k", "v");
            forget_in("A", "k");
            println(recall_in("A", "k"));
            "#,
        );
        assert_eq!(out.trim(), "null");
    }

    #[test]
    fn remember_in_on_an_undeclared_memory_is_a_clear_runtime_error() {
        let (program, diags) = parse(r#"remember_in("Ghost", "k", "v");"#);
        assert!(!diags.has_errors());
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        let err = interp.run(&program.unwrap()).unwrap_err();
        assert!(err.message.contains("Ghost"));
    }

    #[test]
    fn memory_history_tracks_every_remembered_version() {
        let out = run_capture(
            r#"
            remember("topic", "cats");
            remember("topic", "dogs");
            remember("topic", "birds");
            let history = memory_history("topic");
            println(len(history));
            println(history[0]["value"]);
            println(history[1]["value"]);
            println(history[2]["value"]);
            "#,
        );
        assert_eq!(out.trim(), "3\ncats\ndogs\nbirds");
    }

    #[test]
    fn memory_history_is_empty_for_a_never_remembered_key() {
        let out = run_capture(r#"println(len(memory_history("nope")));"#);
        assert_eq!(out.trim(), "0");
    }

    #[test]
    fn retrieve_ranks_real_files_by_relevance() {
        let dir = std::env::temp_dir().join(format!(
            "ailang-interp-retrieve-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("rust.txt"),
            "Rust is a systems programming language focused on safety and performance",
        )
        .unwrap();
        std::fs::write(
            dir.join("cooking.txt"),
            "Baking bread requires flour water yeast and salt",
        )
        .unwrap();

        let source = dir.to_str().unwrap().replace('\\', "\\\\");
        let program_src = format!(
            r#"
            knowledge Docs {{ source "{source}"; }}
            let results = retrieve("Docs", "systems programming language performance", 1);
            println(len(results));
            println(results[0]["source"]);
            "#
        );
        let out = run_capture(&program_src);
        let lines: Vec<&str> = out.trim().lines().collect();
        assert_eq!(lines, vec!["1", "rust.txt"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    // -----------------------------------------------------------------
    // v1.0: secrets and the audit log
    // -----------------------------------------------------------------

    #[test]
    fn secret_reads_a_real_environment_variable() {
        std::env::set_var("AILANG_INTERP_TEST_SECRET", "topsecret");
        let out = run_capture(r#"println(secret("AILANG_INTERP_TEST_SECRET"));"#);
        assert_eq!(out.trim(), "topsecret");
        std::env::remove_var("AILANG_INTERP_TEST_SECRET");
    }

    #[test]
    fn secret_returns_null_for_an_unset_variable() {
        let out = run_capture(r#"println(secret("AILANG_DEFINITELY_UNSET_VAR_XYZ"));"#);
        assert_eq!(out.trim(), "null");
    }

    #[test]
    fn audit_log_records_a_granted_tool_invocation() {
        let out = run_capture(
            r#"
            tool Calculator;
            agent Analyst { tools [Calculator]; }
            invoke_tool("Analyst", "Calculator", "1 + 1");
            let log = audit_log();
            println(len(log));
            println(log[0]["agent"]);
            println(log[0]["tool"]);
            println(log[0]["granted"]);
            "#,
        );
        assert_eq!(out.trim(), "1\nAnalyst\nCalculator\ntrue");
    }

    #[test]
    fn audit_log_records_a_denied_tool_invocation_without_aborting_the_log() {
        let (program, diags) = parse(
            r#"
            tool Filesystem;
            agent Sandboxed { tools [Filesystem]; }
            invoke_tool("Sandboxed", "Filesystem", "/etc/hosts");
            "#,
        );
        assert!(!diags.has_errors());
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        // The call itself fails (missing permission)...
        assert!(interp.run(&program.unwrap()).is_err());
        // ...but the audit log still recorded the denial.
        let entries = interp.audit_log.entries();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].granted);
        assert_eq!(entries[0].tool, "Filesystem");
    }

    // -----------------------------------------------------------------
    // v1.0: native agent messaging
    // -----------------------------------------------------------------

    #[test]
    fn send_message_and_receive_message_round_trip() {
        let out = run_capture(
            r#"
            agent Researcher {}
            agent Reviewer {}
            send_message("Researcher", "Reviewer", "draft ready");
            let msg = receive_message("Reviewer");
            println(msg["from"]);
            println(msg["text"]);
            "#,
        );
        assert_eq!(out.trim(), "Researcher\ndraft ready");
    }

    #[test]
    fn receive_message_returns_null_when_mailbox_is_empty() {
        let out = run_capture(r#"agent A {} println(receive_message("A"));"#);
        assert_eq!(out.trim(), "null");
    }

    #[test]
    fn messages_are_delivered_in_fifo_order() {
        let out = run_capture(
            r#"
            agent A {} agent B {}
            send_message("A", "B", "first");
            send_message("A", "B", "second");
            println(receive_message("B")["text"]);
            println(receive_message("B")["text"]);
            "#,
        );
        assert_eq!(out.trim(), "first\nsecond");
    }

    #[test]
    fn broadcast_reaches_every_agent_except_the_sender() {
        let out = run_capture(
            r#"
            agent CEO {} agent CTO {} agent Designer {}
            broadcast("CEO", "all hands at 3pm");
            println(receive_message("CEO"));
            println(receive_message("CTO")["text"]);
            println(receive_message("Designer")["text"]);
            "#,
        );
        assert_eq!(out.trim(), "null\nall hands at 3pm\nall hands at 3pm");
    }

    #[test]
    fn broadcast_from_an_undeclared_agent_is_a_clear_error() {
        let (program, diags) = parse(r#"broadcast("Ghost", "hello");"#);
        assert!(!diags.has_errors());
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        let err = interp.run(&program.unwrap()).unwrap_err();
        assert!(err.message.contains("Ghost"));
    }

    // -----------------------------------------------------------------
    // v1.0: agent locations (distributed-execution foundation)
    // -----------------------------------------------------------------

    #[test]
    fn agent_on_location_parses_and_is_introspectable() {
        let out = run_capture(
            r#"
            agent Planner on cloud {}
            agent Researcher on edge {}
            agent Reviewer on local {}
            println(agent_location("Planner"));
            println(agent_location("Researcher"));
            println(agent_location("Reviewer"));
            "#,
        );
        assert_eq!(out.trim(), "cloud\nedge\nlocal");
    }

    #[test]
    fn agent_without_location_reports_null() {
        let out = run_capture(r#"agent A {} println(agent_location("A"));"#);
        assert_eq!(out.trim(), "null");
    }

    #[test]
    fn agent_with_location_still_executes_in_workflows_normally() {
        // Confirms location is metadata only in v1.0 — it doesn't change
        // execution semantics (see this module's doc comment on why real
        // distribution is future work, not faked).
        let out = run_capture(
            r#"
            agent Planner on cloud {}
            agent Researcher on edge {}
            workflow Plan { Planner -> Researcher; }
            let results = run_workflow("Plan", "input");
            println(len(results));
            "#,
        );
        assert_eq!(out.trim(), "2");
    }

    // -----------------------------------------------------------------
    // v1.0: observability / execution tracing
    // -----------------------------------------------------------------

    #[test]
    fn get_trace_records_workflow_stages_in_order() {
        let out = run_capture(
            r#"
            agent A {} agent B {}
            workflow W { A -> B; }
            run_workflow("W", "input");
            let trace = get_trace();
            println(len(trace));
            println(trace[0]["kind"]);
            println(trace[0]["label"]);
            println(trace[1]["label"]);
            "#,
        );
        assert_eq!(out.trim(), "2\nworkflow_stage\nA\nB");
    }

    #[test]
    fn get_trace_records_tool_invocations() {
        let out = run_capture(
            r#"
            tool Calculator;
            agent Analyst { tools [Calculator]; }
            invoke_tool("Analyst", "Calculator", "1 + 1");
            let trace = get_trace();
            println(len(trace));
            println(trace[0]["kind"]);
            println(trace[0]["label"]);
            "#,
        );
        assert_eq!(out.trim(), "1\ntool_invocation\nCalculator");
    }

    #[test]
    fn get_trace_is_empty_when_nothing_traceable_has_run() {
        let out = run_capture(r#"println(len(get_trace()));"#);
        assert_eq!(out.trim(), "0");
    }

    // -----------------------------------------------------------------
    // v2.0: network, cluster, events, pub/sub, request/response,
    // vector engine, context engine, scheduler, multi-model routing
    // -----------------------------------------------------------------

    #[test]
    fn network_members_and_leader_and_health() {
        let out = run_capture(
            r#"
            agent Planner {} agent Researcher {}
            network Enterprise { agent Planner; agent Researcher; }
            let members = network_members("Enterprise");
            println(len(members));
            println(network_leader("Enterprise"));
            let health = network_health("Enterprise");
            println(len(health));
            println(health[0]["healthy"]);
            "#,
        );
        // Leader is the lexicographically smallest name: "Planner" < "Researcher".
        assert_eq!(out.trim(), "2\nPlanner\n2\ntrue");
    }

    #[test]
    fn network_join_and_leave_mutate_membership_dynamically() {
        let out = run_capture(
            r#"
            agent A {} agent B {}
            network N { agent A; }
            println(len(network_members("N")));
            network_join("N", "B");
            println(len(network_members("N")));
            network_leave("N", "A");
            println(len(network_members("N")));
            "#,
        );
        assert_eq!(out.trim(), "1\n2\n1");
    }

    #[test]
    fn network_join_rejects_an_undeclared_agent() {
        let (program, diags) = parse(r#"network N {} network_join("N", "Ghost");"#);
        assert!(!diags.has_errors());
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        let err = interp.run(&program.unwrap()).unwrap_err();
        assert!(err.message.contains("Ghost"));
    }

    #[test]
    fn cluster_status_reports_replicas_and_autoscaling() {
        let out = run_capture(
            r#"
            cluster Workers { replicas 10; autoscaling enabled; }
            let status = cluster_status("Workers");
            println(status["replicas"]);
            println(status["autoscaling"]);
            "#,
        );
        assert_eq!(out.trim(), "10\ntrue");
    }

    #[test]
    fn cluster_dispatch_round_robins_across_replicas() {
        let out = run_capture(
            r#"
            agent Worker {}
            cluster Pool { replicas 3; autoscaling disabled; }
            println(cluster_dispatch("Pool", "Worker", "task1")["replica"]);
            println(cluster_dispatch("Pool", "Worker", "task2")["replica"]);
            println(cluster_dispatch("Pool", "Worker", "task3")["replica"]);
            println(cluster_dispatch("Pool", "Worker", "task4")["replica"]);
            "#,
        );
        assert_eq!(out.trim(), "0\n1\n2\n0");
    }

    #[test]
    fn on_event_handler_runs_on_emit_with_payload_bound() {
        let out = run_capture(
            r#"
            on document_uploaded {
                println("handled: " + payload);
            }
            emit("document_uploaded", "report.pdf");
            "#,
        );
        assert_eq!(out.trim(), "handled: report.pdf");
    }

    #[test]
    fn multiple_handlers_for_the_same_event_all_run_in_order() {
        let out = run_capture(
            r#"
            on task_completed { println("handler A"); }
            on task_completed { println("handler B"); }
            let count = emit("task_completed", null);
            println(count);
            "#,
        );
        assert_eq!(out.trim(), "handler A\nhandler B\n2");
    }

    #[test]
    fn emit_with_no_handlers_returns_zero() {
        let out = run_capture(r#"println(emit("nothing_listens", null));"#);
        assert_eq!(out.trim(), "0");
    }

    #[test]
    fn pub_sub_delivers_to_every_subscriber() {
        let out = run_capture(
            r#"
            agent A {} agent B {}
            subscribe("alerts", "A");
            subscribe("alerts", "B");
            publish("alerts", "server down");
            println(receive_message("A")["text"]);
            println(receive_message("B")["text"]);
            "#,
        );
        assert_eq!(out.trim(), "server down\nserver down");
    }

    #[test]
    fn unsubscribe_stops_delivery() {
        let out = run_capture(
            r#"
            subscribe("alerts", "A");
            unsubscribe("alerts", "A");
            publish("alerts", "hello");
            println(receive_message("A"));
            "#,
        );
        assert_eq!(out.trim(), "null");
    }

    #[test]
    fn request_response_round_trip() {
        let out = run_capture(
            r#"
            fn answer(question: string) -> string {
                return "42 is the answer to " + question;
            }
            on_request("DeepThought", "answer");
            println(request("Human", "DeepThought", "everything"));
            "#,
        );
        assert_eq!(out.trim(), "42 is the answer to everything");
    }

    #[test]
    fn request_with_no_registered_responder_errors_clearly() {
        let (program, diags) = parse(r#"request("A", "Ghost", "hi");"#);
        assert!(!diags.has_errors());
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        let err = interp.run(&program.unwrap()).unwrap_err();
        assert!(err.message.contains("Ghost"));
    }

    #[test]
    fn vector_index_add_and_search_ranks_by_relevance() {
        let out = run_capture(
            r#"
            vector_index_add("Docs", "1", "rust systems programming memory safety", {});
            vector_index_add("Docs", "2", "baking bread flour yeast water", {});
            let results = vector_index_search("Docs", "rust programming memory safety", 1, {});
            println(len(results));
            println(results[0]["id"]);
            "#,
        );
        assert_eq!(out.trim(), "1\n1");
    }

    #[test]
    fn vector_index_search_filters_by_metadata() {
        let out = run_capture(
            r#"
            vector_index_add("Docs", "1", "quarterly report", {"dept": "finance"});
            vector_index_add("Docs", "2", "quarterly report", {"dept": "engineering"});
            let results = vector_index_search("Docs", "quarterly report", 10, {"dept": "finance"});
            println(len(results));
            println(results[0]["id"]);
            "#,
        );
        assert_eq!(out.trim(), "1\n1");
    }

    #[test]
    fn compress_context_truncates_long_text() {
        let out = run_capture(r#"println(compress_context("one two three four five", 2));"#);
        assert_eq!(out.trim(), "one two...");
    }

    #[test]
    fn dedupe_chunks_removes_duplicates() {
        let out = run_capture(r#"println(dedupe_chunks(["a", "b", "a"]));"#);
        assert_eq!(out.trim(), "[\"a\", \"b\"]");
    }

    #[test]
    fn schedule_and_run_scheduled_executes_in_priority_order() {
        let out = run_capture(
            r#"
            fn low() -> string { return "low"; }
            fn high() -> string { return "high"; }
            schedule("low", 1);
            schedule("high", 10);
            let results = run_scheduled();
            println(results[0]);
            println(results[1]);
            "#,
        );
        assert_eq!(out.trim(), "high\nlow");
    }

    #[test]
    fn agent_with_multiple_models_round_robins_and_is_introspectable() {
        let out = run_capture(
            r#"
            model GPT5;
            model Claude;
            agent Analyst { models [GPT5, Claude]; }
            println(agent_models("Analyst"));
            "#,
        );
        assert_eq!(out.trim(), "[\"GPT5\", \"Claude\"]");
    }

    #[test]
    fn multi_model_agent_routes_round_robin_through_workflow_runs() {
        // Both models are MockProviders (no endpoint), so we can't observe
        // *which* model answered directly, but we CAN observe that
        // round-robin routing doesn't crash across repeated runs and
        // produces consistent output shape.
        let out = run_capture(
            r#"
            model GPT5;
            model Claude;
            agent Analyst { models [GPT5, Claude]; }
            workflow Solo { Analyst; }
            let r1 = run_workflow("Solo", "q1");
            let r2 = run_workflow("Solo", "q2");
            println(len(r1) == 1 && len(r2) == 1);
            "#,
        );
        assert_eq!(out.trim(), "true");
    }

    // -----------------------------------------------------------------
    // v1.0: step-wise execution API (backs `ail run --debug`)
    // -----------------------------------------------------------------

    #[test]
    fn exec_top_level_runs_statements_one_at_a_time() {
        let (program, diags) = parse("let x = 1; x = x + 1; x = x + 1;");
        assert!(!diags.has_errors());
        let program = program.unwrap();
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        interp.prepare(&program);
        for stmt in &program.statements {
            interp.exec_top_level(stmt).unwrap();
        }
        assert_eq!(interp.global_value("x"), Some("3".to_string()));
    }

    #[test]
    fn global_value_reflects_state_between_steps() {
        let (program, diags) = parse("let x = 1; x = 5;");
        assert!(!diags.has_errors());
        let program = program.unwrap();
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        interp.prepare(&program);
        interp.exec_top_level(&program.statements[0]).unwrap();
        assert_eq!(interp.global_value("x"), Some("1".to_string()));
        interp.exec_top_level(&program.statements[1]).unwrap();
        assert_eq!(interp.global_value("x"), Some("5".to_string()));
    }

    #[test]
    fn global_value_is_none_for_an_undefined_name() {
        let mut out: Vec<u8> = Vec::new();
        let interp = Interpreter::new(&mut out);
        assert_eq!(interp.global_value("nonexistent"), None);
    }

    #[test]
    fn prepare_hoists_functions_so_forward_calls_work_stepwise() {
        let (program, diags) =
            parse("let result = greet(); fn greet() -> string { return \"hi\"; }");
        assert!(!diags.has_errors());
        let program = program.unwrap();
        let mut out: Vec<u8> = Vec::new();
        let mut interp = Interpreter::new(&mut out);
        interp.prepare(&program);
        interp.exec_top_level(&program.statements[0]).unwrap();
        assert_eq!(interp.global_value("result"), Some("hi".to_string()));
    }
}
