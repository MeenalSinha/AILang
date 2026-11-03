// kernel/src/lib.rs
//! # ailang-kernel
//!
//! The **AILang Intelligence Kernel** — the AI Operating System core.
//!
//! ## What this is
//!
//! The kernel is the foundational resource-management layer introduced in
//! AILang v3.0. It manages *intelligence resources* (agents, models, memory,
//! knowledge, tools, goals, context, devices) in the same way a traditional
//! OS kernel manages CPU, memory, storage, network, processes, and devices.
//!
//! ## Architecture
//!
//! ```text
//! AILang Application
//!        ↓
//! AILang Runtime (ailang-interpreter)
//!        ↓
//! Intelligence Kernel  ← this crate
//!        ↓
//! Resource Managers (agent_process, model_fabric, memory_fabric, ...)
//!        ↓
//! Hardware / Cloud / Edge
//! ```
//!
//! ## Threading model
//!
//! All kernel structs use `Arc<Mutex<>>` rather than `Rc<RefCell<>>` — the
//! kernel is designed to be shared across threads in a future async runtime.
//! The v3.0 interpreter still runs single-threaded but the kernel API is
//! ready for a future async executor.
//!
//! ## V3.0 honest limitations
//!
//! - The planner uses a simple heuristic decomposition, not a real planning LLM.
//! - Agent supervision is tracked in-process; actual restart requires the
//!   interpreter to re-execute the agent's body block.
//! - Model failover selects an alternative; the actual inference call is still
//!   handled by `ailang-ai-runtime`'s `ModelProvider` implementations.
//! - Events are dispatched synchronously in the same thread.

pub mod agent_process;
pub mod context_manager;
pub mod event_bus;
pub mod goal_engine;
pub mod health_monitor;
pub mod isolation;
pub mod model_fabric;
pub mod planner;
pub mod resource_manager;
pub mod supervisor;

pub use agent_process::{AgentProcess, AgentProcessConfig, AgentProcessTable, AgentState, ResourceAllocation};
pub use context_manager::{ContextEntry, ContextKind, ContextManager};
pub use event_bus::{Event, EventBus};
pub use goal_engine::{ExecutionPlan, Goal, GoalConstraints, GoalEngine, GoalStatus, PlannedTask};
pub use health_monitor::{AlertSeverity, ComponentHealth, HealthAlert, HealthMonitor, HealthStatus};
pub use isolation::{AppNamespace, ApplicationIsolation};
pub use model_fabric::{FailoverPolicy, ModelCapability, ModelFabric, ModelLocation, ModelSpec, ModelStatus};
pub use planner::{Planner, TaskGraph};
pub use resource_manager::{ResourceKind, ResourceManager, ResourceSpec};
pub use supervisor::{ChildSpec, ChildStatus, Supervisor, SupervisionStrategy};

