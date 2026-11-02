# AILang

**An Agentic Systems Programming Language where AI is a first-class
programming concept instead of a library.**

**v2.0** — real distributed-intelligence primitives: agent networks with
dynamic membership and leader election, agent clusters with round-robin
dispatch, real request/response and pub/sub agent communication, a real
event system, real multi-model routing, a real embedding-backed vector
engine, and real context compression — all genuinely working,
single-process implementations of the concepts they name. See
[`CHANGELOG.md`](./CHANGELOG.md) for the complete v0.1 → v2.0 history and
[`docs/roadmap.md`](./docs/roadmap.md) for what's next (true multi-machine
execution, real cloud deployment, a full security framework, enterprise
identity, a GUI IDE).

```ail
agent Planner on cloud {} agent Researcher on edge {} agent Reviewer {}

network Enterprise {
    agent Planner; agent Researcher; agent Reviewer;
}

cluster Workers { replicas 4; autoscaling enabled; }

on task_completed {
    println("done: " + payload);
}

println("leader: " + network_leader("Enterprise"));
let result = cluster_dispatch("Workers", "Reviewer", "review this PR");
emit("task_completed", "PR #42 reviewed");
```

```text
$ ail run examples/v20/01_distributed_agent_network.ail
Members: ["Planner", "Researcher", "Engineer", "Reviewer"]
Leader: Engineer
...
```

## What's real vs. an explicit scope cut

Every "distributed intelligence" capability in this project is labeled
honestly — see [`ai_runtime/src/lib.rs`](./ai_runtime/src/lib.rs)'s
crate-level doc table for the authoritative version, and
[`CHANGELOG.md`](./CHANGELOG.md)'s v2.0 entry for the framing. In short:
network membership, leader election, cluster dispatch, event handling,
request/response, pub/sub, multi-model routing, vector search, and
context compression are all **real, working, tested** algorithms — not
stubs. What's explicitly **not** in this release, named rather than
faked: true multi-machine execution (everything still runs in one
process), real Kubernetes/cloud/edge provisioning, a full security
framework (mTLS, zero-trust, hardware key storage, secret rotation),
enterprise identity (OAuth/SSO/RBAC), a package/agent marketplace with
real signing, a plugin system, and a graphical IDE — each needs real
infrastructure (actual machines, actual identity providers, a GUI
toolchain) this project doesn't have and won't simulate having.

## What's in this release

| Area | Status |
|---|---|
| Lexer, parser, AST, semantic analysis, type checker | ✅ Full v0.1–v2.0 grammar, zero regressions |
| Interpreter | ✅ Executes every construct across all four releases |
| Compiler optimizer | ✅ Real constant-folding + DCE, `--optimize` |
| **AI Runtime Foundation** (`ailang-ai-runtime`) | ✅ Providers, memory, tools, workflow/team, RAG, messaging, security, tracing, **networks, clusters, events, vector engine, context engine, scheduler (v2.0)** |
| Standard library | ✅ ~50 functions across v0.1–v2.0, all real implementations or clearly labeled placeholders |
| CLI (`ail`) | ✅ `run build check fmt repl test init new version deploy`; `--optimize`/`--trace`/`--debug` |
| REPL | ✅ Persistent session state, full keyword/builtin completion |
| Formatter | ✅ Canonical, idempotent for every construct including v2.0 additions |
| Package manager | ✅ `ai.toml` + `ai.lock` + real semver constraint checking + `ail deploy` |
| Language Server | ✅ Diagnostics (parse+semantic+types), hover, real go-to-definition/find-references, completion |
| Debugger | ✅ Minimal real interactive stepper (`ail run --debug`) |
| Tests | ✅ 385 tests across every crate, 23 golden tests, 11 `.ail` integration tests — all passing, zero warnings |
| Examples | ✅ 10 v0.1 + 10 v0.5 + 4 v1.0 + 4 v2.0 = 28 example programs |

## Quick start

```sh
cargo build --release
./target/release/ail run examples/v20/01_distributed_agent_network.ail
```

See [`docs/getting-started.md`](./docs/getting-started.md) for the full
walkthrough.

## Documentation

* [`docs/getting-started.md`](./docs/getting-started.md) — install, run, REPL, tests
* [`docs/language-reference.md`](./docs/language-reference.md) — full syntax & semantics, including v2.0's networks/clusters/events/vector-engine sections
* [`docs/architecture.md`](./docs/architecture.md) — how the compiler pipeline and AI runtime are structured, and why
* [`docs/roadmap.md`](./docs/roadmap.md) — known v2.0 limitations and what's next
* [`CHANGELOG.md`](./CHANGELOG.md) — the complete v0.1 → v2.0 history, every audit finding included
* [`CONTRIBUTING.md`](./CONTRIBUTING.md) — coding conventions, how to add a new construct

## Repository layout

```text
ailang/
├── compiler/
│   ├── diagnostics/ ast/ lexer/ parser/ semantic/ types/ runtime/ interpreter/
│   └── optimizer/       real constant-folding + DCE, wired via --optimize
├── ai_runtime/           AI Runtime Foundation: providers, memory, tools,
│                         workflows, RAG, messaging, security, tracing,
│                         networks, clusters, vector engine, scheduler
├── std/                  stdlib documentation (stdlib is native, not .ail)
├── cli/                  the `ail` binary + golden tests (cli/tests/)
├── repl/                 interactive REPL
├── formatter/             `ail fmt` implementation
├── package_manager/       ai.toml + ai.lock + semver + deploy
├── language_server/       `ail-lsp` binary + real symbol index
├── benches/               std::time-based benchmarks
├── examples/               10 v0.1 examples
│   ├── v05/                 10 v0.5 AI-native examples
│   ├── v10/                 4 v1.0 production-track examples
│   └── v20/                 4 v2.0 distributed-intelligence examples
├── tests/
│   ├── integration/          .ail files run via `ail test` (v0.1–v2.0)
│   └── golden/                fixture outputs, incl. golden/v05/, v10/, v20/
├── docs/                   architecture, roadmap, language reference, getting started
├── CHANGELOG.md              full v0.1 → v2.0 history
├── ai.toml                   this project's own AILang-level manifest
├── Cargo.toml                the Rust workspace
└── README.md
```

## Building and testing

```sh
cargo build --workspace              # everything
cargo test --workspace               # 385 unit/integration tests across every crate
./target/debug/ail test tests/integration   # 11 .ail integration tests (v0.1–v2.0)
cargo run -p ailang-benches --release       # lex/parse/interpret benchmarks
```

### A note on the pinned toolchain

This build targets Rust 1.75 for reproducibility. Dependencies that would
otherwise be the obvious choice — `tower-lsp`/`tokio` for the language
server, `rustyline` for the REPL, `criterion` for benchmarks, `insta` for
snapshot tests — were swapped for small, dependency-light, hand-rolled
equivalents because their current versions require a newer Rust edition
than 1.75 supports. Every release since has held the same line: v2.0's
vector engine, scheduler, and network/cluster machinery are all built on
`std` plus the crates already in the dependency tree, adding no new
toolchain-compatibility risk. Each swap is documented at the top of the
relevant file, along with exactly what re-introducing the "real"
dependency looks like on a newer toolchain — in every case it's a
self-contained, single-crate change.

## License

MIT — see [`LICENSE`](./LICENSE).
