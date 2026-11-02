# AILang v0.1 Language Reference

This describes the language as actually implemented by
`compiler/lexer`, `compiler/parser`, `compiler/semantic`, `compiler/types`,
and `compiler/interpreter` — every example below has a corresponding test
in one of those crates or in `tests/integration/`.

## Lexical structure

* **Comments:** `// line comment` and `/* block comment */` (non-nested).
* **Identifiers:** `[A-Za-z_][A-Za-z0-9_]*`.
* **Whitespace:** insignificant (no Python-style indentation rules; blocks
  use braces).
* **Semicolons** terminate statements.

## Literals

| Type | Syntax | Notes |
|---|---|---|
| `int` | `42`, `0` | 64-bit signed (`i64`) |
| `float` | `3.14`, `0.5` | 64-bit (`f64`); note `1.` and `.5` are **not** valid — both digit groups are required |
| `bool` | `true`, `false` | |
| `string` | `"hello\n"` | Escapes: `\n \t \r \\ \" \'` |
| `char` | `'a'` | Single character |
| `null` | `null` | Also the sole value of type `null` |

## Variables

```ail
let x = 10;
const PI = 3.14159;
let y: int = 5;       // explicit type annotation
let z;                 // no initializer -> semantic error for `const`,
                        // runtime value `null` for `let`
```

`const` requires an initializer (checked by `ailang-semantic`, error
`S004`) and cannot be reassigned (checked at runtime, and by
`ailang-semantic`'s `S007` when the target is a plain identifier).

## Types

Primitives: `int`, `float`, `bool`, `string`, `char`, `null`, `void`.

Compound type *annotations* (not yet full generic types, see
`docs/roadmap.md`):

```ail
let numbers: [int] = [1, 2, 3];
let ages: {string: int} = {"Alice": 20};
```

Function types: `fn(int, int) -> int`.

Type checking (`ailang-types`) infers `let` types from initializers when no
annotation is given, and checks:

* `if`/`while` conditions must be `bool`
* function call arity and argument types
* array/map indexing operand types
* operator operand types (`+ - * / %`, comparisons, logical `&& ||`)

Names it can't resolve (forward references to constructs the checker
doesn't model, like unregistered stdlib overloads) degrade to
`Type::Unknown` and are **not** treated as errors — see
`compiler/types/src/lib.rs`'s module doc.

## Operators

| Category | Operators |
|---|---|
| Arithmetic | `+ - * / %` |
| Comparison | `== != > < >= <=` |
| Logical | `&& \|\| !` (short-circuit) |
| Assignment | `= += -= *= /=` |

Precedence (highest to lowest): unary (`- !`) > `* / %` > `+ -` >
comparisons > `==`/`!=` > `&&` > `||` > assignment.

`+` on two `string`s concatenates; `+` between a `string` and any other
value stringifies the other side (`"x = " + 5` => `"x = 5"`).

## Control flow

```ail
if age >= 18 {
    print("Adult");
} else if age >= 13 {
    print("Teen");
} else {
    print("Kid");
}

while running {
    update();
}

for i in 0..10 {      // exclusive: 0..9
    print(i);
}
for i in 0..=10 {     // inclusive: 0..10
    print(i);
}
```

`break` and `continue` are supported inside `while`/`for`; using them
outside a loop is a semantic error (`S006`).

## Functions

```ail
fn add(a: int, b: int) -> int {
    return a + b;
}
```

* Parameters and return types are optional annotations (an unannotated
  parameter type-checks as `Unknown`).
* Functions are values (`Value::Function` at runtime) bound in the
  enclosing scope; recursion and forward/mutual recursion both work
  because function declarations are hoisted before execution
  (`Interpreter::hoist_functions`, mirrored by
  `SemanticAnalyzer::hoist_functions`).
* `return` outside a function is a semantic error (`S005`).

## Collections

```ail
let numbers = [1, 2, 3];
numbers[0] = 100;               // mutation
let n = len(numbers);

let ages = {"Alice": 20, "Bob": 25};
ages["Charlie"] = 30;            // insert-or-update
let a = ages["Alice"];
```

Arrays and maps have reference semantics (they're `Rc<RefCell<..>>` under
the hood — see `ailang-runtime`), so assigning an array to another variable
and mutating one is visible through the other. Map lookup in v0.1 is
`O(n)` (see `ailang_runtime::MapRef`'s doc comment for why, and what a
hash-based upgrade looks like).

## Modules and imports

```ail
import std.io;

module math {
    fn square(x: int) -> int {
        return x * x;
    }
}

println(square(5)); // note: NOT math.square(5) — see limitation below
```

**v0.1 limitation:** `module` hoists its members into the *enclosing*
scope rather than creating a real namespace, so `math.square(5)` (qualified
access) doesn't work yet — call `square(5)` directly. `import` statements
are parsed and checked for use (unused imports warn, code `S001`) but don't
yet resolve real files — see `docs/roadmap.md`.

## Standard library

Always in scope, no `import` required:

| Function | Signature | Notes |
|---|---|---|
| `print(...)` | `(any...) -> void` | writes each argument with no separator or trailing newline |
| `println(...)` | `(any...) -> void` | like `print`, plus a trailing newline |
| `input(prompt?)` | `(string?) -> string` | optionally writes `prompt` first; reads one line from stdin (trailing newline stripped) |
| `len(x)` | `(string \| array \| map) -> int` | character count / element count / entry count |
| `type(x)` | `(any) -> string` | one of `"int" "float" "bool" "string" "char" "null" "void" "array" "map" "function"` |
| `exit(code?)` | `(int?) -> never` | flushes output, exits the process (default code `0`) |
| `assert(cond, msg?)` | `(bool, string?) -> void` | raises a runtime error with `msg` (default `"assertion failed"`) if `cond` is falsy — this is what `ail test` uses to determine pass/fail |
| `clock()` | `() -> float` | seconds since the Unix epoch |
| `to_string(x)` | `(any) -> string` | uses the same formatting as `print` |
| `parse_int(s)` | `(string) -> int` | errors on malformed input |
| `parse_float(s)` | `(string) -> float` | errors on malformed input |

See `compiler/interpreter/src/stdlib.rs` for the exact implementations.

## Truthiness

Used by `if`/`while`/`&&`/`\|\|`/`!`: `false`, `null`, `void`, `0`, `0.0`,
`""`, `[]`, and `{}` are falsy; everything else (including any function
value) is truthy.

## v0.5: AI-native constructs

Everything below was added in v0.5. `agent model workflow tool memory
knowledge prompt reasoning schema pipeline parallel permission deploy team
embedding dataset vector task` and `generate` are now real keywords (no
longer just reserved names — see `CHANGELOG.md`).

### Agents, models, tools, permissions

```ail
model GPT5;
model LocalModel {
    endpoint "http://localhost:11434/api/generate";
}

tool Search;
permission internet;

agent Researcher {
    model GPT5;              // references a declared `model`
    memory persistent;       // or `temporary`, or a named `memory Name;`
    tools [Search];          // each must be a declared `tool`
    permissions [internet];  // each must be a declared `permission`
}
```

All four `agent` fields are optional and order-independent. Every name an
`agent` references (`model`, named `memory`, each `tool`, each
`permission`) must be declared *somewhere* in the program — declaration
order doesn't matter (everything is hoisted before cross-reference
checking runs) — checked by `ailang-semantic` (error `S011`).

A `model` with no `endpoint` is backed by a deterministic
`MockProvider` (no network access, safe for tests/CI). A `model` with an
`endpoint` is backed by a real `std::net`-based HTTP client talking
plain `http://` (no TLS) — see `ai_runtime::HttpModelProvider`.

Calling a tool: `invoke_tool(agent_name: string, tool_name: string,
...args) -> value`. The agent must have declared that tool in its `tools`
list. Built-in tools (matched by exact name): `Calculator` (real arithmetic
evaluation, no permission needed), `Filesystem` (real file reads, needs
`filesystem`), `HTTP` (real GET requests, needs `internet`), `Search` and
`Browser` (labeled placeholders, need `internet`). If `tool_name` isn't a
built-in, `invoke_tool` falls back to calling an ordinary `fn` of the same
name as a **custom tool** — no extra syntax required:

```ail
tool Doubler;
agent A { tools [Doubler]; }
fn Doubler(x: int) -> int { return x * 2; }
invoke_tool("A", "Doubler", 21) // => 42
```

### Memory

```ail
remember("favorite_drink", "coffee");
recall("favorite_drink");   // "coffee" (exact key match)
recall("favorite");         // substring match against keys or values;
                             // most-recently-remembered match wins on a tie
forget("favorite_drink");   // recall now returns null

memory UserMemory persistent;
memory SessionMemory temporary;
```

`remember`/`recall`/`forget` operate on one global in-memory store in
v0.5 (agent-scoped memory routing via a named `memory` declaration is
tracked in `docs/roadmap.md`).

### Prompts

```ail
let topic = "quantum computing";
prompt system = "You are an expert researcher. Topic: ${topic}";
prompt long_form = """
Multi-line templates use triple quotes.
Topic: ${topic}
""";

render("system") // => "You are an expert researcher. Topic: quantum computing"
```

`${name}` is replaced with `name`'s current value (stringified) at
`render()` time, looked up in the caller's scope — so the same prompt can
be rendered repeatedly against different bindings. An interpolation target
that isn't defined anywhere in the program is a compile-time warning
(`S012`); one that's defined but not yet assigned when `render()` runs
substitutes an empty string.

### Schemas and `generate`

```ail
schema Person {
    name: string;
    age: int;
}

let p = generate Person;
// p == {"name": "", "age": 0} — deterministic type-appropriate defaults
```

`generate SchemaName` always uses the deterministic `MockProvider` in
v0.5 — there's no real model-backed structured generation yet (see
`docs/roadmap.md`). The result is a `map` value, indexable by field name.

### Workflows and teams

```ail
agent Researcher {} agent Writer {} agent Reviewer {}

workflow Paper {
    Researcher -> Writer -> Reviewer;
}

let results = run_workflow("Paper", "the history of tea");
// results == [{"stage": "Researcher", "output": "..."}, {"stage": "Writer", "output": "..."}, ...]
// each stage's output becomes the next stage's input prompt
```

```ail
team Startup {
    CEO;
    CTO;
}

let results = run_team("Startup", "should we pivot?");
// results == [{"member": "CEO", "output": "..."}, {"member": "CTO", "output": "..."}]
// every member runs against the SAME input (fan-out, not a pipeline)
```

Every stage/member name must be a declared `agent` (checked by
`ailang-semantic`); a workflow chain may not visit the same agent twice
(the one "cycle" shape a linear chain can express — error `S010`).

### Parallel execution

```ail
parallel {
    task_a();
    task_b();
    task_c();
}
```

v0.5 executes the statements in a `parallel` block **sequentially**, not
concurrently (see `docs/roadmap.md` for why). What's real: every statement
runs even if an earlier one raises a runtime error — unlike a plain `{ }`
block, which stops at the first error — with the first error (if any)
reported only after everything has run.

### Knowledge

```ail
knowledge Docs {
    source "./docs";
}
```

Registers a named knowledge source. Groundwork only in v0.5 — no indexing
or retrieval yet (see `docs/roadmap.md`).

### Generic placeholder declarations

`reasoning`, `embedding`, `dataset`, `vector`, `pipeline`, `task`, and
`deploy` all parse and execute (binding a named placeholder value you can
call `type()` on) but have no further behavior yet:

```ail
reasoning ChainOfThought;
dataset Reviews { source "./reviews.csv"; }
```

### New standard library functions

| Function | Signature | Status |
|---|---|---|
| `remember(key, value)` | `(string, string) -> void` | Real |
| `recall(query)` | `(string) -> string \| null` | Real |
| `forget(key)` | `(string) -> void` | Real |
| `invoke_tool(agent, tool, ...)` | `(string, string, any...) -> value` | Real for built-ins and custom tools; see above |
| `run_workflow(name, input)` | `(string, string) -> [{string: string}]` | Real, sequential |
| `run_team(name, input)` | `(string, string) -> [{string: string}]` | Real, sequential fan-out |
| `render(prompt_name)` | `(string) -> string` | Real |
| `summarize(text)` | `(string) -> string` | Real (first-sentence heuristic; see `docs/roadmap.md`) |
| `classify(text, categories)` | `(string, [string]) -> string \| null` | Real (bag-of-words overlap) |
| `embed(text)` | `(string) -> [float]` | Real (feature hashing, 8 dimensions) |
| `translate(text, lang)` | `(string, string) -> string` | Placeholder — tagged passthrough, not a real translation |

## Error handling and diagnostics

There is no `try`/`catch`. Runtime errors (division by zero, index out of
bounds, calling a non-function, wrong argument count, `const`
reassignment, missing tool permission, unreachable local model endpoint,
etc.) abort execution with a `RuntimeError` carrying the source `Span` of
the failing expression, rendered the same way compile-time diagnostics are
(see `docs/architecture.md`).

## v1.0: agent locations, messaging, memory, security, and stdlib expansion

### Agent locations

```ail
agent Planner on cloud { model GPT5; }
agent Researcher on edge {}
agent Reviewer on local {}

println(agent_location("Planner")); // "cloud"
```

`on <location>` is optional and free-form (`cloud`/`edge`/`local` are
conventions, not reserved words — any identifier works). It's real,
introspectable metadata; execution still happens in a single process (see
`docs/roadmap.md` for why real multi-machine distribution is future
work). `agent_location(name) -> string | null`.

### Native agent messaging

```ail
agent Researcher {} agent Reviewer {}

send_message("Researcher", "Reviewer", "draft ready");
let msg = receive_message("Reviewer");
// msg == {"from": "Researcher", "text": "draft ready", "timestamp": 1234567890.123}

broadcast("Researcher", "standup in 5 minutes"); // reaches every OTHER declared agent
```

Real FIFO mailboxes per agent, delivered in send order.
`receive_message` returns `null` when a mailbox is empty. Delivered via
explicit function calls rather than an `A -> B` operator — see
`CHANGELOG.md`'s v1.0 entry for why.

### Agent/team-scoped, versioned memory

```ail
memory UserMemory persistent;
remember_in("UserMemory", "favorite_drink", "coffee");
recall_in("UserMemory", "favorite_drink"); // "coffee"
forget_in("UserMemory", "favorite_drink");

// Multiple agents referencing the same named memory share it — this IS
// "team memory," with no separate concept needed:
agent CEO { memory UserMemory; }
agent CTO { memory UserMemory; }
```

The original `remember`/`recall`/`forget` (operating on one global store)
are unchanged and still work exactly as in v0.5. New in v1.0:
`remember_in`/`recall_in`/`forget_in` take a memory name as their first
argument, and `memory_history(key) -> [{value, timestamp}]` returns every
value ever `remember`ed under `key` (the global store), oldest first —
real versioning, not just overwrite-in-place.

### Native RAG (`retrieve`)

```ail
knowledge Docs { source "./docs"; }

let results = retrieve("Docs", "how does the borrow checker work", 3);
// [{"source": "rust.md", "text": "...", "score": 0.73}, ...]
```

Real: reads every file directly inside the source directory (non-
recursive), splits each into ~200-word chunks, embeds each chunk with the
same real feature-hashing `embed()` from v0.5, and ranks by real cosine
similarity. `k` (default 3) caps the number of results.

### Security: `secret` and `audit_log`

```ail
let api_key = secret("OPENAI_API_KEY"); // reads an env var, or null if unset
```

`secret` is the **only** way an AILang program reads the host process's
environment — there's no other env-var access anywhere in the standard
library, so "where did this value come from" always has one answer.

```ail
tool Filesystem; permission filesystem;
agent Sandboxed { tools [Filesystem]; } // no permissions granted

invoke_tool("Sandboxed", "Filesystem", "/etc/hosts"); // raises a runtime error

let log = audit_log();
// [{"timestamp": ..., "agent": "Sandboxed", "tool": "Filesystem", "permission": "filesystem", "granted": false}]
```

Every `invoke_tool` call — granted or denied — is recorded. Call
*arguments* are deliberately never logged (there's no way to know if one
might be a secret), only the who/what/when/allowed metadata.

### Observability: execution tracing

```ail
let trace = get_trace();
// [{"kind": "workflow_stage", "label": "Researcher", "elapsed_seconds": 0.0012}, ...]
```

Records `workflow_stage`, `team_member`, and `tool_invocation` events as
they happen. `ail run --trace` prints the same data as a readable
timeline after the program finishes.

### Standard library additions

| Function | Signature | Notes |
|---|---|---|
| `push(array, value)` | `([T], T) -> int` | appends in place, returns new length |
| `pop(array)` | `([T]) -> T \| null` | removes and returns the last element |
| `reverse(array)` | `([T]) -> void` | reverses in place |
| `sort(array)` | `([T]) -> void` | in-place sort; errors on mixed/incomparable element types |
| `json_encode(value)` | `(any) -> string` | errors on functions or AI-native values (agent, model, ...) |
| `json_decode(text)` | `(string) -> any` | errors on invalid JSON |
| `read_file(path)` | `(string) -> string` | real file read |
| `write_file(path, content)` | `(string, string) -> void` | real file write |
| `http_get(url)` | `(string) -> string` | real HTTP/1.1 GET, `http://` only (see v0.5's `HttpModelProvider`) |
| `http_post(url, body)` | `(string, string) -> string` | real HTTP/1.1 POST |
| `hash(value)` | `(any) -> string` | non-cryptographic FNV-1a; explicitly not for security-sensitive use |
| `sleep(seconds)` | `(int \| float) -> void` | real thread sleep |
| `memory_history(key)` | `(string) -> [{value, timestamp}]` | see memory section above |
| `remember_in`/`recall_in`/`forget_in` | see memory section above | |
| `send_message`/`receive_message`/`broadcast` | see messaging section above | |
| `agent_location(name)` | `(string) -> string \| null` | |
| `secret(name)` | `(string) -> string \| null` | |
| `audit_log()` | `() -> [{...}]` | |
| `get_trace()` | `() -> [{...}]` | |

### CLI additions

```sh
ail run --optimize file.ail   # constant-fold + dead-code-eliminate before running
ail run --trace file.ail      # print an execution timeline after running
ail run --debug file.ail      # step through top-level statements interactively
ail deploy <target>           # bundle ai.toml + every .ail file into dist/<target>/
```

## v2.0: networks, clusters, events, and the distributed-intelligence stdlib

**Framing note**: every construct below is a real, working, single-process
implementation of the described concept — not a simulation dressed up to
look like real multi-machine distribution. See `CHANGELOG.md`'s v2.0
entry and `docs/roadmap.md` for exactly what's real vs. explicitly future
work (true multi-machine execution, real Kubernetes/cloud deployment, a
full security framework, enterprise identity, a GUI IDE).

### Networks

```ail
agent Planner {} agent Researcher {} agent Engineer {} agent Reviewer {}

network Enterprise {
    agent Planner;
    agent Researcher;
    agent Engineer;
    agent Reviewer;
}

network_members("Enterprise")  // ["Planner", "Researcher", "Engineer", "Reviewer"]
network_leader("Enterprise")   // "Engineer" — lexicographically smallest name
network_health("Enterprise")   // [{"member": "Planner", "healthy": true}, ...]

network_join("Enterprise", "Intern");   // real, mutable membership
network_leave("Enterprise", "Reviewer");
```

`network_leader` uses a real, deterministic algorithm (lexicographically
smallest member name) — a genuine computation, but a single-process stand-
in for real distributed leader election (Raft/Paxos need actual multi-
node communication). `network_health` always reports every member
healthy, since there's nothing to actually be unhealthy in one process.

### Clusters

```ail
cluster AIWorkers {
    replicas 100;
    autoscaling enabled;
}

cluster_status("AIWorkers") // {"replicas": 100, "autoscaling": true}

let result = cluster_dispatch("AIWorkers", "SomeAgent", "input text");
// {"replica": 3, "output": "..."} — real round-robin replica selection,
// real model invocation via SomeAgent's provider
```

`autoscaling` is `enabled` or `disabled` (not an expression). `replicas`
must evaluate to an `int`. `cluster_dispatch` genuinely rotates through
`0..replicas` and genuinely invokes a model; "replica N" is a rotation
label, not evidence of a second process.

### Events

```ail
on document_uploaded {
    println("indexing " + payload);
}
on document_uploaded {
    println("notifying about " + payload);
}

let handler_count = emit("document_uploaded", "report.pdf");
// runs BOTH handlers, in registration order; handler_count == 2
```

`payload` is bound automatically inside every `on <event>` handler body —
it's whatever `emit`'s second argument was, not something you declare
yourself. Multiple handlers for the same event name are valid and all
run. `emit` returns how many handlers ran (`0` if none are registered for
that name).

### Agent-to-agent communication

```ail
// Request/response
fn answer(question: string) -> string { return "42"; }
on_request("DeepThought", "answer");
request("Human", "DeepThought", "the ultimate question") // "42"

// Publish/subscribe
subscribe("alerts", "OnCallEngineer");
publish("alerts", "service degraded");
receive_message("OnCallEngineer") // {"from": "topic:alerts", "text": "service degraded", ...}
```

`request` calls whatever function was registered via `on_request` for the
target name and returns its result directly (a real synchronous RPC
call), erroring if nothing is registered. `subscribe`/`publish`/
`unsubscribe` are layered on the same real mailboxes `send_message`/
`receive_message` use from v1.0 — a `publish`ed message is just delivered
to every current subscriber's mailbox.

### Multi-model agents

```ail
model GPT5; model Claude;

agent Analyst {
    models [GPT5, Claude];
}

agent_models("Analyst") // ["GPT5", "Claude"]
```

When an agent has a `models` list, `run_workflow`/`run_team`/
`cluster_dispatch` round-robin across it (call 1 uses `models[0]`, call 2
uses `models[1]`, wrapping around) instead of using a single `model`.
This is real rotation, not capability/latency/cost-aware routing (which
needs real per-provider telemetry this foundation doesn't have).

### Native vector engine

```ail
vector_index_add("Docs", "1", "rust systems programming", {"lang": "en"});
vector_index_add("Docs", "2", "baking bread recipes", {"lang": "en"});

let results = vector_index_search("Docs", "systems programming languages", 3, {});
// [{"id": "1", "text": "...", "score": 0.8, "metadata": {...}}, ...]
// ranked by real cosine similarity of real feature-hashing embeddings

vector_index_search("Docs", "rust", 3, {"lang": "en"}) // metadata-filtered
```

In-memory, single embedding backend (the same `embed()` used elsewhere),
linear-scan search — real for the scale this foundation targets, not a
production ANN index.

### Context engine

```ail
compress_context("a very long piece of text here", 3) // "a very long..."
dedupe_chunks(["x", "x", "y"]) // ["x", "y"]
```

Real, deterministic word-count truncation and exact/near-duplicate
(case/whitespace-insensitive) removal — not LLM-judged semantic chunking
or prioritization.

### Priority scheduler

```ail
fn low_priority_task() -> string { return "ran low"; }
fn high_priority_task() -> string { return "ran high"; }

schedule("low_priority_task", 1);
schedule("high_priority_task", 10);

run_scheduled() // ["ran high", "ran low"] — highest priority first,
                // FIFO among equal priorities
```

A real priority queue; not resource/cost/latency/GPU/energy-aware
scheduling.

### New standard library functions

| Function | Signature | Notes |
|---|---|---|
| `network_members(name)` | `(string) -> [string]` | |
| `network_leader(name)` | `(string) -> string \| null` | |
| `network_health(name)` | `(string) -> [{member, healthy}]` | |
| `network_join(name, agent)` | `(string, string) -> void` | agent must be declared |
| `network_leave(name, agent)` | `(string, string) -> void` | |
| `cluster_status(name)` | `(string) -> {replicas, autoscaling}` | |
| `cluster_dispatch(cluster, agent, input)` | `(string, string, string) -> {replica, output}` | |
| `on_request(agent, function_name)` | `(string, string) -> void` | |
| `request(from, to, ...args)` | `(string, string, any...) -> value` | |
| `subscribe`/`unsubscribe(topic, agent)` | `(string, string) -> void` | |
| `publish(topic, text)` | `(string, string) -> void` | |
| `emit(event, payload)` | `(string, any) -> int` | returns handler count |
| `vector_index_add(index, id, text, metadata?)` | `(string, string, string, map?) -> void` | |
| `vector_index_search(index, query, k, filter?)` | `(string, string, int, map?) -> [{...}]` | |
| `compress_context(text, max_words)` | `(string, int) -> string` | |
| `dedupe_chunks(array)` | `([string]) -> [string]` | |
| `schedule(function_name, priority)` | `(string, int) -> void` | |
| `run_scheduled()` | `() -> [value]` | |
| `agent_models(name)` | `(string) -> [string]` | |


