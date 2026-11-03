//! # ailang-runtime
//!
//! Runtime value representation (`Value`), lexically-scoped environments,
//! and the call stack used by the tree-walk interpreter.
//!
//! ## Garbage collection
//!
//! v0.1 has no tracing garbage collector: heap objects (`Array`, `Map`,
//! `String`, closures) are reference-counted via [`Rc`]/[`RefCell`], which
//! is sufficient for acyclic programs and is what most tree-walk
//! interpreters start with (see Crafting Interpreters' `jlox`). Collection
//! strategy is fully abstracted behind the [`Value`] API and the
//! [`Environment`] API — nothing outside this crate touches `Rc`/`RefCell`
//! directly — so a future tracing or generational collector can be dropped
//! in behind the same interface without changing the interpreter or
//! standard library call sites.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use ailang_ast::{Block, MemoryKind, Param, TypeExpr};

/// A heap-allocated, mutable array. Shared via `Rc<RefCell<..>>` so that
/// array parameters exhibit reference semantics, matching the mutable
/// collection semantics implied by the language spec.
pub type ArrayRef = Rc<RefCell<Vec<Value>>>;

/// A heap-allocated, mutable map. v0.1 uses a `Vec` of pairs with
/// equality-based lookup rather than a hash map: `Value` includes `Float`,
/// which has no total `Eq`/`Hash` impl, and map keys in v0.1 programs are
/// small in practice. This is explicitly a foundation choice: the `MapRef`
/// type alias is the only thing call sites depend on, so swapping in a real
/// hash map (keyed by a `HashableValue` newtype) later is a one-file change.
pub type MapRef = Rc<RefCell<Vec<(Value, Value)>>>;

#[derive(Clone)]
pub struct FunctionValue {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Rc<Block>,
    pub closure: Environment,
}

impl fmt::Debug for FunctionValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<fn {}>", self.name)
    }
}

// ---------------------------------------------------------------------
// v0.5 AI-native runtime values. These are plain data structs (no
// behavior) deliberately: *executing* an agent, running a workflow, or
// invoking a tool is `ailang-ai-runtime`'s job (it depends on this crate
// for `Value`; nothing here depends back on it, keeping the dependency
// graph a DAG). This crate only needs to represent the values well enough
// for the interpreter to store, pass around, and print them.
// ---------------------------------------------------------------------

/// Which permission/memory a value refers to is always a name (there's no
/// runtime handle-passing beyond that in v0.5) — see `ailang-ai-runtime`'s
/// `PermissionSet` and `MemoryStore` for where names get resolved to
/// actual behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentValue {
    pub name: String,
    pub model: Option<String>,
    /// `Some(Ok(named_memory))` for `memory Name;`, `Some(Err(kind))` for
    /// the inline `persistent`/`temporary` form, `None` if unspecified.
    pub memory: Option<Result<String, MemoryKind>>,
    pub tools: Vec<String>,
    pub permissions: Vec<String>,
    /// v1.0: where this agent is declared to run (`agent Planner on
    /// cloud { ... }`), if specified. A free-form name, not a fixed enum
    /// — `cloud`/`edge`/`local` are conventions, not reserved words. See
    /// `ailang-interpreter`'s module doc for what v1.0 does with this
    /// (real data, simulated single-process execution).
    pub location: Option<String>,
    /// v2.0: `models [GPT5, Claude];` — if non-empty, `ailang-interpreter`
    /// round-robins across these instead of using `model` directly for
    /// workflow/team/cluster execution. Empty when the agent used the
    /// singular `model` field (or neither).
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelValue {
    pub name: String,
    pub endpoint: Option<String>,
    pub config: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolValue {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionValue {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryHandleValue {
    pub name: String,
    pub kind: MemoryKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptValue {
    pub name: String,
    /// The raw template text, `${name}`-interpolation markers intact.
    /// Interpolation happens at render time (`ailang-ai-runtime::render_prompt`),
    /// not at declaration time, so a prompt can be declared before the
    /// variables it interpolates are given their final values.
    pub template: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaValue {
    pub name: String,
    pub fields: Vec<(String, TypeExpr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowValue {
    pub name: String,
    pub stages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamValue {
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeValue {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiPlaceholderValue {
    pub keyword: &'static str,
    pub name: String,
}

/// A named, introspectable grouping of agents. Unlike `TeamValue`'s
/// fixed-at-declaration member list, `members` is mutable at runtime
/// (`network_join`/`network_leave` in `ailang-interpreter`) — real
/// dynamic membership, within the scope of "real" this single-process
/// foundation supports (see `ailang-ai-runtime`'s crate doc for the line
/// between real and simulated in v2.0).
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkValue {
    pub name: String,
    pub members: RefCell<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClusterValue {
    pub name: String,
    pub replicas: i64,
    pub autoscaling: bool,
}

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(Rc<String>),
    Char(char),
    Null,
    Void,
    Array(ArrayRef),
    Map(MapRef),
    Function(Rc<FunctionValue>),
    /// A standard-library builtin, identified by name and dispatched by the
    /// interpreter (native functions need access to the interpreter's I/O
    /// sink for `print`/`input`, so they aren't plain closures here).
    Native(Rc<str>),
    // ---- v0.5 AI-native values ----
    Agent(Rc<AgentValue>),
    Model(Rc<ModelValue>),
    Tool(Rc<ToolValue>),
    Permission(Rc<PermissionValue>),
    MemoryHandle(Rc<MemoryHandleValue>),
    Prompt(Rc<PromptValue>),
    Schema(Rc<SchemaValue>),
    Workflow(Rc<WorkflowValue>),
    Team(Rc<TeamValue>),
    Knowledge(Rc<KnowledgeValue>),
    AiPlaceholder(Rc<AiPlaceholderValue>),
    // ---- v2.0 distributed-intelligence values ----
    Network(Rc<NetworkValue>),
    Cluster(Rc<ClusterValue>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::String(_) => "string",
            Value::Char(_) => "char",
            Value::Null => "null",
            Value::Void => "void",
            Value::Array(_) => "array",
            Value::Map(_) => "map",
            Value::Function(_) | Value::Native(..) => "function",
            Value::Agent(_) => "agent",
            Value::Model(_) => "model",
            Value::Tool(_) => "tool",
            Value::Permission(_) => "permission",
            Value::MemoryHandle(_) => "memory",
            Value::Prompt(_) => "prompt",
            Value::Schema(_) => "schema",
            Value::Workflow(_) => "workflow",
            Value::Team(_) => "team",
            Value::Knowledge(_) => "knowledge",
            Value::AiPlaceholder(p) => p.keyword,
            Value::Network(_) => "network",
            Value::Cluster(_) => "cluster",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Null => false,
            Value::Void => false,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.borrow().is_empty(),
            Value::Map(m) => !m.borrow().is_empty(),
            Value::Function(_) | Value::Native(..) => true,
            Value::Char(c) => *c != '\0',
            Value::Agent(_)
            | Value::Model(_)
            | Value::Tool(_)
            | Value::Permission(_)
            | Value::MemoryHandle(_)
            | Value::Prompt(_)
            | Value::Schema(_)
            | Value::Workflow(_)
            | Value::Team(_)
            | Value::Knowledge(_)
            | Value::AiPlaceholder(_)
            | Value::Network(_)
            | Value::Cluster(_) => true,
        }
    }

    pub fn string(s: impl Into<String>) -> Value {
        Value::String(Rc::new(s.into()))
    }

    pub fn array(v: Vec<Value>) -> Value {
        Value::Array(Rc::new(RefCell::new(v)))
    }

    pub fn map(entries: Vec<(Value, Value)>) -> Value {
        Value::Map(Rc::new(RefCell::new(entries)))
    }

    pub fn native(name: &str) -> Value {
        Value::Native(Rc::from(name))
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => {
                (*a as f64) == *b
            }
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Void, Value::Void) => true,
            (Value::Array(a), Value::Array(b)) => *a.borrow() == *b.borrow(),
            (Value::Map(a), Value::Map(b)) => *a.borrow() == *b.borrow(),
            // v0.5 AI-native values compare equal only by identity of their
            // declared name — there's no meaningful structural equality
            // for "is this the same agent" beyond that in v0.5.
            (Value::Agent(a), Value::Agent(b)) => a.name == b.name,
            (Value::Model(a), Value::Model(b)) => a.name == b.name,
            (Value::Tool(a), Value::Tool(b)) => a.name == b.name,
            (Value::Permission(a), Value::Permission(b)) => a.name == b.name,
            (Value::MemoryHandle(a), Value::MemoryHandle(b)) => a.name == b.name,
            (Value::Prompt(a), Value::Prompt(b)) => a.name == b.name,
            (Value::Schema(a), Value::Schema(b)) => a.name == b.name,
            (Value::Workflow(a), Value::Workflow(b)) => a.name == b.name,
            (Value::Team(a), Value::Team(b)) => a.name == b.name,
            (Value::Knowledge(a), Value::Knowledge(b)) => a.name == b.name,
            (Value::AiPlaceholder(a), Value::AiPlaceholder(b)) => a.name == b.name,
            (Value::Network(a), Value::Network(b)) => a.name == b.name,
            (Value::Cluster(a), Value::Cluster(b)) => a.name == b.name,
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    write!(f, "{n:.1}")
                } else {
                    write!(f, "{n}")
                }
            }
            Value::Bool(b) => write!(f, "{b}"),
            Value::String(s) => write!(f, "{s}"),
            Value::Char(c) => write!(f, "{c}"),
            Value::Null => write!(f, "null"),
            Value::Void => write!(f, "void"),
            Value::Array(a) => {
                write!(f, "[")?;
                for (i, v) in a.borrow().iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if matches!(v, Value::String(_)) {
                        write!(f, "\"{v}\"")?;
                    } else {
                        write!(f, "{v}")?;
                    }
                }
                write!(f, "]")
            }
            Value::Map(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.borrow().iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Value::Function(func) => write!(f, "<fn {}>", func.name),
            Value::Native(name) => write!(f, "<native fn {name}>"),
            Value::Agent(a) => write!(f, "<agent {}>", a.name),
            Value::Model(m) => write!(f, "<model {}>", m.name),
            Value::Tool(t) => write!(f, "<tool {}>", t.name),
            Value::Permission(p) => write!(f, "<permission {}>", p.name),
            Value::MemoryHandle(m) => write!(f, "<memory {}>", m.name),
            Value::Prompt(p) => write!(f, "<prompt {}>", p.name),
            Value::Schema(s) => write!(f, "<schema {}>", s.name),
            Value::Workflow(w) => write!(f, "<workflow {}>", w.name),
            Value::Team(t) => write!(f, "<team {}>", t.name),
            Value::Knowledge(k) => write!(f, "<knowledge {}>", k.name),
            Value::AiPlaceholder(p) => write!(f, "<{} {} (placeholder)>", p.keyword, p.name),
            Value::Network(n) => write!(f, "<network {}>", n.name),
            Value::Cluster(c) => write!(f, "<cluster {}>", c.name),
        }
    }
}

#[derive(Debug)]
struct Binding {
    value: Value,
    is_const: bool,
}

#[derive(Debug, Default)]
struct EnvData {
    values: HashMap<String, Binding>,
    parent: Option<Environment>,
}

/// A lexically-scoped environment. Cheaply `Clone`-able (it's an `Rc`
/// handle) so closures can capture their defining environment by value.
#[derive(Debug, Clone)]
pub struct Environment(Rc<RefCell<EnvData>>);

impl Environment {
    pub fn new() -> Self {
        Environment(Rc::new(RefCell::new(EnvData::default())))
    }

    pub fn child(parent: &Environment) -> Self {
        Environment(Rc::new(RefCell::new(EnvData {
            values: HashMap::new(),
            parent: Some(parent.clone()),
        })))
    }

    pub fn define(&self, name: &str, value: Value, is_const: bool) {
        self.0
            .borrow_mut()
            .values
            .insert(name.to_string(), Binding { value, is_const });
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        let data = self.0.borrow();
        if let Some(b) = data.values.get(name) {
            return Some(b.value.clone());
        }
        match &data.parent {
            Some(p) => p.get(name),
            None => None,
        }
    }

    /// Returns `Err(())` if the variable is `const` (caller should raise a
    /// runtime error), or `Err(())` if undefined.
    pub fn assign(&self, name: &str, value: Value) -> Result<(), AssignError> {
        let mut data = self.0.borrow_mut();
        if let Some(b) = data.values.get_mut(name) {
            if b.is_const {
                return Err(AssignError::ConstReassignment);
            }
            b.value = value;
            return Ok(());
        }
        match &data.parent {
            Some(p) => {
                let p = p.clone();
                drop(data);
                p.assign(name, value)
            }
            None => Err(AssignError::Undefined),
        }
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignError {
    Undefined,
    ConstReassignment,
}

/// A single call-stack frame, used for stack traces and recursion-depth
/// limiting (guards against runaway recursion crashing the host process).
#[derive(Debug, Clone)]
pub struct Frame {
    pub function_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct CallStack {
    frames: Vec<Frame>,
    pub max_depth: usize,
}

impl CallStack {
    pub fn new(max_depth: usize) -> Self {
        Self {
            frames: Vec::new(),
            max_depth,
        }
    }

    pub fn push(&mut self, function_name: impl Into<String>) -> Result<(), String> {
        if self.frames.len() >= self.max_depth {
            return Err(format!(
                "stack overflow: exceeded maximum call depth of {}",
                self.max_depth
            ));
        }
        self.frames.push(Frame {
            function_name: function_name.into(),
        });
        Ok(())
    }

    pub fn pop(&mut self) {
        self.frames.pop();
    }

    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    pub fn trace(&self) -> Vec<String> {
        self.frames
            .iter()
            .rev()
            .map(|f| f.function_name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_shadowing() {
        let parent = Environment::new();
        parent.define("x", Value::Int(1), false);
        let child = Environment::child(&parent);
        child.define("x", Value::Int(2), false);
        assert_eq!(child.get("x"), Some(Value::Int(2)));
        assert_eq!(parent.get("x"), Some(Value::Int(1)));
    }

    #[test]
    fn environment_assign_propagates_to_parent() {
        let parent = Environment::new();
        parent.define("x", Value::Int(1), false);
        let child = Environment::child(&parent);
        child.assign("x", Value::Int(42)).unwrap();
        assert_eq!(parent.get("x"), Some(Value::Int(42)));
    }

    #[test]
    fn const_reassignment_errors() {
        let env = Environment::new();
        #[allow(clippy::approx_constant)]
        env.define("PI", Value::Float(3.14), true); // intentional approx for const-reassignment test
        let err = env.assign("PI", Value::Float(3.0)).unwrap_err();
        assert_eq!(err, AssignError::ConstReassignment);
    }

    #[test]
    fn call_stack_overflow_detection() {
        let mut stack = CallStack::new(2);
        stack.push("a").unwrap();
        stack.push("b").unwrap();
        assert!(stack.push("c").is_err());
    }

    #[test]
    fn value_equality_across_int_float() {
        assert_eq!(Value::Int(2), Value::Float(2.0));
    }
}
