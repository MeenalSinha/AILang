# AILang Roadmap

## v0.1 — Foundation (shipped)

Lexer, parser, AST, semantic analyzer, type checker, tree-walk
interpreter, CLI, REPL, formatter, package-manager foundation, LSP
foundation, standard library, tests, benchmarks, examples, and
documentation — deliberately no AI-specific constructs. See
`CHANGELOG.md` for what shipped and the audit fixes that preceded v0.5.

## v0.5 — AI-Native Primitives (shipped)

Every construct from the v0.5 upgrade — `agent`, `model`, `tool`,
`permission`, `memory`, `prompt`, `schema` + `generate`, `workflow`,
`parallel`, `team`, `knowledge`, and the seven generic placeholder
keywords — implemented end-to-end: lexer, parser, semantic analysis (with
cross-reference validation and hoisting), type checker, interpreter,
formatter, and LSP foundation. See `CHANGELOG.md` for the full list and
`docs/language-reference.md` for grammar and semantics.

## v1.0 — Production-Track AI Runtime (shipped)

Real constant-folding/DCE optimization (`--optimize`), agent/team-scoped
versioned memory, native file-backed RAG retrieval, native in-process
agent messaging, agent location metadata, real secret handling and audit
logging, real execution tracing (`--trace`), a real LSP symbol index
powering go-to-definition/find-references, a minimal real interactive
debugger (`--debug`), a substantially expanded standard library (arrays,
JSON, filesystem, HTTP, hashing, sleep), real semantic-version constraint
checking in the package manager, and a real `ail deploy` bundler. See
`CHANGELOG.md` for the complete list and exactly what's real vs. a
documented scope cut for each.

### Known v1.0 limitations, tracked here rather than hidden

* **No true multi-machine distributed execution.** `agent Name on
  location { ... }` is real, introspectable metadata
  (`agent_location()`), and native agent messaging
  (`send_message`/`receive_message`/`broadcast`) genuinely works — but
  everything still executes in one process, sequentially. Real
  distribution needs `ailang-runtime`'s `Value`/`Environment` migrated
  from `Rc`/`RefCell` to `Arc`/`Mutex` (the same prerequisite `parallel {
  }`'s real concurrency has needed since v0.5), plus actual network
  transport between processes/machines — a workspace-wide runtime change,
  not an incremental addition. This is also why `parallel {}` still runs
  sequentially in v1.0.
* **No policy engine, capability tokens, or package/supply-chain
  signing.** `secret()` and `audit_log()` are real and load-bearing, but
  they're a security *foundation*, not the full "Security Framework" (role-
  based permissions beyond capability names, a policy DSL, cryptographic
  signing, OS-level sandboxing) the original brief describes. That needs
  infrastructure — a policy language, a trust/signing model, a threat
  model to design against — that's a genuinely separate project from a
  language runtime shipped as source.
* **No remote package registry.** Semantic-version constraints are now
  *real* (parsed, compared, enforced against `ai.lock`), but only for
  local-path dependencies, same as v0.1/v0.5. A registry needs hosting,
  auth, and a trust model — out of scope for this release.
* **No real hosted model providers.** Unchanged from v0.5: `ModelProvider`
  is the extension point; `MockProvider` (deterministic, offline) and
  `HttpModelProvider` (real local-network HTTP) are what ship. Adding
  OpenAI/Anthropic/etc. is a new struct implementing the existing trait,
  not a language change — but it needs API keys and reliable third-party
  network access this sandboxed build/test environment can't depend on.
* **The debugger is statement-level, not expression-level.** `ail run
  --debug` steps between top-level statements; it can't step *into* a
  function call or pause mid-expression, because the tree-walk
  interpreter's `exec_stmt`/`eval_expr` are plain recursive Rust calls,
  not a resumable state machine. A bytecode VM (see below) would make
  real step-into practical.
* **The LSP symbol index is flat, not scope-aware.** Go-to-definition and
  find-references work well for the common case (most names are unique in
  a small-to-medium program) but don't disambiguate two different `x`
  variables in two different functions. Real scope-aware resolution needs
  `ailang-semantic`'s analyzer to expose a proper "what does this
  identifier at this position refer to" query API, not just diagnostics.
* **`generate` still always uses `MockProvider`.** No `generate Person
  using SomeModel` syntax yet.
* **Workflows are still a linear chain**, not a branching/joining DAG.
* **This release's toolchain constraint** (inherited from v0.1/v0.5, still
  true): dependencies are pinned to versions that build under Rust 1.75.
  Every new v1.0 capability that needed real I/O (`HttpModelProvider`-based
  `http_get`/`http_post`, file I/O, hashing, semver parsing) was
  deliberately built on `std` alone rather than adding a new instance of
  this problem.

## v2.0 — Distributed Intelligence (shipped)

`network`/`cluster` declarations, real dynamic network membership and
deterministic leader election, real round-robin cluster task dispatch,
real request/response RPC and pub/sub agent communication, a real
synchronous event system, real multi-model round-robin routing, a real
embedding-backed vector engine with metadata filtering, and real context
compression/deduplication. See `CHANGELOG.md` for the complete list and
the explicit framing note on why true multi-machine distribution,
Kubernetes/cloud deployment, a full security framework, enterprise
identity, and a GUI IDE are named as future work rather than simulated.

### Known v2.0 limitations, tracked here rather than hidden

* **Still single-process.** `network`/`cluster`/agent locations model
  distributed concepts with real data structures and algorithms, but
  execution never leaves one process. Real distribution needs the
  `Rc`/`RefCell` → `Arc`/`Mutex` runtime migration flagged since v1.0,
  plus actual network transport between processes/machines.
* **Leader election and health checks are deterministic simulations,
  not real distributed consensus.** `elect_leader` picks the
  lexicographically smallest member name — reproducible and real as an
  algorithm, but not Raft/Paxos, because there's only one process to
  reach consensus *across*.
* **`cluster_dispatch`'s "replica" is a rotation label, not a second
  process.** Round-robin selection and the resulting model invocation are
  both real; nothing is actually running on a second CPU core or machine.
* **The scheduler is priority-only.** No resource/cost/latency/GPU/
  energy-awareness — that needs real telemetry (actual resource usage,
  actual provider cost/latency data) this offline-by-default foundation
  doesn't have.
* **The vector engine is in-memory, single-backend.** No persistence
  across process runs, no pluggable embedding models (always the
  feature-hashing `embed`), no ANN index for large-scale search (linear
  scan, fine at the scale this foundation targets).
* **No real deployment targets.** `ail deploy` bundles a project and
  writes a manifest (see v1.0); it does not provision Kubernetes, Docker,
  serverless, or bare-metal infrastructure.
* **No policy engine, mTLS, zero-trust networking, hardware-backed key
  storage, or secret rotation.** v1.0's `secret()`/`audit_log()` remain
  the real (if foundational) security surface.
* **No enterprise identity** (organizations/teams/users/roles/OAuth/SSO/
  RBAC/ABAC) — needs real identity provider integration.
* **No package/agent marketplace, signing, or security scanning** — needs
  real cryptographic and hosted-registry infrastructure; the package
  manager's real semver checking (v1.0) remains local-only.
* **No plugin system, and no AI Studio GUI** — both are substantial,
  separate engineering efforts (a stable dynamic-loading plugin ABI; a
  full graphical application) out of scope for this release.
* **This release's toolchain constraint** (inherited, still true):
  everything new in v2.0 — the vector engine, scheduler, event system,
  network/cluster machinery — is built on `std` plus the crates already
  in the dependency tree, adding no new toolchain-compatibility risk.

## v2.1+ — Future Work

Roughly in order of "most requested, most achievable next":

* **Real multi-machine distributed execution**, building on the
  `Arc`/`Mutex` migration prerequisite — `network`/`cluster`/agent
  messaging already provide the language-level model to extend.
* **Real deployment targets** (actual Kubernetes/Docker/serverless
  provisioning) once there's a real execution target to provision *for*.
* **A policy engine and capability tokens**, building on `secret()`/
  `audit_log()`.
* **A pluggable embedding backend** for the vector engine, and an ANN
  index for larger-scale search.
* **Resource/cost/latency-aware scheduling**, once there's real
  multi-provider telemetry to schedule against.
* **Real hosted model providers** behind `ModelProvider`.
* **A real bytecode VM or JIT**, sitting alongside the tree-walk
  interpreter (consuming the same `ailang_ast::Program`), unlocking real
  step-into debugging and better throughput.
* **Branching/joining DAG workflows** with real graph-cycle detection,
  replacing the linear-chain grammar.
* **A remote package registry**, scope-aware LSP navigation, real
  module-path resolution, a comment-preserving formatter, generics, and
  incremental compilation — all carried over unresolved from earlier
  releases' roadmaps.

## Toolchain follow-ups (not language features)

Re-introduce `tower-lsp` + `tokio` in `language_server/`, `rustyline` (or
`reedline`) in `repl/`, `criterion` in `benches/`, and `insta` in
`cli/tests/golden.rs` once the project's minimum-supported Rust version is
bumped past whatever stabilizes Cargo's `edition2024` feature. Every new
v1.0 capability that could have added a similar dependency-compatibility
risk (`ailang-ai-runtime`'s HTTP client, the package manager's semver
parser, JSON encode/decode) was deliberately built on `std` (plus the
already-present `serde`/`serde_json`/`toml`) instead, so this list hasn't
grown.
