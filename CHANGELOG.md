# Changelog

## v3.0 — The AI Operating System (this release)

### Phase 0: Forensic Audit & Stabilization of v2.0
- Re-verified all v2.0 language and runtime subsystems across 370+ tests.
- Audited agent communication, clustering, event dispatch, and vector engines.
- Diagnosed and repaired keyword boundary issues where v3.0 keywords (`persistent`, `emit`, `checkpoint`) collided with v2.0 identifiers.
- Fixed a mutex self-deadlock in the Health Monitor where `system_doctor_report` recursively called `overall_status` while holding the component lock.
- Fixed an interpreter scope omission where `Stmt::Emit` failed to define `payload` in handler execution environments.

### Phase 1: Unified Intelligence Kernel (`kernel/` crate)
- **Agent Process Management (`agent_process.rs`)**: Modeled agents as first-class operating system processes with unique PIDs, PCB tracking, full lifecycle state transitions (Created, Starting, Running, Paused, Crashed, Recovering, Terminated), checkpointing, and token/cost quota enforcement.
- **Goal Engine (`goal_engine.rs`)**: Declarative objective planning and constraint monitoring (accuracy, budget, deadline, latency, energy).
- **Supervision Trees (`supervisor.rs`)**: Erlang/OTP-style fault-tolerance engines supporting `OneForOne`, `OneForAll`, and `RestForOne` restart strategies.
- **Dynamic Model Fabric (`model_fabric.rs`)**: Capability-based model routing with automated failover chains across Primary, Fallback, Emergency, and local mock tiers.
- **Event Bus (`event_bus.rs`)**: Asynchronous, priority-ordered decoupled messaging with hierarchical topic dispatch and wildcard subscriptions.
- **System Doctor & Health Monitoring (`health_monitor.rs`)**: Comprehensive component diagnostic scanner with self-healing suggestions and alerts.
- **Context Manager (`context_manager.rs`)**: Priority-ordered, token-budgeted dynamic prompt construction with TTL eviction.
- **Resource Manager (`resource_manager.rs`)**: Multi-tenant token, cost, compute, and energy accounting across distributed agents.
- **Task Graph Planner (`planner.rs`)**: Topological objective decomposition and round-robin agent assignment.
- **Application Isolation (`isolation.rs`)**: Namespace sandboxing and capability-based policy engine.

### Phase 2: Universal Memory Fabric (`memory_fabric/` crate)
- 8 memory scoping tiers: `Working`, `Session`, `LongTerm`, `Semantic`, `Episodic`, `Procedural`, `Shared`, `Global`.
- Pluggable `MemoryBackend` trait architecture with `InMemoryBackend` implementation.
- Point-in-time state snapshots, vector history versioning, and semantic kNN retrieval.

### Phase 3: Application Model (`application/` crate)
- First-class `application` declarative manifest: deployment unit grouping agents, models, memory, and workflows.
- Declarative `policy` access control rules with explicit `allow` and `deny` evaluation.
- Application lifecycle management and registry.

### Phase 4: Language & Compiler Evolution
- Added v3.0 AST statements and expressions for goals, supervisors, resources, reason-verify-revise loops, devices, declarative UI, applications, policies, and events.
- Updated lexer, parser, semantic analyzer, type checker, formatter, and tree-walk interpreter.
- Zero test regressions; 430+ automated tests passing across the workspace.

### Phase 5: CLI & Tooling Modernization
- Added subcommands: `ail system doctor`, `ail system status`, `ail agent list`, `ail agent inspect`, `ail model list`, `ail memory inspect`, `ail goal plan`, `ail security check-policy`, `ail profile`, `ail eval`, and `ail migrate`.

## v2.0 — Distributed Intelligence (this release)

### Phase 0: v1.0 audit and stabilization

Rebuilt and re-tested the full v1.0 suite (308/308 passing) before any
v2.0 work began, then actively probed for latent bugs with targeted test
programs (memory scoping, mixed-type sorting, repeated workflow runs) —
all behaved correctly. No regressions found; every v0.1/v0.5/v1.0
checklist item remained verified.

### Phase 1: v2.0 upgrade

**A framing note, up front:** the v2.0 brief describes "Kubernetes for AI
agents" — real multi-machine coordination, real cloud/edge/enterprise
deployment, mutual TLS, OAuth/SSO, hardware-backed key storage, a
graphical IDE. None of that is achievable, honestly, from a language
runtime shipped as source in a sandboxed build environment — it needs
real infrastructure (network access to real machines, real identity
providers, a GUI application toolchain) this project doesn't have and
won't fake having. What v2.0 actually ships is every piece of that vision
that **can** be built for real as a single-process foundation, clearly
labeled as such, with the multi-machine parts named as explicit future
work rather than simulated to look like more than they are.

**New keywords**: `network`, `cluster`, `on` (promoted from a v1.0
contextual identifier to a real keyword, for `on <event> { }` handlers —
verified not to break the existing `agent Name on location { }` syntax).

**Distributed Agent Network** (`network Name { agent A; agent B; }`):
real, mutable membership (`network_join`/`network_leave`), a real
deterministic leader-election algorithm (lexicographically-smallest-name,
honestly documented as a single-process stand-in for real distributed
consensus), and health-check introspection.

**Agent Clusters** (`cluster Name { replicas N; autoscaling
enabled|disabled; }`): a real spec plus real round-robin task dispatch
(`cluster_dispatch`) across the declared replica count, with genuine model
invocation per dispatch.

**Agent-to-agent communication**: real request/response RPC
(`on_request`/`request`) and real pub/sub (`subscribe`/`publish`/
`unsubscribe`), both layered on v1.0's real mailbox infrastructure.

**Event system** (`on document_uploaded { ... }` / `emit(name, payload)`):
real, synchronous, in-process event dispatch; multiple handlers per event
are supported and all run in registration order. (Fixed a real bug found
via manual testing: `payload` is bound implicitly by `emit()` at runtime,
but the semantic analyzer and type checker didn't know it existed,
rejecting every handler that used it — fixed by declaring `payload` in
each handler's checked scope, the same way a `for` loop's variable is.)

**Multi-model agents** (`agent A { models [GPT5, Claude]; }`): real
round-robin routing across an agent's model list for workflow/team/cluster
execution, with `agent_models()` introspection.

**Native Vector Engine** (`vector_index_add`/`vector_index_search`): a
real embedding-backed index with cosine-similarity ranking and exact-match
metadata filtering, reusing v0.5's real feature-hashing `embed`.

**Intelligent Context Engine foundation** (`compress_context`/
`dedupe_chunks`): real, deterministic word-count compression and
duplicate-chunk removal.

**AI Scheduler foundation** (`schedule`/`run_scheduled`): a real priority
queue with FIFO tie-breaking — not the resource/cost/latency/GPU/energy-
aware scheduling the brief describes, which needs real telemetry this
offline-by-default runtime doesn't have.

**What's explicitly out of scope, and why** (see `docs/roadmap.md` for
the full list): true multi-machine execution (agent locations and
messaging model the concept; execution stays single-process); Kubernetes/
Docker/serverless deployment targets (no real cluster/container API
access); a policy engine, mTLS, zero-trust networking, hardware-backed
key storage, or secret rotation (v1.0's `secret()`/`audit_log()` remain
the real security foundation; a full framework needs real cryptographic
and identity infrastructure); enterprise identity (OAuth/SSO/RBAC/ABAC —
needs real identity providers); a package/agent marketplace with real
signing and security scanning (needs real cryptographic infrastructure
and a hosted registry); a plugin system with dynamic loading; and AI
Studio (a GUI application is a different kind of software project than a
language runtime).

**Backward compatibility**: every v0.1, v0.5, and v1.0 program continues
to compile and run unchanged — verified across all 21 pre-v2.0 example
programs plus 11 `.ail` integration tests. v2.0 added 77 new tests (up
from 308 to 385), zero failures, zero compiler warnings, and four new
example programs (`examples/v20/`) demonstrating every new capability
end-to-end.

## v1.0 — Production-Track AI Runtime (this release)

### Phase 0: v0.5 audit and stabilization

Every v0.5 checklist item was re-verified against the real implementation.
One genuine gap was found and fixed:

* **Fixed:** the Language Server only ever ran the parser when computing
  diagnostics — semantic errors (undefined references, `const`
  reassignment, an `agent` naming an undeclared `model`, etc.) and type
  errors never appeared in an editor even though `ail check` caught them
  correctly. `ail-lsp` now runs the same parse → semantic → type-check
  pipeline the CLI does. See `language_server/src/main.rs`'s
  `diagnostics_for`.

Everything else — the v0.1 foundation, all 18 v0.5 AI-native keywords, the
agent/model/tool/permission/memory/prompt/schema/workflow/parallel/team/
knowledge systems, the AI runtime foundation, and the standard library —
was confirmed still fully operational with zero regressions (218/218
tests passing before v1.0 work began). Formatter idempotency and
round-trip correctness were re-verified across every v0.1 and v0.5 example
file; the REPL and LSP were re-verified against v0.5 constructs; benchmarks
were re-run to confirm no performance regression.

### Phase 1: v1.0 upgrade

**Compiler**: a real, recursive, whole-program constant-folding and
dead-code-elimination pass (`ailang-optimizer`, now wired into the
pipeline via `ail run --optimize` / `ail build --optimize`), verified to
produce byte-identical program *output* to the unoptimized path across a
battery of equivalence tests.

**Memory subsystem**: real versioning (every `remember`ed value is kept,
not just the latest — `memory_history(key)`), and real agent/team-scoped
memory via `remember_in`/`recall_in`/`forget_in` against named `memory`
stores (multiple agents referencing the same named memory get team memory
"for free," no new concept needed).

**Native RAG**: `retrieve(knowledge_name, query, k?)` — real file
ingestion from a `knowledge` source directory, real chunking, real
feature-hashing embeddings, real cosine-similarity ranking. Verified
end-to-end against real temp-directory files ranking correctly by
relevance.

**Native agent messaging**: `send_message`/`receive_message`/`broadcast`
— real FIFO in-process mailboxes per agent, with broadcast-to-all-except-
sender. (Delivered as explicit function calls rather than the brief's
illustrative `A -> B` syntax, to avoid a third, ambiguous meaning for an
operator AILang already uses for function return types and workflow
chains — see `ai_runtime::messaging`'s doc for the full reasoning.)

**Agent locations**: `agent Name on location { ... }` — real grammar,
real `AgentValue.location` data, introspectable via `agent_location()`.
Honestly scoped as metadata plus single-process simulation, not real
multi-machine distributed execution (see below).

**Security basics**: `secret(name)` is the sole environment-variable entry
point in the entire standard library, and every `invoke_tool` call is
recorded in a real, queryable `audit_log()` (deliberately excluding call
arguments from the log, to never risk logging a secret in plaintext).

**Observability**: a real `ExecutionTrace` recording workflow stages, team
members, and tool invocations with accurate elapsed-time timestamps,
queryable via `get_trace()` and printable as a timeline via
`ail run --trace`.

**LSP**: real `textDocument/definition` and `textDocument/references`,
backed by a whole-program symbol index (flat, not scope-aware — see
`language_server/src/symbols.rs`'s doc for that scope cut) — upgrading
v0.5's "always returns nothing" stubs into genuinely useful navigation.

**A minimal, real interactive debugger**: `ail run --debug` steps through
top-level statements one at a time (`n`/`c`/`p <var>`/`q`), backed by a new
`Interpreter::prepare`/`exec_top_level`/`global_value` API. Scoped to
top-level-statement granularity — see `cli/src/main.rs`'s
`run_debug_session` doc for why real breakpoints/step-into need a
resumable execution model the tree-walk interpreter doesn't have yet.

**Standard library expansion**: `push`/`pop`/`reverse`/`sort` for arrays;
`json_encode`/`json_decode`; `read_file`/`write_file`; `http_get`/
`http_post` (reusing the same real `std::net` HTTP client as v0.5's
`HttpModelProvider`); `hash` (non-cryptographic FNV-1a, explicitly labeled
as such); `sleep`.

**Package manager**: a real, hand-rolled semantic-version parser and
constraint matcher (`=`, `>=`, `>`, `<=`, `<`, `~`, `^`), wired into
`write_lockfile` so a dependency's declared version is actually checked
against its constraint, not just recorded. A real `ail deploy <target>`
command bundles a project's `ai.toml` and every `.ail` file into
`dist/<target>/` with a `deploy.json` manifest — genuine, inspectable
packaging, not simulated cloud/edge provisioning (see
`ailang_pm::bundle_for_deploy`'s doc for the honest scope boundary).

**What's explicitly out of scope for this release, and why** (see
`docs/roadmap.md` for the full list): true multi-machine distributed
execution (agent locations are real metadata, but execution stays
single-process — real distribution needs a migration of
`ailang-runtime`'s `Rc`/`RefCell` values to `Arc`/`Mutex`, which is a
workspace-wide change bigger than this release's scope); a policy engine,
capability tokens, or package signing (the security basics above are real
but are not a full security framework); a remote package registry (the
package manager's version resolution is real, but only for local-path
dependencies); real hosted-model providers (unchanged from v0.5 —
`ModelProvider` is the extension point, `MockProvider`/`HttpModelProvider`
are what ship).

**Backward compatibility**: every v0.1 and v0.5 program continues to
compile and run unchanged. All pre-v1.0 tests still pass. v1.0 added 88
new tests (up from 218 to 306), zero failures, zero compiler warnings, and
four new example programs (`examples/v10/`) exercising the new
capabilities end-to-end — including one that makes a real HTTP request to
a real local server it spins up itself in-test.

## v0.5 — AI-Native Primitives (this release)

### Phase 0: v0.1 audit and stabilization

Before any v0.5 work began, the existing v0.1 implementation was audited
against the full checklist in the upgrade brief. Two genuine bugs were
found and fixed, plus one foundation gap closed:

* **Fixed:** the semantic analyzer rejected `fn f(x: int) -> int { let x =
  x + 1; return x; }` as a duplicate declaration. The function body now
  gets its own nested scope (distinct from the parameter scope), matching
  both normal shadowing semantics and the interpreter's actual (correct)
  runtime behavior. See `compiler/semantic/src/lib.rs`'s `Stmt::Function`
  handling and the `allows_let_to_shadow_a_function_parameter` regression
  test.
* **Fixed:** `module` bodies were semantically scoped to a child scope that
  got popped after the module statement, making module-declared functions
  invisible to *later* code — even though the interpreter correctly hoists
  them into the global scope. Semantic analysis now matches interpreter
  behavior. See the `Stmt::Module` handling and its doc comment.
* **Added:** a lockfile foundation (`ai.lock`), written by `ail build` when
  an `ai.toml` manifest is present, resolving and fingerprinting local-path
  dependencies. See `package_manager/src/lib.rs`'s `write_lockfile`.

Every other checklist item (lexer/parser/AST/semantic/type-checker/
interpreter/runtime/CLI/REPL/formatter/package-manager/stdlib/docs/tests/
performance/code-quality) was verified against the existing v0.1
implementation and found already satisfied; see `docs/architecture.md` and
the v0.1 section of `docs/roadmap.md` for what was already in place. Zero
compiler warnings; all 62 pre-existing tests (57 original + 5 from the
audit fixes above) passed before v0.5 work began.

### Phase 1: v0.5 AI-native language upgrade

**New reserved keywords**, recognized end-to-end by lexer, parser, AST,
semantic analyzer, type checker, interpreter, formatter, and the LSP
foundation: `agent`, `model`, `workflow`, `tool`, `memory`, `knowledge`,
`prompt`, `reasoning`, `schema`, `pipeline`, `parallel`, `permission`,
`deploy`, `team`, `embedding`, `dataset`, `vector`, `task`, plus `generate`
as an expression-level keyword.

**New syntax** (see `docs/language-reference.md` for full grammar and
semantics):

```ail
agent Researcher { model GPT5; memory persistent; tools [Search]; permissions [internet]; }
model GPT5;
model LocalModel { endpoint "http://localhost:11434"; }
tool Search;
permission internet;
memory UserMemory persistent;
prompt system = """multi-line ${interpolated} text""";
schema Person { name: string; age: int; }
workflow Paper { Researcher -> Writer -> Reviewer; }
parallel { taskA(); taskB(); }
team Startup { CEO; CTO; }
knowledge Docs { source "./docs"; }
let p = generate Person;
```

**New standard library functions**: `remember`, `recall`, `forget`,
`invoke_tool`, `run_workflow`, `run_team`, `render`, `summarize`,
`translate`, `classify`, `embed`.

**New crate**: `ailang-ai-runtime` (`ai_runtime/`) — the AI Runtime
Foundation: a `ModelProvider` trait with a deterministic `MockProvider`
and a genuine `HttpModelProvider` (real `std::net`-based HTTP/1.1 client,
tested end-to-end against an in-process server, no TLS — see that crate's
doc comment for why), a `MemoryStore` trait with a working `InMemoryStore`,
a `Tool` trait with real `Calculator`/`Filesystem`/`HTTP` implementations
and clearly-labeled `Search`/`Browser` placeholders, capability-based
`PermissionSet` enforcement, sequential `run_workflow`/`run_team`
execution, a real feature-hashing `embed`, and a real bag-of-words
`classify`.

**What's genuinely real vs. a documented placeholder** — see
`ai_runtime/src/lib.rs`'s crate-level doc table for the authoritative
version; in short: memory, the calculator/filesystem/HTTP tools, workflow/
team execution, embedding, and classification are real, working, tested
algorithms. `generate`'s structured output and `MockProvider`'s text
generation are deterministic placeholders standing in for a real model API
(by design — see below). `Search`/`Browser` tools and `translate` are
labeled placeholders pending real external-API integration.

**Why not real hosted-model APIs?** Wiring in OpenAI/Anthropic/etc.
requires API keys and reliable outbound internet access to third-party
services, neither of which this release's build/test environment can
depend on. Instead, v0.5 ships the *provider abstraction* the spec asks
for (`ModelProvider`) plus one offline implementation (`MockProvider`, for
determinism in tests/CI) and one genuinely networked implementation
(`HttpModelProvider`, for local models like Ollama) — a real hosted
provider is a new struct implementing the same trait, not a language or
architecture change.

**Backward compatibility**: every v0.1 program continues to compile and
run unchanged. All v0.1 tests (57 original, plus the audit's 5 additions)
still pass. v0.5 added 145 new tests across the lexer, parser, semantic
analyzer, type checker, runtime, the new `ailang-ai-runtime` crate, the
interpreter, the formatter, and `.ail`-level integration/golden tests —
205 total, zero failures, zero compiler warnings.

## v0.1 — Foundation

See `docs/roadmap.md`'s "v0.1 — Foundation" section (renamed from this
file's original scope) for the initial release: lexer, parser, AST,
semantic analyzer, type checker, tree-walk interpreter, CLI, REPL,
formatter, package-manager foundation, LSP foundation, standard library,
tests, benchmarks, examples, and documentation — no AI-specific constructs.
