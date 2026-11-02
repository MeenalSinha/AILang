//! # ailang-ai-runtime
//!
//! **AI Runtime Foundation** for AILang v0.5.
//!
//! This crate is the "dedicated AI runtime" called for by the v0.5 upgrade
//! spec: agent lifecycle data, a model-provider abstraction, prompt
//! rendering, a tool registry with permission enforcement, and
//! workflow/team execution. It depends only on `ailang-runtime` (for
//! [`ailang_runtime::Value`] and the AI-native value structs defined there)
//! and `ailang-parser`/`ailang-ast` (for the tiny self-contained arithmetic
//! evaluator the calculator tool uses); `ailang-interpreter` depends on
//! this crate, not the other way around, keeping the workspace's crate
//! graph a DAG (see `docs/architecture.md`).
//!
//! ## What's real vs. a documented placeholder
//!
//! Being upfront about this here, once, rather than scattering caveats:
//!
//! | Component | Status |
//! |---|---|
//! | [`memory::InMemoryStore`] | **Real.** A working, tested key/value + substring-recall store. |
//! | [`providers::MockProvider`] | **Real, deterministic algorithm** — not a real LLM. Text "generation" and schema-filling are documented, inspectable algorithms (see its doc comment), not calls to an external model. |
//! | [`providers::HttpModelProvider`] | **Real network I/O** — a genuine, from-scratch HTTP/1.1 client over `std::net::TcpStream` (no TLS; see its doc comment), for talking to a local model server (e.g. Ollama-style `endpoint "http://localhost:11434"`). |
//! | [`tools::CalculatorTool`] | **Real** — evaluates arithmetic via a small self-contained expression evaluator. |
//! | [`tools::FilesystemTool`] | **Real** file I/O, gated by the `filesystem` permission. |
//! | [`tools::HttpTool`] | **Real** HTTP GET (same std-only client as the model provider), gated by the `internet` permission. |
//! | [`tools::SearchTool`], [`tools::BrowserTool`] | **Documented placeholders.** A real implementation needs an external search API / a headless browser, neither of which this foundation depends on; they return a clearly-labeled "not available" result rather than fabricating one. |
//! | [`workflow::run_workflow`] / [`workflow::run_team`] | **Real, sequential execution** through registered agents/providers. |
//! | [`embedding::embed`] | **Real** (if intentionally simple) feature-hashing embedding — a genuine, deterministic algorithm, not a stand-in for a trained model's embedding space. |
//! | [`classify::classify`] | **Real** bag-of-words keyword-overlap classifier. |
//! | [`knowledge::retrieve`] | **Real** (v1.0) — reads real files from a `knowledge` source directory, chunks them, embeds each chunk with `embed`, and ranks by real cosine similarity. Non-recursive directory scan and word-count chunking are v1.0 scope cuts, not placeholders; see that module's doc. |
//! | [`security::get_secret`] / [`security::AuditLog`] | **Real** (v1.0) — the sole environment-variable entry point in the stdlib, and a genuine in-process audit trail for every `invoke_tool` call. Not a full security framework (no policy engine, no capability tokens, no signing) — see that module's doc. |
//! | [`messaging::MailboxRegistry`] | **Real** (v1.0) — genuine FIFO in-process message queues per agent, with broadcast. Synchronous/single-process, not distributed across threads or machines; see that module's doc for why and what "native agent messaging" means here. |
//! | [`trace::ExecutionTrace`] | **Real** (v1.0) — records workflow/team/tool events with accurate elapsed-time timestamps and renders a readable timeline (`ail run --trace`). Not full distributed observability (no metrics aggregation, no cost tracking); see that module's doc. |
//! | [`network::elect_leader`] / [`network::health_check`] | **Real, deterministic algorithm** (v2.0) — genuinely computed leader election and health status, but a single-process simulation, not a real distributed consensus protocol (Raft/Paxos need actual multi-node communication this release doesn't have). See that module's doc. |
//! | [`messaging::TopicRegistry`] | **Real** (v2.0) — genuine pub/sub, layered on the same real mailboxes `send_message`/`receive_message` use. |
//! | [`vector::VectorIndex`] | **Real** (v2.0) — real embedding-backed indexing, cosine-similarity search, and exact-match metadata filtering. In-memory only, single embedding backend (the hashing-trick `embed`); see that module's doc. |
//! | [`context::compress_context`] / [`context::dedupe_chunks`] | **Real, deterministic algorithms** (v2.0) — word-count compression and exact/near-duplicate removal. Not semantic (LLM-judged) chunking or prioritization; see that module's doc. |
//! | [`scheduler::PriorityScheduler`] | **Real** (v2.0) — a genuine priority queue with FIFO tie-breaking. Not resource/cost/latency/GPU/energy-aware scheduling (needs real telemetry this offline foundation doesn't have); see that module's doc. |
//! | Parallel execution | Not in this crate — see `ailang-interpreter`'s module doc for why v0.5's `parallel { }` runs sequentially today. |

pub mod classify;
pub mod context;
pub mod embedding;
pub mod knowledge;
pub mod memory;
pub mod messaging;
pub mod network;
pub mod permission;
pub mod prompt;
pub mod providers;
pub mod scheduler;
pub mod schema;
pub mod security;
pub mod tools;
pub mod trace;
pub mod vector;
pub mod workflow;

pub use classify::classify;
pub use context::{compress_context, dedupe_chunks};
pub use embedding::embed;
pub use knowledge::{retrieve, RetrievedChunk};
pub use memory::{InMemoryStore, MemoryStore};
pub use messaging::{MailboxRegistry, Message, TopicRegistry};
pub use network::{elect_leader, health_check, HealthStatus};
pub use permission::PermissionSet;
pub use prompt::render_prompt;
pub use providers::{HttpModelProvider, MockProvider, ModelProvider, ProviderRegistry};
pub use scheduler::PriorityScheduler;
pub use schema::fill_schema_defaults;
pub use security::{get_secret, AuditEntry, AuditLog};
pub use tools::{Tool, ToolRegistry};
pub use trace::{ExecutionTrace, TraceEvent};
pub use vector::{VectorIndex, VectorSearchResult};
pub use workflow::{run_team, run_workflow, StageResult};
